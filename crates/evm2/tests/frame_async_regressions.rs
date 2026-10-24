//! Async prefix validation must preserve the frame transaction rollback contract.

#![cfg(feature = "async")]

mod common;

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{FrameMode, FrameReceipt, NONCE_MANAGER, TransactionFees, nonce_slot};
use alloy_primitives::{Address, B256, Bytes, U256};
use common::{APPROVE_BOTH, CountingRunner, SENDER};
use evm2::{
    BaseEvmTypes, DatabaseError, Evm, Inspector, Precompiles,
    bytecode::Bytecode,
    ethereum::TxEnvelope,
    evm::{
        AccountInfo, DynDatabase, InMemoryDB,
        r#async::{AsyncDatabase, AsyncDb},
    },
    interpreter::InstrStop,
};
use std::{
    future::{Future, pending},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

const SUSPEND: Address = Address::with_last_byte(0xfe);

fn database() -> InMemoryDB {
    let mut database = common::database([(SENDER, APPROVE_BOTH)]);
    database.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    database
}

fn evm() -> Evm<'static, BaseEvmTypes> {
    common::evm(database())
}

fn transaction(keyed: bool) -> Recovered<TxEnvelope> {
    common::recovered(TxEip8141 {
        fees: TransactionFees {
            max_fee: U256::from(1_000_000),
            max_priority_fee_per_gas: U256::ONE,
        },
        nonce_keys: Some(if keyed { vec![U256::from(1)] } else { vec![] }),
        ..common::payload(vec![common::frame(FrameMode::Verify, SENDER)])
    })
}

fn ready<F: Future>(future: F) -> F::Output {
    match pin!(future).as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("synchronous host unexpectedly suspended"),
    }
}

const fn assert_send<F: Future + Send>(future: F) -> F {
    future
}

#[test]
fn async_prefix_returns_payment_facts_and_discards_legacy_and_keyed_consumption() {
    for keyed in [false, true] {
        let mut evm = evm();
        let transaction = transaction(keyed);
        let expected = evm.validate_frame_prefix(&transaction, 1).unwrap();
        let actual = ready(evm.validate_frame_prefix_async(&transaction, 1)).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.payer, SENDER);
        assert!(actual.sender_approved);
        assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
        assert_eq!(
            evm.state_mut()
                .storage(&NONCE_MANAGER)
                .unwrap()
                .into_slot(nonce_slot(SENDER, U256::from(1)))
                .unwrap()
                .get()
                .current,
            U256::ZERO,
        );
        assert!(evm.transact(&transaction).unwrap().discard().status);
    }
}

#[test]
fn send_prefix_future_requires_and_preserves_verified_host_fields() {
    let mut evm = evm();
    evm.evm_is_send::<InMemoryDB, Precompiles<BaseEvmTypes>>();
    let transaction = transaction(true);
    let validation =
        ready(assert_send(evm.validate_frame_prefix_async_send(&transaction, 1))).unwrap();
    assert_eq!(validation.payer, SENDER);
    assert_eq!(validation.prefix_end, 1);
    assert!(evm.transact(&transaction).unwrap().discard().status);
}

struct SuspendAfterApproval(Arc<AtomicUsize>);

impl Inspector<BaseEvmTypes> for SuspendAfterApproval {
    fn frame_end(
        &mut self,
        _: usize,
        _: &FrameReceipt,
        _: &Bytes,
        _: InstrStop,
        host: &mut Evm<'_, BaseEvmTypes>,
    ) {
        self.0.fetch_add(1, Ordering::Relaxed);
        let _ = host.state_mut().account(&SUSPEND);
    }
}

struct SuspendingDatabase(InMemoryDB);

impl AsyncDatabase for SuspendingDatabase {
    type Error = DatabaseError;

    async fn get_account(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if address == SUSPEND {
            pending::<()>().await;
        }
        DynDatabase::get_account(&mut self.0, &address)
    }

    async fn get_code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        DynDatabase::get_code_by_hash(&mut self.0, &code_hash)
    }

    async fn get_storage(&mut self, address: Address, key: U256) -> Result<U256, Self::Error> {
        DynDatabase::get_storage(&mut self.0, &address, &key)
    }

    async fn get_block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        DynDatabase::get_block_hash(&mut self.0, &number)
    }
}

#[test]
fn cancelling_suspended_prefix_restores_runner_nonce_state_and_execution_guard() {
    for keyed in [false, true] {
        let mut evm = evm();
        evm.set_database(AsyncDb::new(SuspendingDatabase(database())));
        let calls = Arc::new(AtomicUsize::new(0));
        let suspensions = Arc::new(AtomicUsize::new(0));
        evm.set_interpreter_runner(CountingRunner(calls.clone()));
        evm.set_inspector(SuspendAfterApproval(suspensions.clone()));
        let transaction = transaction(keyed);
        {
            let mut validation = pin!(evm.validate_frame_prefix_async(&transaction, 1));
            assert!(validation.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
            assert_eq!(suspensions.load(Ordering::Relaxed), 1);
            assert_eq!(calls.load(Ordering::Relaxed), 0);
        }
        evm.clear_inspector();
        assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
        assert_eq!(evm.state_mut().account(&SENDER).unwrap().balance(), U256::from(1_000_000_000));
        assert!(ready(evm.transact_async(&transaction)).unwrap().discard().status);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn async_prefix_error_does_not_poison_following_full_execution() {
    let mut evm = evm();
    let transaction = transaction(false);
    assert!(ready(evm.validate_frame_prefix_async(&transaction, 0)).is_err());
    assert!(evm.transact(&transaction).unwrap().discard().status);
}
