//! EVM interpreter.

pub mod gas;
pub use gas::{Gas, GasTracker, MemoryGas};

#[macro_use]
mod utils;

/// Base EVM instruction implementations.
///
/// This module is not part of the stable API and has no stability guarantees.
#[doc(hidden)]
pub mod instructions;
pub use instructions::i256;

pub(crate) mod dispatch;

#[doc(hidden)] // For macro only. Not public API.
pub mod private;

pub mod opcode;
pub use opcode::op;

mod ctrl;
pub use ctrl::{BytecodeRef, Pc};

mod stack;
pub(crate) use stack::{RawStack, StackBacking};
pub use stack::{Stack, StackMut, StackRef, Word};

mod memory;
pub use memory::Memory;

mod message;
pub use message::{Message, MessageExt, MessageKind, derive_create_destination};

mod host;
pub use host::{Host, MessageResult, MessageResultExt};

mod runtime;
pub(crate) use runtime::InterpreterPool;
pub use runtime::{Interpreter, InterpreterState};

/// Instruction result type.
pub type Result<T = (), E = InstrStop> = core::result::Result<T, E>;

/// Result of executing an EVM instruction.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum InstrStop {
    /// Encountered a `STOP` opcode
    #[default]
    Stop = 1, // Start at 1 so that `Result<(), _>::Ok(())` is 0.
    /// Return from the current call.
    Return,
    /// Self-destruct the current contract.
    SelfDestruct,

    // Revert Codes
    /// Revert the transaction.
    Revert = 0x10,
    /// Exceeded maximum call depth.
    CallTooDeep,
    /// Insufficient funds for transfer.
    OutOfFunds,

    // Halt Codes
    /// Out of gas error.
    OutOfGas = 0x20,
    /// Out of gas error encountered during memory expansion.
    MemoryOOG,
    /// The memory limit of the EVM has been exceeded.
    MemoryLimitOOG,
    /// Out of gas error encountered during the execution of a precompiled contract.
    PrecompileOOG,
    /// Out of gas error encountered while calling an invalid operand.
    InvalidOperandOOG,
    /// Out of gas error encountered while checking for reentrancy sentry.
    ReentrancySentryOOG,
    /// Invalid `CALL` with value transfer in static context.
    CallNotAllowedInsideStatic,
    /// Invalid state modification in static call.
    StateChangeDuringStaticCall,
    /// Undefined opcode.
    OpcodeNotFound,
    /// Encountered the designated invalid opcode (`0xFE`).
    InvalidFEOpcode,
    /// Invalid jump destination. Dynamic jumps points to invalid not jumpdest opcode.
    InvalidJump,
    /// The feature or opcode is not activated in this version of the EVM.
    NotActivated,
    /// Attempting to pop a value from an empty stack.
    StackUnderflow,
    /// Attempting to push a value onto a full stack.
    StackOverflow,
    /// Invalid memory or storage offset.
    OutOfOffset,
    /// Address collision during contract creation.
    CreateCollision,
    /// Error in precompiled contract execution.
    PrecompileError,
    /// Nonce overflow.
    NonceOverflow,
    /// Exceeded contract size limit during creation.
    CreateContractSizeLimit,
    /// Created contract starts with invalid bytes (`0xEF`).
    CreateContractStartingWithEF,
    /// Exceeded init code size limit (EIP-3860:  Limit and meter initcode).
    CreateInitCodeSizeLimit,
    /// Fatal external error. Returned by database.
    FatalExternalError,
    /// Invalid encoding of an instruction's immediate operand.
    InvalidImmediateEncoding,
    /// Attempted to change a protocol system contract's code.
    SystemContractCodeChange,
}

impl InstrStop {
    /// Returns whether execution completed successfully.
    #[doc(alias = "is_ok")]
    #[inline]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Stop | Self::Return | Self::SelfDestruct)
    }

    /// Returns whether execution reverted without an exceptional halt.
    #[inline]
    pub const fn is_revert(self) -> bool {
        matches!(self, Self::Revert | Self::CallTooDeep | Self::OutOfFunds)
    }

    /// Returns whether execution halted exceptionally.
    #[inline]
    pub const fn is_halt(self) -> bool {
        !self.is_success() && !self.is_revert()
    }

    /// Returns whether execution ran out of gas or exceeded the memory limit.
    #[inline]
    pub const fn is_out_of_gas(self) -> bool {
        matches!(
            self,
            Self::OutOfGas
                | Self::MemoryOOG
                | Self::MemoryLimitOOG
                | Self::PrecompileOOG
                | Self::InvalidOperandOOG
                | Self::ReentrancySentryOOG
        )
    }

    /// Returns whether execution hit a fatal host/extension boundary error.
    #[inline]
    pub const fn is_fatal(self) -> bool {
        matches!(self, Self::FatalExternalError)
    }
}

impl core::fmt::Display for InstrStop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Stop => "Stop",
            Self::Return => "Return",
            Self::SelfDestruct => "SelfDestruct",
            Self::Revert => "execution reverted",
            Self::CallTooDeep => "call too deep",
            Self::OutOfFunds => "out of funds",
            Self::OutOfGas => "out of gas",
            Self::MemoryOOG => "out of gas: memory expansion",
            Self::MemoryLimitOOG => "out of gas: memory limit exceeded",
            Self::PrecompileOOG => "out of gas: precompile",
            Self::InvalidOperandOOG => "out of gas: invalid operand",
            Self::ReentrancySentryOOG => "out of gas: reentrancy sentry",
            Self::CallNotAllowedInsideStatic => "call not allowed inside static call",
            Self::StateChangeDuringStaticCall => "state change during static call",
            Self::OpcodeNotFound => "opcode not found",
            Self::InvalidFEOpcode => "invalid 0xFE opcode",
            Self::InvalidJump => "invalid jump destination",
            Self::NotActivated => "feature or opcode not activated",
            Self::StackUnderflow => "stack underflow",
            Self::StackOverflow => "stack overflow",
            Self::OutOfOffset => "out of offset",
            Self::CreateCollision => "create collision",
            Self::PrecompileError => "precompile error",
            Self::NonceOverflow => "nonce overflow",
            Self::CreateContractSizeLimit => "create contract size limit",
            Self::CreateContractStartingWithEF => "create contract starting with 0xEF",
            Self::CreateInitCodeSizeLimit => "create initcode size limit",
            Self::FatalExternalError => "fatal external error",
            Self::InvalidImmediateEncoding => "invalid immediate encoding",
            Self::SystemContractCodeChange => "system contract code change",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostError;
    use alloc::string::ToString;
    use revm::context_interface::result::{HaltReason, OutOfGasError};

    #[test]
    fn halt_error_messages_match_revm() {
        for (native, reference) in [
            (InstrStop::OutOfGas, HaltReason::OutOfGas(OutOfGasError::Basic)),
            (InstrStop::MemoryOOG, HaltReason::OutOfGas(OutOfGasError::Memory)),
            (InstrStop::MemoryLimitOOG, HaltReason::OutOfGas(OutOfGasError::MemoryLimit)),
            (InstrStop::PrecompileOOG, HaltReason::OutOfGas(OutOfGasError::Precompile)),
            (InstrStop::InvalidOperandOOG, HaltReason::OutOfGas(OutOfGasError::InvalidOperand)),
            (InstrStop::ReentrancySentryOOG, HaltReason::OutOfGas(OutOfGasError::ReentrancySentry)),
            (InstrStop::OpcodeNotFound, HaltReason::OpcodeNotFound),
            (InstrStop::InvalidFEOpcode, HaltReason::InvalidFEOpcode),
            (InstrStop::InvalidJump, HaltReason::InvalidJump),
            (InstrStop::NotActivated, HaltReason::NotActivated),
            (InstrStop::StackUnderflow, HaltReason::StackUnderflow),
            (InstrStop::StackOverflow, HaltReason::StackOverflow),
            (InstrStop::OutOfOffset, HaltReason::OutOfOffset),
            (InstrStop::CreateCollision, HaltReason::CreateCollision),
            (InstrStop::PrecompileError, HaltReason::PrecompileError),
            (InstrStop::NonceOverflow, HaltReason::NonceOverflow),
            (InstrStop::CreateContractSizeLimit, HaltReason::CreateContractSizeLimit),
            (InstrStop::CreateContractStartingWithEF, HaltReason::CreateContractStartingWithEF),
            (InstrStop::CreateInitCodeSizeLimit, HaltReason::CreateInitCodeSizeLimit),
            (InstrStop::StateChangeDuringStaticCall, HaltReason::StateChangeDuringStaticCall),
            (InstrStop::CallNotAllowedInsideStatic, HaltReason::CallNotAllowedInsideStatic),
            (InstrStop::OutOfFunds, HaltReason::OutOfFunds),
            (InstrStop::CallTooDeep, HaltReason::CallTooDeep),
        ] {
            assert_eq!(native.to_string(), reference.to_string());
            assert_eq!(HostError::from(native).to_string(), reference.to_string());
        }
    }
}
