//! Deterministic in-flight Bogota assertion diff and event indexes.

use super::Evm;
use crate::{
    EvmTypes, HostError,
    ethereum::eip7906::{
        ACCOUNT_BALANCE_CHANGED, ACCOUNT_CODE_HASH_CHANGED, ACCOUNT_NONCE_CHANGED,
        ACCOUNT_STORAGE_CHANGED, TxDiffParam, TxTraceParam,
    },
    interpreter::{Host, InstrStop},
};
use alloc::vec::Vec;
use alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, U256, map::HashMap};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameDiff {
    balances: Vec<(Address, U256, U256)>,
    slots: Vec<(Address, U256, U256, U256)>,
    deployments: Vec<(Address, U256)>,
    flags: Vec<(Address, u8)>,
    address_slots: HashMap<Address, Vec<usize>>,
    address_events: HashMap<Address, Vec<usize>>,
    topic_events: HashMap<U256, Vec<usize>>,
}

impl FrameDiff {
    fn capture<T: EvmTypes>(host: &Evm<'_, T>) -> Self {
        let mut diff = Self::default();
        for a in host.state.transaction_account_changes() {
            let before = a.original.cloned().unwrap_or_default();
            let after = a.current.cloned().unwrap_or_default();
            let mut flags = 0;
            if before.nonce != after.nonce {
                flags |= ACCOUNT_NONCE_CHANGED;
            }
            if before.balance != after.balance {
                flags |= ACCOUNT_BALANCE_CHANGED;
                diff.balances.push((a.address, before.balance, after.balance));
            }
            for (&key, slot) in &a.storage.slots {
                if slot.value.original != slot.value.current {
                    flags |= ACCOUNT_STORAGE_CHANGED;
                    diff.slots.push((a.address, key, slot.value.original, slot.value.current));
                }
            }
            if before.code_hash != after.code_hash {
                flags |= ACCOUNT_CODE_HASH_CHANGED;
            }
            // The reference view observes net code installation, including host code
            // changes, rather than the account-lifetime CREATE marker. Delegation
            // designators change the code hash but are not deployed contracts.
            if before.code_hash == KECCAK256_EMPTY
                && after.code_hash != KECCAK256_EMPTY
                && !after.code.as_ref().is_some_and(|code| code.is_eip7702())
            {
                diff.deployments.push((a.address, U256::from_be_bytes(after.code_hash.0)));
            }
            if flags != 0 {
                diff.flags.push((a.address, flags));
            }
        }
        diff.balances.sort_unstable_by_key(|v| v.0);
        diff.slots.sort_unstable_by_key(|v| (v.0, v.1));
        diff.deployments.sort_unstable_by_key(|v| v.0);
        diff.flags.sort_unstable_by_key(|v| v.0);
        for (index, (address, ..)) in diff.slots.iter().enumerate() {
            diff.address_slots.entry(*address).or_default().push(index);
        }
        for (index, log) in host.state.logs().iter().enumerate() {
            diff.address_events.entry(log.address).or_default().push(index);
            for (topic_index, topic) in log.data.topics().iter().enumerate().skip(1) {
                if !log.data.topics()[1..topic_index].contains(topic) {
                    diff.topic_events.entry(U256::from_be_bytes(topic.0)).or_default().push(index);
                }
            }
        }
        diff
    }
}

impl<T: EvmTypes> Evm<'_, T> {
    fn ensure_frame_diff(&mut self) -> Option<()> {
        if !self.frame_tx.as_ref()?.is_post_tx() {
            return None;
        }
        if self.frame_tx.as_ref()?.diff.is_none() {
            let diff = FrameDiff::capture(self);
            self.frame_tx.as_mut()?.diff = Some(diff);
        }
        Some(())
    }

    pub(crate) fn frame_trace_value(&mut self, param: U256, index: U256) -> Option<U256> {
        self.ensure_frame_diff()?;
        let selector = TxTraceParam::try_from(u8::try_from(param).ok()?).ok()?;
        if selector.requires_zero_index() && !index.is_zero() {
            return None;
        }
        let i = usize::try_from(index).ok()?;
        let rt = self.frame_tx.as_ref()?;
        let d = rt.diff.as_ref()?;
        let logs = self.state.logs();
        Some(match selector {
            TxTraceParam::BalancesChanged => U256::from(d.balances.len()),
            TxTraceParam::SlotsChanged => U256::from(d.slots.len()),
            TxTraceParam::ContractsDeployed => U256::from(d.deployments.len()),
            TxTraceParam::BalanceChangeAddress => {
                U256::from_be_slice(d.balances.get(i)?.0.as_slice())
            }
            TxTraceParam::BalanceBefore => d.balances.get(i)?.1,
            TxTraceParam::BalanceAfter => d.balances.get(i)?.2,
            TxTraceParam::SlotChangeAddress => U256::from_be_slice(d.slots.get(i)?.0.as_slice()),
            TxTraceParam::SlotKey => d.slots.get(i)?.1,
            TxTraceParam::SlotValueBefore => d.slots.get(i)?.2,
            TxTraceParam::SlotValueAfter => d.slots.get(i)?.3,
            TxTraceParam::DeployedAddress => {
                U256::from_be_slice(d.deployments.get(i)?.0.as_slice())
            }
            TxTraceParam::DeployedCodeHash => d.deployments.get(i)?.1,
            TxTraceParam::EventsCount => U256::from(logs.len()),
            TxTraceParam::EventAddress => U256::from_be_slice(logs.get(i)?.address.as_slice()),
            TxTraceParam::EventTopicCount => U256::from(logs.get(i)?.data.topics().len()),
            TxTraceParam::EventTopic0
            | TxTraceParam::EventTopic1
            | TxTraceParam::EventTopic2
            | TxTraceParam::EventTopic3 => U256::from_be_bytes(
                logs.get(i)?.data.topics().get(usize::from(u8::from(selector) - 0x0f))?.0,
            ),
            TxTraceParam::EventDataLength => U256::from(logs.get(i)?.data.data.len()),
            TxTraceParam::GasPreCharge => rt.approval.gas_pre_charge,
            TxTraceParam::GasPayerAddress => U256::from_be_slice(rt.approval.payer?.as_slice()),
        })
    }

    pub(crate) fn frame_diff_value(
        &mut self,
        param: U256,
        in2: U256,
        in3: U256,
        skip_cold: bool,
    ) -> Result<(U256, bool), HostError> {
        self.ensure_frame_diff().ok_or(InstrStop::OpcodeNotFound)?;
        let selector = u8::try_from(param)
            .ok()
            .and_then(|v| TxDiffParam::try_from(v).ok())
            .ok_or(InstrStop::OpcodeNotFound)?;
        if selector.requires_zero_third_operand() && !in3.is_zero() {
            return Err(InstrStop::OpcodeNotFound.into());
        }
        let word = in2.to_be_bytes::<32>();
        let address = Address::from_slice(&word[12..]);
        if selector.is_storage_lookup() {
            // A cold lookup that cannot pay must not load even the account. Such reads
            // would otherwise incorrectly appear in the block access list.
            if skip_cold && !self.state.is_storage_warm(&address, &in3) {
                return Err(InstrStop::OutOfGas.into());
            }
            let load = self.sload(&address, &in3, skip_cold)?;
            let value = if selector == TxDiffParam::SlotValueBefore {
                self.state.storage(&address)?.into_slot(in3)?.get().original
            } else {
                load.value
            };
            return Ok((value, load.is_cold));
        }
        if selector.is_account_lookup() {
            let load = self.load_account(&address, false, skip_cold)?;
            let account = self.state.account(&address)?;
            let before = account.original().cloned().unwrap_or_default();
            let value = match selector {
                TxDiffParam::BalanceBefore => before.balance,
                TxDiffParam::BalanceAfter => account.balance(),
                TxDiffParam::CodeHashBefore => U256::from_be_bytes(before.code_hash.0),
                TxDiffParam::CodeHashAfter => U256::from_be_bytes(account.code_hash().0),
                _ => unreachable!(),
            };
            return Ok((value, load.is_cold));
        }
        let d = self.frame_tx.as_ref().unwrap().diff.as_ref().unwrap();
        let index = usize::try_from(in3).map_err(|_| InstrStop::OpcodeNotFound)?;
        let count = |entries: Option<&Vec<usize>>| Some(U256::from(entries.map_or(0, Vec::len)));
        let at = |entries: Option<&Vec<usize>>| entries?.get(index).copied().map(U256::from);
        let selected = match selector {
            TxDiffParam::AddressSlotsCount => count(d.address_slots.get(&address)),
            TxDiffParam::AddressSlotIndex => at(d.address_slots.get(&address)),
            TxDiffParam::AddressEventsCount => count(d.address_events.get(&address)),
            TxDiffParam::AddressEventIndex => at(d.address_events.get(&address)),
            TxDiffParam::AccountChangeFlags => Some(U256::from(
                d.flags.binary_search_by_key(&address, |v| v.0).ok().map_or(0, |i| d.flags[i].1),
            )),
            TxDiffParam::TopicEventsCount => count(d.topic_events.get(&in2)),
            TxDiffParam::TopicEventIndex => at(d.topic_events.get(&in2)),
            _ => None,
        };
        Ok((selected.ok_or(InstrStop::OpcodeNotFound)?, false))
    }

    pub(crate) fn frame_event_data(&self, index: U256) -> Option<Bytes> {
        if !self.frame_tx.as_ref()?.is_post_tx() {
            return None;
        }
        self.state.logs().get(usize::try_from(index).ok()?).map(|l| l.data.data.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BaseEvmTypes, Precompiles, SpecId,
        bytecode::Bytecode,
        env::BlockEnvExt,
        ethereum::ethereum_tx_registry,
        evm::{AccountInfo, InMemoryDB, frame::FrameTransactionRuntime},
    };
    use alloc::sync::Arc;
    use alloy_eips::eip8141::{Frame, FrameMode};

    #[test]
    fn deployment_view_uses_net_code_installation_and_excludes_delegation() {
        let deployed = Address::with_last_byte(0xa1);
        let delegated = Address::with_last_byte(0xa2);
        let replaced = Address::with_last_byte(0xa3);
        let unchanged = Address::with_last_byte(0xa4);
        let code = Bytecode::new_legacy(Bytes::from_static(&[0x00]));
        let mut db = InMemoryDB::default();
        db.insert_account_info(&replaced, AccountInfo::default().with_code(code.clone()));
        db.insert_account_info(&unchanged, AccountInfo::default().with_code(code.clone()));
        let mut host = Evm::<BaseEvmTypes>::new(
            SpecId::BOGOTA,
            BlockEnvExt::default(),
            ethereum_tx_registry(SpecId::BOGOTA),
            db,
            Precompiles::base(SpecId::BOGOTA),
        );
        host.state.account(&deployed).unwrap().set_code_slow(code.clone());
        host.state.account(&delegated).unwrap().set_code_slow(Bytecode::new_eip7702(deployed));
        host.state
            .account(&replaced)
            .unwrap()
            .set_code_slow(Bytecode::new_legacy(Bytes::from_static(&[0x5f, 0x00])));
        host.state.account(&unchanged).unwrap();

        let diff = FrameDiff::capture(&host);
        assert_eq!(diff.deployments, [(deployed, U256::from_be_bytes(code.hash_slow().0))]);
        assert_eq!(
            diff.flags,
            [
                (deployed, ACCOUNT_CODE_HASH_CHANGED),
                (delegated, ACCOUNT_CODE_HASH_CHANGED),
                (replaced, ACCOUNT_CODE_HASH_CHANGED),
            ]
        );
    }

    #[test]
    fn unaffordable_diff_reads_do_not_load_accounts_and_successful_reads_warm_independently() {
        let address = Address::with_last_byte(0x90);
        let slot = U256::from(9);
        let mut db = InMemoryDB::default();
        db.insert_account_info(&address, AccountInfo::default().with_balance(U256::from(11)));
        db.insert_account_storage(&address, &slot, &U256::from(33));
        let mut host = Evm::<BaseEvmTypes>::new(
            SpecId::BOGOTA,
            BlockEnvExt::default(),
            ethereum_tx_registry(SpecId::BOGOTA),
            db,
            Precompiles::base(SpecId::BOGOTA),
        );
        let mut runtime = FrameTransactionRuntime::new(address);
        runtime.payload = Arc::new(alloy_consensus::TxEip8141 {
            frames: alloc::vec![Frame { mode: FrameMode::PostTx, ..Default::default() }],
            ..Default::default()
        });
        host.frame_tx = Some(runtime);
        let address = U256::from_be_slice(address.as_slice());
        for (selector, third) in [(0, slot), (2, U256::ZERO)] {
            assert_eq!(
                host.frame_diff_value(U256::from(selector), address, third, true),
                Err(HostError::Halt(InstrStop::OutOfGas))
            );
            assert_eq!(host.state.transaction_account_changes().count(), 0);
        }
        assert_eq!(
            host.frame_diff_value(U256::ZERO, address, slot, false).unwrap(),
            (U256::from(33), true)
        );
        assert_eq!(
            host.frame_diff_value(U256::from(1), address, slot, true).unwrap(),
            (U256::from(33), false)
        );
        // A storage lookup does not make the containing account warm.
        assert_eq!(
            host.frame_diff_value(U256::from(2), address, U256::ZERO, true),
            Err(HostError::Halt(InstrStop::OutOfGas))
        );
        assert_eq!(
            host.frame_diff_value(U256::from(3), address, U256::ZERO, false).unwrap(),
            (U256::from(11), true)
        );
        assert_eq!(
            host.frame_diff_value(U256::from(2), address, U256::ZERO, true).unwrap(),
            (U256::from(11), false)
        );
        // Live lookups do not add unchanged accounts or slots to the captured view.
        assert_eq!(host.frame_trace_value(U256::ZERO, U256::ZERO), Some(U256::ZERO));
        assert_eq!(host.frame_trace_value(U256::from(1), U256::ZERO), Some(U256::ZERO));
    }
}
