//! Warm ordinary execution and assertion-diff scaling in the existing EVM benchmark target.

use alloy_consensus::{TxEip8141, TxLegacy, transaction::Recovered};
use alloy_eips::eip8141::{Frame, FrameLimits, FrameMode};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use criterion::{Criterion, black_box};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{RecoveredTxEnvelope, TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
};
use std::{env, time::Duration};

const CALLER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);
const ASSERTION: Address = Address::with_last_byte(0xc0);

struct Case {
    name: String,
    spec: SpecId,
    db: InMemoryDB,
    tx: RecoveredTxEnvelope,
}

pub(crate) fn bench(c: &mut Criterion) {
    if matches!(env::var("EVM2_BENCH_SHARD").as_deref(), Ok("jit" | "jit-mainnet")) {
        return;
    }
    let mut cases = Vec::new();
    for spec in [SpecId::CANCUN, SpecId::AMSTERDAM, SpecId::BOGOTA] {
        for slots in [0, 1] {
            cases.push(ordinary(spec, slots));
        }
    }
    for slots in [0, 8, 64] {
        cases.push(frames(slots));
    }
    // Bogota's frame support must not change the ordinary transaction result.
    for slots in 0..=1 {
        let mut before = cases[2 + slots].evm();
        let mut after = cases[4 + slots].evm();
        assert_eq!(
            before.call_tx(&cases[2 + slots].tx).unwrap(),
            after.call_tx(&cases[4 + slots].tx).unwrap()
        );
    }
    let mut group = c.benchmark_group("frame");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(10);
    for case in cases {
        let mut evm = case.evm();
        let result = evm.call_tx(&case.tx).unwrap();
        assert!(result.status);
        if let Some(frame) = result.frame {
            let slots = case.db.cache.storage.get(&BODY).map_or(0, |storage| storage.slots.len());
            assert_eq!(
                frame.outputs.last().unwrap().as_ref(),
                U256::from(slots).to_be_bytes::<32>()
            );
        }
        #[cfg(evm2_bench_allocations)]
        allocation::measure(&case, &mut evm);
        group.bench_function(&case.name, |b| {
            b.iter(|| black_box(evm.call_tx(black_box(&case.tx)).unwrap()));
        });
    }
    group.finish();
}

impl Case {
    fn evm(&self) -> Evm<'static, BaseEvmTypes> {
        Evm::new(
            self.spec,
            BlockEnvExt::default(),
            ethereum_tx_registry(self.spec),
            self.db.clone(),
            Precompiles::base(self.spec),
        )
    }
}

fn database(slots: u8, frame: bool) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    let caller_code =
        if frame { Bytes::from_static(&[0x60, 3, 0x5f, 0x5f, 0xaa]) } else { Bytes::new() };
    db.insert_account_info(
        &CALLER,
        AccountInfo::default()
            .with_balance(U256::from(1_000_000_000_u64))
            .with_code(Bytecode::new_legacy(caller_code)),
    );
    let mut code = Vec::new();
    for slot in 0..slots {
        code.extend_from_slice(&[0x60, 2, 0x60, slot, 0x55]);
        db.insert_account_storage(&BODY, &U256::from(slot), &U256::ONE);
    }
    code.push(0x00);
    db.insert_account_info(
        &BODY,
        AccountInfo::default().with_balance(U256::ONE).with_code(Bytecode::new_legacy(code.into())),
    );
    db.insert_account_info(
        &ASSERTION,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[
            0x5f, 0x60, 1, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3,
        ]))),
    );
    db
}

fn ordinary(spec: SpecId, slots: u8) -> Case {
    Case {
        name: format!("ordinary_{spec:?}_slots_{slots}"),
        spec,
        db: database(slots, false),
        tx: Recovered::new_unchecked(
            TxEnvelope::Legacy(TxLegacy {
                to: TxKind::Call(BODY),
                gas_limit: 1_000_000,
                value: U256::ONE,
                ..Default::default()
            }),
            CALLER,
        ),
    }
}

fn frames(slots: u8) -> Case {
    Case {
        name: format!("assertion_diff_slots_{slots}"),
        spec: SpecId::BOGOTA,
        db: database(slots, true),
        tx: Recovered::new_unchecked(
            TxEnvelope::Eip8141(TxEip8141 {
                chain_id: 1,
                sender: CALLER,
                nonce_keys: Some(vec![]),
                frames: [
                    (FrameMode::Verify, CALLER, 3),
                    (FrameMode::Sender, BODY, 0),
                    (FrameMode::Assert, ASSERTION, 0),
                ]
                .into_iter()
                .map(|(mode, target, flags)| Frame {
                    mode,
                    target: target.into(),
                    flags,
                    limits: FrameLimits { execution: 1_000_000, state: 100_000 },
                    ..Default::default()
                })
                .collect(),
                ..Default::default()
            }),
            Address::ZERO,
        ),
    }
}

// Counting is opt-in so normal timing benchmarks keep the ordinary system allocator.
// RUSTFLAGS='--cfg evm2_bench_allocations' EVM2_BENCH_SHARD=frame cargo bench --profile dev
//   -p evm2-cli --bench evm -- --test
#[cfg(evm2_bench_allocations)]
mod allocation {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    #[derive(Clone, Copy, Default)]
    struct Counts {
        calls: usize,
        bytes: usize,
    }

    thread_local! {
        static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
    }

    struct Allocator;

    #[global_allocator]
    static ALLOCATOR: Allocator = Allocator;

    fn record(bytes: usize) {
        let _ = COUNTS.try_with(|counter| {
            if let Some(mut counts) = counter.get() {
                counts.calls += 1;
                counts.bytes += bytes;
                counter.set(Some(counts));
            }
        });
    }

    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let result = unsafe { System.alloc(layout) };
            if !result.is_null() {
                record(layout.size());
            }
            result
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let result = unsafe { System.alloc_zeroed(layout) };
            if !result.is_null() {
                record(layout.size());
            }
            result
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let result = unsafe { System.realloc(pointer, layout, size) };
            if !result.is_null() {
                record(size);
            }
            result
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }
    }

    pub(super) fn measure(case: &Case, evm: &mut Evm<'_, BaseEvmTypes>) {
        const ITERATIONS: usize = 1_000;
        for _ in 0..10 {
            black_box(evm.call_tx(&case.tx).unwrap());
        }
        COUNTS.with(|counts| counts.set(Some(Counts::default())));
        for _ in 0..ITERATIONS {
            black_box(evm.call_tx(&case.tx).unwrap());
        }
        let counts = COUNTS.with(|counts| counts.replace(None).unwrap());
        println!(
            "allocations {}: {} calls/tx, {} requested bytes/tx",
            case.name,
            counts.calls as f64 / ITERATIONS as f64,
            counts.bytes as f64 / ITERATIONS as f64
        );
    }
}
