//! Pure EIP-8141 claim verifiers. Failure halts the call and consumes all forwarded gas.

use crate::{
    interpreter::GasTracker,
    precompiles::{PrecompileError, PrecompileHalt, PrecompileOutput, PrecompileResult},
    utils::bool_to_bytes32,
};
use alloy_eips::eip8141::{LEANSPHINCS_SIGNATURE_LENGTH, SECP256K1N, SECP256R1N};
use alloy_primitives::{Address, U256, keccak256};

/// Execution gas consumed by the secp256k1 verifier, excluding frame-entry access.
pub const SECP256K1_GAS: u64 = 3_000;
/// Execution gas consumed by the P-256 verifier, excluding frame-entry access.
pub const P256_GAS: u64 = 6_900;
/// Execution gas consumed by the leanSPHINCS verifier, excluding frame-entry access.
pub const LEANSPHINCS_GAS: u64 = 13_200;

const _: () = assert!(lean_sphincs::SIGNATURE_BYTES == LEANSPHINCS_SIGNATURE_LENGTH);
const _: () = assert!(lean_sphincs::PUBLIC_KEY_BYTES == 32 && lean_sphincs::MESSAGE_BYTES == 32);

fn charge(gas: &mut GasTracker, cost: u64) -> Result<(), PrecompileError> {
    if gas.spend(cost).is_err() {
        gas.set_remaining(0);
        return Err(PrecompileHalt::OutOfGas.into());
    }
    Ok(())
}

fn outcome(valid: bool, gas: &mut GasTracker) -> PrecompileResult {
    if !valid {
        gas.set_remaining(0);
        return Err(PrecompileHalt::InvalidFrameProof.into());
    }
    Ok(PrecompileOutput::new(bool_to_bytes32(true)))
}

/// Verifies exactly `key[32] || msg[32] || v[1] || r[32] || s[32]`.
/// The key is a left-padded address and v is 0 or 1. Invalid inputs halt the call.
pub fn secp256k1(input: &[u8], gas: &mut GasTracker) -> PrecompileResult {
    charge(gas, SECP256K1_GAS)?;
    let valid = if let Ok(input) = <&[u8; 129]>::try_from(input) {
        let r = U256::from_be_slice(&input[65..97]);
        let s = U256::from_be_slice(&input[97..129]);
        input[64] <= 1
            && r > U256::ZERO
            && r < SECP256K1N
            && s > U256::ZERO
            && s <= SECP256K1N / U256::from(2)
            && crate::precompiles::crypto()
                .secp256k1_ecrecover(
                    arrayref::array_ref![input, 65, 64],
                    input[64],
                    arrayref::array_ref![input, 32, 32],
                )
                .is_ok_and(|key| key.as_slice() == &input[..32])
    } else {
        false
    };
    outcome(valid, gas)
}

/// Verifies exactly `key[32] || msg[32] || r[32] || s[32] || qx[32] || qy[32]`.
/// The key must match the public-key address, and s must be low. Invalid inputs halt.
pub fn p256(input: &[u8], gas: &mut GasTracker) -> PrecompileResult {
    charge(gas, P256_GAS)?;
    let valid = if let Ok(input) = <&[u8; 192]>::try_from(input) {
        let r = U256::from_be_slice(&input[64..96]);
        let s = U256::from_be_slice(&input[96..128]);
        let key = Address::from_slice(&keccak256(&input[128..192])[12..]).into_word();
        r > U256::ZERO
            && r < SECP256R1N
            && s > U256::ZERO
            && s <= SECP256R1N / U256::from(2)
            && key.as_slice() == &input[..32]
            && crate::precompiles::crypto().secp256r1_verify_signature(
                arrayref::array_ref![input, 32, 32],
                arrayref::array_ref![input, 64, 64],
                arrayref::array_ref![input, 128, 64],
            )
    } else {
        false
    };
    outcome(valid, gas)
}

/// Verifies exactly `pk[32] || msg[32] || signature[7740]` with the leanSPHINCS parameters.
/// Verification borrows the signature and performs no account or storage access.
pub fn leansphincs(input: &[u8], gas: &mut GasTracker) -> PrecompileResult {
    charge(gas, LEANSPHINCS_GAS)?;
    let valid = input.len() == 64 + LEANSPHINCS_SIGNATURE_LENGTH
        && lean_sphincs::verify(
            &lean_sphincs::B256::from_slice(&input[..32]),
            &lean_sphincs::B256::from_slice(&input[32..64]),
            &input[64..],
        );
    outcome(valid, gas)
}
