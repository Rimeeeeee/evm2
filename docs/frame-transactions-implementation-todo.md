# Frame transactions: full implementation TODO

Prepared 22 October 2026. This is an implementation backlog, not a claim that frame transactions have been implemented or tested.

## Checkout and scope

- [x] Clone the linked `Rimeeeeee/evm2` fork and check out `bogota-plan`.
- [x] Replace the former local personal-fork checkout, as requested. The working repository is `C:\Users\soubh\Desktop\Sreth\evm2`; `origin` is `https://github.com/Rimeeeeee/evm2.git`.
- [x] Read both proposal documents and inspect the relevant evm2 interfaces and pinned revm frame executor, nonce helpers, and assertion selectors.

Proposal commit: `ea45b124061e3f4de6dde8c7b4556c577340d324`. Its implementation base is `185be4b37d44f22cf81f71cf0568ea710b54a66d`, also the upstream `main` SHA observed during this review. The proposal commit adds documentation only. The base supports transaction types 0–4 and Amsterdam building blocks; it has no native implementation of the four frame EIPs.

**Planning assumption:** deliver exact Bogota devnet 1 compatibility first. A later coordinated draft update has a separate checklist. Finalize that choice in P00 before implementing protocol behavior. Never mix the two interpretations under one fork identity.

“Full implementation” here includes EIP-8141 execution, EIP-8250 nonces, EIP-7906 assertions, EIP-8272 integration, interpreter/JIT/AOT parity, inspectors, the Alloy adapter, and Reth admission/RPC/storage/block integration. Engine completion and an interoperable node are separate gates. Frame-containing blocks may use the existing serial fallback at the first node release; enabling BAL parallel execution has its own gate.

All unchecked items are future work. Paths below are repository-relative within the named component. New module/API names are proposals, not existing interfaces. Keep implementation in the normal Ethereum type family, follow revm semantics, and update consumers directly without transitional compatibility wrappers.

## Protocol versions that must stay separate

| Area | Bogota compatibility target | Pinned newer-draft target |
|---|---|---|
| Empty-message signature in signing payload | Replace signature bytes with an empty RLP byte string | Replace bytes with their length as an RLP integer |
| Signature-data gas | Price actual signature bytes as in the locked implementation | Empty-message signature pricing depends on length |
| Legacy nonce after keyed activation | Singleton `nonce_keys = [0]`; empty list invalid | Empty key list selects legacy; zero is an ordinary keyed domain |
| Maximum sequence | Reject `u64::MAX` | Nonempty keys plus maximum sequence select single-use sentinel behavior |
| Mode 3 | `PostTx` / `POST_TX` | `ASSERT` |
| Assertion opcodes | `TXTRACE=0xb6`, `TXDIFF=0xb7`, `EVENTDATACOPY=0xb8` | `TXTRACE=0xb6`, `EVENTDATACOPY=0xb7`; no `TXDIFF` |
| Assertion selectors | Deployment table; precharge/payer at `0x14`/`0x15` | Separate code/nonce tables; precharge/payer at `0x19`/`0x1a` |

The newer-draft column refers to the proposal's pinned EIPs commit, not a promise about future drafts. Bogota `TXDIFF` account/storage queries can read and warm state; its transaction-local queries differ. Do not apply the newer draft's captured-data-only rules to every Bogota introspection operation.

## Source pins and ownership

| Component | Immutable baseline | Responsibility |
|---|---|---|
| evm2 | `185be4b37d44f22cf81f71cf0568ea710b54a66d` | Execution, gas, journal, opcodes, native outcomes |
| Reth | `paradigmxyz/reth-oss@7bde440574a2afef4f718f32c02b03320f8d57c1` | Pool, RPC, block/payload policy, networking, persistence |
| Reference revm | `Soubhik-10/revm@b2109aeb7a69505617c629538e9014bd20148e5c` | Bogota differential oracle |
| Alloy adapter | `Soubhik-10/evm@5c35db18a87dd6122b7d1c0ab690d275fe13fa3a` | Generic EVM/block execution boundary |
| Alloy | `Rimeeeeee/alloy@1ff4bf799bc78b0913561fa3e44bc65d0444c638` | Envelopes, signing, receipts, RPC/provider types |
| EIP primitives | `Soubhik-10/eips@921ddebb486507d74808239b51e180e7600196d1` | Shared frame/signature/nonce/receipt definitions |
| Reth core | `Soubhik-10/reth-core@c42b71acb8e12de6b2b01238bbc1ce739510bdd3` | Primitive traits and compact codecs |
| Inspectors | `Soubhik-10/revm-inspectors@b736efa83f749ad15190ec3fbc3c95c6ca42d3cc` | Reference frame trace behavior |
| Newer EIP drafts | `ethereum/EIPs@46005d745a5d921702f12d64a1b85e712e2b6051` | Separate draft-update target |

The removed personal checkout had candidate work at `Soubhik-10/evm2@c4d675af51b74e12b43f00eb08701d266b875f2a`. Its executor/opcode/atomic-rollback tests may be useful port material. It predates this base, lacks the complete proposed suite, and uses interpreter fallback for frame opcodes. Retrieve individual pieces from that immutable commit if useful; do not treat it as a finished implementation or apply it wholesale.

## P00 — Freeze the protocol and implementation contracts

Owner: all components. Dependencies: none.

- [ ] **P00.01** Record the selected chain target, fork activation, envelope schema before/after keyed nonces, opcode bytes, selector tables, signing transformation, and nonce semantics in one checked-in protocol manifest.
- [ ] **P00.02** Pin the reference node, all patched dependency revisions, fixture release/archive checksums, deployed contract runtimes, and test-tool revisions. Keep live branch heads out of conformance comparisons.
- [ ] **P00.03** Agree the native full-execution, executable-prefix, result, state-stream, and inspector contracts with the Alloy adapter. Distinguish protocol invalidity, valid execution failure, and database/fatal errors.
- [ ] **P00.04** Specify rollback invariants for nested calls, top-level frames, atomic groups, assertion-body rollback, invalid transactions, prefix simulation, and EVM reuse. Include approvals, gas ownership, logs, warmth, and BAL access records.
- [ ] **P00.05** Confirm whether EIP-7819 `SETDELEGATE` is required by the deployment target. Neither the proposal's evm2 base nor its pinned revm guarantees that opcode; track it separately if required.
- [ ] **P00.06** Inventory later Reth mempool-policy fixes against the pinned devnet branch. Select which policy fixes to retain without silently changing consensus behavior.

Acceptance: every later package names the same protocol manifest; unresolved API choices are resolved before their consumers are written.

## P01 — Shared payload, signing, receipt, and dependency support

Owner: Alloy consensus/EIPs/RPC/network/provider and reth-core. Dependencies: P00.

Files: Alloy `crates/consensus/src/transaction/{eip8141,envelope,typed,tx_type,pooled}.rs`, `crates/consensus/src/receipt/`; EIPs `crates/eip8141/src/`; RPC `crates/rpc-types-eth/src/transaction/`; reth-core `crates/codecs/src/alloy/`.

- [ ] **P01.01** Align a coherent dependency graph exposing type `0x06`, frames, dual limits, signatures, full-width fees, optional blob fields, nonce payloads, statuses, and receipts. Audit duplicate versions/features and lockfile provenance.
- [ ] **P01.02** Complete canonical RLP encode/decode and schema activation rules, including empty versus fixed-width address/message fields, canonical integers, bounds, trailing fields, malformed lists, and rejected legacy outer fields.
- [ ] **P01.03** Implement the selected canonical signing payload and transaction hash in the shared consensus layer. Test transaction-hash witnesses, explicit-message signatures, mixed schemes, signature substitution, and exact published/locked hash vectors.
- [ ] **P01.04** Preserve the payload's explicit sender throughout sealing, generic `Recovered` wrappers, builders, and submission. A frame envelope has no new outer ECDSA signature or recovered-sender requirement.
- [ ] **P01.05** Preserve U256 fee/chain fields through supported interfaces; document any chain-configuration limits explicitly. Test values above u64/u128 and reject overflow before narrowing.
- [ ] **P01.06** Complete frame receipt consensus encoding, ordered status/gas/log fields, bloom behavior, receipt root inputs, and compact codecs. Keep outputs/debug fields outside wire encodings unless the chosen protocol includes them.
- [ ] **P01.07** Complete request/response serde, partial unsigned frame requests, frame transaction builders, blobless/blob-bearing pooled variants, and sidecar wrappers. Cross-check wire/JSON/compact round trips.
- [ ] **P01.08** Keep signing and wire definitions shared; place active-schedule intrinsic gas/reservation calculations in the engine boundary and cross-check adapter consumers against it.

Acceptance: shared vectors and round trips agree across all selected dependencies before VM parity is judged.

## P02 — Fork activation, native transaction entry, and typed outcomes

Owner: evm2. Dependencies: P00, P01.

Files: `crates/evm2/src/{spec_id.rs,ethereum/mod.rs,version/features.rs,version/mod.rs,version/opcode_config.rs,evm/registry.rs,evm/tx.rs,evm/env.rs,evm/config.rs}` and `Cargo.toml`. Proposed orchestration module: `ethereum/eip8141.rs`.

- [ ] **P02.01** Add the selected Bogota spec/configuration, parsing/display, variant counts, generic dispatch macros, version inheritance, opcode activation, and relevant precompile schedule. Prove Amsterdam and older behavior stays unchanged.
- [ ] **P02.02** Add the native frame envelope variant, conversions, accessors, and type-6 registry entry using `prepare` plus `execute`. Gate keyed-schema activation explicitly rather than inferring it from a nonempty payload.
- [ ] **P02.03** Audit `Transaction` delegation for scalar target/input/nonce/gas and narrow fee fields. Ensure the handler reads the original frame payload; no canonical fee or reservation depends on a truncated generic accessor.
- [ ] **P02.04** Add prepared frame context containing validated payload/hash, intrinsic/floor gas, execution/state reservations, effective price, blob charge, and transaction-start legacy nonce. Do not reuse ordinary sender precharge/nonce-bump preparation.
- [ ] **P02.05** Add native typed frame outcome data: payer/precharge, all ordered receipts and outputs, actual gas dimensions, assertion failure classification, and common transaction status. Preserve result extension mapping and commit/discard/detach APIs.
- [ ] **P02.06** Add deterministic invalid-transaction errors for schema, structure, signatures, approval, nonce, arithmetic, and missing payer. Preserve database/host failures as fatal errors instead of ordinary frame reverts.
- [ ] **P02.07** Check complete static structure and signatures before state-dependent reads where the baseline requires it. Add multi-error/read-order regressions, including missing or malformed BAL data.
- [ ] **P02.08** Integrate standard `BaseEvmTypes`, sync/async entrypoints where supported, configuration overrides, and custom precompiles. Do not require a private Reth-only EVM family for standard frame execution.

Acceptance: pre-fork type 6 is rejected; preparation executes no user frames; native outcome and error contracts are ready for all backends.

## P03 — Transaction runtime and complete checkpoint discipline

Owner: evm2 state/host. Dependencies: P02.

Files: `crates/evm2/src/evm/{mod.rs,state/mod.rs,state/journal.rs,state/account.rs,state/tracked.rs}`. Proposed modules: `evm/frame.rs` and an in-flight diff module.

- [ ] **P03.01** Store immutable payload separately from mutable current-frame context, approval state, payer/precharge, legacy nonce snapshot, ordered outcomes, and state-gas ownership records.
- [ ] **P03.02** Journal approval effects at nested call boundaries. Commit them only on successful child/root return; parent revert, exceptional halt, or inspector override must undo the matching effects.
- [ ] **P03.03** Define coordinated state/runtime checkpoints for a frame, an atomic group, and the execution-body boundary. Each rollback restores approval and receipt/gas ownership mutations as well as account/storage state.
- [ ] **P03.04** Preserve transaction-start account/storage originals across top-level frames. Earlier accepted transactions in the EVM overlay must be part of this transaction's prestate.
- [ ] **P03.05** Reset frame memory, return-data state, and transient storage at the protocol boundary; retain transaction-shared persistent state, originals, and warmth according to the chosen revision.
- [ ] **P03.06** Implement the baseline's warmth rollback rules separately from BAL access recording. Atomic rollback restores the proper warmth snapshot while access evidence needed by BAL remains available.
- [ ] **P03.07** Clear frame runtime, checkpoints, ownership/diff caches, and interpreter frame stacks on success, error, discard, dropped transaction handles, and prefix exits. Test a normal transaction immediately after each path.
- [ ] **P03.08** Delay final account destruction and transaction-original cleanup until assertions finish. Preserve existing state-stream ownership instead of committing directly into the backing database.

Acceptance: targeted rollback tests compare state, approvals, logs, warmth, ownership, and access records after every exit class.

## P04 — Independent gas pools, ownership, refund, and settlement

Owner: evm2 gas/state and JIT gas paths. Dependencies: P02, P03; compiled verification also requires P12.

Files: `crates/evm2/src/interpreter/gas.rs`, SSTORE/CALL/CREATE instruction paths, `ethereum/eip8141.rs` (new), `evm/tx.rs`; `crates/jit/{builtins/src/gas.rs,builtins/src/lib.rs,context/src/lib.rs}`.

Use these independent quantities, with checked arithmetic and the active schedule:

```text
I = intrinsic execution gas; F = calldata floor
E = sum of declared frame execution limits; S = sum of declared state limits
execution reservation = max(I + E, F)
state reservation = S
maximum gas = max(I + E, F) + S

net state used = sum of final attributed frame state gas
fee gas = max(total spent - capped refund - net state used, F) + net state used
block execution used = max(total spent - net state used, F)   # before refund
block state used = net state used
```

- [ ] **P04.01** Add an isolated-state gas mode/constructor for frame execution. State exhaustion must fail even with abundant execution gas; preserve ordinary EIP-8037 reservoir spill for ordinary transactions.
- [ ] **P04.02** Propagate pool mode, budgets, returned gas, and rollback through nested CALL/CREATE, stipends, account creation, code deposit, delegation handling, and precompile/early-failure paths.
- [ ] **P04.03** Compute intrinsic gas, signature work, calldata/nonce pricing, value costs, floor, and reservation from the selected revision. Check sums/products and apply the transaction cap to the correct execution reservation dimension.
- [ ] **P04.04** Track the frame that funded each state-growth charge. A later slot restoration reduces that owner's attributed usage; it must not create spendable state gas for the restoring frame.
- [ ] **P04.05** Journal ownership/refill changes across nested calls, frame failures, atomic groups, and assertion rollback. Cover charge/restore/recreate sequences and reverted restorations across different top-level frames.
- [ ] **P04.06** Preserve signed refund deltas across frames, remove rolled-back contributions, and clamp/cap once at transaction settlement. Test a later frame reversing an earlier refund.
- [ ] **P04.07** Debit maximum gas times the full-width max fee plus blob gas at the block blob base fee when payment is approved. Enforce the blob fee cap separately; reject checked-payment overflow.
- [ ] **P04.08** Settle once at effective gas price; return excess to the payer and apply beneficiary/base-fee/blob accounting under the inherited rules. Assertions observe precharge before final settlement.
- [ ] **P04.09** Make fee/receipt gas explicitly frame-aware in native results and every adapter/RPC/estimation consumer. Test `spent=180000, state=100000, floor=100000, refund=0` produces fee gas `200000`.
- [ ] **P04.10** Test execution-only and state-only exhaustion, zero budgets, floor-binding payment, refunds, custom schedules, and maximum U256 fee arithmetic. Compare pre-refund block capacity separately from charged fee gas.

Acceptance: one authoritative reservation/settlement calculation agrees across engine, prefix exposure, block admission, receipt building, and RPC.

## P05 — Canonical ordered frame executor

Owner: evm2. Dependencies: P02–P04; uses P06–P10 as those packages land.

Files: proposed `crates/evm2/src/ethereum/eip8141.rs`; existing `evm/mod.rs` message execution and state APIs. Reference: pinned revm `crates/handler/src/eip8141.rs`.

- [ ] **P05.01** Validate frame count `1..=64`, modes, address encodings, reserved flag bits, scope restrictions, allowed values, expiry fields, blob fields, atomic links, and trailing assertion shape before execution. Preserve baseline validation ordering.
- [ ] **P05.02** Resolve empty targets to the explicit sender. Set caller and frame-wide `ORIGIN` for DEFAULT/VERIFY/SENDER/POST_TX modes, including every nested depth; prevent SENDER execution before execution approval.
- [ ] **P05.03** Warm only the initial protocol-required accounts. Charge each frame target's entry access before subsequent balance/code/delegation reads; do not prewarm all targets or `ENTRY_POINT`.
- [ ] **P05.04** Execute each frame with fresh frame-local resources and its declared independent budgets through existing message execution. Reuse active precompiles, ordinary code, delegation resolution, and eligible default-account verification.
- [ ] **P05.05** Preserve protocol order for access charges, affordability, value transfer/account growth, code resolution, and dispatch. Test insufficient entry gas/funds, empty code, delegated code, and fatal database failures.
- [ ] **P05.06** Make any failed VERIFY invalidate and discard the whole transaction. Treat ordinary frame revert/halt as frame failure with the correct gas and continuation rules; require a payer before successful finalization.
- [ ] **P05.07** Implement atomic groups using the flag that links a frame to its successor. Reject invalid terminal/VERIFY/assertion/scope combinations. Failure rolls back the whole group and skips its unexecuted remainder.
- [ ] **P05.08** Emit one receipt/output position per declared frame. Preserve already executed frames' statuses/execution gas after group rollback, clear the prescribed logs/state gas/refunds, and emit status-2 zero-gas skipped entries.
- [ ] **P05.09** Capture the body checkpoint immediately after the first successful payer-setting frame. Share the same canonical loop with full execution and executable-prefix simulation.
- [ ] **P05.10** Finalize fees, delayed destruction, state streams, receipt gas attribution, and runtime cleanup once. Never implement frames by repeatedly calling ordinary `transact()`.

Acceptance: a mixed multi-frame transaction matches the reference for validity, ordered outcomes, state, gas, and error ordering, including atomic failures and entry failures.

## P06 — Signature verification, default accounts, and APPROVE

Owner: evm2. Dependencies: P01–P05.

Files: proposed `ethereum/eip8141.rs`, `interpreter/instructions/frame.rs`, frame runtime, host implementation; existing secp256k1/P-256 crypto modules.

- [ ] **P06.01** Validate every protocol signature before frames, including entries unused by a selected prefix. Implement exact scheme lengths, signer resolution, message rules, curve checks, and low-s constraints.
- [ ] **P06.02** Preserve ARBITRARY witnesses for application verification; enforce their protocol structure without inventing protocol cryptographic verification. Test unused malformed signatures still fail.
- [ ] **P06.03** Implement eligible default-account verification/self-payment/sponsorship from the baseline. Preserve differences among empty accounts, contract senders, active precompiles, and EIP-7702 delegation.
- [ ] **P06.04** Implement APPROVE operand order, memory expansion, return bytes, allowed scope, current/resolved target checks, and successful exit of the current EVM call.
- [ ] **P06.05** Apply execution approval, payer assignment, nonce consumption, state-gas charges, and precharge atomically at the correct approval point. Test missing/insufficient payment and repeated/partial approval scopes.
- [ ] **P06.06** Support the VERIFY static-context exception only where the selected protocol permits it. POST_TX/assertion mode receives no approval exception; ordinary static calls retain their restrictions.
- [ ] **P06.07** Prove a successful nested approval survives a successful parent, and disappears when that child/parent/root frame reverts or halts. Include returned data, empty data, and nested delegation cases.
- [ ] **P06.08** Keep signature-placeholder allowances in explicit simulation configuration. They must be disabled for block import, payload validation, and public propagation.

Acceptance: default-code and bytecode verification produce equivalent approval facts and rollback behavior where the protocol defines equivalence.

## P07 — Frame introspection instructions and host contract

Owner: evm2 interpreter/host. Dependencies: P02–P06.

Files: `crates/evm2/src/interpreter/{opcode.rs,host.rs,instructions/mod.rs}`, proposed `instructions/frame.rs`, `version/{mod.rs,opcode_config.rs}`, and the EVM host implementation.

| Instruction | Byte in both selected versions | Required coverage |
|---|---|---|
| APPROVE | `0xaa` | Returning/halting behavior, scopes, memory and rollback; P06 |
| TXPARAM | `0xb0` | Payload/hash/fees/nonce selectors; legacy snapshot and keyed selectors |
| FRAMEDATALOAD | `0xb1` | Frame/data indexing and exact load boundary rules |
| FRAMEDATACOPY | `0xb2` | Source range, destination memory expansion and copy gas |
| FRAMEPARAM | `0xb3` | Complete frame selector/index table |
| SIGPARAM | `0xb4` | Complete signature selector/index table |
| SIGDATACOPY | `0xb5` | Signature bytes, source range, expansion and copy gas |

- [ ] **P07.01** Add metadata, stack effects, static/dynamic gas, names, fork mapping, exports, and explicit host methods for every instruction in the table.
- [ ] **P07.02** Implement every selector from the pinned protocol tables, including empty target/signer interpretation, current frame context, full-width fees, and transaction-start legacy nonce.
- [ ] **P07.03** Check full U256 indices/selectors/offsets and checked range addition before narrowing. Test oversized operands, invalid selectors, empty data, exact-end access, and zero-length copies with invalid sources.
- [ ] **P07.04** Implement each instruction's own padding/bounds rules; do not assume all copies behave like CALLDATACOPY. Charge memory/copy gas in the baseline's order.
- [ ] **P07.05** Define behavior outside a frame transaction and before activation. Include ordinary transactions at the newer fork, nested calls, and static execution.
- [ ] **P07.06** Add per-opcode positive/negative vectors and parity tests across all interpreter dispatch tables. Add EIP-7906 instructions through P10 and compiled translations through P12.

Acceptance: the complete selector/operand/gas table is executable and tested; no instruction silently truncates a U256 operand.

## P08 — Executable validation-prefix API

Owner: evm2, consumed by Alloy/Reth. Dependencies: P05–P07, P09 when keyed nonces are active.

Files: `crates/evm2/src/evm/{mod.rs,registry.rs,inspector.rs}`, proposed frame executor. Proposed API name: `validate_frame_prefix`.

- [ ] **P08.01** Add an API that validates the complete static payload/signatures, executes the requested policy-selected prefix, and stops at successful payment approval using the canonical frame loop.
- [ ] **P08.02** Reject empty/out-of-bounds prefixes, unsuccessful prefix frames, missing payer, and a caller's requested boundary that disagrees with the actual payable boundary. Execute no frame after payment approval.
- [ ] **P08.03** Return payer, maximum exposure, actual exclusive prefix boundary, sender approval, signature work, execution gas, and state gas in native typed metadata.
- [ ] **P08.04** Discard state, precharge, nonce consumption, approvals, and runtime on success and all error paths. Leave accepted prior state intact and test EVM reuse immediately afterwards.
- [ ] **P08.05** Preserve configured precompiles and inspector hooks, including synthetic default verification, early entry failures, nested/reverted calls, and fatal errors. Keep policy observations even when the observed call reverts.
- [ ] **P08.06** Keep policy shape/bans supplied by Reth. Use interpreted execution for admission until a compiled backend can prove equivalent per-opcode observation; a missing hook must never silently permit admission.
- [ ] **P08.07** Document that existing `validate_tx()` performs preparation only and does not replace executable validation. Test full/prefix preparation and exposure calculations agree exactly.

Acceptance: prefix simulation is read-only to its caller yet reports the same payer/approval/gas facts as the corresponding beginning of full execution.

## P09 — EIP-8250 nonce domains and approval-time consumption

Owner: evm2 state/host plus shared EIP helpers. Dependencies: P01–P06.

Files: proposed `crates/evm2/src/ethereum/eip8250.rs`, frame host/runtime, `evm/state/` protocol-storage operations; EIPs `crates/eip8141/src/nonce.rs`. Reference: pinned revm `context/src/context.rs` and `context/src/journal.rs`.

- [ ] **P09.01** Match schema presence to activation. For Bogota, reject empty key lists, accept `[0]` as legacy, forbid zero combined with other keys, require sorted unique keys, enforce at most 16, and reject maximum sequence.
- [ ] **P09.02** Use the shared sender/key storage-key and key-set-hash helpers. Read every selected domain against transaction prestate before any frame executes; reject any mismatch before consuming a domain.
- [ ] **P09.03** Snapshot the transaction-start legacy nonce for introspection even with keyed lanes. Do not increment legacy or keyed nonces during preparation.
- [ ] **P09.04** At payment approval, determine all first-use charges and ensure the active frame's state pool can fund the entire set. Atomically update every selected domain, payer precharge, and approval metadata.
- [ ] **P09.05** Increment the current legacy account nonce when consuming the legacy domain; prior deploy/CREATE activity may have changed it. Consume keyed sequences exactly as the selected Bogota schema requires.
- [ ] **P09.06** Implement protocol storage reads/writes that record BAL evidence without accidentally warming ordinary user SLOAD/SSTORE access. Keep nonce storage in the deployed `NONCE_MANAGER` account.
- [ ] **P09.07** Obtain keyed access and first-use state costs from the active schedule. Cross-check the pinned schedule's 12,100 execution and 97,920 first-use state gas and the legacy account-creation charge.
- [ ] **P09.08** Preserve successfully approved nonce consumption across later assertion-body rollback; undo it with an approving call/frame rollback or whole-transaction invalidity. Cover partial-funding failure with no surviving key writes.
- [ ] **P09.09** Add nonce selectors/calldata pricing and tests for first/subsequent use, overlaps/disjoint sets, block ordering, delegation/deployment effects, custom schedules, and unchanged cold-access costs after protocol reads.
- [ ] **P09.10** Coordinate genesis/deployment of the ordinary nonce-manager contract. Do not install it as a VM precompile or add unrequested fork-boundary state writes/checks.

Acceptance: replay-domain state, approval rollback, gas, and BAL records match the Bogota baseline; draft single-use semantics remain in P21.

## P10 — EIP-7906 assertion suffix and consensus introspection

Owner: evm2 state/runtime/interpreter. Dependencies: P03–P07, P09.

Files: proposed frame/diff modules, `evm/state/{mod.rs,account.rs,tracked.rs}`, `evm/tx.rs`, `interpreter/{host.rs,opcode.rs,instructions/frame.rs}`, and version tables.

- [ ] **P10.01** Implement contiguous trailing `PostTx` mode-3 frames. Enforce read-only execution throughout nested subtrees and reject atomic groups containing or crossing into the suffix.
- [ ] **P10.02** On assertion revert/halt, restore the body boundary after the payer-setting frame. Retain validation-prefix effects, payer/nonce approval and payment obligations; remove the body's prescribed state/log/refund/output effects.
- [ ] **P10.03** Preserve executed frames' statuses and execution gas under the baseline's rollback rules. Keep the failed assertion's outcome and skip remaining assertions with status 2, zero gas, empty logs/outputs.
- [ ] **P10.04** Build transaction-prestate/net-change views from the in-flight overlay and captured originals. Use accepted prior state and BAL-positioned prestate; never substitute the older backing database or post-finalization `account_changes()`.
- [ ] **P10.05** Implement Bogota `TXTRACE` selectors `0x00..0x15`: balances, storage slots, deployments, events/topics/data lengths, actual precharge, and payer. Reproduce the selected baseline's enumeration and lifecycle rules.
- [ ] **P10.06** Implement Bogota `TXDIFF=0xb7` and its complete selector table. Preserve live account/storage query host reads, warmth and gas where specified; transaction-local index/count queries must follow their separate rules.
- [ ] **P10.07** Implement Bogota `EVENTDATACOPY=0xb8`, exact event/source bounds, checked U256 range addition, memory expansion, and copy gas. Add zero-length, overflow, bad-index, and end-of-buffer vectors.
- [ ] **P10.08** Define deterministic numeric address/slot ordering, net-change collapse, return-to-original omission, nonexistence, deployment, delegation-byte hashing, nonce changes, and SELFDESTRUCT observation under the selected tables.
- [ ] **P10.09** Include canonical protocol transfer logs and precharge in the proper observation view; assertions execute before final fee refund/reward and delayed destruction cleanup. Keep event emission order.
- [ ] **P10.10** Retain canonical consensus event data even when optional debug/log output collection is disabled. Avoid live reads for operations defined only over captured data, and preserve BAL access records after rolled-back writes.
- [ ] **P10.11** Cache derived sorted/indexed views at the assertion boundary and invalidate them on rollback or relevant mutation. Do not re-sort the complete transaction diff on every indexed instruction.
- [ ] **P10.12** Port the pinned revm assertion/journal tests and add combined payer/deploy/keyed-nonce/slot-restoration/atomic/assertion cases. Prove enabling or disabling inspectors never changes the assertion result.

Acceptance: assertion outcome and surviving state match the selected protocol independently of debug tracing, with exact receipt, gas, and event behavior.

## P11 — EIP-8272 recent-root verification

Owner: shared EIPs, ordinary evm2 execution, Reth environment/pool, chain deployment. Prerequisites: P07–P09 for engine/shared work; close node-specific items together with P14–P15.

Files: Alloy `crates/eips/src/eip8272.rs`; evm2 `evm/env.rs`, existing `interpreter/instructions/block.rs`; Reth `ethereum/node/src/frame_validation.rs`, pool `validate/{frame_policy.rs,frame_inspector.rs}` and dependency tracking.

- [ ] **P11.01** Preserve canonical verifier address/runtime/code hash, domain-separated hashes, 8,192-entry ring, 8,191 usable window, 16-reference maximum, and 72-byte tuple encoding with the published reference vector.
- [ ] **P11.02** Execute the verifier as an ordinary deployed contract/VERIFY frame. Reuse STATICCALL/storage/gas and existing SLOTNUM; add neither a new envelope field nor a special precompile/opcode.
- [ ] **P11.03** Propagate actual payload `slotNumber` into block execution. Pool simulation uses canonical-head slot plus one; payload inclusion revalidates using the actual payload slot, never timestamp-derived or wall-clock slot estimates.
- [ ] **P11.04** In Reth, recognize optional expiry then optional recent-root verification before deployment/account validation. Validate shape before sender lookup; apply exact-runtime, exact-top-level-address, declared-SLOAD/SLOTNUM policy exceptions only.
- [ ] **P11.05** Record slot/hash/first-expired-slot and code/deployment dependencies. Revalidate changes on both sides of reorgs, including removal without a replacement write, and remove metadata atomically on replacement/eviction.
- [ ] **P11.06** Test ages 0/1/8191/8192, ring overwrite, multiple updates in a slot, absent entries, duplicate tuples, code mismatch, nested/delegated bypass attempts, predeployment behavior, expiry, and actual-slot inclusion failure.

Acceptance: ordinary consensus execution and public-pool recognition agree with the pinned verifier behavior; no policy exception leaks to arbitrary contracts.

## P12 — JIT/AOT and every interpreter dispatch backend

Owner: evm2 JIT/interpreter. Dependencies: P04–P10. Follow `crates/jit/AGENTS.md`.

Files: `crates/jit/codegen/src/{bytecode/,compiler/mod.rs,compiler/translate/mod.rs,lib.rs}`, `builtins/src/{lib.rs,gas.rs}`, `context/src/lib.rs`, `runtime/src/{evm2_evm.rs,runtime/storage.rs}`.

- [ ] **P12.01** Register new fork/opcode metadata in analysis and all interpreter dispatch forms. Classify APPROVE as a returning/halting instruction with correct stack/gas/control-flow effects, including invalid pre-fork execution.
- [ ] **P12.02** Implement compiled translations/builtins for every frame and selected assertion opcode. Maintain identical full-width operand checks, host interaction, memory safety, gas order, and stop reasons.
- [ ] **P12.03** Supply current frame context, payload, approvals and dynamic origin to compiled code. The same code hash must execute correctly in VERIFY, SENDER, DEFAULT and assertion contexts.
- [ ] **P12.04** Port isolated gas and ownership hooks to compiled SSTORE/CALL/CREATE/account creation/code deposit paths. Test ordinary bytecode containing no frame opcode inside a frame transaction; it still requires frame gas semantics.
- [ ] **P12.05** Audit cached environment pointers and field offsets in the shared execution context. Refresh correctly between top-level frames and after memory changes; preserve nested-call lifetimes.
- [ ] **P12.06** Update cache/spec identity and bump ABI version when layouts or generated-code semantics change. Reject old persisted artifacts and test cache reuse across fork boundaries and frame modes.
- [ ] **P12.07** Implement or explicitly constrain inspection support. Keep public-prefix execution interpreted until compiled opcode observation covers reverted calls and all policy hooks; do not count fallback as full compiled parity.
- [ ] **P12.08** Run the same semantic corpus through `auto`, `tco`, `packed`, `single_return`, `unpacked`, JIT and AOT. Record platform-unavailable backends as untested and run them on supported CI workers.
- [ ] **P12.09** Test cold/warm compiled caches, persisted AOT reload, approval exits, huge operands, static mode, state-only exhaustion and body rollback. Remove temporary opcode-based fallback only after these gates pass.

Acceptance: compiled execution actually handles frame transactions and matches interpreter state, receipts, outputs, gas dimensions, and errors.

## P13 — Frame-aware inspection and trace finalization

Owner: evm2 core hooks, `evm2-inspectors`, Reth trace callers. Dependencies: P05–P10, P12 for compiled observation.

Files: `crates/evm2/src/evm/{inspector.rs,mod.rs}`; `crates/inspectors/src/tracing/{mod.rs,types.rs,builder/geth.rs,builder/parity.rs}`, JS/debug/mux tracer paths.

- [ ] **P13.01** Expose frame index/mode/target and lifecycle hooks for code, precompiles, synthetic default verification, entry failure, nested calls, and execution completion.
- [ ] **P13.02** Add the synthetic transaction root and ordered top-level frame nodes with correct depth, caller/origin, output, execution/state gas, and log attribution.
- [ ] **P13.03** Reconcile final canonical receipts with trace nodes for skipped, early-failed, atomic-rolled-back and assertion-rolled-back frames. Final receipt attribution can change after a frame finishes.
- [ ] **P13.04** Wire final-result reconciliation into every adapter/RPC trace path, not just inspector unit tests. The proposal found no production caller for the reference inspector's finalization method; verify the new path end to end.
- [ ] **P13.05** Port Geth, parity, JS, opcode/debug and mux behavior. Keep optional debug truncation independent of consensus assertion/event data and keep admission-policy observation complete.
- [ ] **P13.06** Add RPC-level trace tests for default VERIFY, sponsored payment, precompile entry failure, nested reverted approval, skipped atomic frames and assertion rollback; compare nodes/logs with canonical outcomes.

Acceptance: traces represent the final transaction accurately, and every policy-relevant executed opcode is observed even inside reverted subcalls.

## P14 — Native Alloy execution adapter and Reth EVM boundary

Owner: Alloy execution adapter plus Reth Ethereum EVM integration. Dependencies: P01–P10, P13.

Files in `alloy-evm`: `crates/evm/src/{evm.rs,traits.rs,tx.rs,eth/mod.rs,eth/env.rs,eth/block.rs,eth/receipt_builder.rs,rpc/transaction.rs,block/}`. Reth: `crates/ethereum/evm/` and shared execution traits.

- [ ] **P14.01** Implement normal/inspected full execution and executable-prefix calls against evm2's native result/errors. Preserve configured precompiles, inspectors and simulation flags.
- [ ] **P14.02** Convert shared envelopes without losing explicit sender, dual limits, full-width fields, signatures, keyed payloads or blobs. Derive canonical schedule-aware reservations once and cross-check cached/RPC conversions.
- [ ] **P14.03** Map chain activation and actual block/payload environments, including slot number and blob parameters, into the selected engine version. Preserve historical fork behavior and system-call execution.
- [ ] **P14.04** Integrate database reads, accepted overlays, state streaming, detach/commit/discard, block accumulators, hooks and system calls using native evm2 ownership. Do not reconstruct a complete revm map to keep an old signature.
- [ ] **P14.05** Maintain independent execution/state reservation and used-gas counters in the block executor. Keep receipt cumulative fee gas, header/block gas and miner-fee accounting distinct under the selected schedule.
- [ ] **P14.06** Build shared/Reth frame receipts from native execution facts and ordered canonical logs; compute cumulative values at block scope, not in the transaction handler.
- [ ] **P14.07** Audit generic adapter/consumer assumptions about one target/output/nonce, scalar gas limits, ordinary status/bloom, and `revm::ResultAndState`. Update exposed interfaces directly.
- [ ] **P14.08** Work through the proposal's direct-backend-reference inventory for database/account/bytecode/errors/inspectors/state bundles. Track any upstream adapter work as an explicit dependency; do not assume it is already delivered.
- [ ] **P14.09** Test a mixed block of ordinary, blob, delegated, frame, keyed and failing-assertion transactions, plus system calls, with commit/discard/detach paths and state/receipt roots.

Acceptance: an adapter-driven block executes and persists native evm2 results with no missing generic backend integration, while ordinary transaction behavior remains compatible.

## P15 — Reth public pool, reservations, and revalidation

Owner: Reth. Dependencies: P08, P09, P11.01–P11.03, P13, P14. Complete P11's pool-policy/dependency items in this package.

Files: `crates/ethereum/node/src/{frame_validation.rs,node.rs}`, `crates/transaction-pool/src/validate/{frame_policy.rs,frame_inspector.rs,frame_state.rs,eth.rs}`, `ordering.rs`, `identifier.rs`, `traits.rs`, `pool/{mod.rs,txpool.rs,frame_revalidation.rs,best.rs}`, and `maintain.rs`.

- [ ] **P15.01** Replace revm-specific prefix calls with the native adapter API while pinning canonical header/state for each validation snapshot. Preserve deployment, nonce, payer solvency and dependency metadata.
- [ ] **P15.02** Port public-prefix shape/work limits, deploy/account/payment validation, opcode bans, expiry/recent-root exceptions, and helper/delegated-code dependencies to evm2 inspection.
- [ ] **P15.03** Keep public admission/gossip policy outside consensus. Prove a policy-ineligible but consensus-valid transaction still executes through private submission/imported blocks as intended.
- [ ] **P15.04** Preserve payer exposure, exclusive noncanonical-paymaster limits, reservations and eviction. Rejected replacement must leave the old transaction/reservation intact; changing payer transfers exposure atomically.
- [ ] **P15.05** Preserve full-width priority/replacement fees and blob bump rules. Audit all `(sender, nonce)` identity and best-iterator assumptions for the chosen keyed schema and one-public-pending-frame-per-sender policy.
- [ ] **P15.06** Retain head/reorg withdrawal/revalidation and cancellation handling. Prevent stale validation from resurrecting removed/replaced transactions or overwriting newer payer metadata.
- [ ] **P15.07** Record all relevant factory/helper/payer code/storage dependencies without changing EVM warmth. Reverted-call policy violations must still reject public admission.
- [ ] **P15.08** Reconcile the later policy fixes selected in P00, including unpaid-validation attempt bounds and propagation. Keep dependency-selective revalidation as an optimization after correctness.
- [ ] **P15.09** Test concurrent replacement/cancellation/head-change races, payer insolvency, fee overflow, dependency changes, recent-root expiry/reorgs, and public/private propagation flags.

Acceptance: pool admission, reservations and canonical-head updates reproduce the selected policy while consensus validity remains a separate result.

## P16 — Reth RPC, simulation, estimation, and nonce queries

Owner: Reth RPC plus Alloy RPC/provider. Dependencies: P08–P15.

Files: `crates/rpc/rpc-eth-api/src/{core.rs,helpers/state.rs,helpers/call.rs,helpers/estimate.rs,helpers/transaction.rs,helpers/trace.rs}`; `crates/rpc/rpc-eth-types/src/{simulate.rs,receipt.rs,error/}`; debug/trace handlers.

- [ ] **P16.01** Port keyed nonce queries at historical, selected and pending state with revision-correct defaults and errors; preserve distinction from ordinary scalar account nonce APIs.
- [ ] **P16.02** Implement frame simulation with full execution outcomes, prefix policy result, payer/exposure, per-frame outputs, dual gas, and canonical logs. Report consensus validity, execution success and public-pool eligibility separately.
- [ ] **P16.03** Preserve already-signed frame limits and witnesses. For unsigned requests, fill omitted execution/state limits using real execution before signing; do not binary-search an outer gas field that does not exist.
- [ ] **P16.04** Bound simulation resources and placeholder controls explicitly. Ensure malformed signatures, placeholders and simulation-only relaxations cannot enter normal consensus or propagated pool paths.
- [ ] **P16.05** Preserve explicit sender, nonce schema, actual slot/environment and fee widths during request completion/conversion. Keep arbitrary application witnesses intact during estimation.
- [ ] **P16.06** Return payer/frame receipts and globally indexed flattened logs consistently in transaction, receipt, simulation, filter, debug and trace responses. Invoke canonical trace finalization before serialization.
- [ ] **P16.07** Add raw request/response regressions for signed submission, unsigned filling, failing validation/assertions, skipped frames, keyed historical queries, blobs, custom fees, RPC caps and exact error mapping.

Acceptance: wallet/provider-built frames round-trip through submission, mining, querying, estimation, simulation and tracing with no backend-specific fields leaking into the public contract.

## P17 — Networking, receipts, persistence, and payload building

Owner: Reth networking/primitives/storage/payload plus shared codecs. Dependencies: P01, P14–P16.

Files: `crates/ethereum/{primitives/,payload/src/lib.rs,evm/src/receipt.rs}`, `crates/net/eth-wire-types/src/receipts.rs`, transaction fetcher/blobstore paths, `crates/trie/common/src/ordered_root.rs`, provider/static-file/history/export/pruning consumers named in the source inventory.

- [ ] **P17.01** Preserve type-6 envelope/body/pooled transport and sidecar presence rules. Apply KZG/blob storage/fetch tracking only to blob-bearing frames; reject malformed/missing/unexpected sidecars correctly.
- [ ] **P17.02** Implement frame receipt representation and RLP/EIP-2718/compact/compression/wire variants, including the selected bloom rules and ordered flattened logs.
- [ ] **P17.03** Use frame consensus receipt encoding for trie roots, engine receipt verification, static files, database reads and downloads. Test serialization parity across all retrieval paths.
- [ ] **P17.04** Preserve receipt/state outcomes in stages, execution bundles, history/ERA/ERE export/import, filters, log pruning, and receipt reconstruction.
- [ ] **P17.05** Apply per-dimension capacity checks and actual-payload-slot validation during payload building/import; use actual effective miner fees and correct cumulative/header gas. Preserve explicit sender in recovered wrappers.
- [ ] **P17.06** Test peer propagation, local/private submission, blobless/blob-bearing blocks, receipt roots, restart/retrieval, export/import and pruning/filter round trips against the baseline.

Acceptance: a frame transaction and its receipt remain consensus-identical across gossip, block inclusion, storage, restart and historical serving.

## P18 — BAL and ordered parallel execution

Owner: evm2 state/BAL, adapter block executor, Reth scheduler. Dependencies: P03, P09–P17.

Files: evm2 `evm/bal/bal_context.rs`, `evm/state/{pending.rs,stream.rs}`, `evm/tx.rs`, `examples/bal_parallel.rs`; Reth engine payload validator and `payload_processor/bal/execute.rs`.

- [ ] **P18.01** Retain the existing serial fallback for every frame-containing block in the initial migrated node. Prove serial execution records protocol nonce, payer, reverted-access and assertion data correctly.
- [ ] **P18.02** Position all nonce/payer/recent-root/assertion-before reads at the true transaction prestate with one transaction block-access index. State streams must include prefix writes surviving failed assertions.
- [ ] **P18.03** Before enabling parallelism, make each worker execute one entire transaction with independent reservation metadata. Top-level frames within a transaction remain strictly ordered.
- [ ] **P18.04** In the ordered consumer, check execution and state reservations before surfacing speculative execution errors. The first admission/validation/BAL error must match serial order.
- [ ] **P18.05** Reconstruct BAL and compare state/receipt roots, frame outcomes, gas dimensions, cumulative gas and global log indices for serial and parallel mixed blocks.
- [ ] **P18.06** Cover overlapping keyed lanes, reused payers, prior accepted writes, slot restoration, atomic/assertion rollback and malformed/missing BAL with multi-error precedence regressions.
- [ ] **P18.07** Extend `bal_parallel.rs` beyond legacy transfers and remove the frame-block fallback only in a separately reviewed change with correctness and performance evidence.

Acceptance: P18.01–P18.02 are required for the initial serial node. Claim parallel frame support only after P18.03–P18.07 pass.

## P19 — Fixture ingestion, differential runner, and CI coverage

Owner: evm2/EEST/CLI, node integration tests. Dependencies: P00–P17; build alongside engine work.

Files: `crates/eest/src/{tx.rs,types.rs,forks.rs,compiled.rs,blockchaintest/}`, `crates/eest/tests/eest.rs`, `crates/cli/src/` replay/capture/fuzzer paths, fixture setup scripts, `.config/nextest.toml`, `.github/workflows/ci.yml`.

- [ ] **P19.01** Add Bogota fork naming/mapping and frame fields to state/blockchain fixture parsers, including raw transaction bytes, signatures, dual gas, nonce schema and expected exceptions. Reject unsupported formats explicitly.
- [ ] **P19.02** Extend CLI replay/capture/export and fuzz transaction generation/conversion so type 6 can be reproduced and compared; remove exhaustive-match assumptions that only types 0–4 exist.
- [ ] **P19.03** Pin revision-matched valid/invalid fixtures and golden signing/encoding vectors. Separate Bogota and newer-draft corpora; a draft vector is not evidence that the old baseline is wrong.
- [ ] **P19.04** Build a reproducible differential runner against the locked revm/node: compare validity/errors, surviving account/storage state, payer, ordered receipts/outputs/logs, gas/refunds/reservations and roots.
- [ ] **P19.05** Port existing reference executor, journal, inspected-prefix, frame-adapter, provider/network, trace and Reth end-to-end tests. Link each high-risk invariant to concrete regression cases.
- [ ] **P19.06** Add generated sequences for approvals/reverts, atomic links, nonce sets, cross-frame state restoration and assertion lifecycle. Minimize and retain any discovered mismatch as a fixed regression.
- [ ] **P19.07** Enable interpreter, inspected, JIT and AOT discovery of the same frame corpus. Record counts, fixture SHA, skipped cases and backend availability; use full compiled subsets for qualification.
- [ ] **P19.08** Add CI jobs that run the new corpus rather than silently inheriting the default EEST exclusion or only a compiled smoke subset. Check supported no-default-feature/serde/custom-host configurations touched by the change.

Acceptance: reproducible failures identify the exact protocol revision, fixture, execution backend and differing consensus data.

## P20 — Release qualification and definition of done

Owner: all components. Dependencies: P00–P19; P18 parallel items only for a parallel-support claim.

- [ ] **P20.01** Run formatting, workspace lint/docs, targeted unit/integration suites, all interpreter dispatch variants and actual JIT/AOT execution on supported platforms. Record unavailable platforms/backends as outstanding work.
- [ ] **P20.02** Run full revision-matched frame EEST state/blockchain suites and compare with the pinned baseline. Explain every skip; parser rejection or interpreter fallback cannot masquerade as compiled conformance.
- [ ] **P20.03** Run Hive/devnet block import and payload production across clients. Compare invalid-block rejection, roots, receipts, fee/block gas, blobs, slots, and first-error ordering.
- [ ] **P20.04** Prove public admission/mining, private consensus-valid policy failures, reorg/revalidation, sponsorship, keyed nonces, assertions and recent roots through RPC and persisted node state.
- [ ] **P20.05** Run ordinary-transaction regression suites through Amsterdam and older forks; measure ordinary execution and diff-query/allocation overhead for changed hot paths. Avoid adding frame costs when frame support is inactive.
- [ ] **P20.06** Review simulation flag isolation, placeholder rejection, fatal database propagation, whole-transaction cleanup, and no stale runtime after dropped/discarded/error handles.
- [ ] **P20.07** Remove obsolete VM/inspector patches only after migrated consumers pass; retain shared protocol-type patches until upstream versions contain them. Publish final dependency/fork/fixture pins and known limits.
- [ ] **P20.08** Attach qualification evidence to each component change: commands, revisions, counts, results, root/gas comparisons, serial/parallel scope, and remaining exceptions. Leave any unproven acceptance checkbox open.

Useful verification commands after implementation exists (these were not run for this planning document):

```powershell
cargo fmt --all --check
cargo cl
cargo docs
cargo nextest run -p evm2
cargo nextest run -p evm2-inspectors
cargo nextest run -p evm2-jit-codegen --test codegen
cargo nextest run -p evm2-jit-runtime

# Ordinary EEST is excluded by the default nextest filter; include it explicitly.
cargo nextest run -p evm2-eest --test eest --ignore-default-filter

# On a supported LLVM/JIT/AOT worker, qualify the full selected corpus.
$env:EVM2_COMPILED_EEST_SUBSET = 'all'
cargo nextest run -p evm2-eest --features jit --test eest --ignore-default-filter
Remove-Item Env:\EVM2_COMPILED_EEST_SUBSET
```

Select the pinned fixture root/release before running those commands. Repeat the interpreter corpus with `EVM2_DISPATCH_BACKEND` set to each supported backend; passing a backend name alone does not prove that platform executed it. Use the repository's custom-root support to run only the chosen frame corpus during development and its full required regression corpus for qualification. Node/Hive commands must be recorded from the actual integrated node and fixture setup rather than invented here.

Completion gates:

| Claim | Required evidence |
|---|---|
| Engine supports frame transactions | P01–P10, P12–P13 and engine portions of P19/P20 pass with exact Bogota semantics |
| Node interoperates on Bogota | Engine gate plus P11, P14–P17, serial P18.01–P18.02, node/Hive qualification |
| Parallel frame blocks supported | Node gate plus every remaining P18 item and serial/parallel equivalence |
| Newer draft conformant | Separate P21 protocol update and its revised qualification; Bogota parity alone is insufficient |

## P21 — Separate coordinated newer-draft update

Owner: all components. Dependencies: explicit target selection, updated manifest and vectors. This package is conditional on choosing the newer draft; it is not part of unchanged Bogota interoperability.

- [ ] **P21.01** Pin the intended EIP revisions and introduce an explicit coordinated protocol identity/activation. Update all clients, tools and fixtures together; do not auto-detect competing semantics from payload contents.
- [ ] **P21.02** Change empty-message signature hash replacement to signature byte length encoded as an RLP integer. Price those signature bytes uniformly by length; update signing tools, caches, gas/reservations and substitution vectors.
- [ ] **P21.03** Change legacy selection to empty nonce keys, make zero an ordinary keyed domain, retain sorted uniqueness/max-key bounds, and update decode validation, RPC defaults, pool identity and nonce pricing together.
- [ ] **P21.04** Add nonempty-key maximum-sequence single-use validation: require all slots unused, write the sentinel atomically at approval, and apply its special sequence calldata-pricing exemption. Preserve legacy maximum-sequence rules separately.
- [ ] **P21.05** Rename mode 3 to ASSERT consistently, remove TXDIFF, move EVENTDATACOPY to `0xb7`, and implement all revised TXTRACE selectors through `0x1a`. Update compiler metadata, caches, JSON/client naming and fixtures.
- [ ] **P21.06** Add separate deterministic net balance/storage/code-hash/nonce tables, empty-account values and stored-delegation hashing. Ensure revised TXTRACE/EVENTDATACOPY read only captured data without live state/BAL/warmth effects.
- [ ] **P21.07** Re-run signing, nonce, assertion, gas, encoding, provider, pool/RPC, all-backend and inter-client qualification against the new corpus. Compare the old runtime only for behavior shared by both versions.

## Suggested reviewable landing order

These are change packages, not calendar estimates. Keep fixes for each new behavior and its meaningful regressions together.

| Order | Deliverable | Checklist scope / dependency |
|---|---|---|
| 1 | Protocol manifest, pinned oracle/corpus, native API agreement | P00; start P19 scaffolding |
| 2 | Shared type-6 graph and canonical vectors | P01 |
| 3 | Native fork/envelope/preparation/results and runtime journal | P02–P03 |
| 4 | Independent gas pools and state-growth ownership | P04; start matching P12 gas hooks |
| 5 | Ordered frame executor, signatures, APPROVE, introspection | P05–P07 |
| 6 | Executable prefix and keyed nonce transitions | P08–P09 |
| 7 | Assertion rollback and consensus diff/introspection | P10 |
| 8 | Full compiled support and canonical trace lifecycle | P12–P13; complete backend P19 coverage |
| 9 | Native adapter, environments, block/state/receipt integration | P14; EIP-8272 environment part of P11 |
| 10 | Pool/recent-root policy, RPC and retained node consumers | P11, P15–P17; retain serial P18 fallback |
| 11 | Differential EEST, node/Hive/devnet qualification | P19–P20 and serial P18 correctness |
| 12 | Ordered BAL parallel enablement | Remaining P18, separately qualified |
| Optional protocol update | Newer-draft signing/nonces/assertions | P21 with revised manifest and qualification |

First concrete implementation deliverable: P00 manifest plus P01 golden vectors and P02 native result/API contracts. Gas/rollback architecture should be reviewed before building downstream node conversions around it.

## Evidence and references

- [Migration proposal at the cloned commit](https://github.com/Rimeeeeee/evm2/blob/ea45b124061e3f4de6dde8c7b4556c577340d324/docs/bogota-devnet-1-evm2-migration.md): sections 3 and 5–15 support the protocol split, engine work, ownership and qualification gates.
- [Companion source inventory](https://github.com/Rimeeeeee/evm2/blob/ea45b124061e3f4de6dde8c7b4556c577340d324/docs/bogota-devnet-1-source-inventory.md): complete component/file map and direct backend coupling.
- [evm2 base transaction/result interfaces](https://github.com/alloy-rs/evm2/tree/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src): locally inspected to verify the missing type-6 path, ordinary gas formula, journal and prefix API gap.
- [Pinned reference frame executor](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs): preparation, frame loop, prefix cleanup, atomic/body checkpoints and settlement were inspected directly.
- [Pinned assertion opcode and selector definitions](https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/eip7906.rs): Bogota opcode layout and state-reading TXDIFF distinction.
- [Pinned nonce helpers](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/nonce.rs) and [shared constants](https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/constants.rs): legacy `[0]`, sorted key limits, hashing and protocol constants were inspected directly.
- [Newer-draft EIP snapshot](https://github.com/ethereum/EIPs/tree/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS): EIP-8141/8250/8272/7906 revision cited by the proposal; used here for the separately identified update scope.

Verification performed for this task: checkout/remote/commit and proposal-base checks, source inspection, and checklist/path validation. No Rust implementation, compilation, unit tests, EEST, Hive or devnet runs were performed.
