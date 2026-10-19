use super::Memory;
use crate::{BaseEvmTypes, EvmTypesHost, bytecode::Bytecode};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use core::ops::Range;

/// EVM message kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageKind {
    /// Regular `CALL` message.
    #[default]
    Call,
    /// `DELEGATECALL` message.
    DelegateCall,
    /// `CALLCODE` message.
    CallCode,
    /// `CREATE` message.
    Create,
    /// `CREATE2` message.
    Create2,
    /// `STATICCALL` message.
    StaticCall,
}

impl MessageKind {
    /// Returns `true` if the message is CREATE or CREATE2.
    #[inline]
    pub const fn is_create(&self) -> bool {
        matches!(self, Self::Create | Self::Create2)
    }
}

/// Input data of a [`Message`].
///
/// `CALL`-family instructions do not copy their input out of the caller's memory: the message
/// carries the [`SharedBuffer`](Self::SharedBuffer) range instead, which stays valid because the
/// caller frame is suspended until the message returns. Create messages and the top-level
/// transaction message hold [`Bytes`](Self::Bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallInput {
    /// Owned input bytes.
    Bytes(Bytes),
    /// Range of the memory of the frame that executes the message's `CALL`-family instruction.
    ///
    /// Call hooks receive that frame as their interpreter argument; resolve the range with
    /// [`CallInput::as_bytes`] on its memory. A running frame reads its own calldata with
    /// [`Interpreter::input`](crate::interpreter::Interpreter::input) instead, since its memory
    /// is not the caller's.
    SharedBuffer(Range<usize>),
}

impl Default for CallInput {
    #[inline]
    fn default() -> Self {
        Self::Bytes(Bytes::new())
    }
}

impl From<Bytes> for CallInput {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl CallInput {
    /// Returns the length of the input in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::SharedBuffer(range) => range.len(),
        }
    }

    /// Returns whether the input is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the owned bytes, or `None` for a shared-buffer input.
    #[inline]
    pub const fn as_owned(&self) -> Option<&Bytes> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::SharedBuffer(_) => None,
        }
    }

    /// Returns the input bytes, resolving a shared-buffer range against `caller_memory`.
    ///
    /// # Panics
    ///
    /// Panics if a shared-buffer range is out of bounds of `caller_memory`, which means it is not
    /// the memory of the frame that created the message.
    #[inline]
    #[track_caller]
    pub fn as_bytes<'a>(&'a self, caller_memory: &'a Memory) -> &'a [u8] {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::SharedBuffer(range) => match caller_memory.as_slice().get(range.clone()) {
                Some(bytes) => bytes,
                None => shared_buffer_out_of_bounds(range, caller_memory.len()),
            },
        }
    }

    /// Returns the input as owned bytes, copying a shared-buffer range out of `caller_memory`.
    ///
    /// # Panics
    ///
    /// Panics under the same condition as [`CallInput::as_bytes`].
    #[inline]
    #[track_caller]
    pub fn to_bytes(&self, caller_memory: &Memory) -> Bytes {
        match self {
            Self::Bytes(bytes) => bytes.clone(),
            Self::SharedBuffer(_) => Bytes::copy_from_slice(self.as_bytes(caller_memory)),
        }
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn shared_buffer_out_of_bounds(range: &Range<usize>, len: usize) -> ! {
    panic!("shared call input {range:?} is out of bounds of the caller memory (len {len})")
}

/// Frame-local EVM call/create message executed by the interpreter for an EVM type family.
pub type Message<T = BaseEvmTypes> = MessageExt<<T as EvmTypesHost>::MessageExt>;

/// Frame-local EVM call/create message, parameterized by extension data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageExt<E = ()> {
    /// Message kind.
    pub kind: MessageKind,
    /// Current call depth.
    pub depth: u16,
    /// Gas available to this message.
    pub gas_limit: u64,
    /// EIP-8037 state-gas reservoir inherited from the parent frame.
    ///
    /// The reservoir is a shared pool threaded down into child frames and
    /// reconciled back on return by
    /// [`GasTracker::merge_child_gas`](crate::interpreter::GasTracker::merge_child_gas).
    /// Zero for non-Amsterdam execution.
    pub reservoir: u64,
    /// The account this message targets.
    ///
    /// Its meaning depends on [`MessageExt::kind`]: for call-family messages it is the account
    /// whose context is executed; for `CREATE`/`CREATE2` it is the address of the
    /// yet-to-be-created contract, derived when the message is constructed (from the creator
    /// and nonce, or from the salt and init-code hash).
    pub destination: Address,
    /// Address requested by the call opcode or transaction, before EIP-7702 code resolution.
    ///
    /// Unlike `destination`, this remains the requested callee for `CALLCODE` and
    /// `DELEGATECALL`. For create messages it is the derived contract address.
    pub call_target: Address,
    /// Immediate caller.
    pub caller: Address,
    /// Call input data, or initcode for create messages.
    pub input: CallInput,
    /// Value transferred with the message.
    pub value: U256,
    /// Bytecode this frame executes: the code at [`MessageExt::code_address`] (the resolved
    /// delegate's code for an EIP-7702 delegated call), or the initcode for create messages.
    ///
    /// Resolved by the message's producer when it is constructed, so frames never load accounts
    /// for code.
    pub code: Bytecode,
    /// Address whose code is being executed. This can differ from `destination` for `CALLCODE`,
    /// `DELEGATECALL`, and EIP-7702 delegated-code execution.
    pub code_address: Address,
    /// Whether native precompile dispatch is disabled for this frame because its bytecode was
    /// loaded through an EIP-7702 delegation designation.
    pub disable_precompiles: bool,
    /// Whether the immediate caller frame is executing in a static context. Combined with a
    /// `STATICCALL` kind, this determines whether the new frame is static.
    pub caller_is_static: bool,
    /// CREATE2 salt. Ignored for other message kinds.
    pub salt: B256,
    /// EVM type-specific extension data.
    pub ext: E,
    #[doc(hidden)] // Not public API. Please use an existing constructor.
    pub _non_exhaustive: (),
}

impl<E> MessageExt<E> {
    /// Replaces the extension, preserving all other message fields.
    #[inline]
    pub fn with_ext<F>(self, ext: F) -> MessageExt<F> {
        self.map_ext(|_| ext)
    }

    /// Transforms the extension, preserving all other message fields.
    #[inline]
    pub fn map_ext<F>(self, f: impl FnOnce(E) -> F) -> MessageExt<F> {
        let Self {
            kind,
            depth,
            gas_limit,
            reservoir,
            destination,
            call_target,
            caller,
            input,
            value,
            code,
            code_address,
            disable_precompiles,
            caller_is_static,
            salt,
            ext,
            _non_exhaustive,
        } = self;
        MessageExt {
            kind,
            depth,
            gas_limit,
            reservoir,
            destination,
            call_target,
            caller,
            input,
            value,
            code,
            code_address,
            disable_precompiles,
            caller_is_static,
            salt,
            ext: f(ext),
            _non_exhaustive,
        }
    }
}

/// Derives the contract address a create message deploys to, i.e. its
/// [`destination`](MessageExt::destination).
///
/// `nonce` is the creator's (pre-bump) nonce and is only used by the `CREATE` scheme; `CREATE2`
/// derives the address from `salt` and the initcode hash instead.
#[inline]
pub fn derive_create_destination(
    kind: MessageKind,
    caller: &Address,
    salt: &B256,
    init_code: &[u8],
    nonce: u64,
) -> Address {
    match kind {
        MessageKind::Create2 => caller.create2(salt, keccak256(init_code)),
        _ => caller.create(nonce),
    }
}
