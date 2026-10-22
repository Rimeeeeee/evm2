# Bogota devnet 1 migration: complete source inventory

Companion to [the migration assessment](bogota-devnet-1-evm2-migration.md). Research date: 22 October 2026. All external links identify immutable commits; line numbers refer to those commits, not to a mutable checkout.

## 1. Scope and reproduction

`paradigmxyz/reth-oss/bogota-devnet-1` is pinned at `7bde440574a2afef4f718f32c02b03320f8d57c1`. The evm2 target is `185be4b37d44f22cf81f71cf0568ea710b54a66d`. The branch diff uses merge base `040bef7b319c7ec145689e03fe78710e11986db6`. Locked dependency commits appear in section 2 of the assessment.

The inventories have different meanings: the branch delta includes all changed files; the backend reference list locates direct coupling; the feature search locates explicit frame/EIP mentions. None is a substitute for the ownership and behavior analysis in the assessment. In particular, an ordinary shared executor can require migration without mentioning an EIP by number.

Read-only commands used for the Reth inventories:

```powershell
git diff --numstat 040bef7b319c7ec145689e03fe78710e11986db6 7bde440574a2afef4f718f32c02b03320f8d57c1
git grep -n -E "revm::|reth_revm::|revm_inspectors::" 7bde440574a2afef4f718f32c02b03320f8d57c1 -- crates bin examples
```

Feature searches used Rust source files and the following expression, including comments:

```text
\b(Eip8141|FrameTransaction|eip8141|eip8250|eip8272|eip7906|TXTRACE|TXDIFF|EVENTDATACOPY|PostTx)\b|EIP-(8141|8250|8272|7906)
```

The current draft snapshots were downloaded from `ethereum/EIPs` at `46005d745a5d921702f12d64a1b85e712e2b6051`. They were compared with the four user-linked EIP pages. Current source and pinned-download hashes agreed at audit time. This audit did not run the node, compiler or consensus fixture suites.

## 2. Entire Reth branch delta

123 files; 9,240 additions and 2,453 deletions. Counts include tests, dependency lock updates, CI and general receipt adaptations. These are the complete branch changes relative to the merge base, not a list of items to move into evm2.

| File | Added | Deleted |
|---|---:|---:|
| [.github/scripts/hive/build_simulators.sh](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/.github/scripts/hive/build_simulators.sh) | 16 | 3 |
| [.github/scripts/hive/load_images.sh](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/.github/scripts/hive/load_images.sh) | 41 | 15 |
| [.github/scripts/hive/run_simulator.sh](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/.github/scripts/hive/run_simulator.sh) | 7 | 0 |
| [.github/workflows/hive.yml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/.github/workflows/hive.yml) | 467 | 196 |
| [.github/workflows/lint.yml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/.github/workflows/lint.yml) | 2 | 1 |
| [Cargo.lock](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Cargo.lock) | 1048 | 1255 |
| [Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Cargo.toml) | 86 | 31 |
| [Dockerfile](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Dockerfile) | 1 | 1 |
| [Dockerfile.depot](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Dockerfile.depot) | 1 | 1 |
| [README.md](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/README.md) | 1 | 1 |
| [bin/reth-bb/src/evm.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/bin/reth-bb/src/evm.rs) | 10 | 7 |
| [crates/chain-state/src/notifications.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chain-state/src/notifications.rs) | 3 | 18 |
| [crates/chain-state/src/test_utils.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chain-state/src/test_utils.rs) | 53 | 26 |
| [crates/chainspec/src/spec.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chainspec/src/spec.rs) | 85 | 4 |
| [crates/cli/commands/Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/cli/commands/Cargo.toml) | 1 | 1 |
| [crates/cli/commands/src/test_vectors/compact.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/cli/commands/src/test_vectors/compact.rs) | 113 | 6 |
| [crates/cli/util/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/cli/util/src/lib.rs) | 1 | 1 |
| [crates/engine/primitives/src/message.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/primitives/src/message.rs) | 4 | 0 |
| [crates/engine/tree/src/tree/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/mod.rs) | 55 | 1 |
| [crates/engine/tree/src/tree/payload_processor/bal/execute.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/execute.rs) | 67 | 12 |
| [crates/engine/tree/src/tree/payload_processor/bal/worker.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/worker.rs) | 7 | 6 |
| [crates/engine/tree/src/tree/payload_processor/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/mod.rs) | 34 | 4 |
| [crates/engine/tree/src/tree/payload_processor/prewarm.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/prewarm.rs) | 60 | 60 |
| [crates/engine/tree/src/tree/payload_processor/receipt_root_task.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/receipt_root_task.rs) | 8 | 18 |
| [crates/engine/tree/src/tree/payload_validator.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_validator.rs) | 21 | 4 |
| [crates/era-utils/src/export/ere.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era-utils/src/export/ere.rs) | 38 | 1 |
| [crates/era-utils/src/history.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era-utils/src/history.rs) | 28 | 45 |
| [crates/era/src/era1/types/execution.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era/src/era1/types/execution.rs) | 11 | 10 |
| [crates/era/src/ere/types/execution.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era/src/ere/types/execution.rs) | 19 | 16 |
| [crates/era/src/test_utils.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era/src/test_utils.rs) | 10 | 3 |
| [crates/errors/src/error.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/errors/src/error.rs) | 1 | 1 |
| [crates/ethereum/consensus/src/validation.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/consensus/src/validation.rs) | 7 | 1 |
| [crates/ethereum/evm/Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/Cargo.toml) | 2 | 0 |
| [crates/ethereum/evm/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/lib.rs) | 59 | 7 |
| [crates/ethereum/evm/src/receipt.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/receipt.rs) | 9 | 11 |
| [crates/ethereum/evm/tests/execute.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/tests/execute.rs) | 1 | 1 |
| [crates/ethereum/node/Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/Cargo.toml) | 2 | 0 |
| [crates/ethereum/node/src/frame_validation.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs) | 265 | 0 |
| [crates/ethereum/node/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/lib.rs) | 1 | 0 |
| [crates/ethereum/node/src/node.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/node.rs) | 12 | 3 |
| [crates/ethereum/node/tests/e2e/frame_transactions.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/frame_transactions.rs) | 514 | 0 |
| [crates/ethereum/node/tests/e2e/main.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/main.rs) | 1 | 0 |
| [crates/ethereum/node/tests/e2e/rpc.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/rpc.rs) | 24 | 6 |
| [crates/ethereum/node/tests/it/testing.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/it/testing.rs) | 3 | 1 |
| [crates/ethereum/payload/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/payload/src/lib.rs) | 33 | 7 |
| [crates/ethereum/primitives/Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/Cargo.toml) | 4 | 3 |
| [crates/ethereum/primitives/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/lib.rs) | 6 | 6 |
| [crates/ethereum/primitives/src/pooled.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/pooled.rs) | 267 | 0 |
| [crates/ethereum/primitives/src/receipt.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/receipt.rs) | 687 | 47 |
| [crates/evm/evm/src/execute.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/execute.rs) | 45 | 10 |
| [crates/evm/evm/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/lib.rs) | 2 | 0 |
| [crates/evm/execution-types/src/chain.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/chain.rs) | 2 | 12 |
| [crates/evm/execution-types/src/execution_outcome.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/execution_outcome.rs) | 43 | 55 |
| [crates/net/downloaders/src/receipt_file_client.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/downloaders/src/receipt_file_client.rs) | 18 | 40 |
| [crates/net/eth-wire-types/src/receipts.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/eth-wire-types/src/receipts.rs) | 29 | 22 |
| [crates/net/network/src/metrics.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/network/src/metrics.rs) | 10 | 0 |
| [crates/net/network/src/transactions/fetcher.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/network/src/transactions/fetcher.rs) | 12 | 11 |
| [crates/net/network/tests/it/requests.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/network/tests/it/requests.rs) | 8 | 24 |
| [crates/node/core/src/args/database.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/database.rs) | 1 | 1 |
| [crates/node/core/src/args/datadir_args.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/datadir_args.rs) | 1 | 1 |
| [crates/node/core/src/args/debug.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/debug.rs) | 1 | 1 |
| [crates/node/core/src/args/dev.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/dev.rs) | 1 | 1 |
| [crates/node/core/src/args/engine.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/engine.rs) | 1 | 1 |
| [crates/node/core/src/args/jit.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/jit.rs) | 1 | 1 |
| [crates/node/core/src/args/log.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/log.rs) | 1 | 1 |
| [crates/node/core/src/args/network.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/network.rs) | 1 | 1 |
| [crates/node/core/src/args/rpc_server.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/rpc_server.rs) | 1 | 1 |
| [crates/node/core/src/args/static_files.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/static_files.rs) | 1 | 1 |
| [crates/node/core/src/args/storage.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/args/storage.rs) | 1 | 1 |
| [crates/node/core/src/cli/config.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/node/core/src/cli/config.rs) | 3 | 3 |
| [crates/payload/primitives/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/primitives/src/lib.rs) | 8 | 23 |
| [crates/payload/primitives/src/payload.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/primitives/src/payload.rs) | 20 | 1 |
| [crates/prune/prune/Cargo.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/prune/prune/Cargo.toml) | 1 | 0 |
| [crates/prune/prune/src/segments/user/receipts_by_logs.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/prune/prune/src/segments/user/receipts_by_logs.rs) | 10 | 3 |
| [crates/rpc/rpc-builder/tests/it/http.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-builder/tests/it/http.rs) | 9 | 1 |
| [crates/rpc/rpc-eth-api/src/core.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/core.rs) | 30 | 4 |
| [crates/rpc/rpc-eth-api/src/helpers/call.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/call.rs) | 337 | 25 |
| [crates/rpc/rpc-eth-api/src/helpers/estimate.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs) | 252 | 10 |
| [crates/rpc/rpc-eth-api/src/helpers/pending_block.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/pending_block.rs) | 1 | 1 |
| [crates/rpc/rpc-eth-api/src/helpers/state.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/state.rs) | 90 | 5 |
| [crates/rpc/rpc-eth-api/src/helpers/transaction.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/transaction.rs) | 107 | 1 |
| [crates/rpc/rpc-eth-types/src/error/api.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/api.rs) | 1 | 0 |
| [crates/rpc/rpc-eth-types/src/error/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/mod.rs) | 40 | 3 |
| [crates/rpc/rpc-eth-types/src/fee_history.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/fee_history.rs) | 1 | 1 |
| [crates/rpc/rpc-eth-types/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/lib.rs) | 1 | 0 |
| [crates/rpc/rpc-eth-types/src/receipt.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/receipt.rs) | 84 | 16 |
| [crates/rpc/rpc-eth-types/src/simulate.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/simulate.rs) | 253 | 15 |
| [crates/rpc/rpc/src/eth/filter.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/filter.rs) | 78 | 94 |
| [crates/rpc/rpc/src/eth/helpers/transaction.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/helpers/transaction.rs) | 240 | 3 |
| [crates/stages/stages/src/stages/execution/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/stages/stages/src/stages/execution/mod.rs) | 8 | 0 |
| [crates/storage/provider/src/providers/database/provider.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/database/provider.rs) | 7 | 7 |
| [crates/storage/provider/src/providers/static_file/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/static_file/mod.rs) | 2 | 2 |
| [crates/storage/provider/src/test_utils/blocks.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/test_utils/blocks.rs) | 30 | 30 |
| [crates/storage/provider/src/test_utils/mock.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/test_utils/mock.rs) | 4 | 6 |
| [crates/storage/rpc-provider/src/rpc_response.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/rpc-provider/src/rpc_response.rs) | 1 | 1 |
| [crates/transaction-pool/src/blobstore/tracker.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/blobstore/tracker.rs) | 33 | 3 |
| [crates/transaction-pool/src/error.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/error.rs) | 22 | 0 |
| [crates/transaction-pool/src/identifier.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/identifier.rs) | 28 | 1 |
| [crates/transaction-pool/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/lib.rs) | 12 | 0 |
| [crates/transaction-pool/src/maintain.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/maintain.rs) | 24 | 8 |
| [crates/transaction-pool/src/ordering.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/ordering.rs) | 191 | 3 |
| [crates/transaction-pool/src/pool/best.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/best.rs) | 8 | 7 |
| [crates/transaction-pool/src/pool/blob.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/blob.rs) | 1 | 1 |
| [crates/transaction-pool/src/pool/frame_revalidation.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/frame_revalidation.rs) | 154 | 0 |
| [crates/transaction-pool/src/pool/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/mod.rs) | 203 | 32 |
| [crates/transaction-pool/src/pool/parked.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/parked.rs) | 5 | 2 |
| [crates/transaction-pool/src/pool/pending.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/pending.rs) | 2 | 1 |
| [crates/transaction-pool/src/pool/state.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/state.rs) | 5 | 5 |
| [crates/transaction-pool/src/pool/txpool.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/txpool.rs) | 346 | 49 |
| [crates/transaction-pool/src/test_utils/mock.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/test_utils/mock.rs) | 13 | 0 |
| [crates/transaction-pool/src/traits.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/traits.rs) | 241 | 12 |
| [crates/transaction-pool/src/validate/eth.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/eth.rs) | 326 | 28 |
| [crates/transaction-pool/src/validate/frame_inspector.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs) | 541 | 0 |
| [crates/transaction-pool/src/validate/frame_policy.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_policy.rs) | 492 | 0 |
| [crates/transaction-pool/src/validate/frame_state.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_state.rs) | 537 | 0 |
| [crates/transaction-pool/src/validate/mod.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/mod.rs) | 29 | 3 |
| [crates/trie/common/src/lib.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/common/src/lib.rs) | 1 | 1 |
| [crates/trie/common/src/ordered_root.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/common/src/ordered_root.rs) | 3 | 3 |
| [deny.toml](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/deny.toml) | 15 | 0 |
| [examples/beacon-api-sidecar-fetcher/src/mined_sidecar.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/beacon-api-sidecar-fetcher/src/mined_sidecar.rs) | 4 | 4 |
| [examples/custom-beacon-withdrawals/src/main.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/custom-beacon-withdrawals/src/main.rs) | 4 | 1 |
| [examples/db-access/src/main.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/db-access/src/main.rs) | 1 | 1 |
| [testing/testing-utils/src/generators.rs](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/testing/testing-utils/src/generators.rs) | 5 | 7 |

## 3. Direct Reth backend references

107 Rust files in `crates/`, `bin/` and `examples/`. Each link points to the first direct reference; further matching line numbers are listed for navigation. Tests and examples are included. This is a migration audit list, not a claim that every file needs a semantic change.

| File / first reference | Other matching lines |
|---|---|
| [bin/reth-bb/src/evm_config.rs:32](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/bin/reth-bb/src/evm_config.rs#L32) | 100, 111, 122, 132, 187, 192, 197, 199, 217, 222, 227, 229 |
| [bin/reth-bb/src/evm.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/bin/reth-bb/src/evm.rs#L24) | — |
| [bin/reth/src/lib.rs:150](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/bin/reth/src/lib.rs#L150) | — |
| [crates/chain-state/src/in_memory.rs:1127](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chain-state/src/in_memory.rs#L1127) | — |
| [crates/chain-state/src/memory_overlay.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chain-state/src/memory_overlay.rs#L16) | 322 |
| [crates/chain-state/src/test_utils.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chain-state/src/test_utils.rs#L24) | 471 |
| [crates/cli/commands/src/re_execute.rs:23](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/cli/commands/src/re_execute.rs#L23) | — |
| [crates/e2e-test-utils/src/testsuite/setup.rs:13](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/e2e-test-utils/src/testsuite/setup.rs#L13) | — |
| [crates/engine/execution-cache/src/cached_state.rs:17](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/execution-cache/src/cached_state.rs#L17) | 1064, 1425, 1426 |
| [crates/engine/execution-cache/src/txpool.rs:5](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/execution-cache/src/txpool.rs#L5) | 62, 64 |
| [crates/engine/invalid-block-hooks/src/witness.rs:9](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/invalid-block-hooks/src/witness.rs#L9) | 16, 421, 422, 425, 427 |
| [crates/engine/tree/src/tree/instrumented_state.rs:310](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/instrumented_state.rs#L310) | — |
| [crates/engine/tree/src/tree/metrics.rs:639](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/metrics.rs#L639) | — |
| [crates/engine/tree/src/tree/mod.rs:39](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/mod.rs#L39) | 44 |
| [crates/engine/tree/src/tree/payload_processor/bal/execute.rs:33](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/execute.rs#L33) | 351, 353, 443, 598, 763, 1220, 1229, 1268, 1281, 1314, 1326, 1364, 1387 |
| [crates/engine/tree/src/tree/payload_processor/bal/worker.rs:10](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/worker.rs#L10) | — |
| [crates/engine/tree/src/tree/payload_processor/mod.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/mod.rs#L24) | 35, 722, 723 |
| [crates/engine/tree/src/tree/payload_processor/prewarm.rs:34](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/prewarm.rs#L34) | — |
| [crates/engine/tree/src/tree/payload_validator.rs:161](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_validator.rs#L161) | 1254 |
| [crates/engine/tree/src/tree/precompile_cache.rs:10](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/precompile_cache.rs#L10) | 262, 263 |
| [crates/engine/tree/src/tree/state_root_strategy/mod.rs:1307](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/state_root_strategy/mod.rs#L1307) | 1389 |
| [crates/engine/tree/src/tree/txpool_prewarm/worker.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/txpool_prewarm/worker.rs#L16) | — |
| [crates/engine/util/src/reorg.rs:22](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/util/src/reorg.rs#L22) | — |
| [crates/ethereum/evm/src/build.rs:13](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/build.rs#L13) | — |
| [crates/ethereum/evm/src/factory.rs:9](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/factory.rs#L9) | — |
| [crates/ethereum/evm/src/lib.rs:36](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/lib.rs#L36) | 49, 50, 397 |
| [crates/ethereum/evm/tests/execute.rs:25](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/tests/execute.rs#L25) | — |
| [crates/ethereum/node/src/engine_ssz_witness.rs:10](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/engine_ssz_witness.rs#L10) | 78 |
| [crates/ethereum/node/src/frame_validation.rs:9](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L9) | — |
| [crates/ethereum/node/src/node.rs:65](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/node.rs#L65) | — |
| [crates/ethereum/node/tests/e2e/eip6780.rs:42](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/eip6780.rs#L42) | — |
| [crates/ethereum/node/tests/e2e/selfdestruct.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/selfdestruct.rs#L20) | — |
| [crates/ethereum/node/tests/e2e/simulate.rs:66](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/simulate.rs#L66) | — |
| [crates/ethereum/payload/src/lib.rs:34](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/payload/src/lib.rs#L34) | 41 |
| [crates/evm/evm/src/aliases.rs:8](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/aliases.rs#L8) | — |
| [crates/evm/evm/src/either.rs:61](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/either.rs#L61) | 69 |
| [crates/evm/evm/src/execute.rs:25](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/execute.rs#L25) | 200, 765 |
| [crates/evm/evm/src/lib.rs:35](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/lib.rs#L35) | — |
| [crates/evm/evm/src/test_utils.rs:4](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/test_utils.rs#L4) | — |
| [crates/evm/execution-types/src/chain.rs:703](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/chain.rs#L703) | — |
| [crates/evm/execution-types/src/execute.rs:3](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/execute.rs#L3) | — |
| [crates/evm/execution-types/src/execution_outcome.rs:11](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/execution_outcome.rs#L11) | 431 |
| [crates/exex/exex/src/backfill/job.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/exex/exex/src/backfill/job.rs#L20) | — |
| [crates/exex/exex/src/backfill/test_utils.rs:19](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/exex/exex/src/backfill/test_utils.rs#L19) | — |
| [crates/payload/basic/src/lib.rs:25](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/basic/src/lib.rs#L25) | — |
| [crates/revm/src/cached.rs:7](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/cached.rs#L7) | 11, 20, 168 |
| [crates/revm/src/database.rs:7](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/database.rs#L7) | — |
| [crates/revm/src/lib.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/lib.rs#L24) | 31 |
| [crates/revm/src/test_utils.rs:165](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/test_utils.rs#L165) | — |
| [crates/revm/src/witness.rs:4](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/witness.rs#L4) | 165, 176, 189 |
| [crates/rpc/rpc-eth-api/src/helpers/bal.rs:8](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/bal.rs#L8) | — |
| [crates/rpc/rpc-eth-api/src/helpers/block.rs:19](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/block.rs#L19) | — |
| [crates/rpc/rpc-eth-api/src/helpers/call.rs:34](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/call.rs#L34) | 54, 59, 232, 509 |
| [crates/rpc/rpc-eth-api/src/helpers/config.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/config.rs#L16) | — |
| [crates/rpc/rpc-eth-api/src/helpers/estimate.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs#L16) | 29 |
| [crates/rpc/rpc-eth-api/src/helpers/pending_block.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/pending_block.rs#L20) | 35 |
| [crates/rpc/rpc-eth-api/src/helpers/state.rs:27](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/state.rs#L27) | — |
| [crates/rpc/rpc-eth-api/src/helpers/trace.rs:18](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/trace.rs#L18) | 19 |
| [crates/rpc/rpc-eth-api/src/helpers/transaction.rs:40](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/transaction.rs#L40) | — |
| [crates/rpc/rpc-eth-types/src/cache/db.rs:4](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/cache/db.rs#L4) | 6, 36 |
| [crates/rpc/rpc-eth-types/src/cache/mod.rs:14](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/cache/mod.rs#L14) | — |
| [crates/rpc/rpc-eth-types/src/error/api.rs:8](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/api.rs#L8) | 9 |
| [crates/rpc/rpc-eth-types/src/error/mod.rs:14](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/mod.rs#L14) | 24, 30, 479, 480, 482 |
| [crates/rpc/rpc-eth-types/src/simulate.rs:30](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/simulate.rs#L30) | 655, 721, 728, 808 |
| [crates/rpc/rpc/src/debug.rs:27](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/debug.rs#L27) | 47, 48, 1515 |
| [crates/rpc/rpc/src/eth/bundle.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/bundle.rs#L20) | — |
| [crates/rpc/rpc/src/eth/helpers/types.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/helpers/types.rs#L20) | 25, 38, 54, 74 |
| [crates/rpc/rpc/src/eth/sim_bundle.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/sim_bundle.rs#L24) | — |
| [crates/rpc/rpc/src/otterscan.rs:24](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/otterscan.rs#L24) | 25 |
| [crates/rpc/rpc/src/reth.rs:162](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/reth.rs#L162) | — |
| [crates/rpc/rpc/src/testing.rs:42](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/testing.rs#L42) | 48 |
| [crates/rpc/rpc/src/trace.rs:33](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/trace.rs#L33) | 34 |
| [crates/rpc/rpc/src/validation.rs:36](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/validation.rs#L36) | — |
| [crates/snap-sync/src/attempt.rs:139](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/snap-sync/src/attempt.rs#L139) | — |
| [crates/stages/stages/src/stages/execution/mod.rs:21](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/stages/stages/src/stages/execution/mod.rs#L21) | 808 |
| [crates/stages/stages/src/stages/execution/slot_preimages.rs:13](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/stages/stages/src/stages/execution/slot_preimages.rs#L13) | 205 |
| [crates/stages/stages/tests/pipeline.rs:34](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/stages/stages/tests/pipeline.rs#L34) | — |
| [crates/stages/stages/tests/preimage.rs:44](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/stages/stages/tests/preimage.rs#L44) | — |
| [crates/storage/errors/src/provider.rs:10](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/errors/src/provider.rs#L10) | — |
| [crates/storage/provider/src/changesets_utils/state_reverts.rs:2](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/changesets_utils/state_reverts.rs#L2) | — |
| [crates/storage/provider/src/lib.rs:49](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/lib.rs#L49) | — |
| [crates/storage/provider/src/providers/blockchain_provider.rs:1076](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/blockchain_provider.rs#L1076) | — |
| [crates/storage/provider/src/providers/consistent.rs:30](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/consistent.rs#L30) | 1432 |
| [crates/storage/provider/src/providers/database/provider.rs:75](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/database/provider.rs#L75) | 4008, 5334 |
| [crates/storage/provider/src/providers/state/latest.rs:268](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/state/latest.rs#L268) | — |
| [crates/storage/provider/src/test_utils/blocks.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/test_utils/blocks.rs#L16) | — |
| [crates/storage/provider/src/test_utils/mock.rs:1172](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/test_utils/mock.rs#L1172) | — |
| [crates/storage/provider/src/writer/mod.rs:25](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/writer/mod.rs#L25) | 31, 32 |
| [crates/storage/rpc-provider/src/lib.rs:1329](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/rpc-provider/src/lib.rs#L1329) | — |
| [crates/storage/storage-api/src/macros.rs:66](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-api/src/macros.rs#L66) | — |
| [crates/storage/storage-api/src/noop.rs:573](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-api/src/noop.rs#L573) | — |
| [crates/storage/storage-api/src/state_writer.rs:7](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-api/src/state_writer.rs#L7) | — |
| [crates/storage/storage-api/src/state.rs:14](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-api/src/state.rs#L14) | — |
| [crates/storage/storage-overlay/src/builder.rs:21](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-overlay/src/builder.rs#L21) | 810 |
| [crates/storage/storage-overlay/src/manager.rs:961](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-overlay/src/manager.rs#L961) | — |
| [crates/storage/storage-overlay/src/provider.rs:769](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/storage-overlay/src/provider.rs#L769) | 1062 |
| [crates/transaction-pool/src/validate/eth.rs:41](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/eth.rs#L41) | 1618, 1632, 1639, 1678 |
| [crates/transaction-pool/src/validate/frame_inspector.rs:9](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs#L9) | 326, 386, 518 |
| [crates/trie/common/src/hashed_state.rs:20](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/common/src/hashed_state.rs#L20) | 827 |
| [crates/trie/common/src/target_v2.rs:6](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/common/src/target_v2.rs#L6) | — |
| [crates/trie/db/src/state.rs:362](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/db/src/state.rs#L362) | — |
| [crates/trie/parallel/src/state_root_task.rs:16](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/parallel/src/state_root_task.rs#L16) | 527 |
| [crates/trie/trie/src/hashed_cursor/mod.rs:5](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/trie/src/hashed_cursor/mod.rs#L5) | 118 |
| [examples/custom-beacon-withdrawals/src/main.rs:11](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/custom-beacon-withdrawals/src/main.rs#L11) | 24 |
| [examples/custom-evm/src/main.rs:8](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/custom-evm/src/main.rs#L8) | 21 |
| [examples/custom-inspector/src/main.rs:23](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/custom-inspector/src/main.rs#L23) | — |
| [examples/custom-state-root/src/main.rs:49](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/examples/custom-state-root/src/main.rs#L49) | — |

## 4. Explicit feature-bearing source files

These searches exclude arbitrary hexadecimal matches and include EIP names in comments. A file can appear in several earlier categories. Shared EIP-8141 types carry parts of EIP-8250 and EIP-7906, so there is not necessarily one crate or file per EIP.

### paradigmxyz/reth-oss (33 files)

Revision: `7bde440574a2afef4f718f32c02b03320f8d57c1`.

- [crates/chainspec/src/spec.rs:888](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chainspec/src/spec.rs#L888)
- [crates/cli/commands/src/test_vectors/compact.rs:3](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/cli/commands/src/test_vectors/compact.rs#L3)
- [crates/engine/tree/src/tree/payload_processor/bal/execute.rs:1254](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/execute.rs#L1254)
- [crates/era-utils/src/export/ere.rs:92](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era-utils/src/export/ere.rs#L92)
- [crates/ethereum/node/src/frame_validation.rs:4](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L4)
- [crates/ethereum/node/tests/e2e/frame_transactions.rs:1](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/frame_transactions.rs#L1)
- [crates/ethereum/payload/src/lib.rs:381](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/payload/src/lib.rs#L381)
- [crates/ethereum/primitives/src/pooled.rs:6](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/pooled.rs#L6)
- [crates/ethereum/primitives/src/receipt.rs:9](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/receipt.rs#L9)
- [crates/net/eth-wire-types/src/receipts.rs:90](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/eth-wire-types/src/receipts.rs#L90)
- [crates/net/network/src/metrics.rs:590](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/network/src/metrics.rs#L590)
- [crates/payload/primitives/src/lib.rs:193](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/primitives/src/lib.rs#L193)
- [crates/payload/primitives/src/payload.rs:68](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/primitives/src/payload.rs#L68)
- [crates/rpc/rpc-eth-api/src/core.rs:225](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/core.rs#L225)
- [crates/rpc/rpc-eth-api/src/helpers/call.rs:308](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/call.rs#L308)
- [crates/rpc/rpc-eth-api/src/helpers/estimate.rs:84](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs#L84)
- [crates/rpc/rpc-eth-api/src/helpers/state.rs:67](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/state.rs#L67)
- [crates/rpc/rpc-eth-types/src/error/api.rs:132](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/api.rs#L132)
- [crates/rpc/rpc-eth-types/src/error/mod.rs:231](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/mod.rs#L231)
- [crates/rpc/rpc-eth-types/src/receipt.rs:84](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/receipt.rs#L84)
- [crates/rpc/rpc-eth-types/src/simulate.rs:625](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/simulate.rs#L625)
- [crates/rpc/rpc/src/eth/helpers/transaction.rs:139](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc/src/eth/helpers/transaction.rs#L139)
- [crates/transaction-pool/src/blobstore/tracker.rs:121](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/blobstore/tracker.rs#L121)
- [crates/transaction-pool/src/error.rs:248](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/error.rs#L248)
- [crates/transaction-pool/src/ordering.rs:161](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/ordering.rs#L161)
- [crates/transaction-pool/src/pool/mod.rs:704](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/mod.rs#L704)
- [crates/transaction-pool/src/pool/txpool.rs:833](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/txpool.rs#L833)
- [crates/transaction-pool/src/test_utils/mock.rs:423](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/test_utils/mock.rs#L423)
- [crates/transaction-pool/src/traits.rs:1502](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/traits.rs#L1502)
- [crates/transaction-pool/src/validate/eth.rs:81](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/eth.rs#L81)
- [crates/transaction-pool/src/validate/frame_inspector.rs:5](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs#L5)
- [crates/transaction-pool/src/validate/frame_policy.rs:1](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_policy.rs#L1)
- [crates/transaction-pool/src/validate/frame_state.rs:36](https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_state.rs#L36)

### Soubhik-10/revm (33 files)

Revision: `b2109aeb7a69505617c629538e9014bd20148e5c`.

- [bins/revme/src/cmd/blockchaintest.rs:6](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/bins/revme/src/cmd/blockchaintest.rs#L6)
- [bins/revme/src/cmd/statetest/runner.rs:168](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/bins/revme/src/cmd/statetest/runner.rs#L168)
- [crates/bytecode/src/opcode.rs:187](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/bytecode/src/opcode.rs#L187)
- [crates/context/interface/src/cfg.rs:73](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/cfg.rs#L73)
- [crates/context/interface/src/cfg/gas.rs:38](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/cfg/gas.rs#L38)
- [crates/context/interface/src/host.rs:12](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/host.rs#L12)
- [crates/context/interface/src/journaled_state.rs:134](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/journaled_state.rs#L134)
- [crates/context/interface/src/local.rs:10](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/local.rs#L10)
- [crates/context/interface/src/result.rs:66](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/result.rs#L66)
- [crates/context/interface/src/transaction.rs:5](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/transaction.rs#L5)
- [crates/context/interface/src/transaction/eip8141.rs:1](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/transaction/eip8141.rs#L1)
- [crates/context/interface/src/transaction/transaction_type.rs:18](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/transaction/transaction_type.rs#L18)
- [crates/context/src/cfg.rs:145](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/cfg.rs#L145)
- [crates/context/src/context.rs:521](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/context.rs#L521)
- [crates/context/src/journal.rs:21](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal.rs#L21)
- [crates/context/src/journal/inner.rs:63](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal/inner.rs#L63)
- [crates/context/src/local.rs:14](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/local.rs#L14)
- [crates/context/src/tx.rs:6](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/tx.rs#L6)
- [crates/handler/src/eip8141.rs:1](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L1)
- [crates/handler/src/frame.rs:183](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/frame.rs#L183)
- [crates/handler/src/handler.rs:168](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/handler.rs#L168)
- [crates/handler/src/lib.rs:12](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/lib.rs#L12)
- [crates/handler/src/precompile_provider.rs:350](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/precompile_provider.rs#L350)
- [crates/handler/src/validation.rs:4](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/validation.rs#L4)
- [crates/inspector/src/handler.rs:5](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/inspector/src/handler.rs#L5)
- [crates/interpreter/src/instructions.rs:15](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/instructions.rs#L15)
- [crates/interpreter/src/instructions/frame.rs:1](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/instructions/frame.rs#L1)
- [crates/interpreter/src/interpreter_action/call_inputs.rs:146](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/interpreter_action/call_inputs.rs#L146)
- [crates/primitives/src/eip7906.rs:1](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/eip7906.rs#L1)
- [crates/primitives/src/hardfork.rs:79](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/hardfork.rs#L79)
- [crates/primitives/src/lib.rs:27](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/lib.rs#L27)
- [examples/custom_precompile_journal/src/custom_evm.rs:333](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/examples/custom_precompile_journal/src/custom_evm.rs#L333)
- [examples/custom_precompile_journal/src/main.rs:101](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/examples/custom_precompile_journal/src/main.rs#L101)

### Rimeeeeee/alloy (24 files)

Revision: `1ff4bf799bc78b0913561fa3e44bc65d0444c638`.

- [crates/consensus-any/src/receipt/envelope.rs:20](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus-any/src/receipt/envelope.rs#L20)
- [crates/consensus/src/block/mod.rs:329](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/block/mod.rs#L329)
- [crates/consensus/src/receipt/envelope.rs:11](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/receipt/envelope.rs#L11)
- [crates/consensus/src/receipt/receipt2.rs:272](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/receipt/receipt2.rs#L272)
- [crates/consensus/src/size.rs:99](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/size.rs#L99)
- [crates/consensus/src/transaction/eip8141.rs:9](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/eip8141.rs#L9)
- [crates/consensus/src/transaction/envelope.rs:41](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/envelope.rs#L41)
- [crates/consensus/src/transaction/mod.rs:19](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/mod.rs#L19)
- [crates/consensus/src/transaction/pooled.rs:4](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/pooled.rs#L4)
- [crates/consensus/src/transaction/tx_type.rs:44](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/tx_type.rs#L44)
- [crates/consensus/src/transaction/typed.rs:67](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/typed.rs#L67)
- [crates/eips/src/eip2718.rs:395](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/eip2718.rs#L395)
- [crates/eips/src/eip8141.rs:1](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/eip8141.rs#L1)
- [crates/eips/src/eip8272.rs:1](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/eip8272.rs#L1)
- [crates/eips/src/lib.rs:66](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/lib.rs#L66)
- [crates/network/src/any/either.rs:686](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/src/any/either.rs#L686)
- [crates/network/src/ethereum/builder.rs:121](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/src/ethereum/builder.rs#L121)
- [crates/network/src/ethereum/mod.rs:18](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/src/ethereum/mod.rs#L18)
- [crates/network/tests/eip8141_regressions.rs:10](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/tests/eip8141_regressions.rs#L10)
- [crates/provider/tests/frame_submission.rs:9](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/provider/tests/frame_submission.rs#L9)
- [crates/rpc-types-eth/src/simulate.rs:8](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/rpc-types-eth/src/simulate.rs#L8)
- [crates/rpc-types-eth/src/transaction/frame.rs:3](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/rpc-types-eth/src/transaction/frame.rs#L3)
- [crates/rpc-types-eth/src/transaction/frame_serde.rs:4](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/rpc-types-eth/src/transaction/frame_serde.rs#L4)
- [crates/rpc-types-eth/src/transaction/request.rs:18](https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/rpc-types-eth/src/transaction/request.rs#L18)

### Soubhik-10/evm (13 files)

Revision: `5c35db18a87dd6122b7d1c0ab690d275fe13fa3a`.

- [crates/evm/src/block/system_calls/eip7002.rs:67](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/block/system_calls/eip7002.rs#L67)
- [crates/evm/src/block/system_calls/eip7251.rs:69](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/block/system_calls/eip7251.rs#L69)
- [crates/evm/src/block/system_calls/eip8282.rs:102](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/block/system_calls/eip8282.rs#L102)
- [crates/evm/src/either.rs:58](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/either.rs#L58)
- [crates/evm/src/eth/block.rs:115](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/block.rs#L115)
- [crates/evm/src/eth/mod.rs:18](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/mod.rs#L18)
- [crates/evm/src/eth/receipt_builder.rs:9](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/receipt_builder.rs#L9)
- [crates/evm/src/eth/spec_id.rs:24](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/spec_id.rs#L24)
- [crates/evm/src/evm.rs:97](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/evm.rs#L97)
- [crates/evm/src/rpc/transaction.rs:7](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/rpc/transaction.rs#L7)
- [crates/evm/src/traits.rs:13](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/traits.rs#L13)
- [crates/evm/src/tx.rs:20](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/tx.rs#L20)
- [crates/evm/tests/frame_adapters.rs:6](https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/tests/frame_adapters.rs#L6)

### Soubhik-10/eips (12 files)

Revision: `921ddebb486507d74808239b51e180e7600196d1`.

- [crates/eip8141/src/address.rs:10](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/address.rs#L10)
- [crates/eip8141/src/constants.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/constants.rs#L1)
- [crates/eip8141/src/error.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/error.rs#L1)
- [crates/eip8141/src/frame.rs:6](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/frame.rs#L6)
- [crates/eip8141/src/lib.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/lib.rs#L1)
- [crates/eip8141/src/macros.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/macros.rs#L1)
- [crates/eip8141/src/nonce.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/nonce.rs#L1)
- [crates/eip8141/src/receipt.rs:6](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/receipt.rs#L6)
- [crates/eip8141/src/signature.rs:6](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/signature.rs#L6)
- [crates/eip8141/tests/encoding.rs:109](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/tests/encoding.rs#L109)
- [crates/eip8141/tests/proptest.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/tests/proptest.rs#L1)
- [crates/eip8141/tests/serde.rs:1](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/tests/serde.rs#L1)

### Soubhik-10/reth-core (5 files)

Revision: `c42b71acb8e12de6b2b01238bbc1ce739510bdd3`.

- [crates/codecs/src/alloy/receipt.rs:13](https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/receipt.rs#L13)
- [crates/codecs/src/alloy/transaction/eip8141.rs:5](https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/transaction/eip8141.rs#L5)
- [crates/codecs/src/alloy/transaction/ethereum.rs:59](https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/transaction/ethereum.rs#L59)
- [crates/codecs/src/alloy/transaction/mod.rs:24](https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/transaction/mod.rs#L24)
- [crates/codecs/src/alloy/transaction/txtype.rs:6](https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/transaction/txtype.rs#L6)

### Soubhik-10/revm-inspectors (6 files)

Revision: `b736efa83f749ad15190ec3fbc3c95c6ca42d3cc`.

- [src/tracing/builder/geth.rs:335](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/builder/geth.rs#L335)
- [src/tracing/builder/parity.rs:221](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/builder/parity.rs#L221)
- [src/tracing/js/mod.rs:326](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/js/mod.rs#L326)
- [src/tracing/mod.rs:100](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/mod.rs#L100)
- [src/tracing/types.rs:106](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/types.rs#L106)
- [tests/it/frame_transaction.rs:6](https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/tests/it/frame_transaction.rs#L6)

## 5. Additional evm2 integration files

The main assessment references these existing target files. New module names in that report are proposals and are deliberately absent here.

- [Cargo.toml:61](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/Cargo.toml#L61)
- [crates/eest/src/forks.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/forks.rs#L1)
- [crates/eest/src/tx.rs:77](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/tx.rs#L77)
- [crates/eest/src/types.rs:177](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/types.rs#L177)
- [crates/evm2/examples/bal_parallel.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/examples/bal_parallel.rs#L1)
- [crates/evm2/src/ethereum/mod.rs:40](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L40)
- [crates/evm2/src/evm/bal/bal_context.rs:171](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/bal/bal_context.rs#L171)
- [crates/evm2/src/evm/config.rs:22](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/config.rs#L22)
- [crates/evm2/src/evm/env.rs:12](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/env.rs#L12)
- [crates/evm2/src/evm/handler.rs:41](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/handler.rs#L41)
- [crates/evm2/src/evm/inspector.rs:14](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/inspector.rs#L14)
- [crates/evm2/src/evm/mod.rs:969](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/mod.rs#L969)
- [crates/evm2/src/evm/registry.rs:187](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/registry.rs#L187)
- [crates/evm2/src/evm/state/account.rs:181](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/account.rs#L181)
- [crates/evm2/src/evm/state/journal.rs:7](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/journal.rs#L7)
- [crates/evm2/src/evm/state/mod.rs:773](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/mod.rs#L773)
- [crates/evm2/src/evm/state/pending.rs:30](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/pending.rs#L30)
- [crates/evm2/src/evm/state/stream.rs:9](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/stream.rs#L9)
- [crates/evm2/src/evm/state/tracked.rs:9](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/tracked.rs#L9)
- [crates/evm2/src/evm/tx.rs:91](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/tx.rs#L91)
- [crates/evm2/src/interpreter/gas.rs:327](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/gas.rs#L327)
- [crates/evm2/src/interpreter/host.rs:114](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/host.rs#L114)
- [crates/evm2/src/interpreter/instructions/block.rs:83](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/instructions/block.rs#L83)
- [crates/evm2/src/interpreter/instructions/mod.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/instructions/mod.rs#L1)
- [crates/evm2/src/interpreter/opcode.rs:451](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/opcode.rs#L451)
- [crates/evm2/src/spec_id.rs:62](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/spec_id.rs#L62)
- [crates/evm2/src/version/features.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/features.rs#L1)
- [crates/evm2/src/version/mod.rs:744](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/mod.rs#L744)
- [crates/evm2/src/version/opcode_config.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/opcode_config.rs#L1)
- [crates/inspectors/src/tracing/builder/geth.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/builder/geth.rs#L1)
- [crates/inspectors/src/tracing/builder/parity.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/builder/parity.rs#L1)
- [crates/inspectors/src/tracing/mod.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/mod.rs#L1)
- [crates/inspectors/src/tracing/types.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/types.rs#L1)
- [crates/jit/builtins/src/gas.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/builtins/src/gas.rs#L1)
- [crates/jit/builtins/src/lib.rs:432](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/builtins/src/lib.rs#L432)
- [crates/jit/codegen/src/compiler/mod.rs:1](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/compiler/mod.rs#L1)
- [crates/jit/codegen/src/compiler/translate/mod.rs:859](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/compiler/translate/mod.rs#L859)
- [crates/jit/codegen/src/lib.rs:19](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/lib.rs#L19)
- [crates/jit/context/src/lib.rs:28](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/context/src/lib.rs#L28)
- [crates/jit/runtime/src/evm2_evm.rs:29](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/runtime/src/evm2_evm.rs#L29)
- [crates/jit/runtime/src/runtime/storage.rs:9](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/runtime/src/runtime/storage.rs#L9)
