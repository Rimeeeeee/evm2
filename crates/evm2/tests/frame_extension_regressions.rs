//! Frame execution through custom EVM families, precompiles, and inspector overrides.

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_eips::eip8141::{ENTRY_POINT, Frame, FrameMode, FrameStatus, NONCE_MANAGER};
use alloy_primitives::{Address, Bytes, Log, TxKind, U256};
use common::{APPROVE_BOTH, BODY, SENDER};
use evm2::{
    BaseEvmConfigSelector, Evm, EvmTypesHost, Inspector, PrecompileError, PrecompileHalt, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::AccountInfo,
    interpreter::{
        GasTracker, Host, InstrStop, Interpreter, Message, MessageKind, MessageResult,
        MessageResultExt,
    },
    precompile::{PrecompileOutput, PrecompileProvider},
    precompiles::{MovePrecompileError, PrecompileId},
    registry::HandlerError,
};
use std::borrow::Cow;

const CUSTOM: Address = Address::with_last_byte(0xf0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Marker(u8);

impl Default for Marker {
    fn default() -> Self {
        Self(11)
    }
}

#[derive(Default)]
struct Extension {
    frames: Vec<(usize, FrameMode)>,
    calls: Vec<(u16, Address, Address)>,
    ends: Vec<(u16, InstrStop)>,
    precompiles: Vec<(u16, bool)>,
    logs: usize,
    initialized: usize,
    completed: usize,
}

struct CustomTypes;

impl EvmTypesHost for CustomTypes {
    type ConfigSelector = BaseEvmConfigSelector;
    type SpecId = SpecId;
    type Tx = TxEnvelope;
    type EvmExt = Extension;
    type MessageExt = Marker;
    type MessageResultExt = Marker;
    type TxEnvExt = Marker;
    type TxResultExt = Marker;
    type BlockEnvExt = Marker;
    type Host<'a> = Evm<'a, Self>;
}

struct CustomPrecompiles;

impl PrecompileProvider<CustomTypes> for CustomPrecompiles {
    fn addresses(&self) -> Vec<Address> {
        vec![CUSTOM]
    }

    fn precompile_ids(&self) -> Vec<(Address, PrecompileId)> {
        vec![(CUSTOM, PrecompileId::Custom(Cow::Borrowed("frame-extension-test")))]
    }

    fn contains(&self, address: &Address) -> bool {
        *address == CUSTOM
    }

    fn move_precompiles(
        &mut self,
        moves: &[(Address, Address)],
    ) -> Result<(), MovePrecompileError> {
        match moves.iter().find(|(source, destination)| source != destination) {
            Some((source, _)) => Err(MovePrecompileError::NotAPrecompile(*source)),
            None => Ok(()),
        }
    }

    fn execute(
        &mut self,
        evm: &mut Evm<'_, CustomTypes>,
        message: &Message<CustomTypes>,
        gas: &mut GasTracker,
    ) -> Option<Result<PrecompileOutput, PrecompileError>> {
        if !self.contains(&message.call_target) {
            return None;
        }
        Some((|| {
            assert_eq!(message.ext, Marker(77));
            assert_eq!(evm.block().ext, Marker(42));
            assert_eq!(gas.state_gas_isolated(), evm.frame_transaction_active());
            let is_static = message.kind == MessageKind::StaticCall || message.caller_is_static;
            evm.ext_mut().precompiles.push((message.depth, is_static));
            gas.spend(37)?;
            if !is_static {
                gas.spend_state(113)?;
                evm.state_mut().storage(&CUSTOM)?.into_slot(U256::ZERO)?.set(U256::from(99));
                evm.log(Log::new_unchecked(CUSTOM, vec![], Bytes::from_static(b"custom")));
            }
            match message.input.first().copied().unwrap_or_default() {
                0 => Ok(PrecompileOutput::new(Bytes::from_static(b"ok"))),
                1 => Err(PrecompileError::Revert(Bytes::from_static(b"reverted"))),
                2 => Err(PrecompileHalt::Other(Cow::Borrowed("test halt")).into()),
                3 => Err(PrecompileError::fatal(std::io::Error::other("custom precompile failed"))),
                _ => unreachable!(),
            }
        })())
    }
}

#[derive(Default)]
struct Observe {
    override_before: bool,
    override_after: Option<InstrStop>,
}

impl Inspector<CustomTypes> for Observe {
    fn initialize_interp(&mut self, interp: &mut Interpreter<'_, '_, CustomTypes>) {
        assert_eq!(interp.message().ext, Marker(77));
        assert_eq!(interp.tx_env().ext, Marker(11));
        assert_eq!(interp.host().block().ext, Marker(42));
        interp.host().ext_mut().initialized += 1;
    }

    fn call(
        &mut self,
        interp: &mut Interpreter<'_, '_, CustomTypes>,
        message: &mut Message<CustomTypes>,
    ) -> Option<MessageResult<CustomTypes>> {
        assert_eq!(message.ext, Marker(11));
        message.ext = Marker(77);
        interp.host().ext_mut().calls.push((message.depth, message.call_target, message.caller));
        (self.override_before && message.depth == 1).then(|| {
            let mut gas = GasTracker::new_isolated(message.gas_limit, message.reservoir);
            gas.spend(37).unwrap();
            MessageResultExt {
                stop: InstrStop::Return,
                gas,
                output: Bytes::from_static(b"override"),
                ..Default::default()
            }
        })
    }

    fn call_end(
        &mut self,
        interp: &mut Interpreter<'_, '_, CustomTypes>,
        message: &Message<CustomTypes>,
        result: &mut MessageResult<CustomTypes>,
    ) {
        assert_eq!(message.ext, Marker(77));
        assert_eq!(result.ext, Marker(11));
        if message.depth == 1
            && let Some(stop) = self.override_after
        {
            result.stop = stop;
        }
        result.ext = Marker(88);
        interp.host().ext_mut().ends.push((message.depth, result.stop));
    }

    fn log(&mut self, _: &Log, host: &mut Evm<'_, CustomTypes>) {
        host.ext_mut().logs += 1;
    }

    fn frame_start(&mut self, index: usize, frame: &Frame, host: &mut Evm<'_, CustomTypes>) {
        host.ext_mut().frames.push((index, frame.mode));
    }

    fn frame_transaction_end(
        &mut self,
        result: &evm2::TxResult<CustomTypes>,
        host: &mut Evm<'_, CustomTypes>,
    ) {
        assert_eq!(result.ext, Marker(11));
        assert_eq!(result.frame.as_ref().unwrap().payer, SENDER);
        host.ext_mut().completed += 1;
    }
}

fn frame(mode: FrameMode, target: Address, selector: u8) -> Frame {
    Frame { data: Bytes::copy_from_slice(&[selector]), ..common::frame(mode, target) }
}

fn tx(frames: Vec<Frame>) -> Recovered<TxEnvelope> {
    common::recovered(common::payload(frames))
}

fn evm(body: &[u8], inspector: Observe) -> Evm<'static, CustomTypes> {
    let mut db = common::database([(SENDER, APPROVE_BOTH), (BODY, body), (CUSTOM, &[])]);
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    let mut evm = Evm::new_with_ext(
        SpecId::BOGOTA,
        BlockEnvExt::<()>::default().with_ext(Marker(42)),
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        CustomPrecompiles,
        Extension::default(),
    );
    evm.set_inspector(inspector);
    evm
}

fn stored(evm: &mut Evm<'_, CustomTypes>) -> U256 {
    evm.state_mut().storage(&CUSTOM).unwrap().into_slot(U256::ZERO).unwrap().current()
}

#[test]
fn custom_type_family_preserves_extensions_precompile_context_and_frame_hooks() {
    let mut evm = evm(&[], Observe::default());
    let tx = tx(vec![
        frame(FrameMode::Verify, SENDER, 0),
        frame(FrameMode::Default, CUSTOM, 0),
        frame(FrameMode::Sender, CUSTOM, 0),
        frame(FrameMode::Assert, CUSTOM, 0),
    ]);
    let result = evm.transact(&tx).unwrap().commit();
    assert!(result.status);
    assert_eq!(result.ext, Marker(11));
    assert_eq!(result.state_gas_spent, 226);
    let outcome = result.frame.unwrap();
    assert_eq!(
        outcome.receipts.iter().map(|receipt| receipt.gas_used.state).collect::<Vec<_>>(),
        [0, 113, 113, 0]
    );
    assert_eq!(
        outcome.receipts.iter().map(|receipt| receipt.logs.len()).collect::<Vec<_>>(),
        [0, 1, 1, 0]
    );
    assert_eq!(outcome.outputs[1..], vec![Bytes::from_static(b"ok"); 3]);
    assert_eq!(stored(&mut evm), U256::from(99));
    assert_eq!(
        evm.ext().calls,
        [
            (0, SENDER, ENTRY_POINT),
            (0, CUSTOM, ENTRY_POINT),
            (0, CUSTOM, SENDER),
            (0, CUSTOM, ENTRY_POINT)
        ]
    );
    assert_eq!(evm.ext().precompiles, [(0, false), (0, false), (0, true)]);
    assert_eq!(evm.ext().initialized, 1);
    assert_eq!(evm.ext().logs, 2);
    assert_eq!(evm.ext().ends.len(), 4);
    assert_eq!(
        evm.ext().frames,
        [
            (0, FrameMode::Verify),
            (1, FrameMode::Default),
            (2, FrameMode::Sender),
            (3, FrameMode::Assert)
        ]
    );
    assert_eq!(evm.ext().completed, 1);
}

#[test]
fn custom_precompile_revert_halt_and_fatal_paths_rollback_and_allow_reuse() {
    for selector in 0..4 {
        let mut evm = evm(&[], Observe::default());
        let failed = tx(vec![
            frame(FrameMode::Verify, SENDER, 0),
            frame(FrameMode::Sender, CUSTOM, selector),
        ]);
        if selector == 3 {
            assert!(matches!(evm.transact(&failed), Err(HandlerError::Fatal(_))));
            assert_eq!(stored(&mut evm), U256::ZERO);
            assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
            assert!(evm.logs().is_empty());
            assert_eq!(evm.ext().completed, 0);
            let valid =
                tx(vec![frame(FrameMode::Verify, SENDER, 0), frame(FrameMode::Sender, CUSTOM, 0)]);
            assert!(evm.transact(&valid).unwrap().commit().status);
            assert_eq!(stored(&mut evm), U256::from(99));
            continue;
        }
        let result = evm.transact(&failed).unwrap().commit();
        let receipt = &result.frame.as_ref().unwrap().receipts[1];
        assert_eq!(
            receipt.status,
            if selector == 0 { FrameStatus::Success } else { FrameStatus::Failure }
        );
        assert_eq!(receipt.gas_used.state, if selector == 0 { 113 } else { 0 });
        assert_eq!(receipt.logs.len(), usize::from(selector == 0));
        assert_eq!(stored(&mut evm), if selector == 0 { U256::from(99) } else { U256::ZERO });
        assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
        let frame = result.frame.unwrap();
        assert_eq!(
            frame.stops[1],
            match selector {
                0 => InstrStop::Return,
                1 => InstrStop::Revert,
                _ => InstrStop::PrecompileError,
            }
        );
        assert_eq!(
            frame.outputs[1],
            match selector {
                0 => Bytes::from_static(b"ok"),
                1 => Bytes::from_static(b"reverted"),
                _ => Bytes::new(),
            }
        );
    }
}

#[test]
fn nested_inspector_overrides_settle_state_gas_and_rollback_custom_precompile_effects() {
    // CALL custom with empty data and zero value, ignore the success bit, then STOP.
    let body = [0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x60, 0xf0, 0x61, 0x27, 0x10, 0xf1, 0x00];
    let tx = tx(vec![frame(FrameMode::Verify, SENDER, 0), frame(FrameMode::Sender, BODY, 0)]);
    let mut execution_gas = None;
    for (before, after) in [
        (false, None),
        (true, None),
        (false, Some(InstrStop::Revert)),
        (false, Some(InstrStop::PrecompileError)),
    ] {
        let mut evm = evm(&body, Observe { override_before: before, override_after: after });
        let result = evm.transact(&tx).unwrap().commit();
        assert!(result.status);
        let receipt = &result.frame.as_ref().unwrap().receipts[1];
        assert_eq!(receipt.status, FrameStatus::Success);
        assert_eq!(receipt.gas_used.state, if before || after.is_some() { 0 } else { 113 });
        assert_eq!(receipt.logs.len(), usize::from(!before && after.is_none()));
        assert_eq!(
            stored(&mut evm),
            if before || after.is_some() { U256::ZERO } else { U256::from(99) }
        );
        assert_eq!(
            evm.ext().calls,
            [(0, SENDER, ENTRY_POINT), (0, BODY, SENDER), (1, CUSTOM, BODY)]
        );
        assert_eq!(evm.ext().ends.iter().map(|&(depth, _)| depth).collect::<Vec<_>>(), [0, 1, 0]);
        assert_eq!(evm.ext().precompiles.len(), usize::from(!before));
        assert_eq!(evm.ext().logs, usize::from(!before));
        if after.is_some_and(|stop| stop.is_halt()) {
            assert_eq!(receipt.gas_used.execution, execution_gas.unwrap() + 10_000 - 37);
        } else {
            assert_eq!(
                *execution_gas.get_or_insert(receipt.gas_used.execution),
                receipt.gas_used.execution
            );
        }
    }
}

#[test]
fn ordinary_nested_call_end_overrides_rollback_state_logs_and_state_gas_too() {
    let body = [0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x60, 0xf0, 0x61, 0x27, 0x10, 0xf1, 0x00];
    let caller = Address::with_last_byte(0xd0);
    let tx = Recovered::new_unchecked(
        TxEnvelope::Legacy(TxLegacy {
            gas_limit: 200_000,
            to: TxKind::Call(BODY),
            ..Default::default()
        }),
        caller,
    );
    let mut execution_gas = None;
    for stop in [None, Some(InstrStop::Revert), Some(InstrStop::PrecompileError)] {
        let mut evm = evm(&body, Observe { override_before: false, override_after: stop });
        evm.overlay_db_mut().insert_account_info(
            &caller,
            AccountInfo::default().with_balance(U256::from(1_000_000_000)),
        );
        let result = evm.transact(&tx).unwrap().commit();
        assert!(result.status);
        assert!(result.frame.is_none());
        assert_eq!(result.state_gas_spent, if stop.is_some() { 0 } else { 113 });
        assert_eq!(result.logs.len(), usize::from(stop.is_none()));
        assert_eq!(stored(&mut evm), if stop.is_some() { U256::ZERO } else { U256::from(99) });
        assert_eq!(evm.state_mut().account(&caller).unwrap().nonce(), 1);
        assert_eq!(evm.ext().calls, [(0, BODY, caller), (1, CUSTOM, BODY)]);
        assert_eq!(evm.ext().ends.iter().map(|&(depth, _)| depth).collect::<Vec<_>>(), [1, 0]);
        assert_eq!(evm.ext().logs, 1);
        if stop.is_some_and(|stop| stop.is_halt()) {
            assert_eq!(result.execution_gas_spent(), execution_gas.unwrap() + 10_000 - 37);
        } else {
            assert_eq!(
                *execution_gas.get_or_insert(result.execution_gas_spent()),
                result.execution_gas_spent()
            );
        }
    }
}

#[test]
fn reverted_nested_approval_cannot_authorize_the_next_sender_frame() {
    // Empty calldata approves. Nonempty calldata calls self with empty calldata, observes a
    // successful nested approval, and then reverts its enclosing default-mode frame.
    let sender = [
        0x36, 0x15, 0x60, 0x13, 0x57, 0x5f, 0x5f, 0x5f, 0x5f, 0x60, 0xa0, 0x61, 0x27, 0x10, 0xfa,
        0x50, 0x5f, 0x5f, 0xfd, 0x5b, 0x60, 3, 0x5f, 0x5f, 0xaa,
    ];
    for with_fresh_approval in [false, true] {
        let mut evm = evm(&[0x00], Observe::default());
        evm.overlay_db_mut().insert_account_info(
            &SENDER,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(&sender))),
        );
        let mut outer = frame(FrameMode::Default, SENDER, 1);
        outer.flags = 3;
        let mut frames = vec![outer];
        if with_fresh_approval {
            let mut fresh = frame(FrameMode::Verify, SENDER, 0);
            fresh.data = Bytes::new();
            frames.push(fresh);
        }
        frames.push(frame(FrameMode::Sender, BODY, 0));
        if with_fresh_approval {
            let result = evm.transact(&tx(frames)).unwrap().commit();
            let outcome = result.frame.unwrap();
            assert_eq!(
                outcome.receipts.iter().map(|receipt| receipt.status).collect::<Vec<_>>(),
                [FrameStatus::Failure, FrameStatus::Success, FrameStatus::Success]
            );
            assert_eq!(outcome.payer, SENDER);
            assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
            assert_eq!(evm.ext().calls.len(), 4);
            assert_eq!(evm.ext().ends.len(), 4);
        } else {
            assert_eq!(
                evm.transact(&tx(frames)).unwrap_err(),
                HandlerError::InvalidFrameTransaction(
                    "EIP-8141 SENDER frame executed before execution approval"
                )
            );
            assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
            assert_eq!(evm.ext().calls.len(), 2);
            assert_eq!(evm.ext().ends.len(), 2);
        }
    }
}

#[test]
fn setcodefrom_recent_root_address_is_an_ordinary_contract() {
    let recent_root = alloy_eips::eip8272::RECENT_ROOT_ADDRESS;
    let mut code = vec![0x73];
    code.extend_from_slice(BODY.as_slice());
    code.extend_from_slice(&[0xa7, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3]);
    let db = common::database([
        (SENDER, APPROVE_BOTH),
        (BODY, &[0x00][..]),
        (recent_root, code.as_slice()),
    ]);
    let mut evm = common::evm(db);
    let tx = common::recovered(common::payload(vec![
        common::frame(FrameMode::Verify, SENDER),
        common::frame(FrameMode::Sender, recent_root),
    ]));
    let result = evm.call_tx(&tx).unwrap();
    let frame = result.frame.unwrap();
    assert_eq!(frame.stops[1], InstrStop::Return);
    assert_eq!(frame.receipts[1].status, FrameStatus::Success);
    assert_eq!(U256::from_be_slice(&frame.outputs[1]), U256::ONE);
}
