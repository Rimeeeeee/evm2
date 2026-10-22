//! Transaction-owner release and safe interpreter reuse after errors and unwinding.

use alloy_consensus::{TxEip8141, TxLegacy, transaction::Recovered};
use alloy_eips::eip8141::{Frame, FrameLimits, FrameMode};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use core::ops::Deref;
use evm2::{
    BaseEvmTypes, Evm, Inspector, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
    interpreter::Interpreter,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);

#[derive(Debug)]
struct OwnedPayload {
    bytes: Vec<u8>,
    _alive: Arc<()>,
}

impl AsRef<[u8]> for OwnedPayload {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn tracked_data() -> (Bytes, Weak<()>) {
    type InnerBytes = <Bytes as Deref>::Target;
    let alive = Arc::new(());
    let weak = Arc::downgrade(&alive);
    let data =
        Bytes(InnerBytes::from_owner(OwnedPayload { bytes: vec![42; 16_384], _alive: alive }));
    (data, weak)
}

fn transaction(data: Bytes, allow_approval: bool) -> Recovered<TxEnvelope> {
    Recovered::new_unchecked(
        TxEnvelope::Eip8141(TxEip8141 {
            chain_id: 1,
            sender: SENDER,
            nonce_keys: Some(vec![U256::ZERO]),
            frames: [
                (FrameMode::Verify, SENDER, if allow_approval { 3 } else { 0 }, Bytes::new()),
                (FrameMode::Sender, BODY, 0, data),
            ]
            .into_iter()
            .map(|(mode, target, flags, data)| Frame {
                mode,
                target: target.into(),
                flags,
                data,
                limits: FrameLimits { execution: 100_000, state: 300_000 },
                ..Default::default()
            })
            .collect(),
            ..Default::default()
        }),
        Address::ZERO,
    )
}

fn evm() -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    for (address, code) in [
        (SENDER, &[0x60, 3, 0x5f, 0x5f, 0xaa][..]),
        (BODY, &[0x60, 42, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x00][..]),
    ] {
        db.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    Evm::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        Precompiles::base(SpecId::BOGOTA),
    )
}

#[test]
fn repeated_frame_lifecycles_release_payload_owners_and_bound_cache_entries() {
    let mut evm = evm();
    let valid = transaction(Bytes::new(), true);
    assert!(evm.call_tx(&valid).unwrap().status);
    let expected_cache_sizes = (
        evm.overlay_db().cache.accounts.len(),
        evm.overlay_db().cache.contracts.len(),
        evm.overlay_db().cache.storage.len(),
    );
    for iteration in 0..112 {
        let (data, weak) = tracked_data();
        let tx = transaction(data, iteration % 7 != 6);
        match iteration % 7 {
            0 => {
                evm.validate_frame_prefix(&tx, 1).unwrap();
            }
            1 => {
                assert!(evm.validate_frame_prefix(&tx, 2).is_err());
            }
            2 => {
                drop(evm.transact(&tx).unwrap());
            }
            3 => {
                assert!(evm.transact(&tx).unwrap().discard().status);
            }
            4 => {
                drop(evm.transact(&tx).unwrap().detach());
            }
            5 => {
                assert!(evm.call_tx(&tx).unwrap().status);
            }
            6 => {
                assert!(evm.transact(&tx).is_err());
            }
            _ => unreachable!(),
        }
        drop(tx);
        assert!(weak.upgrade().is_none(), "payload retained after iteration {iteration}");
        assert!(evm.logs().is_empty());
        // The accepted overlay deliberately caches reads, but repeating the same state must not
        // create a new cache entry, journal, or payload owner for every simulated transaction.
        assert_eq!(
            (
                evm.overlay_db().cache.accounts.len(),
                evm.overlay_db().cache.contracts.len(),
                evm.overlay_db().cache.storage.len(),
            ),
            expected_cache_sizes
        );
        assert!(evm.call_tx(&valid).unwrap().status);
    }
}

#[derive(Clone, Copy)]
enum PanicHook {
    FrameStart,
    Step,
}

struct PanicInBody(PanicHook);

impl Inspector<BaseEvmTypes> for PanicInBody {
    fn frame_start(&mut self, index: usize, _frame: &Frame, _host: &mut Evm<'_, BaseEvmTypes>) {
        if matches!(self.0, PanicHook::FrameStart) && index == 1 {
            panic!("frame entry interrupted");
        }
    }

    fn step(&mut self, interp: &mut Interpreter<'_, '_, BaseEvmTypes>) {
        if matches!(self.0, PanicHook::Step) && interp.message().destination == BODY {
            panic!("frame interpreter interrupted");
        }
    }
}

#[test]
fn frame_unwind_releases_payload_owners_before_reusing_the_evm() {
    let mut evm = evm();
    for iteration in 0..32 {
        let (data, weak) = tracked_data();
        let tx = transaction(data, true);
        let hook = if iteration % 2 == 0 { PanicHook::FrameStart } else { PanicHook::Step };
        evm.set_inspector(PanicInBody(hook));
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _ = evm.call_tx(&tx);
            }))
            .is_err()
        );
        let _ = evm.clear_inspector();
        drop(tx);
        assert!(weak.upgrade().is_none(), "payload retained after unwind {iteration}");
        assert!(evm.logs().is_empty());
        assert!(evm.call_tx(&transaction(Bytes::new(), true)).unwrap().status);
    }
}

struct PanicOnce {
    panic: bool,
    steps: Arc<AtomicUsize>,
}

impl Inspector<BaseEvmTypes> for PanicOnce {
    fn step(&mut self, _interp: &mut Interpreter<'_, '_, BaseEvmTypes>) {
        self.steps.fetch_add(1, Ordering::Relaxed);
        if core::mem::take(&mut self.panic) {
            panic!("ordinary interpreter interrupted");
        }
    }
}

#[test]
fn ordinary_inspector_unwind_restores_the_frame_pointer_and_discards_scratch() {
    let mut evm = evm();
    let caller = Address::with_last_byte(0xe0);
    evm.overlay_db_mut().insert_account_info(
        &caller,
        AccountInfo::default().with_balance(U256::from(1_000_000_000)),
    );
    let steps = Arc::new(AtomicUsize::new(0));
    evm.set_inspector(PanicOnce { panic: true, steps: steps.clone() });
    let tx = Recovered::new_unchecked(
        TxEnvelope::Legacy(TxLegacy {
            gas_limit: 500_000,
            to: TxKind::Call(BODY),
            ..Default::default()
        }),
        caller,
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = evm.call_tx(&tx);
        }))
        .is_err()
    );
    assert_eq!(steps.load(Ordering::Relaxed), 1);
    assert!(evm.logs().is_empty());
    // The same inspector now runs normally. A dangling current_frame pointer would make the next
    // inspected entry access a freed interpreter; stale scratch would also consume nonce zero.
    assert!(evm.call_tx(&tx).unwrap().status);
    assert!(steps.load(Ordering::Relaxed) > 1);
    assert!(evm.call_tx(&transaction(Bytes::new(), true)).unwrap().status);
}
