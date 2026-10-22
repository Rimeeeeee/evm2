//! Differential checks against the pinned Bogota revm implementation.

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{
    Frame, FrameLimits, FrameMode, FrameReceipt, NONCE_MANAGER, nonce_calldata,
};
use alloy_primitives::{Address, B256, Bytes, Log, U256};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
};
use revm::{
    Context, ExecuteEvm, MainBuilder, MainContext,
    context::{TxEnv, result::ExecutionResult},
    context_interface::transaction::FrameTransaction,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId as RevmSpec,
};
use revm_bogota as revm;
use std::collections::BTreeMap;

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);
const OTHER: Address = Address::with_last_byte(0xc0);

#[derive(Clone)]
struct Case {
    name: String,
    tx: TxEip8141,
    body: Bytes,
    other: Bytes,
    sender_code: Bytes,
    storage: Vec<(Address, U256, U256)>,
    basefee: u64,
    expected_valid: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Account {
    balance: U256,
    nonce: u64,
    code_hash: B256,
    storage: BTreeMap<U256, U256>,
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    success: bool,
    spent: u64,
    state_gas: u64,
    refunded: u64,
    floor: u64,
    fee_gas: u64,
    block_execution: u64,
    payer: Address,
    receipts: Vec<FrameReceipt>,
    outputs: Vec<Bytes>,
    logs: Vec<Log>,
    accounts: BTreeMap<Address, Account>,
}

fn frame(mode: FrameMode, target: Address) -> Frame {
    Frame {
        mode,
        flags: if mode == FrameMode::Verify { 3 } else { 0 },
        target: target.into(),
        limits: FrameLimits { execution: 100_000, state: 300_000 },
        ..Default::default()
    }
}

fn base_case(name: &str, body: &[u8], other: &[u8], suffix: Vec<Frame>) -> Case {
    let mut frames = vec![frame(FrameMode::Verify, SENDER)];
    frames.extend(suffix);
    Case {
        name: name.into(),
        tx: TxEip8141 {
            chain_id: 1,
            sender: SENDER,
            nonce_keys: Some(vec![U256::ZERO]),
            frames,
            ..Default::default()
        },
        body: Bytes::copy_from_slice(body),
        other: Bytes::copy_from_slice(other),
        sender_code: Bytes::from_static(&[0x60, 3, 0x5f, 0x5f, 0xaa]),
        storage: Vec::new(),
        basefee: 0,
        expected_valid: true,
    }
}

fn initial(case: &Case) -> Vec<(Address, Bytes)> {
    vec![(SENDER, case.sender_code.clone()), (BODY, case.body.clone()), (OTHER, case.other.clone())]
}

fn prestate(case: &Case) -> BTreeMap<Address, Account> {
    let mut accounts = initial(case)
        .into_iter()
        .map(|(address, code)| {
            (
                address,
                Account {
                    balance: U256::from(1_000_000_000),
                    code_hash: alloy_primitives::keccak256(code),
                    ..Default::default()
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    accounts.insert(
        NONCE_MANAGER,
        Account { nonce: 1, code_hash: alloy_primitives::KECCAK256_EMPTY, ..Default::default() },
    );
    for &(address, key, value) in &case.storage {
        accounts.get_mut(&address).unwrap().storage.insert(key, value);
    }
    accounts
}

fn native(case: &Case) -> Result<Outcome, String> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    for (address, code) in initial(case) {
        db.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(code)),
        );
    }
    for &(address, key, value) in &case.storage {
        db.insert_account_storage(&address, &key, &value);
    }
    let mut evm = Evm::<BaseEvmTypes>::new(
        SpecId::BOGOTA,
        BlockEnvExt { basefee: U256::from(case.basefee), ..Default::default() },
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        Precompiles::base(SpecId::BOGOTA),
    );
    let tx = Recovered::new_unchecked(TxEnvelope::Eip8141(case.tx.clone()), Address::ZERO);
    let executed = evm.transact(&tx).map_err(|error| format!("{error:?}"))?;
    let mut accounts = prestate(case);
    for change in executed.account_changes() {
        if let Some(info) = change.current.filter(|info| !info.is_empty()) {
            let entry = accounts.entry(change.address).or_default();
            entry.balance = info.balance;
            entry.nonce = info.nonce;
            entry.code_hash = info.code_hash;
            for (key, slot) in &change.storage.slots {
                if slot.value.current.is_zero() {
                    entry.storage.remove(key);
                } else {
                    entry.storage.insert(*key, slot.value.current);
                }
            }
        } else {
            accounts.remove(&change.address);
        }
    }
    let result = executed.discard();
    let frame = result.frame.as_ref().unwrap();
    Ok(Outcome {
        success: result.status,
        spent: result.total_gas_spent,
        state_gas: result.state_gas_spent,
        refunded: result.refunded,
        floor: result.floor_gas,
        fee_gas: result.tx_gas_used(),
        block_execution: result.execution_gas_spent(),
        payer: frame.payer,
        receipts: frame.receipts.clone(),
        outputs: frame.outputs.clone(),
        logs: result.logs,
        accounts,
    })
}

fn oracle(case: &Case) -> Result<Outcome, String> {
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(NONCE_MANAGER, revm::state::AccountInfo::default().with_nonce(1));
    for (address, code) in initial(case) {
        db.insert_account_info(
            address,
            revm::state::AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(revm::bytecode::Bytecode::new_legacy(code)),
        );
    }
    for &(address, key, value) in &case.storage {
        db.insert_account_storage(address, key, value).unwrap();
    }
    let p = &case.tx;
    let frame_transaction = FrameTransaction {
        nonce_keys: p.nonce_keys.clone(),
        nonce_calldata: p
            .nonce_keys
            .as_deref()
            .map(|keys| nonce_calldata(keys, p.nonce))
            .unwrap_or_default(),
        frames: p.frames.clone(),
        signatures: p.signatures.clone(),
        signature_hash: p.signature_hash(),
        max_fee_per_gas: p.fees.max_fee_per_gas,
        max_priority_fee_per_gas: p.fees.max_priority_fee_per_gas,
        max_fee_per_blob_gas: p.fees.max_fee_per_blob_gas,
    };
    let tx = TxEnv::builder()
        .tx_type(Some(6))
        .caller(p.sender)
        .nonce(p.nonce)
        .kind(revm::primitives::TxKind::Call(p.sender))
        .gas_limit(frame_transaction.gas_limit(p.sender).unwrap())
        .gas_priority_fee(Some(0))
        .frame_transaction(frame_transaction)
        .build()
        .unwrap();
    let mut evm = Context::mainnet()
        .modify_cfg_chained(|cfg| {
            cfg.set_spec_and_mainnet_gas_params(RevmSpec::BOGOTA);
            cfg.enable_eip8250 = true;
        })
        .modify_block_chained(|block| block.basefee = case.basefee)
        .with_db(db)
        .build_mainnet();
    let result = evm.transact(tx).map_err(|error| format!("{error:?}"))?;
    let mut accounts = prestate(case);
    for (address, account) in result.state {
        if account.is_selfdestructed() || account.info.is_empty() {
            accounts.remove(&address);
            continue;
        }
        let entry = accounts.entry(address).or_default();
        entry.balance = account.info.balance;
        entry.nonce = account.info.nonce;
        entry.code_hash = account.info.code_hash;
        for (key, slot) in account.storage {
            if slot.present_value.is_zero() {
                entry.storage.remove(&key);
            } else {
                entry.storage.insert(key, slot.present_value);
            }
        }
    }
    let ExecutionResult::FrameTransaction {
        success,
        gas,
        payer,
        logs,
        frame_receipts,
        frame_outputs,
    } = result.result
    else {
        panic!("oracle returned ordinary result");
    };
    Ok(Outcome {
        success,
        spent: gas.total_gas_spent(),
        state_gas: gas.state_gas_spent_final(),
        refunded: gas.inner_refunded(),
        floor: gas.floor_gas(),
        fee_gas: gas.frame_tx_gas_used(),
        block_execution: gas.frame_block_regular_gas_used(),
        payer,
        receipts: frame_receipts,
        outputs: frame_outputs,
        logs,
        accounts,
    })
}

fn cases() -> Vec<Case> {
    let mut cases = vec![
        base_case("payment", &[], &[], vec![]),
        base_case(
            "sstore",
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[],
            vec![frame(FrameMode::Sender, BODY)],
        ),
        base_case(
            "assertion_revert",
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[0x5f, 0x5f, 0xfd],
            vec![
                frame(FrameMode::Sender, BODY),
                frame(FrameMode::PostTx, OTHER),
                frame(FrameMode::PostTx, OTHER),
            ],
        ),
        base_case(
            "assertion_trace",
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[0x5f, 0x60, 1, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3],
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::PostTx, OTHER)],
        ),
    ];
    for keys in [vec![], vec![0], vec![1], vec![1, 2], vec![0, 1]] {
        let mut case = base_case(&format!("nonce_keys_{keys:?}"), &[], &[], vec![]);
        case.expected_valid = !keys.is_empty() && (keys.len() == 1 || !keys.contains(&0));
        case.tx.nonce_keys = Some(keys.into_iter().map(U256::from).collect());
        cases.push(case);
    }
    for state in [0, 1, 97_919, 97_920, 300_000] {
        let mut f = frame(FrameMode::Sender, BODY);
        f.limits.state = state;
        cases.push(base_case(
            &format!("sstore_state_{state}"),
            &[0x60, 1, 0x5f, 0x55, 0x00],
            &[],
            vec![f],
        ));
    }
    for execution in [0, 99, 100, 2_999, 3_000, 100_000] {
        let mut f = frame(FrameMode::Sender, BODY);
        f.limits.execution = execution;
        cases.push(base_case(&format!("entry_execution_{execution}"), &[0x00], &[], vec![f]));
    }
    for selector in 0..=0x16u8 {
        let query = [0x5f, 0x60, selector, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3];
        cases.push(base_case(
            &format!("txtrace_{selector:02x}"),
            &[
                0x60, 42, 0x5f, 0x55, 0x60, 0xab, 0x5f, 0x52, 0x60, 7, 0x60, 9, 0x60, 32, 0x5f,
                0xa2, 0x00,
            ],
            &query,
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::PostTx, OTHER)],
        ));
    }
    for selector in 0..=0x0du8 {
        let operand =
            if selector >= 0x0b { U256::from(7) } else { U256::from_be_slice(BODY.as_slice()) };
        let mut query = vec![0x5f, 0x7f];
        query.extend_from_slice(&operand.to_be_bytes::<32>());
        query.extend_from_slice(&[0x60, selector, 0xb7, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3]);
        cases.push(base_case(
            &format!("txdiff_{selector:02x}"),
            &[0x60, 42, 0x5f, 0x55, 0x60, 7, 0x60, 9, 0x5f, 0x5f, 0xa2, 0x00],
            &query,
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::PostTx, OTHER)],
        ));
    }
    for pattern in 0..32u32 {
        let suffix = (0..5)
            .map(|index| {
                let mut f = frame(FrameMode::Sender, BODY);
                let value = U256::from((pattern >> index) & 1);
                f.data = value.to_be_bytes::<32>().to_vec().into();
                f.limits.state = if index == 0 { 97_920 } else { 0 };
                f
            })
            .collect();
        cases.push(base_case(
            &format!("storage_ownership_{pattern:02x}"),
            &[0x5f, 0x35, 0x5f, 0x55, 0x00],
            &[],
            suffix,
        ));
    }
    cases.extend(additional_cases());
    cases
}

#[test]
fn bogota_execution_matches_pinned_revm() {
    let cases = cases();
    let mut failures = 0;
    for case in &cases {
        let native = native(case);
        let oracle = oracle(case);
        match (&native, &oracle) {
            (Ok(a), Ok(b)) if case.expected_valid && a == b => println!("PASS {}", case.name),
            (Err(_), Err(_)) if !case.expected_valid => {
                println!("INVALID {}: evm2={native:?}; revm={oracle:?}", case.name);
            }
            _ => {
                failures += 1;
                eprintln!("MISMATCH {}\nevm2: {native:#?}\nrevm: {oracle:#?}", case.name);
            }
        }
    }
    println!(
        "{} cases, {failures} mismatches; revm=b2109aeb7a69505617c629538e9014bd20148e5c; primitives=921ddebb486507d74808239b51e180e7600196d1",
        cases.len()
    );
    assert_eq!(failures, 0);
}

fn additional_cases() -> Vec<Case> {
    let store = [0x5f, 0x35, 0x5f, 0x55, 0x00];
    let revert = [0x5f, 0x5f, 0xfd];
    let mut cases = Vec::new();
    for (cap, priority, basefee) in [(1, 1, 0), (9, 2, 3), (10, 0, 3), (3, 3, 3)] {
        for assertion_fails in [false, true] {
            let mut suffix = vec![frame(FrameMode::Sender, BODY)];
            if assertion_fails {
                suffix.push(frame(FrameMode::PostTx, OTHER));
            }
            let mut case = base_case(
                &format!(
                    "paid_cap_{cap}_priority_{priority}_base_{basefee}_assert_{assertion_fails}"
                ),
                &[0x60, 42, 0x5f, 0x55, 0x00],
                &revert,
                suffix,
            );
            case.basefee = basefee;
            case.tx.fees.max_fee_per_gas = U256::from(cap);
            case.tx.fees.max_priority_fee_per_gas = U256::from(priority);
            cases.push(case);
        }
    }
    for values in [vec![0], vec![0, 7], vec![0, 1], vec![0, 7, 0], vec![1, 0, 7], vec![1, 0]] {
        let suffix = values
            .iter()
            .map(|value| {
                let mut f = frame(FrameMode::Sender, BODY);
                f.data = U256::from(*value).to_be_bytes::<32>().to_vec().into();
                f.limits.state = 0;
                f
            })
            .collect();
        let mut case =
            base_case(&format!("nonzero_original_refund_{values:?}"), &store, &[], suffix);
        case.storage.push((BODY, U256::ZERO, U256::from(7)));
        case.tx.fees.max_fee_per_gas = U256::from(3);
        case.tx.fees.max_priority_fee_per_gas = U256::from(1);
        cases.push(case);
    }
    for assertion_fails in [false, true] {
        let mut payer = frame(FrameMode::Verify, OTHER);
        payer.flags = 1;
        let mut suffix = vec![payer, frame(FrameMode::Sender, BODY)];
        if assertion_fails {
            // APPROVE is forbidden in POST_TX, so the payer contract also supplies
            // a deterministic failing assertion after its successful payment prefix.
            suffix.push(frame(FrameMode::PostTx, OTHER));
        }
        let mut case = base_case(
            &format!("external_payer_assert_{assertion_fails}"),
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[0x60, 1, 0x5f, 0x5f, 0xaa],
            suffix,
        );
        case.sender_code = Bytes::from_static(&[0x60, 2, 0x5f, 0x5f, 0xaa]);
        case.tx.fees.max_fee_per_gas = U256::from(9);
        case.tx.fees.max_priority_fee_per_gas = U256::from(2);
        case.basefee = 3;
        cases.push(case);
    }
    let mut first = frame(FrameMode::Sender, BODY);
    first.flags = 4;
    let mut failing = frame(FrameMode::Sender, OTHER);
    failing.flags = 4;
    cases.push(base_case(
        "atomic_storage_rollback_and_skip",
        &[0x60, 42, 0x5f, 0x55, 0x00],
        &revert,
        vec![first.clone(), failing, frame(FrameMode::Sender, BODY)],
    ));
    let mut case = base_case(
        "atomic_nonzero_original_refund_rollback",
        &[0x5f, 0x5f, 0x55, 0x00],
        &revert,
        vec![first, frame(FrameMode::Sender, OTHER)],
    );
    case.storage.push((BODY, U256::ZERO, U256::from(7)));
    cases.push(case);
    cases.push(base_case(
        "created_account_and_code_stream",
        &[
            0x63, 0x60, 0x01, 0x5f, 0xf3, 0x5f, 0x52, 0x60, 4, 0x60, 28, 0x5f, 0xf0, 0x5f, 0x52,
            0x60, 32, 0x5f, 0xf3,
        ],
        &[],
        vec![frame(FrameMode::Sender, BODY)],
    ));
    let mut destroy = vec![0x7f];
    destroy.extend_from_slice(
        &U256::from_be_slice(&[0x60, 2, 0x60, 10, 0x5f, 0x39, 0x60, 2, 0x5f, 0xf3, 0x5f, 0xff])
            .to_be_bytes::<32>(),
    );
    destroy.extend_from_slice(&[
        0x5f, 0x52, 0x60, 12, 0x60, 20, 0x5f, 0xf0, 0x5f, 0x52, 0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x5f,
        0x51, 0x61, 0xc3, 0x50, 0xf1, 0x50, 0x00,
    ]);
    cases.push(base_case(
        "created_then_selfdestructed_stream",
        &destroy,
        &[],
        vec![frame(FrameMode::Sender, BODY)],
    ));
    cases
}
