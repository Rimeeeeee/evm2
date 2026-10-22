//! Bogota transaction-local approvals and reversible state-gas ownership.

use crate::{
    Evm, EvmFeatures, EvmTypes, HostError,
    interpreter::{GasTracker, InstrStop, Message, MessageResult, MessageResultExt},
};
use alloc::vec::Vec;
use alloy_eips::eip8141::{FrameMode, FrameStatus, NONCE_MANAGER, SignatureScheme, nonce_slot};
use alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, U256, map::HashMap};

type StorageKey = U256;

/// EIP-8141 approvals accumulated by successful top-level frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FrameApprovalState {
    /// Account that approved payment.
    pub payer: Option<Address>,
    /// Amount actually deducted from the payer at payment approval.
    pub gas_pre_charge: U256,
    /// Whether execution as the transaction sender has been approved.
    pub sender_approved: bool,
}

impl FrameTransactionRuntime {
    pub(crate) fn tx_param(&self, param: U256, state_left: u64) -> Option<U256> {
        let tx = &self.payload;
        Some(match u8::try_from(param).ok()? {
            0 => U256::from(6),
            1 => U256::from(tx.nonce),
            2 => U256::from_be_slice(tx.sender.as_slice()),
            3 => tx.fees.max_priority_fee_per_gas,
            4 => tx.fees.max_fee_per_gas,
            5 => tx.fees.max_fee_per_blob_gas,
            6 => self.max_cost,
            7 => U256::from(tx.blob_versioned_hashes.len()),
            8 => U256::from_be_bytes(self.signature_hash.0),
            9 => U256::from(tx.frames.len()),
            10 => U256::from(self.current_frame_index),
            11 => U256::from(tx.signatures.len()),
            12 => U256::from(state_left),
            13 if tx.nonce_keys.is_some() => U256::from(self.legacy_nonce),
            14 => U256::from(tx.nonce_keys.as_ref()?.len()),
            15 => U256::from_be_bytes(
                alloy_eips::eip8141::nonce_keys_hash(tx.nonce_keys.as_deref()?).0,
            ),
            16 => *tx.nonce_keys.as_ref()?.first()?,
            _ => return None,
        })
    }

    pub(crate) fn param(&self, index: U256, param: U256) -> Option<U256> {
        let index = usize::try_from(index).ok()?;
        let f = self.payload.frames.get(index)?;
        Some(match u8::try_from(param).ok()? {
            0 => U256::from_be_slice(f.resolved_target(self.payload.sender).as_slice()),
            1 => U256::from(f.limits.execution),
            2 => U256::from(u8::from(f.mode)),
            3 => U256::from(f.flags),
            4 => U256::from(f.data.len()),
            5 if index < self.current_frame_index => {
                U256::from(u8::from(*self.statuses.get(index)?))
            }
            6 => U256::from(u8::from(f.allowed_scope())),
            7 => U256::from(f.is_atomic_batch() as u8),
            8 => f.value,
            9 => U256::from(f.limits.state),
            10 if index < self.current_frame_index => {
                U256::from(*self.execution_gas_used.get(index)?)
            }
            11 if index < self.current_frame_index => U256::from(*self.state_gas_used.get(index)?),
            _ => return None,
        })
    }

    pub(crate) fn signature_param(&self, index: U256, param: U256) -> Option<U256> {
        let sig = self.payload.signatures.get(usize::try_from(index).ok()?)?;
        Some(match u8::try_from(param).ok()? {
            0 if sig.scheme != SignatureScheme::Arbitrary => U256::from_be_slice(
                if sig.signer.is_empty() { self.payload.sender } else { sig.signer_address()? }
                    .as_slice(),
            ),
            1 => U256::from(u8::from(sig.scheme)),
            2 => {
                if sig.signs_transaction_hash() {
                    U256::ZERO
                } else {
                    U256::from_be_bytes(sig.explicit_message()?.0)
                }
            }
            3 if sig.scheme == SignatureScheme::Arbitrary => U256::from(sig.signature.len()),
            _ => return None,
        })
    }

    pub(crate) fn is_post_tx(&self) -> bool {
        self.payload
            .frames
            .get(self.current_frame_index)
            .is_some_and(|f| f.mode == FrameMode::PostTx)
    }
}

impl<T: EvmTypes> Evm<'_, T> {
    pub(crate) fn apply_frame_approval(
        &mut self,
        address: Address,
        scope: U256,
        state_left: u64,
    ) -> Result<u64, HostError> {
        let rt = self.frame_tx.as_ref().ok_or(InstrStop::OpcodeNotFound)?;
        if rt.is_post_tx() {
            return Err(InstrStop::StateChangeDuringStaticCall.into());
        }
        let f = &rt.payload.frames[rt.current_frame_index];
        let scope = u8::try_from(scope).map_err(|_| InstrStop::Revert)?;
        if address != rt.resolved_target || scope == 0 || scope & !(f.flags & 3) != 0 {
            return Err(InstrStop::Revert.into());
        }
        let old = rt.approval_stack.last().copied().unwrap_or(rt.approval);
        let sender = rt.payload.sender;
        let execution = scope & 2 != 0;
        let payment = scope & 1 != 0;
        if execution && (old.sender_approved || address != sender) {
            return Err(InstrStop::Revert.into());
        }
        if payment && (old.payer.is_some() || (!old.sender_approved && !execution)) {
            return Err(InstrStop::Revert.into());
        }
        let keys = rt.payload.nonce_keys.clone();
        let keyed = keys.as_ref().is_some_and(|keys| keys != &[U256::ZERO]);
        let sequence = rt.payload.nonce;
        let maximum = rt.max_cost;
        let mut charged = 0u64;
        let mut precharge = old.gas_pre_charge;
        if payment {
            if keyed {
                for key in keys.as_ref().unwrap() {
                    let first = self
                        .state
                        .storage(&NONCE_MANAGER)?
                        .into_slot(nonce_slot(sender, *key))?
                        .current()
                        .is_zero();
                    if first {
                        charged = charged
                            .checked_add(u64::from(
                                self.version()
                                    .gas_params
                                    .get(crate::version::GasId::SstoreSetState),
                            ))
                            .ok_or(InstrStop::OutOfGas)?;
                    }
                }
            } else {
                let empty = {
                    let account = self.state.account(&sender)?;
                    if account.nonce() == u64::MAX {
                        return Err(InstrStop::OutOfGas.into());
                    }
                    account.get().is_none_or(super::AccountInfo::is_empty)
                };
                if empty {
                    charged = self.version().gas_params.new_account_state_gas();
                }
            }
            let balance = self.state.account(&address)?.balance();
            if self.feature(EvmFeatures::BALANCE_CHECK) && balance < maximum {
                return Err(InstrStop::Revert.into());
            }
            if charged > state_left {
                return Err(InstrStop::OutOfGas.into());
            }
            if keyed {
                let next = sequence.checked_add(1).ok_or(InstrStop::OutOfGas)?;
                for key in keys.unwrap() {
                    self.state
                        .storage(&NONCE_MANAGER)?
                        .into_slot(nonce_slot(sender, key))?
                        .set(U256::from(next));
                }
            } else {
                self.state.account(&sender)?.bump_nonce();
            }
            let fees = self.feature(EvmFeatures::FEE_CHARGE);
            if fees {
                self.state.account(&address)?.set_balance(balance.saturating_sub(maximum));
            } else if !self.feature(EvmFeatures::BALANCE_CHECK) && balance < maximum {
                self.state.account(&address)?.set_balance(maximum);
            }
            precharge = if fees { balance.min(maximum) } else { U256::ZERO };
        }
        let rt = self.frame_tx.as_mut().unwrap();
        let next = FrameApprovalState {
            payer: if payment { Some(address) } else { old.payer },
            gas_pre_charge: precharge,
            sender_approved: old.sender_approved || execution,
        };
        if let Some(current) = rt.approval_stack.last_mut() {
            *current = next;
        } else {
            rt.root_approval = Some(next);
        }
        Ok(charged)
    }

    pub(crate) fn execute_default_frame(
        &mut self,
        message: &Message<T>,
    ) -> Result<Option<MessageResult<T>>, crate::ExecutionError> {
        let Some(rt) = &self.frame_tx else {
            return Ok(None);
        };
        if message.depth != 0
            || rt.payload.frames[rt.current_frame_index].mode != FrameMode::Verify
            || self.precompiles().contains(&message.call_target)
            || self.state.account(&message.destination)?.code_hash() != KECCAK256_EMPTY
        {
            return Ok(None);
        }
        let f = &rt.payload.frames[rt.current_frame_index];
        let scope = f.flags & 3;
        let index = if scope & 2 != 0 { 0 } else { 1 };
        let valid = scope != 0
            && rt.payload.signatures.get(index).is_some_and(|sig| {
                sig.scheme == SignatureScheme::Secp256k1
                    && sig.signs_transaction_hash()
                    && if sig.signer.is_empty() {
                        rt.payload.sender == message.destination
                    } else {
                        sig.signer_address() == Some(message.destination)
                    }
            });
        let mut gas = GasTracker::new_isolated(message.gas_limit, message.reservoir);
        let stop = if valid {
            match self.apply_frame_approval(message.destination, U256::from(scope), gas.reservoir())
            {
                Ok(cost) => {
                    gas.spend_state(cost).expect("approval checked state budget");
                    InstrStop::Return
                }
                Err(HostError::Halt(stop)) => stop,
                Err(HostError::Execution(error)) => return Err(error),
            }
        } else {
            InstrStop::Revert
        };
        gas.settle_gas(stop);
        Ok(Some(MessageResultExt { stop, gas, output: Bytes::new(), ..Default::default() }))
    }
}

/// Per-transaction EIP-8141 interpreter runtime state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameTransactionRuntime {
    pub diff: Option<super::frame_diff::FrameDiff>,
    pub payload: alloc::sync::Arc<alloy_consensus::TxEip8141>,
    pub signature_hash: alloy_primitives::B256,
    pub max_cost: U256,
    /// Sender account nonce before any transaction frame executes.
    pub legacy_nonce: u64,
    /// Index of the currently executing top-level frame.
    pub current_frame_index: usize,
    /// Resolved target of the current top-level frame.
    pub resolved_target: Address,
    /// Statuses of completed top-level frames.
    pub statuses: Vec<FrameStatus>,
    /// Execution-gas usage reported by completed top-level frames.
    pub execution_gas_used: Vec<u64>,
    /// State-gas usage reported by completed top-level frames.
    pub state_gas_used: Vec<u64>,
    /// Committed approval state.
    pub approval: FrameApprovalState,
    /// Approval scopes corresponding to active interpreter call frames.
    pub approval_stack: Vec<FrameApprovalState>,
    /// Approval produced by the completed top-level interpreter frame.
    pub root_approval: Option<FrameApprovalState>,
    /// Revertible state-gas attribution across top-level frames.
    state_gas: FrameStateGas,
}

/// State-gas ownership and rollback bookkeeping, separate from approvals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FrameStateGas {
    /// Top-level frame owning each outstanding storage charge.
    owners: HashMap<(Address, StorageKey), usize>,
    /// Attribution changes retained across top-level frames.
    journal: Vec<FrameStateGasJournalEntry>,
    /// Checkpoints for active interpreter call frames.
    checkpoints: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FrameStateGasJournalEntry {
    Owner { slot: (Address, StorageKey), previous: Option<usize> },
    GasUsed { frame_index: usize, previous: u64 },
}

impl FrameTransactionRuntime {
    /// Creates runtime state for a frame transaction.
    pub(crate) fn new(resolved_target: Address) -> Self {
        Self { resolved_target, ..Self::default() }
    }

    /// Creates runtime state with capacity for all top-level frame results.
    pub(crate) fn with_capacity(resolved_target: Address, frame_count: usize) -> Self {
        Self {
            statuses: Vec::with_capacity(frame_count),
            execution_gas_used: Vec::with_capacity(frame_count),
            state_gas_used: Vec::with_capacity(frame_count),
            ..Self::new(resolved_target)
        }
    }

    /// Enters an interpreter call frame approval scope.
    pub(crate) fn enter_scope(&mut self) {
        let state = self.approval_stack.last().copied().unwrap_or(self.approval);
        self.approval_stack.push(state);
        self.state_gas.checkpoints.push(self.state_gas.journal.len());
    }

    /// Exits an interpreter call frame, committing approvals only on success.
    pub(crate) fn exit_scope(&mut self, success: bool) {
        let state = self.approval_stack.pop();
        let state_gas_checkpoint = self.state_gas.checkpoints.pop();
        if !success && let Some(checkpoint) = state_gas_checkpoint {
            self.revert_state_gas(checkpoint);
        }
        let Some(state) = state else {
            return;
        };
        if !success {
            return;
        }
        if let Some(parent) = self.approval_stack.last_mut() {
            *parent = state;
        } else {
            self.root_approval = Some(state);
        }
    }

    /// Commits approvals emitted by a successful top-level frame.
    pub(crate) const fn commit_root_approval(&mut self) {
        if let Some(state) = self.root_approval.take() {
            self.approval = state;
        }
    }

    /// Returns a checkpoint for state-gas ownership and receipt attribution.
    pub(crate) const fn state_gas_checkpoint(&self) -> usize {
        self.state_gas.journal.len()
    }

    /// Reverts state-gas ownership and receipt attribution to `checkpoint`.
    pub(crate) fn revert_state_gas(&mut self, checkpoint: usize) {
        while self.state_gas.journal.len() > checkpoint {
            match self.state_gas.journal.pop().expect("length checked") {
                FrameStateGasJournalEntry::Owner { slot, previous } => {
                    if let Some(owner) = previous {
                        self.state_gas.owners.insert(slot, owner);
                    } else {
                        self.state_gas.owners.remove(&slot);
                    }
                }
                FrameStateGasJournalEntry::GasUsed { frame_index, previous } => {
                    if let Some(gas_used) = self.state_gas_used.get_mut(frame_index) {
                        *gas_used = previous;
                    }
                }
            }
        }
    }

    /// Attributes a successful storage-creation state-gas charge to the current frame.
    pub(crate) fn record_sstore_state_gas(
        &mut self,
        address: Address,
        key: StorageKey,
        amount: u64,
    ) {
        if amount == 0 {
            return;
        }
        let slot = (address, key);
        let previous_owner = self.state_gas.owners.insert(slot, self.current_frame_index);
        self.state_gas
            .journal
            .push(FrameStateGasJournalEntry::Owner { slot, previous: previous_owner });
        let gas_used = self
            .state_gas_used
            .get_mut(self.current_frame_index)
            .expect("current frame receipt initialized");
        let previous = *gas_used;
        self.state_gas.journal.push(FrameStateGasJournalEntry::GasUsed {
            frame_index: self.current_frame_index,
            previous,
        });
        *gas_used =
            gas_used.checked_add(amount).expect("frame state gas is bounded by its u64 limit");
    }

    /// Applies a storage-restoration refill and returns the amount spendable by this frame.
    pub(crate) fn refill_sstore_state_gas(
        &mut self,
        address: Address,
        key: StorageKey,
        amount: u64,
    ) -> u64 {
        if amount == 0 {
            return 0;
        }
        let slot = (address, key);
        let Some(owner) = self.state_gas.owners.remove(&slot) else {
            return 0;
        };
        self.state_gas
            .journal
            .push(FrameStateGasJournalEntry::Owner { slot, previous: Some(owner) });
        let gas_used =
            self.state_gas_used.get_mut(owner).expect("outstanding charge owner has a receipt");
        let previous = *gas_used;
        self.state_gas
            .journal
            .push(FrameStateGasJournalEntry::GasUsed { frame_index: owner, previous });
        *gas_used = gas_used
            .checked_sub(amount)
            .expect("refill cannot exceed its outstanding state-gas charge");
        if owner == self.current_frame_index { amount } else { 0 }
    }
}
