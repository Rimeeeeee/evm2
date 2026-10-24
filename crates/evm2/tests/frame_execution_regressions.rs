//! Native frame execution matrices for receipts, accounting and transaction-local assertions.

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{
    Frame, FrameLimits, FrameMode, FrameReceipt, FrameStatus, NONCE_MANAGER, nonce_calldata,
    nonce_slot,
};
use alloy_primitives::{Address, B256, Bytes, Log, U256};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId, Version,
    bytecode::Bytecode,
    constants::EIP7708_TRANSFER_TOPIC,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, eip8141::FrameGas, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB, SYSTEM_ADDRESS},
    version::GasId,
};
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
    expected_success: Option<bool>,
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
            nonce_keys: Some(vec![]),
            frames,
            ..Default::default()
        },
        body: Bytes::copy_from_slice(body),
        other: Bytes::copy_from_slice(other),
        sender_code: Bytes::from_static(&[0x60, 3, 0x5f, 0x5f, 0xaa]),
        storage: Vec::new(),
        basefee: 0,
        expected_valid: true,
        expected_success: None,
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
                frame(FrameMode::Assert, OTHER),
                frame(FrameMode::Assert, OTHER),
            ],
        ),
        base_case(
            "assertion_trace",
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[0x5f, 0x60, 1, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3],
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::Assert, OTHER)],
        ),
    ];
    for (keys, valid) in [
        (vec![], true),
        (vec![0], true),
        (vec![1], true),
        (vec![1, 2], true),
        (vec![0, 1], true),
        (vec![0, 0], false),
        (vec![2, 1], false),
        ((0..17).collect(), false),
    ] {
        let mut case = base_case(&format!("nonce_keys_{keys:?}"), &[], &[], vec![]);
        case.expected_valid = valid;
        case.tx.nonce_keys = Some(keys.into_iter().map(U256::from).collect());
        cases.push(case);
    }
    let mut legacy_max = base_case("legacy_max_nonce_is_invalid", &[], &[], vec![]);
    legacy_max.tx.nonce = u64::MAX;
    legacy_max.expected_valid = false;
    cases.push(legacy_max);
    for keys in [vec![U256::ZERO], vec![U256::from(1)], vec![U256::ZERO, U256::from(1)]] {
        for current in [U256::ZERO, U256::from(1), U256::from(u64::MAX), U256::from(1) << 64] {
            let mut case =
                base_case(&format!("single_use_keys_{keys:?}_current_{current}"), &[], &[], vec![]);
            case.tx.nonce_keys = Some(keys.clone());
            case.tx.nonce = u64::MAX;
            case.expected_valid = current.is_zero();
            case.storage.push((NONCE_MANAGER, nonce_slot(SENDER, *keys.last().unwrap()), current));
            cases.push(case);
        }
    }
    let mut sequential_max = base_case("sequential_key_exhaustion", &[], &[], vec![]);
    sequential_max.tx.nonce_keys = Some(vec![U256::ZERO]);
    sequential_max.tx.nonce = u64::MAX - 1;
    sequential_max.storage.push((
        NONCE_MANAGER,
        nonce_slot(SENDER, U256::ZERO),
        U256::from(u64::MAX - 1),
    ));
    cases.push(sequential_max);
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
    for selector in 0..=0x1bu8 {
        let query = [0x5f, 0x60, selector, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3];
        cases.push(base_case(
            &format!("txtrace_{selector:02x}"),
            &[
                0x60, 42, 0x5f, 0x55, 0x60, 0xab, 0x5f, 0x52, 0x60, 7, 0x60, 9, 0x60, 32, 0x5f,
                0xa2, 0x00,
            ],
            &query,
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::Assert, OTHER)],
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
    cases.extend(assertion_cases());
    cases
}

fn additional_cases() -> Vec<Case> {
    let store = [0x5f, 0x35, 0x5f, 0x55, 0x00];
    let revert = [0x5f, 0x5f, 0xfd];
    let mut cases = Vec::new();
    let version = Version::base(SpecId::BOGOTA);
    for (budget_per_max_gas, priority, basefee) in
        [(1, 1, 0), (9, 2, 3), (10, 0, 3), (3, 3, 3), (4, 3, 3)]
    {
        for assertion_fails in [false, true] {
            let mut suffix = vec![frame(FrameMode::Sender, BODY)];
            if assertion_fails {
                suffix.push(frame(FrameMode::Assert, OTHER));
            }
            let mut case = base_case(
                &format!(
                    "paid_budget_{budget_per_max_gas}_priority_{priority}_base_{basefee}_assert_{assertion_fails}"
                ),
                &[0x60, 42, 0x5f, 0x55, 0x00],
                &revert,
                suffix,
            );
            case.basefee = basefee;
            case.tx.fees.max_fee =
                U256::from(FrameGas::calculate(&case.tx, version).unwrap().maximum)
                    * U256::from(budget_per_max_gas);
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
        case.tx.fees.max_fee =
            U256::from(FrameGas::calculate(&case.tx, version).unwrap().maximum) * U256::from(3);
        case.tx.fees.max_priority_fee_per_gas = U256::from(1);
        cases.push(case);
    }
    for assertion_fails in [false, true] {
        let mut payer = frame(FrameMode::Verify, OTHER);
        payer.flags = 1;
        let mut suffix = vec![payer, frame(FrameMode::Sender, BODY)];
        if assertion_fails {
            // APPROVE is forbidden in ASSERT, so the payer contract also supplies
            // a deterministic failing assertion after its successful payment prefix.
            suffix.push(frame(FrameMode::Assert, OTHER));
        }
        let mut case = base_case(
            &format!("external_payer_assert_{assertion_fails}"),
            &[0x60, 42, 0x5f, 0x55, 0x00],
            &[0x60, 1, 0x5f, 0x5f, 0xaa],
            suffix,
        );
        case.sender_code = Bytes::from_static(&[0x60, 2, 0x5f, 0x5f, 0xaa]);
        case.tx.fees.max_fee =
            U256::from(FrameGas::calculate(&case.tx, version).unwrap().maximum) * U256::from(9);
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

fn assertion_cases() -> Vec<Case> {
    // Deploy nonempty stored code, change creator/created account nonces and storage,
    // and emit one event with four topics and a 32-byte data payload ending in 0xab.
    let body = [
        0x63, 0x60, 0x01, 0x5f, 0xf3, 0x5f, 0x52, 0x60, 4, 0x60, 28, 0x5f, 0xf0, 0x50, 0x60, 42,
        0x5f, 0x55, 0x60, 0xab, 0x5f, 0x52, 0x60, 0x44, 0x60, 0x33, 0x60, 0x22, 0x60, 0x11, 0x60,
        32, 0x5f, 0xa4, 0x00,
    ];
    let mut cases = Vec::new();
    for keyed in [false, true] {
        for selector in 0..=0x1au8 {
            let event_index = u8::from((0x12..=0x18).contains(&selector));
            let query = [0x60, event_index, 0x60, selector, 0xb6, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3];
            let mut transfer = frame(FrameMode::Sender, BODY);
            transfer.value = U256::from(7);
            transfer.limits.state = 1_000_000;
            let mut case = base_case(
                &format!("txtrace_net_changes_{selector:02x}_keyed_{keyed}"),
                &body,
                &query,
                vec![transfer, frame(FrameMode::Assert, OTHER)],
            );
            if keyed {
                case.tx.nonce_keys = Some(vec![U256::ZERO]);
            }
            case.expected_success = Some(true);
            cases.push(case);
        }
    }
    for selector in [0, 1, 2, 3, 0x11, 0x19, 0x1a] {
        let query = [0x60, 1, 0x60, selector, 0xb6, 0x00];
        let mut case = base_case(
            &format!("txtrace_scalar_rejects_nonzero_index_{selector:02x}"),
            &body,
            &query,
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::Assert, OTHER)],
        );
        case.tx.frames[1].limits.state = 1_000_000;
        case.expected_success = Some(false);
        cases.push(case);
    }
    for (index, memory, offset, len, success) in [
        (U256::ZERO, U256::ZERO, U256::ZERO, 32, true),
        (U256::ZERO, U256::ZERO, U256::from(31), 4, false),
        (U256::ZERO, U256::from(1), U256::ZERO, 33, false),
        (U256::ZERO, U256::from(1), U256::ZERO, 32, true),
        (U256::ZERO, U256::MAX, U256::from(32), 0, true),
        (U256::ZERO, U256::MAX, U256::MAX, 0, false),
        (U256::from(1), U256::ZERO, U256::ZERO, 32, false),
        (U256::MAX, U256::ZERO, U256::ZERO, 0, false),
    ] {
        let mut query = Vec::new();
        for operand in [U256::from(len), offset, memory, index] {
            query.push(0x7f);
            query.extend_from_slice(&operand.to_be_bytes::<32>());
        }
        query.extend_from_slice(&[0xb7, 0x60, 64, 0x5f, 0xf3]);
        let mut case = base_case(
            &format!("eventdatacopy_{index}_{memory}_{offset}_{len}"),
            &body,
            &query,
            vec![frame(FrameMode::Sender, BODY), frame(FrameMode::Assert, OTHER)],
        );
        case.tx.frames[1].limits.state = 1_000_000;
        case.expected_success = Some(success);
        cases.push(case);
    }
    let mut old_copy = base_case(
        "old_eventdatacopy_byte_is_undefined",
        &body,
        &[0x5f, 0x5f, 0x5f, 0x5f, 0xb8, 0x00],
        vec![frame(FrameMode::Sender, BODY), frame(FrameMode::Assert, OTHER)],
    );
    old_copy.tx.frames[1].limits.state = 1_000_000;
    old_copy.expected_success = Some(false);
    cases.push(old_copy);
    cases
}

fn word(value: U256) -> Bytes {
    Bytes::copy_from_slice(&value.to_be_bytes::<32>())
}

fn address_word(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

fn put_storage(
    accounts: &mut BTreeMap<Address, Account>,
    address: Address,
    key: U256,
    value: U256,
) {
    let storage = &mut accounts.get_mut(&address).unwrap().storage;
    if value.is_zero() {
        storage.remove(&key);
    } else {
        storage.insert(key, value);
    }
}

fn assert_native_case(case: &Case, result: &Outcome) {
    let version = Version::base(SpecId::BOGOTA);
    let gp = &version.gas_params;
    let slot_cost = u64::from(gp.get(GasId::SstoreSetState));
    let create_cost = gp.create_state_gas() + gp.code_deposit_state_gas(1);
    let mut statuses = vec![FrameStatus::Success; case.tx.frames.len()];
    let mut executions = vec![107; case.tx.frames.len()];
    let mut state = vec![0; case.tx.frames.len()];
    let mut outputs = vec![Bytes::new(); case.tx.frames.len()];
    let mut receipt_logs = vec![Vec::new(); case.tx.frames.len()];
    let mut accounts = prestate(case);
    let keys = case.tx.nonce_keys.as_deref().unwrap_or_default();
    if keys.is_empty() {
        accounts.get_mut(&SENDER).unwrap().nonce = 1;
    } else {
        for &key in keys {
            let slot = nonce_slot(SENDER, key);
            if accounts[&NONCE_MANAGER].storage.get(&slot).is_none_or(U256::is_zero) {
                state[0] += slot_cost;
            }
            put_storage(
                &mut accounts,
                NONCE_MANAGER,
                slot,
                U256::from(case.tx.nonce.saturating_add(1)),
            );
        }
    }
    let name = case.name.as_str();
    let mut success = true;
    let mut raw_refund = 0;
    let payer = if name.starts_with("external_payer_") { OTHER } else { SENDER };
    if name.starts_with("external_payer_") {
        executions[1] = 3_007;
        executions[2] = 15_105;
        state[2] = slot_cost;
        if name.ends_with("true") {
            success = false;
            statuses[3] = FrameStatus::Failure;
            executions[3] = case.tx.frames[3].limits.execution;
            state[2] = 0;
        } else {
            put_storage(&mut accounts, BODY, U256::ZERO, U256::from(42));
        }
    } else if name.starts_with("storage_ownership_") {
        // Only the first frame has a slot-creation reservoir. A later attempt to
        // recreate a cleared slot fails its independent state-gas budget.
        let first = !U256::from_be_slice(&case.tx.frames[1].data).is_zero();
        let mut present = first;
        executions[1] = if first { 15_107 } else { 5_107 };
        for index in 2..case.tx.frames.len() {
            let next = !U256::from_be_slice(&case.tx.frames[index].data).is_zero();
            if next && !present {
                statuses[index] = FrameStatus::Failure;
                executions[index] = 100_000;
            } else {
                executions[index] = 207;
                if present && !next {
                    raw_refund += u64::from(gp.get(GasId::SstoreSetRefund));
                }
                present = next;
            }
        }
        state[1] = if present { slot_cost } else { 0 };
        put_storage(&mut accounts, BODY, U256::ZERO, U256::from(u8::from(present)));
    } else if name.starts_with("nonzero_original_refund_") {
        let mut present = U256::from(7);
        for index in 1..case.tx.frames.len() {
            let next = U256::from_be_slice(&case.tx.frames[index].data);
            executions[index] = if index == 1 { 5_107 } else { 207 };
            if present == U256::from(7) && next != present {
                executions[index] += 10_000;
            }
            if next == U256::from(7) && next != present {
                raw_refund += u64::from(gp.get(GasId::SstoreResetRefund));
            }
            present = next;
        }
        if present.is_zero() {
            raw_refund += u64::from(gp.get(GasId::SstoreClearingSlotRefund));
        }
        put_storage(&mut accounts, BODY, U256::ZERO, present);
    } else if name.starts_with("entry_execution_") {
        let limit = case.tx.frames[1].limits.execution;
        executions[1] = limit.min(3_000);
        if limit < 3_000 {
            statuses[1] = FrameStatus::Failure;
        }
    } else if name.starts_with("sstore_state_") {
        let reservoir = case.tx.frames[1].limits.state;
        if reservoir < slot_cost {
            statuses[1] = FrameStatus::Failure;
            executions[1] = 100_000;
        } else {
            executions[1] = 15_105;
            state[1] = slot_cost;
            put_storage(&mut accounts, BODY, U256::ZERO, U256::from(1));
        }
    } else if name == "atomic_storage_rollback_and_skip" {
        executions[1..].copy_from_slice(&[15_105, 3_004, 0]);
        statuses[2] = FrameStatus::Failure;
        statuses[3] = FrameStatus::SkippedAtomicBatch;
    } else if name == "atomic_nonzero_original_refund_rollback" {
        executions[1..].copy_from_slice(&[15_104, 3_004]);
        statuses[2] = FrameStatus::Failure;
    } else if name == "created_account_and_code_stream" {
        // CREATE access + one initcode word, the eight-gas initcode, memory,
        // pushes and the EIP-8037 deployed-code hash word.
        executions[1] = 15_039 + gp.keccak256_word_cost(1);
        state[1] = create_cost;
        let created = BODY.create(0);
        outputs[1] = word(address_word(created));
        accounts.get_mut(&BODY).unwrap().nonce = 1;
        accounts.insert(
            created,
            Account { nonce: 1, code_hash: alloy_primitives::keccak256([0]), ..Default::default() },
        );
    } else if name == "created_then_selfdestructed_stream" {
        // The zero beneficiary is already warm; the newly created zero-balance
        // contract destroys itself without an ETH transfer or surviving account.
        executions[1] = 20_170 + gp.keccak256_word_cost(2);
        state[1] = gp.create_state_gas() + gp.code_deposit_state_gas(2);
        accounts.get_mut(&BODY).unwrap().nonce = 1;
    } else if name == "sstore" || name.starts_with("paid_budget_") {
        executions[1] = 15_105;
        if name.ends_with("assert_true") {
            success = false;
            statuses[2] = FrameStatus::Failure;
            executions[2] = 3_004;
        } else {
            state[1] = slot_cost;
            put_storage(&mut accounts, BODY, U256::ZERO, U256::from(42));
        }
    } else if name == "assertion_revert" {
        success = false;
        executions[1..].copy_from_slice(&[15_105, 3_004, 0]);
        statuses[2] = FrameStatus::Failure;
        statuses[3] = FrameStatus::SkippedAtomicBatch;
    } else if name == "assertion_trace" {
        executions[1..].copy_from_slice(&[15_105, 3_118]);
        state[1] = slot_cost;
        outputs[2] = word(U256::from(1));
        put_storage(&mut accounts, BODY, U256::ZERO, U256::from(42));
    } else if name.starts_with("txtrace_net_changes_")
        || name.starts_with("txtrace_scalar_rejects_")
        || name.starts_with("eventdatacopy_")
        || name == "old_eventdatacopy_byte_is_undefined"
    {
        executions[1] = 29_292 + gp.keccak256_word_cost(1);
        success = case.expected_success.unwrap();
        if success {
            let created = BODY.create(0);
            state[1] = create_cost + slot_cost;
            accounts.get_mut(&BODY).unwrap().nonce = 1;
            accounts.insert(
                created,
                Account {
                    nonce: 1,
                    code_hash: alloy_primitives::keccak256([0]),
                    ..Default::default()
                },
            );
            put_storage(&mut accounts, BODY, U256::ZERO, U256::from(42));
            let value = case.tx.frames[1].value;
            if !value.is_zero() {
                accounts.get_mut(&SENDER).unwrap().balance -= value;
                accounts.get_mut(&BODY).unwrap().balance += value;
                receipt_logs[1].push(Log::new_unchecked(
                    SYSTEM_ADDRESS,
                    vec![
                        EIP7708_TRANSFER_TOPIC,
                        B256::left_padding_from(SENDER.as_slice()),
                        B256::left_padding_from(BODY.as_slice()),
                    ],
                    word(value),
                ));
            }
            receipt_logs[1].push(Log::new_unchecked(
                BODY,
                [0x11, 0x22, 0x33, 0x44].map(|topic| B256::from(U256::from(topic))).to_vec(),
                word(U256::from(0xab)),
            ));
            if name.starts_with("txtrace_net_changes_") {
                let selector = case.other[3];
                executions[2] = 3_119;
                let mut nonce_addresses = vec![BODY, created];
                if keys.is_empty() {
                    nonce_addresses.push(SENDER);
                }
                nonce_addresses.sort_unstable();
                let mut slots = vec![(BODY, U256::ZERO, U256::from(42))];
                if let Some(&key) = keys.first() {
                    slots.push((NONCE_MANAGER, nonce_slot(SENDER, key), U256::from(1)));
                }
                slots.sort_unstable();
                let expected = match selector {
                    0 => U256::from(2),
                    1 => U256::from(slots.len()),
                    2 => U256::from(1),
                    3 => U256::from(nonce_addresses.len()),
                    4 => address_word(SENDER),
                    5 => U256::from(1_000_000_000),
                    6 => U256::from(999_999_993),
                    7 => address_word(slots[0].0),
                    8 => U256::ZERO,
                    9 => slots[0].2,
                    0x0a => slots[0].1,
                    0x0b => address_word(created),
                    0x0c => U256::from_be_bytes(alloy_primitives::KECCAK256_EMPTY.0),
                    0x0d => U256::from_be_bytes(alloy_primitives::keccak256([0]).0),
                    0x0e => address_word(nonce_addresses[0]),
                    0x0f => U256::ZERO,
                    0x10 => U256::from(1),
                    0x11 => U256::from(2),
                    0x12 => address_word(BODY),
                    0x13 => U256::from(4),
                    0x14..=0x17 => {
                        U256::from([0x11, 0x22, 0x33, 0x44][usize::from(selector - 0x14)])
                    }
                    0x18 => U256::from(32),
                    0x19 => U256::ZERO,
                    0x1a => address_word(SENDER),
                    _ => unreachable!(),
                };
                outputs[2] = word(expected);
            } else {
                let memory = U256::from_be_slice(&case.other[67..99]);
                let len = U256::from_be_slice(&case.other[1..33]).to::<usize>();
                executions[2] = 3_026 + 3 * len.div_ceil(32) as u64;
                let mut output = vec![0; 64];
                if len != 0 {
                    output[memory.to::<usize>() + 31] = 0xab;
                }
                outputs[2] = output.into();
            }
        } else {
            statuses[2] = FrameStatus::Failure;
            executions[2] = case.tx.frames[2].limits.execution;
        }
    } else if name.starts_with("txtrace_") {
        let selector = case.other[2];
        success = !matches!(selector, 4..=6 | 0x0b..=0x0d | 0x16 | 0x17 | 0x1b);
        executions[1] = 16_508;
        if success {
            executions[2] = 3_118;
            state[1] = slot_cost;
            put_storage(&mut accounts, BODY, U256::ZERO, U256::from(42));
            receipt_logs[1].push(Log::new_unchecked(
                BODY,
                vec![B256::from(U256::from(9)), B256::from(U256::from(7))],
                word(U256::from(0xab)),
            ));
            outputs[2] = word(match selector {
                0 | 2 | 8 | 0x0a | 0x0f | 0x19 => U256::ZERO,
                1 | 3 | 0x10 | 0x11 => U256::from(1),
                7 | 0x12 => address_word(BODY),
                9 => U256::from(42),
                0x0e | 0x1a => address_word(SENDER),
                0x13 => U256::from(2),
                0x14 => U256::from(9),
                0x15 => U256::from(7),
                0x18 => U256::from(32),
                _ => unreachable!(),
            });
        } else {
            statuses[2] = FrameStatus::Failure;
            executions[2] = case.tx.frames[2].limits.execution;
        }
    }
    assert_eq!(result.success, success, "{}", case.name);
    assert_eq!(result.payer, payer, "{}", case.name);
    assert_eq!(result.receipts.len(), case.tx.frames.len(), "{}", case.name);
    for (index, receipt) in result.receipts.iter().enumerate() {
        assert_eq!(receipt.status, statuses[index], "{} frame {index}", case.name);
        assert_eq!(receipt.gas_used.execution, executions[index], "{} frame {index}", case.name);
        assert_eq!(receipt.gas_used.state, state[index], "{} frame {index}", case.name);
        assert_eq!(receipt.logs, receipt_logs[index], "{} frame {index}", case.name);
    }
    assert_eq!(result.outputs, outputs, "{}", case.name);
    assert_eq!(
        result.logs,
        receipt_logs.into_iter().flatten().collect::<Vec<_>>(),
        "{}",
        case.name
    );

    // These matrices contain no signature entries. Derive the intrinsic and floor
    // directly from their wire bytes, independently of FrameGas::calculate.
    let nonce_data = nonce_calldata(keys, case.tx.nonce);
    let bytes = nonce_data.iter().chain(case.tx.frames.iter().flat_map(|frame| frame.data.iter()));
    let mut byte_count = 0;
    let mut tokens = 0;
    for byte in bytes {
        byte_count += 1;
        tokens += if *byte == 0 { 1 } else { 4 };
    }
    let base = 12_000
        + 475 * case.tx.frames.len() as u64
        + 6_000 * u64::from(!case.tx.frames.iter().all(|frame| frame.value.is_zero()))
        + 12_100 * keys.len() as u64;
    let intrinsic = base + 4 * tokens;
    let floor = base + 64 * byte_count;
    let expected_state = state.iter().sum::<u64>();
    let expected_execution = intrinsic + executions.iter().sum::<u64>();
    let expected_spent = expected_execution + expected_state;
    let refund = raw_refund.min(expected_spent / 5);
    let used = expected_execution.saturating_sub(refund).max(floor) + expected_state;
    assert_eq!(result.state_gas, expected_state, "{}", case.name);
    assert_eq!(result.spent, expected_spent, "{}", case.name);
    assert_eq!(result.floor, floor, "{}", case.name);
    assert_eq!(result.refunded, refund, "{}", case.name);
    assert_eq!(result.fee_gas, used, "{}", case.name);
    assert_eq!(result.block_execution, expected_execution.max(floor), "{}", case.name);

    let base_fee = U256::from(used) * U256::from(case.basefee);
    let tip = (U256::from(used) * case.tx.fees.max_priority_fee_per_gas)
        .min(case.tx.fees.max_fee - base_fee);
    accounts.get_mut(&payer).unwrap().balance -= base_fee + tip;
    if !tip.is_zero() {
        accounts
            .entry(Address::ZERO)
            .or_insert_with(|| Account {
                code_hash: alloy_primitives::KECCAK256_EMPTY,
                ..Default::default()
            })
            .balance += tip;
    }
    assert_eq!(result.accounts, accounts, "{}", case.name);
}

#[test]
fn native_frame_execution_matches_explicit_protocol_expectations() {
    let cases = cases();
    assert_eq!(cases.len(), 189);
    for case in &cases {
        let result = native(case);
        if case.expected_valid {
            assert_native_case(
                case,
                &result.unwrap_or_else(|error| panic!("{}: {error}", case.name)),
            );
        } else {
            assert!(result.is_err(), "{} accepted an invalid payload", case.name);
        }
    }
}
