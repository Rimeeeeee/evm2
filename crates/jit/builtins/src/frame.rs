//! Runtime operations for compiled Bogota frame instructions.

use super::*;

builtins! {

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_approve(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 3],
) -> BuiltinResult {
    let rev![offset, len, scope] = sp;
    if ecx.host().is_assert_frame() {
        return Err(InstrStop::StateChangeDuringStaticCall.into());
    }
    let len = word_to_usize(len.to_u256())?;
    let output = if len == 0 {
        0..0
    } else {
        let offset = word_to_usize(offset.to_u256())?;
        let end = offset.checked_add(len).ok_or(InstrStop::MemoryOOG)?;
        if end > u32::MAX as usize {
            return Err(InstrStop::MemoryLimitOOG.into());
        }
        ensure_memory(ecx, offset, len)?;
        offset as u32..end as u32
    };
    let address = ecx.message().destination;
    let left = ecx.gas.reservoir();
    let charge = ecx
        .host()
        .approve_frame(address, scope.to_u256(), left)
        .map_err(|e| ecx.interpreter_mut().fail(e))?;
    ecx.gas.spend_state(charge)?;
    ecx.interpreter_mut().set_output(output);
    Err(InstrStop::Return.into())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_txparam(
    ecx: &mut EvmContext,
    param: &mut EvmWord,
) -> BuiltinResult {
    let left = ecx.gas.reservoir();
    *param = ecx
        .host()
        .frame_tx_param(param.to_u256(), left)
        .ok_or(InstrStop::OpcodeNotFound)?
        .into();
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_framedataload(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 2],
) -> BuiltinResult {
    let rev![offset, index] = sp;
    let data = ecx.host().frame_data(index.to_u256()).ok_or(InstrStop::OpcodeNotFound)?;
    let offset = word_to_usize_saturated(offset.to_u256());
    *index = U256::from_be_bytes(read_word_padded(&data, offset).0).into();
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_frameparam(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 2],
) -> BuiltinResult {
    let rev![index, param] = sp;
    *param = ecx
        .host()
        .frame_param(index.to_u256(), param.to_u256())
        .ok_or(InstrStop::OpcodeNotFound)?
        .into();
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_sigparam(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 2],
) -> BuiltinResult {
    let rev![index, param] = sp;
    *param = ecx
        .host()
        .frame_signature_param(index.to_u256(), param.to_u256())
        .ok_or(InstrStop::OpcodeNotFound)?
        .into();
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_txtrace(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 2],
) -> BuiltinResult {
    let rev![param, index] = sp;
    *index = ecx
        .host()
        .txtrace(param.to_u256(), index.to_u256())
        .ok_or(InstrStop::OpcodeNotFound)?
        .into();
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_framedatacopy(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 4],
) -> BuiltinResult {
    let rev![memory, offset, len, index] = sp;
    let data = ecx.host().frame_data(index.to_u256()).ok_or(InstrStop::OpcodeNotFound)?;
    copy_frame_bytes(ecx, memory.to_u256(), offset.to_u256(), len.to_u256(), &data, false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_sigdatacopy(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 4],
) -> BuiltinResult {
    let rev![memory, offset, len, index] = sp;
    let data = ecx.host().frame_signature_bytes(index.to_u256()).ok_or(InstrStop::OpcodeNotFound)?;
    copy_frame_bytes(ecx, memory.to_u256(), offset.to_u256(), len.to_u256(), &data, false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __revmc_builtin_eventdatacopy(
    ecx: &mut EvmContext,
    sp: &mut [EvmWord; 4],
) -> BuiltinResult {
    let rev![index, memory, offset, len] = sp;
    let data = ecx.host().event_data(index.to_u256()).ok_or(InstrStop::OpcodeNotFound)?;
    copy_frame_bytes(ecx, memory.to_u256(), offset.to_u256(), len.to_u256(), &data, true)
}

fn copy_frame_bytes(
    ecx: &mut EvmContext,
    memory: U256,
    offset: U256,
    len: U256,
    data: &[u8],
    checked: bool,
) -> BuiltinResult {
    let len = word_to_usize(len)?;
    let offset = word_to_usize_saturated(offset);
    if checked && offset.checked_add(len).is_none_or(|end| end > data.len()) {
        return Err(InstrStop::OutOfOffset.into());
    }
    ecx.gas.spend(ecx.gas_params().copy_cost(len))?;
    if len != 0 {
        let memory = word_to_usize(memory)?;
        ensure_memory(ecx, memory, len)?;
        ecx.memory_mut().set_data(memory, offset, len, data);
    }
    Ok(())
}

}
