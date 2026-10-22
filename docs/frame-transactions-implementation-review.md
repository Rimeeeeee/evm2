# Bogota frame implementation: local review

This change implements the native evm2 execution layer for the pinned Bogota devnet
1 interpretation of EIP-8141, EIP-8250, EIP-7906 and EIP-8272. It also adds native
compiled opcodes, frame trace lifecycle hooks, fixture ingestion and reproducible
conformance tools. **The complete cross-repository migration checklist is not
finished.** The native Alloy execution adapter and Reth node migration remain open;
this change does not establish a working migrated node.

Workspace: `D:\evm2`, branch `bogota-plan`, starting commit
`1fb7124`. Changes are local and uncommitted. Nothing has been pushed.

## Protocol selected

[frame-transactions-protocol.toml](frame-transactions-protocol.toml) is the common
protocol manifest. It pins the reference VM, shared types, node, fixture source,
archive checksum, assertion selectors and provisional verifier runtime. Bogota
uses legacy nonce keys `[0]`, actual signature-byte gas, empty-string substitution
in transaction signing, mode 3 `PostTx`, `TXDIFF=0xb7`, and `EVENTDATACOPY=0xb8`.
The newer draft changes in P21 are intentionally not enabled.

The embedding chain selects `SpecId::BOGOTA` at its configured activation. Keyed
schema presence is checked explicitly against EIP-8250 activation. Shared chain ID
and sequence interfaces remain 64-bit; frame fees and nonce keys remain U256.
EIP-7819 `SETDELEGATE` is absent from the pinned VM and remains outside this change.
The official corpus exercises deployment through ordinary transactions/contracts.

## Implementation map

| Area | Files and review entry points | Result |
|---|---|---|
| Fork/type dispatch | `crates/evm2/src/spec_id.rs`, `version/{mod.rs,features.rs}`, `ethereum/mod.rs`, `evm/registry.rs` | Bogota inherits Amsterdam, registers type 6 and the ten opcodes; original payload sender is authoritative |
| Preparation and resources | `ethereum/eip8141.rs`: `FrameGas`, `PreparedFrameTx`, `prepare_payload`, `validate_structure`, `validate_signatures` | Checked reservations and payment arithmetic; all static payload/signatures checked before state reads; active execution schedule/cap |
| Full execution | `ethereum/eip8141.rs`: `run`, `execute_frame`, `checkpoint`, `rollback` | One ordered loop, independent frame budgets, atomic groups, approval boundary, assertion rollback and single settlement |
| Prefix API | `ethereum/eip8141.rs`: `Evm::validate_frame_prefix`; `evm/mod.rs`: `FrameSimulationGuard` | Complete preparation plus requested payable prefix; returns payer/exposure/work metadata; always interpreted and discards effects, including unwind |
| Mutable frame state | `evm/frame.rs`: `FrameTransactionRuntime`, `apply_frame_approval`, reversible ownership journal | Nested approvals, default-account verification, nonce consumption and state-growth ownership follow the pinned VM |
| Consensus views | `evm/frame_diff.rs`, `ethereum/eip7906.rs` | Captured transaction originals/net changes; sorted/indexed views; live TXDIFF reads preserve access/gas semantics |
| Instructions | `interpreter/instructions/frame.rs`, `interpreter/{opcode.rs,host.rs}` | APPROVE, TXPARAM, FRAMEDATALOAD/COPY, FRAMEPARAM, SIGPARAM, SIGDATACOPY, TXTRACE, TXDIFF, EVENTDATACOPY |
| Gas and state | `interpreter/gas.rs`, `interpreter/runtime.rs`, `evm/state/{mod.rs,account.rs}`, SSTORE interpreter/JIT paths | Isolated state pool; no spill into execution; cross-frame restorations reduce the funding frame's attribution |
| Native results | `evm/tx.rs`: `FrameTransactionResult`, `TxResultExt::tx_gas_used`, `into_receipt` | Payer/precharge, ordered receipts/outputs, rollback/skipped facts; receipt fee gas differs from block gas |
| Compiled execution | `crates/jit/builtins/src/frame.rs`, `builtins/src/ir.rs`, codegen bytecode/translation modules | All ten instructions compile natively; ordinary opcodes in frames use the same isolated gas/host hooks |
| Cache identity | `crates/jit/codegen/src/lib.rs`, `crates/jit/runtime/build.rs` | ABI version 4; AOT executable exports; reload regression compares full native outcomes/state |
| Inspection | `evm/inspector.rs`, `crates/inspectors/src/tracing/{mod.rs,types.rs,debug.rs,mux.rs}` | Synthetic transaction root, top-level frame metadata, skipped/early nodes, final rollback/gas/log reconciliation |
| Fixtures and block execution | `crates/eest/src/{tx.rs,types.rs,fixture_io.rs,binary.rs}`, `blockchaintest/{types.rs,execute.rs}` | Canonical raw frame bytes, malformed vectors, timestamp fork transition, dual block gas, frame receipt roots and BAL commitments |
| Capture/tooling | `crates/cli/src/capture/export.rs`, replay/fuzzer guards, `validation/frame-oracle/`, `scripts/extract_bogota_fixtures.py` | Explicit sender/full fees and raw type-6 capture; locked reference comparisons; unsupported upstream-revm comparisons fail explicitly |
| CI | `.github/workflows/frame-conformance.yml` | Full pinned four-EIP corpus, every dispatch selection, inspected/JIT/AOT, native reload and differential regressions |

Paths in this table are relative to the named crate where abbreviated. New modules
have module documentation and public API documentation. The focused test names
below give direct entry points for the highest-risk behavior.

The private EEST postcard format now starts with `EV2F` and schema byte 1. Existing
unversioned `.bin` fixtures must be regenerated from JSON; they are rejected rather
than decoded with a different field layout. Reth persistent codecs are still part
of the external migration work.

## Contracts for downstream implementation

Use the normal `BaseEvmTypes` and `ethereum::TxEnvelope::Eip8141`. Convert the shared
`TxEip8141` directly; do not recover an invented outer signature or substitute the
address in a generic `Recovered` wrapper for the payload sender.

`transact()` returns the existing executed-transaction handle. Commit/discard,
detach and streaming state ownership remain native. `TxResult.frame` carries
ordered frame data. `FrameTransactionResult::into_receipt(cumulative_gas)` creates
the shared consensus payload; block code supplies cumulative fee gas. Debug
outputs and rollback markers stay outside the receipt wire encoding.

`validate_tx()` performs preparation only. `validate_frame_prefix(tx, prefix_end)`
executes the canonical prefix, requires the selected payable boundary to match,
and returns payer, maximum exposure, exclusive boundary, sender approval, signature
work, execution gas and state gas. It temporarily removes any compiled runner and
restores it on every exit. Reth must supply prefix shape, opcode bans, dependencies,
resource limits, canonical snapshot and public admission policy through inspection.
The engine does not contain public-mempool policy.

An invalid envelope/prefix returns `HandlerError`, an execution revert/halt appears
in ordered valid transaction outcomes, and database failures preserve their native
fatal classification. Exact multi-error precedence across the complete node API
is not qualified by the differential runner.

Block admission must use `FrameGas::calculate(payload, active_version)` for
independent execution/state reservations. Receipt gas is
`max(spent - refund - state, floor) + state`; block execution gas is
`max(spent - state, floor)` before refund. Do not use a truncated generic scalar
gas/fee accessor. Prefix precharge and approved nonces survive assertion-body
rollback. Actual payload slot number must reach `BlockEnv.slot_num`.

The recent-root verifier remains ordinary deployed code. No VM precompile, fork
state installation, special expiry execution, or timestamp-derived slot was added.
Fixtures check deployment, predeployment behavior, ring/window boundaries and
delegated/static execution. Reth still owns exact-code recognition, pool exceptions,
expiry metadata and head/reorg revalidation.

## Regressions and qualification

The archive is `ethereum/execution-specs tests-bogota-devnet@v1.1.1`, source
`9c98e61e17c7659d744a3eed2da621ff65793abf`, SHA-256
`2b7e0cbfa221f09235212880fc6e2d8ad1189fd1865344491488a8d9c762efa6`.
It was downloaded and checksum-verified before extraction. The selected corpus is
225 blockchain and 194 state JSON files in the four requested EIP families,
including `AmsterdamToBogotaAtTime15k`. Engine API/execute-only formats are excluded
by format, not counted as executed VM tests.

| Check | Local evidence |
|---|---|
| Engine unit tests, portable std/parse/serde/async | 678 passed |
| Focused engine integration | 22 passed |
| Official interpreter corpus | 419 files / 1,481 cases passed, zero skips, for `auto`, `tco`, `packed`, `single_return`, `unpacked` |
| Official inspected interpreter corpus | 419 files / 1,481 cases passed, zero skips |
| Official JIT corpus | 419 files / 1,481 cases passed, zero skips or nonempty-code fallbacks |
| Official AOT corpus | 419 files / 1,481 cases passed, zero skips or nonempty-code fallbacks |
| Pinned revm differential | 89 cases, zero mismatches |
| Compiler library suite with LLVM 23 | 2,624 passed |
| Runtime library suite with LLVM 23 | 85 passed, one existing admission microbenchmark ignored |
| Native opcode/JIT/AOT/reload regression | Passed, checks lookup hits and compilation failures as well as complete outcomes/pending state |
| Fixture and inspector library suites, all features | 30 fixture tests and 102 inspector tests passed |
| Focused frame tracing integration | 1 passed |
| Full workspace documentation, all features/private items | Passed; one pre-existing redundant BAL link warning |
| Full workspace lint, all features/targets | Passed; manifest warnings remain for unused workspace/allocator dependencies and three unused shared patches |

Detailed local logs are under ignored `target/`: `engine-tests.log`,
`frame-integration.log`, `bogota-{interpreter,packed,single-return,unpacked,tco,inspected,jit,aot}.log`,
`compiled-lib-tests.log`, `native-frame-tests.log`, `frame-oracle-results.txt`,
`cli-linux-check.log`, `workspace-clippy-final.log`, `workspace-docs-final.log`, and `eest-inspectors-tests.log`. The final shared-cache runs write `bogota-jit-shared.log` and `bogota-aot-shared.log`; the initial per-file-cache AOT run was stopped after demonstrating slow repeated compilation.
They are local evidence, not published CI runs. The new workflow has not run on
GitHub because no branch has been pushed.

The first fixture pass exposed and fixed atomic links ending on POST_TX, account
reads during an unaffordable TXDIFF slot lookup, raw network wrappers being accepted
through fixture metadata, malformed 257-bit metadata stopping JSON ingestion, and
missing timestamp transitions. Invalid wire bytes remain authoritative; the
fixture parser does not truncate invalid values into acceptable transactions.

Focused tests in `crates/evm2/tests/frame_transactions.rs` cover isolated state gas,
the 200,000 fee-gas floor example, explicit sender, assertion rollback, canonical
signatures, keyed protocol storage, active custom caps, cross-frame restoration,
reverted ownership changes, atomic skipping, entry-only OutOfFunds, full-width fees,
recent-root slot/window bounds, BAL-positioned prior state, prefix boundary,
interpreted prefix observation, unwind/runner restoration, signature-before-read
ordering, fatal database propagation and all transaction-handle resolution modes
followed immediately by an ordinary transaction.

`crates/inspectors/tests/frame_transactions.rs` checks final assertion rollback and
skipped nodes. `crates/jit/runtime/src/evm2_evm/frame_tests.rs` executes all ten
instructions and ordinary SSTORE with JIT/AOT, cold/warm caches and persisted AOT
reload, including state exhaustion and assertion failure. The differential tool's
README describes its exact comparison fields and limitations.

The Linux compiler/runtime suites initially needed LLVM 23, its linker, zlib and
zstd development libraries. The compiler memory-counter test races with parallel
tests, so the full compiler/runtime suites were rerun serially and passed. Windows
MSVC cannot build this repository's default GMP backend; portable engine tests ran
on Windows, and compiled/default-feature checks ran in WSL Linux. macOS, cross
targets, Hive and live devnet interoperability have not been qualified here.

## Reth policy selection to retain

The local Reth checkout was inspected read-only at
`d8defd3f746c7bf84f43c2e348fe67d9978aca4a`. The following commits follow the original
`7bde440...` consensus baseline. Retain their policy/operational behavior during the
native port; none belongs in the VM consensus handler.

| Commit | Retained behavior and concrete consumers |
|---|---|
| `f1ec8db640ae8e154dea238291f30836a27ee4cf` | Admission before gossip, prefix/dependency/expiry/head/blob revalidation and factory value restrictions; `transaction-pool/src/validate/{frame_inspector,frame_state}.rs`, pool/maintain and network transaction paths |
| `ca2bd5b6bc91e97925d11377450509c87000c484` | Withdrawn keyed nonce domains/descendants and replacement fee policy; `pool/frame_revalidation.rs:104`, `pool/mod.rs:761`; value-bearing CREATE/CREATE2 rejection at `validate/frame_inspector.rs:214`, before an early balance failure can bypass create hooks |
| `70c953ecef6cdc0b1ea02d4c593c3f69c4d8efce` | Nonce-domain identity, queue cancellation and race-safe metadata/reservation updates; `pool/frame_revalidation.rs:249`, `pool/mod.rs:661`, and frame mempool regressions |
| `d8defd3f746c7bf84f43c2e348fe67d9978aca4a` | Configurable consecutive unpaid validation attempts, default 1; `transaction-pool/src/config.rs:38`, `node/core/src/args/txpool.rs:429`, payload behavior/regressions at `ethereum/payload/src/lib.rs:574` |
| `b6d261f229` | Hive build/cache-image maintenance remains infrastructure work; it changes no VM protocol behavior |

Paths in this table are under Reth `crates/`. Preserve observation even inside
reverted calls. In particular, CALL/CREATE hooks alone cannot implement the
CREATE/CREATE2 value restriction. Public prefix validation stays interpreted for
that reason. Later commits are a selected node-policy overlay, not an unannounced
change to the pinned consensus target.

## Remaining checklist scope

| Package | Status of this change and remaining work |
|---|---|
| P00 | Manifest, rollback contract, source/fixture pins and policy inventory supplied. Cross-repository API agreement remains open |
| P01 | Reuses locked shared type/signing/receipt implementations; external provider/network/compact-codec vector suites and their full graph remain open |
| P02–P10 | Native execution implemented and exercised by official/differential tests. Exhaustive multi-error/read-order, async/custom-host and all nested inspection matrices remain independent acceptance work where the checklist leaves them open |
| P11 | Ordinary verifier execution and actual-slot VM behavior implemented/tested. Node admission/dependency/expiry/reorg consumers remain open |
| P12 | Native interpreter/JIT/AOT paths, full corpus and ABI/reload tests pass locally; additional platform qualification remains open |
| P13 | Core, Geth/parity arena and debug/mux lifecycle implemented. JS frame lifecycle/output semantics and production RPC end-to-end trace tests remain open |
| P14 | Native Alloy EVM/block/database adapter and Reth EVM boundary not implemented. Current pinned adapter contracts expose revm types; replacing those interfaces is required before wiring Reth to these native results |
| P15–P17 | Native Reth pool, RPC/estimation, payload, networking and persistence migration not implemented. Retention requirements above are inventory, not a completed port |
| P18 | Engine BAL-positioned transaction regression and official serial BAL commitments pass. Node serial fallback remains a required integration gate; parallel frame enablement is not claimed |
| P19 | Pinned full-corpus runner, inspected/compiled modes, generated differential cases and CI supplied. Complete CLI frame fuzzing and external node/provider tests remain open |
| P20 | Local execution evidence supplied. Node/Hive/restart/reorg/inter-client, performance and additional-platform gates remain open |
| P21 | Conditional newer-draft migration intentionally excluded from the selected Bogota implementation |

No old revm/inspector patches have been removed from Reth. No frame-block serial
fallback has been removed. A migrated node release requires the unchecked external
packages and their actual integration tests; passing VM fixtures alone does not
close those gates.

## Reproduce

See [the differential runner README](../validation/frame-oracle/README.md) and
the [CI workflow](../.github/workflows/frame-conformance.yml). The conformance runner shares one backend/cache across selected files; every case still gets its own prestate/EVM. On this Windows
checkout the portable checks use `cargo +nightly` and `--target-dir target` because
the global Cargo target directory belongs to a different checkout. Linux commands
use `LLVM_SYS_231_PREFIX=/usr/lib/llvm-23` and `--target-dir target/wsl`.

Always keep both lockfiles and use `--locked`. The shared EIP dependency is declared
upstream through a branch but resolved here at `921ddebb...`. The oracle has an
additional revm-side Alloy dependency at `066ec9e2...`; that separate immutable
resolution is recorded in the manifest and its lockfile. It is not a second Alloy
revision in the native engine workspace.

## Line references for review

Line numbers refer to the current local working tree.

| File:line | Entry point |
|---|---|
| `crates/evm2/src/spec_id.rs:71` | `BOGOTA` |
| `crates/evm2/src/ethereum/mod.rs:58` | `Eip8141(` |
| `crates/evm2/src/ethereum/eip8141.rs:33` | `pub struct FrameGas` |
| `crates/evm2/src/ethereum/eip8141.rs:169` | `fn prepare_payload` |
| `crates/evm2/src/ethereum/eip8141.rs:326` | `fn validate_structure` |
| `crates/evm2/src/ethereum/eip8141.rs:398` | `fn run<T` |
| `crates/evm2/src/ethereum/eip8141.rs:724` | `pub fn validate_frame_prefix` |
| `crates/evm2/src/evm/frame.rs:104` | `pub(crate) fn apply_frame_approval` |
| `crates/evm2/src/evm/frame.rs:254` | `pub(crate) struct FrameTransactionRuntime` |
| `crates/evm2/src/evm/frame_diff.rs:126` | `pub(crate) fn frame_diff_value` |
| `crates/evm2/src/evm/mod.rs:977` | `struct FrameSimulationGuard` |
| `crates/evm2/src/evm/tx.rs:158` | `pub struct FrameTransactionResult` |
| `crates/evm2/src/interpreter/gas.rs:176` | `pub const fn new_isolated` |
| `crates/evm2/src/interpreter/instructions/frame.rs:13` | `pub fn approve` |
| `crates/jit/builtins/src/frame.rs:9` | `pub unsafe extern "C" fn __revmc_builtin_approve` |
| `crates/jit/codegen/src/lib.rs:22` | `pub const ABI_VERSION` |
| `crates/jit/codegen/src/compiler/translate/mod.rs:1093` | `op::APPROVE =>` |
| `crates/jit/runtime/src/evm2_evm/frame_tests.rs:98` | `fn native_frames_match_interpreter` |
| `crates/inspectors/src/tracing/mod.rs:817` | `fn frame_transaction_end` |
| `crates/eest/src/fixture_io.rs:129` | `pub(crate) fn parse_json` |
| `crates/eest/src/execution.rs:155` | `struct ConformanceRunner` |
| `crates/eest/src/blockchaintest/execute.rs:509` | `let wire_transactions` |
| `crates/evm2/tests/frame_transactions.rs:563` | `fn unused_malformed_signature` |
| `crates/evm2/tests/frame_transactions.rs:598` | `fn every_transaction_resolution` |
