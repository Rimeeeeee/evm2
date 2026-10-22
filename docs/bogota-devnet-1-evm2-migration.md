# Bogota devnet 1: Reth → evm2 migration scope

Research date: 22 October 2026. This is a source-based migration assessment and implementation plan; it does not implement or certify the migration.

## 1. Recommendation

Implement frame transaction consensus behavior in **evm2**, expose it through the **Alloy execution adapter**, and retain node policy and infrastructure in **Reth**. Keep transaction/receipt encodings and shared frame types in **Alloy consensus/EIPs**. This is a coordinated migration across these layers, not a replacement of one Cargo dependency.

The largest engine changes are a transaction-level frame executor, revertible approval state, isolated state-gas budgets, state-gas ownership across frames, typed frame outcomes, and consensus-visible transaction diffs. Reth already contains substantial pool, RPC, persistence, sidecar, and payload integration that should survive the backend change.

There are two distinct targets:

1. **Bogota compatibility:** reproduce the exact behavior of `paradigmxyz/reth-oss/bogota-devnet-1` and its locked dependency forks.
2. **Current draft conformance:** implement the four EIPs at the specification revision pinned below.

They currently disagree on signing, nonce domains, assertion instructions, and introspection selectors. Establish Bogota parity first if the migrated node must join that devnet. Implement draft changes as an explicit coordinated protocol update, with corresponding Alloy, RPC, fixture, and client changes. Do not accept both incompatible interpretations under the same fork configuration.

**The current evm2 checkout has useful Amsterdam infrastructure but no implementation of the requested frame transaction suite.** It supports ordinary transaction types 0–4, EIP-8037 state gas, EIP-8038 repricing, EIP-7708 transfer logs, EIP-7843 `SLOTNUM`, BAL state access, and transaction state streaming. Those are building blocks, not frame transaction support.

Evidence: [crates/evm2/src/ethereum/mod.rs:40–79][s1], [crates/evm2/src/ethereum/mod.rs:225–280][s2], [crates/evm2/src/version/mod.rs:744–875][s3], [crates/evm2/src/interpreter/opcode.rs:451][s4].

## 2. Revisions and audit boundaries

The Reth branch head and upstream evm2 HEAD were verified against GitHub during this audit. The local Reth checkout is on `fix/eip8141-mempool-policy`, so its working-tree contents were deliberately excluded from the baseline. The requested Reth branch was exported by commit, and every patched dependency below was downloaded at its **Cargo.lock revision**.

| Component | Repository and immutable revision | Role |
|---|---|---|
| Reth baseline | `paradigmxyz/reth-oss` @ `7bde440574a2afef4f718f32c02b03320f8d57c1` | `bogota-devnet-1`, 14 October 2026 |
| evm2 target | `alloy-rs/evm2` @ `185be4b37d44f22cf81f71cf0568ea710b54a66d` | Matches this checkout's HEAD |
| Patched revm | `Soubhik-10/revm` @ `b2109aeb7a69505617c629538e9014bd20148e5c` | Actual VM behavior used by the Reth branch |
| Alloy execution adapter | `Soubhik-10/evm` @ `5c35db18a87dd6122b7d1c0ab690d275fe13fa3a` | `alloy-evm` 0.39.0 |
| Alloy | `Rimeeeeee/alloy` @ `1ff4bf799bc78b0913561fa3e44bc65d0444c638` | Consensus, RPC, provider, network types, version 2.4.1 |
| Frame primitives | `Soubhik-10/eips` @ `921ddebb486507d74808239b51e180e7600196d1` | `alloy-eip8141` 0.1.1 |
| Reth core | `Soubhik-10/reth-core` @ `c42b71acb8e12de6b2b01238bbc1ce739510bdd3` | Compact codecs and primitive traits |
| Inspectors | `Soubhik-10/revm-inspectors` @ `b736efa83f749ad15190ec3fbc3c95c6ca42d3cc` | Frame-aware tracing implementation |
| EIP documents | `ethereum/EIPs` @ `46005d745a5d921702f12d64a1b85e712e2b6051` | Current draft comparison, retrieved 22 October 2026 |

Dependency provenance: [Cargo.toml:700–754][s5] and [Cargo.lock][s6]. The many crate patches are one coordinated dependency graph; updating only `revm` or only `alloy-consensus` will not migrate it.

The Reth branch delta from merge base `040bef7b319c7ec145689e03fe78710e11986db6` contains **123 files, 9,240 additions, and 2,453 deletions**. This includes dependency churn and unrelated adaptation, so it is not an estimate of code to move. A separate search finds **107 Rust files** in the pinned Reth `crates/`, `bin/`, and `examples/` trees with direct `revm::`, `reth_revm::`, or `revm_inspectors::` references. That measures the broader backend integration surface, not 107 frame implementations.

The [companion source inventory](bogota-devnet-1-source-inventory.md) lists the entire branch delta, direct backend references, and feature-bearing source files across the locked repositories. The tables below identify responsibility and substantive edits.

## 3. Confirmed differences between Bogota and the current drafts

These are source comparisons, not hypothetical risks.

| Area | Locked Bogota implementation | Current EIP draft | Migration consequence |
|---|---|---|---|
| Canonical signing payload | An empty-`msg` signature's bytes are replaced with an empty RLP byte string | Replace them with their **byte length**, encoded as an RLP integer | Changes signature hashes. Update shared encoding, signing tools, caches, test vectors, and VM validation together |
| Intrinsic signature-data pricing | revm counts actual zero/nonzero bytes in every signature | Empty-`msg` signature bytes are priced uniformly by length | Gas/reservations must be invariant under substitutions of hash-elided bytes |
| Legacy nonce selection after EIP-8250 | `nonce_keys = [0]`; empty lists rejected | `nonce_keys = []`; key zero is an ordinary keyed domain | Changes decode validation, state selection, hashes, RPC defaults, pool identity, and gas |
| Key sequence maximum | `u64::MAX` rejected; keyed consumption always increments | Nonempty keys plus `MAX_NONCE_SEQ` mean single-use; require unused slots, then write the sentinel | New validity and approval branches; sentinel sequence encoding is exempt from calldata pricing |
| Assertion frame name | `PostTx = 3` / `POST_TX` | `ASSERT = 3` | Numeric mode is the same; semantics, names and JSON/client expectations need one deliberate version choice |
| EIP-7906 opcodes | `TXTRACE=0xb6`, **`TXDIFF=0xb7`**, `EVENTDATACOPY=0xb8` | `TXTRACE=0xb6`, **`EVENTDATACOPY=0xb7`**, no `TXDIFF` | `0xb7` executes a different operation. This is a bytecode incompatibility, not an API rename |
| `TXTRACE` selectors | Deployment table, no separate code-change and nonce-change enumeration; payer/precharge at `0x15`/`0x14` | Separate code and nonce changes; selectors through `0x1a`, payer/precharge at `0x1a`/`0x19` | Rebuild selector mapping and fixtures; copying the old opcode implementation is insufficient |
| Mempool work policy | Requested branch predates the active local policy-fix branch | EIP-8141 now describes a producer-local bound on consecutive unpaid validation attempts | Keep policy evolution in Reth, separately from VM parity |

Baseline evidence: [crates/consensus/src/transaction/eip8141.rs:27–59][s7], [crates/context/interface/src/transaction/eip8141.rs:83–111][s8], [crates/eip8141/src/nonce.rs:9–51][s9], [crates/handler/src/eip8141.rs:264–280][s10], [crates/context/src/context.rs:955–1027][s11], [crates/primitives/src/eip7906.rs:5–94][s12].

Draft evidence: [EIPS/eip-8141.md:249–262][s13], [EIPS/eip-8141.md:518–566][s14], [EIPS/eip-8250.md:58–102][s15], [EIPS/eip-8250.md:132–179][s16], [EIPS/eip-7906.md:32–118][s17], [EIPS/eip-8141.md:1150–1156][s18].

The pinned runtime is therefore a useful **differential oracle for Bogota**, but not a complete conformance oracle for today's documents. Preserve bluealloy/revm control flow and host interaction shape when implementing evm2 behavior, while applying the explicitly chosen EIP revision. Record deviations from the fork with revision-specific tests; do not add compatibility aliases or silently support both opcode layouts.

## 4. Ownership split

| Responsibility | Owner after migration | What crosses the boundary |
|---|---|---|
| Type `0x06`, frame/signature/fee/nonce fields, RLP, signing hash, sealed envelope | Alloy consensus + shared EIP crate | Typed transaction consumed by evm2 |
| Shared frame modes, statuses, receipts, recent-root hashing/constants | Alloy EIPs | Protocol data definitions, not node policy |
| Protocol signature checks, structure/stateful validity | evm2 | Typed invalid-transaction errors |
| Ordered frame execution, caller/origin, default code, `APPROVE` | evm2 | Frame execution outcome |
| Revertible approvals, atomic groups, assertion body rollback | evm2 | Committed state transition and ordered receipts |
| Frame gas pools, refunds, ownership, payer precharge/refund | evm2 | Explicit execution/state reservations and usage |
| Keyed nonce state reads and atomic consumption | evm2 state/host | Normal storage changes with protocol warming rules |
| `TXTRACE`, event reads, transaction prestate and diff cache | evm2 | Consensus-visible reads independent of debug inspectors |
| Interpreter/JIT/AOT instruction support | evm2 and its JIT crates | Same semantics and host calls across backends |
| Generic EVM/block executor integration, environment and transaction adaptation | Alloy execution adapter | Backend-appropriate types, state streaming and results |
| Public/private pool distinction, prefix shape, opcode restrictions | Reth transaction pool | Inspector/observer attached to evm2 prefix execution |
| Payer reservations, replacement, eviction, dependency indexing, head/reorg updates | Reth | Validation metadata and node storage snapshots |
| Which payload slot to simulate; chain fork activation and contract deployment availability | Reth/chainspec + adapter | Explicit fork/version and `BlockEnv.slot_num` |
| Block scheduling, ordered capacity checks, payload building, BAL worker orchestration | Reth + adapter block executor | Native evm2 results and detached state changes |
| RPC endpoints, request limits, nonce defaults, estimation, simulation presentation | Reth + Alloy RPC types | VM execution APIs and frame results |
| RPC trace trees and serializers | `evm2-inspectors` + Reth RPC | Core frame hooks and final canonical outcomes |
| Peer gossip, sidecar storage/KZG validation, receipt wire protocol | Reth networking + Alloy network types | Standard wire representations |
| Database/static files, compact codecs, pruning, receipt roots/history export | Reth + reth-core | Frame-aware consensus receipts and state changes |

In particular, **do not move the Reth public mempool inspector's bans into the consensus interpreter**. A consensus-valid transaction can be ineligible for public gossip; private transactions and imported blocks must still execute it. Conversely, nonce consumption and assertion diffs are consensus behavior and cannot be implemented solely in a Reth inspector.

## 5. evm2: concrete implementation scope

### 5.1 Entry points, types and fork activation

| Existing evm2 location | Current behavior | Required work |
|---|---|---|
| [crates/evm2/src/ethereum/mod.rs:40–79][s1] | Local unsigned envelope has variants 0–4 | Add the native frame transaction variant and shared Alloy conversion; frame sender must come from the explicit payload |
| [crates/evm2/src/ethereum/mod.rs:137–223][s19] | Delegates the ordinary `Transaction` interface | Audit single-target/single-input/fee-width assumptions; keep full-width frame fields available to the handler |
| [crates/evm2/src/ethereum/mod.rs:225–280][s2] | Fork-gated registry entries for 0–4 | Register type `0x06` under the chosen fork |
| [crates/evm2/src/evm/registry.rs:187–236][s20] | `prepare` then `execute`; validation drops prepared output | Add a frame-specific prepared context and typed errors; share preparation with full/prefix execution |
| [crates/evm2/src/evm/mod.rs:969–1017][s21] | Ordinary execution/discard and validation-only APIs | Add executable prefix validation; `validate_tx()` alone does not execute the validating contracts |
| [crates/evm2/src/evm/config.rs:22–55][s22] | Type family offers EVM/env/result extensions | Put standard Ethereum frame support in the standard execution path; do not require consumers to invent a custom EVM family |
| [crates/evm2/src/spec_id.rs:62–99][s23] | Latest base spec is Amsterdam | Add the selected Bogota/fork configuration, update parsing, generic dispatch, version tables and counts |
| [crates/evm2/src/version/features.rs][s24] and [crates/evm2/src/version/mod.rs:744–875][s3] | Amsterdam gas/opcode/features already exist | Explicit activation for frame instructions, assertion behavior and keyed schema; do not turn them on retroactively for Amsterdam |
| [Cargo.toml:61–80][s25] | Alloy dependency versions start at 2.0.4 | Select a coherent shared-type revision; do not pull in the Reth fork's broad dependency patch table blindly |

Also audit field-width assumptions across the shared interfaces. The draft bounds `chain_id` and frame fee fields at 256 bits, while the current generic evm2 `Transaction` implementation exposes `chain_id` through `Option<u64>` and ordinary fee accessors through narrower types. Keep protocol validation against the original frame payload; reconcile decode/configuration limits deliberately instead of narrowing before validation. Sources: [EIPS/eip-8141.md:139–148](https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L139-L148) and [crates/evm2/src/ethereum/mod.rs:155–190](https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L155-L190).

Recommended new modules, whose names are proposals rather than existing APIs: `ethereum/eip8141.rs` for orchestration, `ethereum/eip8250.rs` for nonce transitions, `interpreter/instructions/frame.rs` for instructions, and a transaction-local runtime/diff module under `evm/`. Reuse shared types rather than making a second wire encoding in evm2.

### 5.2 EIP-8141: transaction lifecycle

The most useful reference is [crates/handler/src/eip8141.rs][s26]. Read it in this order:

| Reference function/area | Lines | evm2 responsibility |
|---|---|---|
| `run`, `FrameValidationResult`, `validate_prefix_with_callbacks` | [crates/handler/src/eip8141.rs:54–229][s27] | One canonical executor with full and prefix modes; prefix changes always discarded |
| `prepare` | [crates/handler/src/eip8141.rs:249–322][s28] | Environment, schema, structure, signatures, nonce checks, initial warming, runtime creation |
| `execute_frames` | [crates/handler/src/eip8141.rs:326–614][s29] | Frame/atomic/body checkpoints, receipt order, VERIFY failures, skipped frames |
| `execute_frame` | [crates/handler/src/eip8141.rs:617–823][s30] | Fresh memory, entry access charge, default code/precompile/contract dispatch, failure gas |
| `finish_transaction` | [crates/handler/src/eip8141.rs:827–900][s31] | Final attributed state gas, transaction refund, payer settlement, outputs and logs |
| Structural/signature/sender checks | [crates/handler/src/eip8141.rs:903–1181][s32] | Deterministic invalidity and read ordering |
| Introspection and approval host implementation | [crates/context/src/context.rs:533–806][s33] | Transaction/frame/signature lookups and approval effects |

Required execution sequence:

1. Validate the active envelope schema, frame count/flags/modes, values, gas arithmetic, fee limits, blob fields, and assertion suffix before running frames. Preserve the fork's check order.
2. Compute the canonical signing hash and check **all** protocol signatures, including entries unrelated to the selected public validation prefix. Use the existing secp256k1/P-256 crypto implementations, with the EIP-specific encodings and low-s checks. `ARBITRARY` witnesses are structurally checked by the protocol; their application verification runs in contract code.
3. Check the selected nonce domains against the transaction's actual prestate. Snapshot the legacy account nonce for `TXPARAM`, even when keyed lanes are used. Do not pre-increment it.
4. Initialize sender/coinbase/precompile warmth under the inherited rules. Do not prewarm every frame target or `ENTRY_POINT`.
5. For each top-level frame: create a checkpoint, clear frame-local memory and transient storage, set caller and origin, charge target access, check value affordability, charge applicable state growth, and dispatch the active precompile, delegated code, contract code, or protocol default behavior.
6. Share persistent state, original storage values, warmth and refund accounting across frames. Restore the appropriate checkpoint on child/frame/batch failure. `ORIGIN` is the top-level frame caller at all nested depths, so it can change between top-level frames.
7. Treat a failed `VERIFY` as transaction invalidity, rolling back the whole transaction. A failed ordinary frame is a frame execution failure; an atomic failure rolls back its group and skips the remainder of the group.
8. Capture the execution-body boundary after the first successful frame that sets the payer. Assertion failures restore that boundary, rather than the beginning of the transaction.
9. Require a payer, settle fees once, finalize delayed destruction once after assertions, and produce the complete ordered receipt/output list.

Do not implement this as repeated calls to `Evm::transact()` or repeated ordinary transaction handlers. They would repeatedly finalize originals, clear warmth, bump nonces, charge fees and expose the wrong assertion prestate.

The ordinary helper [crates/evm2/src/ethereum/mod.rs:431–461][s34] explicitly rejects nondelegated code senders, checks sender funds, warms and bumps the nonce. Those are the wrong combined operations for frame transactions. Likewise, [crates/evm2/src/evm/handler.rs:41–60][s35] defaults to charging the caller before execution. Frame payment must instead happen atomically during `APPROVE`.

### 5.3 Approval state and rollback

`APPROVE` is an instruction that returns successfully from the current EVM call and can produce approval effects. It is not a transaction-level callback that can be applied after contract execution. Preserve operand order, memory expansion, return data, allowed scope, resolved-target/address checks, and the VERIFY static-context exception. ASSERT mode gets no such exception.

Maintain, at minimum:

- Transaction payload/signing hash and current top-level frame index/target/mode.
- Payer, actual precharge, sender execution approval and the pretransaction legacy nonce.
- Approval state at each nested call boundary; only successful child returns propagate it.
- Ordered frame statuses, execution gas, attributed state gas and outputs.
- Revertible state-gas ownership plus frame, atomic-group and execution-body checkpoints.

Reference: [crates/context/interface/src/local.rs:10–152][s36], [crates/interpreter/src/instructions/frame.rs:23–57][s37], [EIPS/eip-8141.md:723–775][s38].

evm2's [crates/evm2/src/evm/state/journal.rs:7–83][s39] and [crates/evm2/src/evm/state/mod.rs:773–843][s40] already restore account/storage/warmth/transient changes and truncate logs. Extend the same checkpoint discipline to approvals, receipt mutations and state-gas ownership. Rolling back `State` while leaving those in `EvmExt` unchanged is incorrect. Add a dedicated per-frame transient reset; `clear_transaction_state()` also clears transaction-wide state and cannot serve that purpose.

### 5.4 Gas: a required semantic change

The current [crates/evm2/src/interpreter/gas.rs:327–339][s41] pays state costs out of the reservoir and then spills into execution gas. Frame transactions prohibit that spill. An abundant execution budget cannot rescue an exhausted state budget. Carry an isolated-state mode through nested CALL/CREATE handling and JIT builtins, while preserving ordinary transaction reservoir behavior.

The fork has an explicit constructor for this distinction: [crates/interpreter/src/gas.rs:62–75][s42]. The existing evm2 constructor [crates/evm2/src/ethereum/mod.rs:501–532][s43] partitions one outer limit under EIP-8037; it is not the frame budget initializer.

Let `I` be intrinsic execution gas, `F` the calldata floor, `E` the sum of declared frame execution limits and `S` the sum of state limits. A frame transaction has:

```text
execution reservation = max(I + E, F)
state reservation     = S
maximum gas           = max(I + E, F) + S

net state used        = sum(final attributed frame state gas)
charged fee gas       = max(total spent - capped refund - net state used, F)
                        + net state used
block execution used  = max(total spent - net state used, F)  [before refund]
block state used      = net state used
```

The payer precharge uses the maximum gas and full-width `max_fee_per_gas`, plus blobs priced at the block's blob base fee. Final payment uses the effective price. Preserve checked arithmetic and inclusion checks on `max_fee_per_blob_gas`.

**evm2's current result formula cannot be reused unchanged.** [crates/evm2/src/evm/tx.rs:91–118][s44] computes ordinary receipt gas as `max(total spent - refund, floor)`. Example: total spent 180,000, state used 100,000, floor 100,000 and zero refund gives 180,000 under that method, but a frame transaction owes **200,000**. Add transaction-aware result semantics and update all fee/receipt/estimation consumers; do not patch only the payer transfer.

Also retain signed refund deltas until transaction settlement. A later frame can reverse an earlier storage change; clamping each frame's refund to zero changes the result.

State growth ownership needs a separate journal: a later `SSTORE` restoring a newly created slot reduces the **earlier paying frame's** state-gas usage. It does not give the restoring frame more spendable state gas. Nested reverts, frame failures and atomic rollback must reverse these receipt changes. See [crates/context/interface/src/local.rs:126–172][s45] and [EIPS/eip-8141.md:568–697][s46].

### 5.5 Instructions, host APIs, results and inspection

Implement the selected revision's `APPROVE` (`0xaa`), `TXPARAM` (`0xb0`), `FRAMEDATALOAD` (`0xb1`), `FRAMEDATACOPY` (`0xb2`), `FRAMEPARAM` (`0xb3`), `SIGPARAM` (`0xb4`), and `SIGDATACOPY` (`0xb5`). Add the chosen EIP-7906 instruction layout separately. Validate full 256-bit operands and each instruction's source bounds; do not truncate selectors or assume all copy instructions have CALLDATACOPY's padding rules.

| evm2 files | Change |
|---|---|
| [crates/evm2/src/interpreter/opcode.rs][s47] | Opcode metadata, names, stack input/output counts |
| [crates/evm2/src/version/mod.rs][s48] and [crates/evm2/src/version/opcode_config.rs][s49] | Fork registration, static gas and function mapping |
| [crates/evm2/src/interpreter/instructions/mod.rs][s50] and proposed `instructions/frame.rs` | Instruction implementations and exports |
| [crates/evm2/src/interpreter/host.rs:114–188][s51] | Explicit frame introspection/approval/diff host methods; preserve the reference host interaction shape |
| [crates/evm2/src/evm/mod.rs:1093–1187][s52] | Hook entry/exit around synthetic and ordinary top-level frames and nested calls |
| [crates/evm2/src/evm/env.rs:12–26][s53] | Frame-aware environment/origin access; keep immutable payload separate from mutable runtime |
| [crates/evm2/src/evm/tx.rs:10–49][s54] | Typed frame outcome: payer, all frame receipts/outputs, actual gas dimensions, failure classification |
| [crates/evm2/src/evm/inspector.rs:14–119][s55] | Frame index/context and reliable boundaries including default code, precompile and early-entry failure |

The VM should return execution facts. RLP receipt encoding belongs in shared consensus types, and cumulative block gas belongs to the block executor. Preserve `InvalidTransaction` versus a valid transaction with frame/assertion execution failure versus a database/fatal error; Reth pool and RPC behavior depends on that distinction.

The prefix API must validate the complete transaction's static fields/signatures, execute only the policy-selected prefix, preserve configured precompiles and inspector behavior, stop at successful payment approval, and discard state/runtime changes on success **and** error. It should return payer, maximum exposure, approval status, actual prefix boundary and measured signature/execution/state gas. Reth supplies structural policy and the trace observer; evm2 supplies execution. Current `validate_tx()` is not an equivalent API.

## 6. EIP-8250: nonce migration

Move protocol nonce handling into evm2, but retain common field validation, key hashing and storage-key helpers in the shared EIP crate.

| Existing reference | Responsibility |
|---|---|
| [crates/eip8141/src/nonce.rs:9–51][s9] | Locked key validation, storage key/hash and priced nonce bytes; needs the draft changes if that target is selected |
| [crates/consensus/src/transaction/eip8141.rs:123–141][s56] and [crates/consensus/src/transaction/eip8141.rs:1070–1131][s57] | Payload representation and schema-dependent RLP |
| [crates/handler/src/eip8141.rs:264–283][s58] and [crates/handler/src/eip8141.rs:1125–1181][s59] | Activation/payload match and pre-execution sequence validity |
| [crates/context/src/context.rs:955–1035][s60] | Approval-time state charge and nonce consumption |
| [crates/context/src/journal.rs:193–253][s61] | Protocol storage operations without ordinary user-access warming |
| [crates/ethereum/node/src/frame_validation.rs:101–115][s62] and [crates/ethereum/node/src/frame_validation.rs:196–223][s63] | Pool snapshot validation and dependency recording; retained in Reth |

For the current draft, implement all of these:

1. Empty keys select the legacy account nonce. Nonempty keys are sorted unique uint256 values, at most 16, including zero as a normal key. Preserve canonical RLP integer checks and uint64 sequence bounds.
2. Read all selected domains before any frame executes. Sequential use requires equality with the submitted sequence. Single-use `MAX_NONCE_SEQ` requires zero in every selected slot.
3. At payment approval, determine first-use state growth, ensure the active frame's state pool covers **all** charges, then atomically consume the entire set, precharge the payer and update approval state. A partial key-set write must never survive failure.
4. Sequential consumption writes sequence plus one; single-use writes the maximum sentinel. Legacy consumption increments the **current** account nonce, which earlier deployment/CREATE activity may have changed; it must not overwrite it with submitted sequence plus one.
5. Preserve this successful validation-prefix transition across execution-body/assertion rollback. Undo it if its approving call/frame reverts or the entire transaction is invalid.
6. Record protocol slots in BAL with normal transaction indices, while leaving ordinary warm/cold status unchanged. These are storage changes under `NONCE_MANAGER`, not new fields in the account trie.
7. Add `TXPARAM` legacy nonce/key count/key-set hash/first-key selectors and correct nonce calldata pricing, including the single-use exemption.

The draft's schedule is 12,100 execution gas per selected keyed slot for protocol access, plus 97,920 state gas for each freshly used slot under the pinned EIP-8037/8038 parameters. Obtain costs from the active schedule, rather than scattering constants through Reth/evm2. Legacy sender-account creation has its own approval-time state charge.

Specification: [EIPS/eip-8250.md:34–229][s64].

Keep the extended nonce query, pending identity/replacement logic and all admission limits in Reth. Keyed lanes permit independent consensus replay domains; they do **not** automatically authorize unlimited public pending transactions per sender. The public policy remains one pending frame transaction per sender.

`NONCE_MANAGER` is an ordinary deployed contract. Fork activation is not a VM system call that installs its code. Genesis/deployment coordination stays with the chain/operator; the draft requires the account to exist before activation but explicitly avoids a client-side fork-boundary check or new activation BAL writes. See [EIPS/eip-8250.md:260–287][s65].

## 7. EIP-8272: recent roots

This EIP adds **no transaction envelope field and no new opcode**. The implementation is primarily shared constants/hashes and Reth public-mempool treatment of a canonical VERIFY frame, plus ordinary EVM execution of its deployed contract.

| Layer | Work |
|---|---|
| Shared Alloy types | Preserve address, canonical runtime/code hash, ring size 8,192, usable window 8,191, max 16 references, 72-byte tuple, domain-separated hashes |
| evm2 | Execute the contract through ordinary STATICCALL, storage access and gas rules; expose existing frame introspection; supply correct `SLOTNUM` |
| Reth environment | Block execution uses payload `slotNumber`; pool simulation uses canonical-head slot plus one; payload inclusion uses the payload's actual slot |
| Reth pool | Recognize optional expiry then optional recent-root verifier before deploy/account validation; check shape before sender-state lookup; enforce exact-code/top-level-only exceptions |
| Reth dependencies | Index `(storage key, expected entry hash, first expired slot)`, code/activation status, and both sides of reorgs; remove/replace metadata atomically |
| Chain/deployment | Supply the ordinary recent-root contract by deployment/genesis policy; activation itself changes no state |

Existing implementation anchors: [crates/eips/src/eip8272.rs:14–54][s66], [crates/transaction-pool/src/validate/frame_policy.rs:103–212][s67], [crates/transaction-pool/src/validate/frame_inspector.rs:82–103][s68], [crates/transaction-pool/src/validate/frame_inspector.rs:178–236][s69], [crates/ethereum/node/src/frame_validation.rs:71–106][s70].

The trace-policy exception allows `SLOTNUM` and only the declared `SLOAD` keys while the exact canonical runtime executes at the correct top-level address. Delegated/nested calls do not inherit it. Do not allow arbitrary reads of the recent-root account or turn it into a precompile.

evm2 already has `BlockEnv.slot_num` and both interpreted/compiled `SLOTNUM`: [crates/evm2/src/evm/env.rs:61–86][s71], [crates/evm2/src/interpreter/instructions/block.rs:83–88][s72], [crates/jit/codegen/src/compiler/translate/mod.rs:859–864][s73]. The remaining problem is correct environment propagation and policy, not another slot opcode. Never derive the slot from timestamp or let wall-clock empty slots advance the pool's simulated slot.

Test ages 0, 1, 8,191 and 8,192; overwrites within a slot; absent entries; duplicate tuples; code mismatch; predeployment failure; and reorg removal without a replacement write. Include the published reference vector. Specification: [EIPS/eip-8272.md:49–57][s74], [EIPS/eip-8272.md:233–307][s75], [EIPS/eip-8272.md:392–412][s76].

## 8. EIP-7906: assertions and consensus diffs

The rollback boundary and diff data belong in evm2 core. Debug tracing is optional and can truncate buffers; an assertion's consensus result cannot depend on enabling an inspector.

For the current draft:

- Mode 3 (`ASSERT`) must be a contiguous trailing suffix, read-only throughout its call subtree. `APPROVE` there is an ordinary static violation. Atomic groups must not include an assertion or run into its first frame.
- On assertion revert or exceptional halt, restore state and approval context to immediately after the payer-setting frame. Keep validation-prefix effects, gas payment and consumed nonce domains. Retain executed frames' execution gas/status, discard rolled-back logs/state gas, skip remaining assertions with status 2 and zero gas.
- Expose separate sorted net tables for balance, storage, code hash and nonce changes. Values before the transaction must precede deploy, approval nonce changes and gas precharge. Repeated writes collapse; return-to-original entries disappear.
- Include precharge in payer balance changes and include protocol transfer logs in events. Assertions run before final refund/reward settlement and delayed destruction cleanup, so their observation point differs from the final persisted state.
- Nonexistent accounts use nonce zero and empty-code hash in the relevant diff tables. Hash stored EIP-7702 delegation bytes without following them. Do not enumerate untouched prestate storage wiped by account lifecycle operations.
- Read only captured state/log data. `TXTRACE` and `EVENTDATACOPY` do not load live state, change warmth, or introduce BAL accesses. `EVENTDATACOPY` rejects an invalid source range even for length zero; check addition without uint256 wraparound.
- Sort state tables by numeric address, then numeric slot; preserve event emission order. Hash-map iteration order is not consensus order.

Source: [EIPS/eip-7906.md:47–193][s77].

The old fork implementation is spread across [crates/primitives/src/eip7906.rs][s78], [crates/context/src/journal.rs:275–365][s79], [crates/context/src/journal.rs:670–954][s80], [crates/context/src/context.rs:655–727][s81], and [crates/interpreter/src/instructions/frame.rs:164–236][s82]. Port orchestration and rollback patterns, but rewrite opcode/selector tables for the current-draft target.

evm2 already stores account and slot originals: [crates/evm2/src/evm/state/account.rs:181–204][s83], [crates/evm2/src/evm/state/tracked.rs:9–53][s84], [crates/evm2/src/evm/state/stream.rs:9–49][s85]. Add an in-flight, deterministic diff view before transaction finalization, using the transaction overlay and logs. Do not query the external database as “before”: accepted previous transactions and BAL-positioned execution can make that database older than this transaction's prestate.

`ExecutedTx::account_changes()` is exposed after execution/finalization; it alone cannot satisfy an opcode running during an assertion. Existing internal [crates/evm2/src/evm/state/mod.rs:940–947][s86] is a useful enumeration building block, but its order is unspecified and lifecycle semantics need the EIP-specific view. Build/cache the view lazily for the assertion suffix and invalidate it on rollback. Avoid re-sorting the entire diff on every indexed opcode call.

Preserve access records after assertion rollback for BAL, while dropping state writes that were rolled back. Audit log collection when `LOG_COLLECTION` is disabled: consensus frame receipts/assertions still need canonical event data. Optional output/debug settings must not change EVM execution results.

## 9. Interpreter, JIT, AOT and inspector work

| evm2 source | Required scope |
|---|---|
| [crates/jit/codegen/src/compiler/translate/mod.rs:744–864][s87] | Frame-aware origin/environment loads and translations for new instructions |
| [crates/jit/builtins/src/lib.rs:432–472][s88] | SSTORE isolated gas and owner/refill hooks |
| [crates/jit/builtins/src/lib.rs:543–871][s89] | CREATE/CALL state pool propagation and rollback; approval and introspection builtins |
| [crates/jit/context/src/lib.rs:28–69][s90] | Execution context access to current frame/runtime and any changed gas layout |
| [crates/jit/builtins/src/gas.rs][s91] | Compiled gas tracker assumptions |
| [crates/jit/codegen/src/compiler/mod.rs][s92] and bytecode analysis under `crates/jit/codegen/src/bytecode/` | Classification, stack effects, halting/returning APPROVE, gas accounting and opcode availability |
| [crates/jit/runtime/src/evm2_evm.rs:29–67][s93] | JIT runner integration and safe interpretation fallback |
| [crates/jit/runtime/src/runtime/storage.rs:9–31][s94] | Runtime key includes code hash/spec; persisted key additionally has ABI version |
| [crates/jit/codegen/src/lib.rs:19–22][s95] | Bump ABI identity when compiled layout/semantics require it; do not load incompatible artifacts |
| [crates/inspectors/src/tracing/mod.rs][s96] and [crates/inspectors/src/tracing/types.rs][s97] | Synthetic transaction root, frame index/context, canonical finalization |
| [crates/inspectors/src/tracing/builder/geth.rs][s98], [crates/inspectors/src/tracing/builder/parity.rs][s99], JS/debug/mux tracers | Frame-aware output, depths, skipped frames, dual gas and logs |

JIT currently specializes on `BaseEvmTypes`, including direct field offsets. Putting all frame functionality exclusively in an ad hoc custom type family would complicate or bypass this integration. Ensure standard frame support works with the normal runner.

Runtime frame mode and isolated gas are not safely inferable solely from contract code hash: one bytecode can run as VERIFY, SENDER or ASSERT in different transactions. Compile context-sensitive checks against runtime state, or explicitly make every needed specialization part of the cache identity. Keep new opcodes fork-gated even when a compiled artifact is reused.

The existing inspector fork is a useful port source: [src/tracing/mod.rs:296–415][s100], [src/tracing/mod.rs:563–637][s101], [src/tracing/types.rs:106–114][s102], [src/tracing/builder/geth.rs:266–339][s103], [src/tracing/builder/parity.rs:164–225][s104].

**Trace wiring needs explicit verification.** The fork documents `finalize_frame_transaction()` as a call required after execution, to reconcile skipped/early-failed frames and final receipts. A search of the inspected Reth and Alloy adapter sources finds no call to that method; the inspector repository has its definition and a unit-test call. Treat final-result reconciliation as an integration item to prove with RPC tests, rather than assuming that the tracing branch already covers it. This is a source-level wiring observation, not a runtime failure reproduced in this audit.

Prefix policy inspection must observe every relevant executed opcode, including reverted child calls. If the JIT cannot supply those observation hooks, use the interpreter for public-prefix simulation until equivalent compiled observation exists. An optimized runner that bypasses policy inspection must not be used for admission.

## 10. Alloy adapter and shared dependency migration

### 10.1 `alloy-evm`

| Locked adapter location | Current dependency | Required backend migration |
|---|---|---|
| [crates/evm/src/evm.rs:73–108][s105] | Returns revm `ResultAndState`; prefix API returns revm `FrameValidationResult` | Define the evm2 execution/prefix boundary and native outcomes |
| [crates/evm/src/eth/mod.rs:228–250][s106] | Executes/inspects revm and calls its prefix handler | Wire evm2 full execution, inspected execution and executable prefix simulation |
| [crates/evm/src/tx.rs:395–445][s107] | Converts consensus fields into revm's boxed frame payload and active-schedule gas limit | Convert to evm2's native envelope/context, retain explicit sender and full-width fee fields |
| [crates/evm/src/traits.rs:45–70][s108] | Transaction interface exposes revm frame payload | Remove backend-specific frame payload leakage from node-facing shared traits |
| [crates/evm/src/eth/env.rs:54–69][s109] | Sets Bogota/keyed activation in revm config | Map chain activation to evm2 version/registry and carry slot/blob parameters |
| [crates/evm/src/eth/block.rs:102–124][s110] | Derives independent gas reservations from revm transaction | Use evm2 validated schedule-aware reservation metadata |
| [crates/evm/src/eth/block.rs:291–503][s111] | Admission, execution, state commit and gas accumulation | Consume evm2 state streams and correct frame/ordinary result formulas |
| [crates/evm/src/eth/receipt_builder.rs:59–88][s112] | Matches `ExecutionResult::FrameTransaction` | Build shared frame receipt payload from native evm2 facts |
| [crates/evm/src/rpc/transaction.rs:88–171][s113] | RPC request-to-revm env conversion | Preserve frame field validation, placeholder controls, fee widths and canonical derived limit |
| `crates/evm/src/block/` state, hooks and system-call integration | revm database/state/result abstractions | Port general backend integration as well as frame additions |

Keep generic block concerns in the adapter or Reth. Expose enough native evm2 data to avoid a revm-shaped emulation layer. The repo has no public API stability guarantee; update consumers directly rather than adding deprecated wrappers, aliases or transitional compatibility shims.

The two existing code paths compute gas from shared consensus data and VM `GasParams`; consolidate or cross-check them so a pool cache/RPC conversion cannot derive a different maximum reservation than the actual engine under a custom schedule. Schedule-independent signing data belongs in Alloy; schedule-dependent execution pricing belongs in evm2, consumed by the adapter.

### 10.2 Shared transaction, receipt, RPC and codec support

Keep these implementations outside evm2 core:

| Shared source | Purpose |
|---|---|
| [crates/eip8141/src/frame.rs][s114], [crates/eip8141/src/signature.rs][s115], [crates/eip8141/src/receipt.rs][s116], [crates/eip8141/src/nonce.rs][s117] | Frame/fee/signature/status definitions, RLP pieces, nonce helpers |
| [crates/consensus/src/transaction/eip8141.rs:1043–1209][s118] | Payload encoding, signing hash and transaction hash |
| [crates/consensus/src/transaction/envelope.rs:525–527][s119], `transaction/typed.rs`, `tx_type.rs`, `pooled.rs` | Type-6 integration and sidecar-capable envelopes |
| [crates/consensus/src/receipt/envelope.rs][s120] and [crates/consensus/src/receipt/receipt2.rs][s121] | Frame receipt envelope and generic receipt behavior |
| [crates/eips/src/eip8272.rs][s122] | Recent-root constants and hashing |
| [crates/rpc-types-eth/src/transaction/frame.rs:6–75][s123], `frame_serde.rs`, `request.rs`, `mod.rs` | Partial frame requests, JSON and RPC receipt/transaction fields |
| [crates/network/src/ethereum/builder.rs][s124], [crates/provider/src/provider/trait.rs][s125] | Building/submitting/querying frame transactions |
| [crates/codecs/src/alloy/transaction/eip8141.rs:8–35][s126], `transaction/ethereum.rs`, `transaction/txtype.rs`, `transaction/mod.rs` | Compact frame transaction/envelope encoding |
| [crates/codecs/src/alloy/receipt.rs:138–163][s127] | Compact frame receipt encoding |

Wallet/provider signing must adopt the same draft revision as the node. A new VM with old signing hashes or a new `nonceKeys` default is not interoperable even if all Rust types compile.

## 11. Reth: retain and adapt

### 11.1 Public mempool and canonical snapshot validation

| Reth file and lines | Keep in Reth | Backend-facing edit |
|---|---|---|
| [crates/ethereum/node/src/frame_validation.rs:24–265][s128] | Pin canonical header/state; configure simulation; collect dependencies, expiry, payer solvency and metadata | Replace revm config/env/result/inspector access with evm2 prefix API |
| [crates/ethereum/node/src/node.rs][s129] | Install configured validation closure in node setup | Instantiate the migrated EVM adapter |
| [crates/transaction-pool/src/validate/frame_policy.rs:98–227][s130] | Prefix shape, bounded work, expiry/recent-root classification | Consume shared types; update chosen nonce/draft semantics |
| [crates/transaction-pool/src/validate/frame_inspector.rs:36–318][s131] | Public trace bans, deploy-only permissions, storage/code dependencies, exact-code exceptions | Rewrite against evm2 inspector/context/state observation; never alter gas warmth while collecting dependencies |
| [crates/transaction-pool/src/validate/frame_state.rs:9–308][s132] | Payer exposure and exclusive paymaster caps, dependency indexes, reservation bookkeeping | Update schema-derived keys/metadata, keep node state management here |
| [crates/transaction-pool/src/validate/eth.rs:506–520][s133] and [crates/transaction-pool/src/validate/eth.rs:702–789][s134] | Public/private routing and propagation decisions; ordinary transaction checks | Use native invalidity/errors and retain contract-sender exceptions only for frame transactions |
| [crates/transaction-pool/src/ordering.rs:87–143][s135] | Full-width fee priority/replacement, including blob price bump | Preserve uint256 values rather than narrowing through ordinary fee accessors |
| [crates/transaction-pool/src/pool/mod.rs:555–638][s136] and [crates/transaction-pool/src/pool/frame_revalidation.rs:18–77][s137] | Withdraw/revalidate pending frames, cancellation, replacement identity | Preserve stale-snapshot and head-change handling |
| [crates/transaction-pool/src/pool/txpool.rs:191–208][s138] and [crates/transaction-pool/src/pool/txpool.rs:1261–1285][s139] | Reservation membership, affected sets, eviction order | Consume the same metadata produced by new simulation |
| [crates/transaction-pool/src/maintain.rs:299–305][s140] and other head/reorg call sites | Trigger revalidation on canonical changes | Remains node work, independent of VM backend |
| [crates/transaction-pool/src/identifier.rs][s141], `traits.rs`, `pool/best.rs` | Identity and iterator/pool behavior | Audit scalar `(sender, nonce)` assumptions for the selected keyed schema |

Required invariants: rejected replacement leaves the old transaction and reservation intact; payer changes transfer exposure atomically; stale revalidation cannot resurrect a removed/replaced transaction; nonpublic frames cannot be accidentally gossiped; code dependencies include helpers/factories as well as the payer; recent-root dependencies are rechecked across reorgs.

The pinned implementation withdraws public frames on head changes and revalidates snapshots. Dependency-selective optimization is a separate improvement, not something to move into evm2. Likewise, the current EIP's optional direct evaluation of protocol-defined prefixes is an optimization after equivalent execution/gas/metadata behavior is demonstrated.

### 11.2 RPC and simulation

| Reth file and lines | Retained behavior requiring migration coverage |
|---|---|
| [crates/rpc/rpc-eth-api/src/core.rs:225–265][s142] | Keyed nonce query and `eth_simulateFrameTransaction` contract |
| [crates/rpc/rpc-eth-api/src/helpers/state.rs:55–104][s143] and [crates/rpc/rpc-eth-api/src/helpers/state.rs:513–545][s144] | Keyed nonce reads and account/query data at selected or pending state |
| [crates/rpc/rpc-eth-api/src/helpers/call.rs:308–541][s145] | Raw simulation, prefix policy result, payer/max exposure and all frame outcomes |
| [crates/rpc/rpc-eth-api/src/helpers/call.rs:1060–1210][s146] | Nonce defaults, simulation-only signature placeholders, RPC caps and request environment |
| [crates/rpc/rpc-eth-api/src/helpers/estimate.rs:79–97][s147] and [crates/rpc/rpc-eth-api/src/helpers/estimate.rs:230–238][s148] | Existing signed frame limits determine the canonical outer reservation; do not binary-search a nonexistent outer gas field |
| [crates/rpc/rpc-eth-api/src/helpers/estimate.rs:351–536][s149] | Fill omitted unsigned frame execution/state limits; verify candidates using actual VM execution |
| [crates/rpc/rpc-eth-api/src/helpers/transaction.rs:590–640][s150] | Transaction preparation/submission and canonical request completion |
| [crates/rpc/rpc-eth-types/src/simulate.rs:625–687][s151] | Per-frame simulation outputs and log mapping |
| [crates/rpc/rpc-eth-types/src/receipt.rs:65–185][s152] | Payer, frame receipts and globally indexed flattened logs |
| [crates/rpc/rpc-eth-types/src/error/mod.rs:225–235][s153] and `error/api.rs` | Fork/schema/execution errors mapped to RPC errors |
| [crates/rpc/rpc-eth-api/src/helpers/trace.rs][s154], RPC debug/trace handlers | Execute against evm2 and finalize frame-aware inspectors before building responses |

Simulation allowances belong in explicit nonconsensus configuration. Placeholder signatures must never become valid in imported blocks or pool propagation. Preserve arbitrary application witnesses and signatures when estimating; a filled limit changes signed data, so filling belongs before signing. Preserve the distinction between consensus validity, execution success, and public-pool eligibility in the simulation response.

### 11.3 Receipts, persistence, networking and payloads

| Reth source | Retain/adapt |
|---|---|
| [crates/ethereum/primitives/src/receipt.rs:24–153][s155] | Standard/frame receipt representation, flattened log access and consensus payload |
| [crates/ethereum/primitives/src/receipt.rs:234–400][s156] | RLP/EIP-2718/compact/compression support |
| [crates/ethereum/primitives/src/receipt.rs:441–446][s157] and [crates/trie/common/src/ordered_root.rs][s158] | Receipt roots must use the frame consensus encoding |
| [crates/ethereum/evm/src/receipt.rs:15–24][s159] | Reth primitive receipt builder consuming shared adapter output |
| [crates/ethereum/primitives/src/pooled.rs:20–47][s160] and [crates/ethereum/primitives/src/pooled.rs:128–195][s161] | Type-6 pooled variant, explicit sender, sidecar rules |
| [crates/net/eth-wire-types/src/receipts.rs:88–101][s162] | Frame receipt wire representation omits ordinary top-level bloom |
| [crates/transaction-pool/src/blobstore/tracker.rs:110–130][s163] and [crates/net/network/src/transactions/fetcher.rs][s164] | Only blob-bearing frames use blob tracking/sidecar handling |
| [crates/ethereum/payload/src/lib.rs:259–270][s165] and [crates/ethereum/payload/src/lib.rs:370–440][s166] | Dimension-aware admission, miner fees and execution results |
| [crates/ethereum/evm/src/lib.rs:139–155][s167] and [crates/ethereum/evm/src/lib.rs:293–306][s168] | Frame transaction conversion bounds and payload-specific fork/environment construction |
| [crates/chainspec/src/spec.rs:888–905][s169] and [crates/chainspec/src/spec.rs:1226–1243][s170] | Bogota activation and genesis interpretation; retain documented Amsterdam fallback only if that chain behavior remains intended |
| [crates/engine/primitives/src/message.rs][s171], [crates/payload/primitives/src/payload.rs][s172] | Detect frame transactions in payloads |
| [crates/storage/provider/src/providers/database/provider.rs][s173], `providers/static_file/mod.rs` | Frame-aware receipt retrieval and storage |
| [crates/era-utils/src/export/ere.rs][s174], `history.rs`, ERA/ERE execution types | History/export/import round trips preserve frame receipts |
| [crates/prune/prune/src/segments/user/receipts_by_logs.rs][s175], RPC filters, receipt-file downloads | Preserve log pruning/filtering and receipt reconstruction |
| [crates/evm/execution-types/src/execution_outcome.rs][s176], `chain.rs`, stages execution and engine receipt-root tasks | Consume migrated receipts/state results throughout node execution |

No change of VM backend should remove the sidecar wrapper or add an outer ECDSA signature. A frame's explicit sender remains authoritative even if transported in a generic `Recovered` wrapper. Full protocol signature validation still occurs before execution.

### 11.4 Broader revm removal is an additional workstream

Beyond frame functionality, `alloy-evm`/Reth still expose revm database traits, state bundles, account/bytecode types, error types, environments, system calls and inspector abstractions. Start with [crates/revm/src/lib.rs][s177], [crates/revm/src/database.rs][s178], [crates/evm/evm/src/lib.rs][s179], [crates/evm/evm/src/execute.rs][s180] and the adapter's `Evm` interface. Then work through the 107 direct-reference files in the companion inventory.

evm2 provides [crates/evm2/src/evm/state/stream.rs:106–183][s181] and [crates/evm2/src/evm/tx.rs:244–324][s182]: state-change sources/sinks, commit, discard and detach. Use that ownership model to feed Reth persistence/BAL/trie consumers. Do not commit to the backing database inside the transaction engine or rebuild a complete revm state map merely to preserve an old adapter signature.

This audit scopes those dependencies; it does not claim that every general Reth→evm2 API redesign is already specified. Integration choices must be made against the actual upstream adapter migration when it lands. The user's stated upcoming migration is the planning premise; no release date or completion promise is inferred here.

## 12. BAL and parallel execution, including `bal_parallel.rs`

The active example [crates/evm2/examples/bal_parallel.rs:1–93][s183] demonstrates transaction independence given an already-built BAL and ordered folding of detached results. It uses legacy transfers and does not implement block capacity admission or the frame validation state machine.

The requested Reth branch explicitly disables BAL parallel execution for **every block containing a frame transaction**: [crates/engine/tree/src/tree/payload_validator.rs:1124–1143][s184]. The source gives two reasons: the ordered commit loop replays standard single-limit reservations, and workers can surface execution failures before ordered admission checks.

Retain that serial fallback for the first migrated node. Enabling frame execution in evm2 does not justify removing it.

Conditions for enabling parallel frame blocks:

1. Workers execute one entire frame transaction each, with independent execution/state reservation metadata. Top-level frames inside a transaction remain ordered and share transaction state.
2. BAL-positioned reads supply the true transaction prestate to legacy/keyed nonce checks, payer solvency, application storage, recent-root verification and assertion “before” values. All those effects share one transaction `block_access_index`.
3. State streams include protocol nonce-manager writes, payer debit/refund and prefix effects surviving an assertion failure. Accesses survive rolled-back writes under BAL rules.
4. The ordered consumer checks each transaction's reservation against remaining execution **and** state capacity, then reports the first canonical transaction/admission/BAL error in serial order. Speculatively discovered later errors cannot overtake it.
5. Receipt cumulative gas, block execution/state totals, flattened log indices and receipt/state/BAL roots match serial execution exactly.
6. Missing/malformed BAL cases preserve static validation-before-state-read behavior. Compare [crates/handler/src/eip8141.rs:1125–1132][s185] and the existing Reth regression [crates/engine/tree/src/tree/payload_processor/bal/execute.rs:1219–1257][s186].
7. Reconstructed BAL and streamed state match serial results for keyed nonce overlaps, reused payers, cross-frame slot restoration, atomic rollback and assertion rollback.

Relevant evm2 code: [crates/evm2/src/evm/bal/bal_context.rs:171–202][s187], [crates/evm2/src/evm/state/pending.rs:30–110][s188], [crates/evm2/src/evm/tx.rs:264–324][s189]. Extend the example with a mixed block once these checks exist, but keep block scheduling and admission orchestration in Reth/its block adapter.

## 13. Implementation sequence and reviewable deliverables

The following are proposed work packages, not calendar estimates. Gas/rollback and adapter boundaries carry more risk than the number of opcode functions suggests.

| Order | Deliverable | Owner | Depends on / acceptance condition |
|---|---|---|---|
| 1 | Pin a protocol revision and fixture corpus; document every deliberate Bogota/draft delta | All repositories | One agreed signing/nonce/opcode ABI; no mixed interpretation |
| 2 | Shared type-6 types, signing/nonce schema and vectors; align dependency graph | Alloy EIPs/consensus/RPC/codecs | Decode/hash/serde/compact round trips; full-width fields |
| 3 | evm2 fork, type-6 registry/preparation and typed results/errors | evm2 | Pre-fork rejection; structure/signatures before state-dependent execution |
| 4 | Isolated state gas, ownership journal, call/frame/batch rollback | evm2 core + JIT gas paths | Exhausted state never spills; cross-frame refill and revert parity |
| 5 | EIP-8141 frame executor, default verification, APPROVE and introspection; executable prefix API | evm2 | Full and inspected prefix parity; state discarded on all prefix exits |
| 6 | EIP-8250 protocol reads/writes and activation | evm2 + shared types | Multi-key atomicity, warming/BAL behavior, approval rollback, selected schema |
| 7 | EIP-7906 assertion suffix and in-flight deterministic diff | evm2 | Correct observation point and body-only rollback, all selectors/bounds |
| 8 | Full opcode/JIT/AOT parity, cache identity and inspector lifecycle | evm2/JIT/inspectors | Every interpreter dispatch backend plus compiled execution agrees |
| 9 | Native adapter execution/env/state streaming/block gas/receipt integration | Alloy execution adapter + Reth EVM layer | End-to-end blocks and system calls work; no revm-shaped shim |
| 10 | Reth pool observer, nonce queries, simulation/estimate/trace and retained networking/storage consumers | Reth | Public/private policy, reservations, reorgs, receipts and RPC regressions |
| 11 | Differential EEST/Hive/devnet validation with serial frame-block fallback | Reth + evm2 | Block validity, roots, gas and errors match chosen baseline |
| 12 | Dimension-aware ordered BAL parallel execution | Reth + adapter | All seven conditions in section 12; separate performance qualification |

Packages 4–8 should land as cohesive engine changes rather than a permanent Reth-only custom executor. Package 9 contains general backend migration work even if some of it is supplied by upstream. After the graph is integrated, remove obsolete VM/inspector patches that are no longer needed; retain the shared protocol/type patches until the relevant upstream versions contain them.

For current-draft conformance, update shared hashes/nonces/instructions before using those fixtures to judge the VM. Do not “fix” differential failures by importing the old fork's incompatible signing or selector behavior into the new target.

## 14. Validation plan and completion gates

### Engine consensus tests

- Static fields, reserved flags/modes, max frame count, atomic grouping, assertion suffix, full-width selector/index/fee overflow, blob cap/hash validation, and all-signatures validation even in prefix-only mode.
- Default-account self-payment and sponsorship, delegated senders, active precompiles, sender target elision, contract senders, approval scopes, repeated approvals, nested successful/reverted approvals, return data and memory expansion.
- Target access charged before balance/delegation reads; frame-local memory and transient storage reset; shared warmth and transaction originals; frame/child/batch rollback.
- Independent state/execution exhaustion at SSTORE, CALL, CREATE, code deposit and approval; cross-frame refill ownership; negative refund deltas; floor-binding payment versus pre-refund block capacity.
- Legacy and keyed nonce domains, first use, subsequent use, overlapping/disjoint sets in block order, approval insufficiency, sentinel rules for the selected revision, transaction-start legacy nonce and no accidental user warming.
- Assertion balance/storage/code/nonce/event tables, sorting, return-to-original elimination, precharge/payer values, deployment/delegation/destruction observation, log inclusion and exact copy bounds.
- Invalid transactions undo all effects; valid assertion failures preserve prefix/nonce/payment and skip remaining assertions; VM reuse after errors leaves no stale frame runtime.

### Node integration tests

- Public prefix shapes and bans, helper/delegated-code behavior, reverted-child policy violations, expiry/recent-root exact-code exceptions and work bounds.
- Payer exposure, fee bump with uint256 values, replacement to a different payer, cancellation, head change during validation, dependency changes and reorgs; no stale re-admission or propagation.
- Raw/submitted/pooled blobless and blob-bearing frame transactions, KZG/sidecar errors, block body and receipt wire round trips, compact/static-file/history round trips and receipt roots.
- Keyed nonce RPC at historical/pending state; frame simulation validity versus execution/pool eligibility; unsigned limit filling; signed-limit preservation; placeholder rejection in consensus; frame-aware traces including skipped and early-failed frames.
- Per-dimension payload admission, miner fees, receipt cumulative gas versus header gas, actual slot revalidation and serial/parallel error ordering.

### Existing tests to port and extend

| Source | Coverage to reuse |
|---|---|
| [crates/handler/src/eip8141.rs:1450–2028][s190] | Frame memory, nonce domains/activation/consumption and rollback |
| [crates/handler/src/eip8141.rs:2054–3304][s191] | Approval scopes, state ownership/refills, entry gas and atomic groups |
| [crates/handler/src/eip8141.rs:3305–3895][s192] | Old assertion/diff/event tests; translate opcode/selector expectations for current draft |
| [crates/context/src/journal.rs:957–1449][s193] | Net diff, selector and state-access cases |
| [crates/inspector/src/handler.rs:374–496][s194] | Inspected prefix stops at payment and preserves hooks |
| [crates/ethereum/node/tests/e2e/frame_transactions.rs:171–514][s195] | Admission/mining, assertion suffix, expiry, atomic body, keyed RPC/pool |
| Reth `frame_policy.rs`, `frame_inspector.rs`, `frame_state.rs`, `pool/txpool.rs` test modules | Public policy, reservations, replacements, cancellation/revalidation |
| [crates/network/tests/eip8141_regressions.rs][s196], [crates/provider/tests/frame_submission.rs][s197], EIP crate encoding/signing/serde tests | Shared stack interoperability |
| [src/tracing/mod.rs:1157–1245][s198] | Finalized frame trace order and skipped-frame reconstruction |

Extend evm2's fixture parsers as part of this work: [crates/eest/src/tx.rs:77–113][s199] currently has scalar transaction fields and no frame payload; [crates/eest/src/types.rs:177–211][s200] and [crates/eest/src/forks.rs][s201] do not yet name Bogota. State tests, blockchain tests, raw transaction decoding and compiled fixture discovery all need the new schema/fork. Run the **same revision-matched corpus** through the old VM and evm2, comparing validity, state, logs, ordered frame receipts/outputs, gas dimensions and errors—not merely success flags.

Run the repo-required format/lint/docs checks once implementation exists, targeted nextest suites, then frame EEST/Hive suites. Force `auto`, `tco`, `packed`, `single_return` and `unpacked` dispatches, plus JIT/AOT. Compiled EEST defaults are subsets: use `EVM2_COMPILED_EEST_SUBSET=all` for the explicit full-coverage gate, and record fixture revision and skipped cases.

No builds, unit tests, EEST runs or devnet sessions were executed for this **report-only** change. Source/link checks validate the report's references, not consensus correctness.

## 15. Decisions to settle before implementation

1. Is the deployment target the existing Bogota devnet wire behavior, the current drafts, or a newly coordinated devnet revision? This determines signing hashes, key-zero/empty-key interpretation, sentinel support and opcode `0xb7`.
2. Which upstream Reth→evm2 adapter interfaces will be available? Resolve native result/state/inspector boundaries before spreading conversions throughout RPC/pool code.
3. Does the target include any additional deployment instructions? The mempool policy names EIP-7819 `SETDELEGATE` (`0xf6`), but a search of the pinned revm interpreter and this evm2 checkout does not show an implementation. Treat it as adjacent scope to confirm, not functionality guaranteed by porting these four EIPs. CREATE/CREATE2-based deploy frames remain a separate supported route.
4. Preserve the initial serial fallback for frame-containing blocks; schedule parallel admission as a distinct deliverable after engine and node parity.
5. Rebase or separately review the later local mempool-policy fixes. The active branch differs from the requested baseline in 21 files, including unpaid-attempt policy, pool identity/revalidation, propagation and tests; those improvements should not be accidentally lost or implicitly treated as already present in Bogota devnet 1.

The proposed architecture avoids making evm2 depend on Reth and preserves the behavior boundary that matters: evm2 determines whether and how a transaction executes; Reth determines when to admit, propagate, schedule, persist and serve it.


<!-- Immutable source references. -->

[s1]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L40-L79
[s2]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L225-L280
[s3]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/mod.rs#L744-L875
[s4]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/opcode.rs#L451
[s5]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Cargo.toml#L700-L754
[s6]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/Cargo.lock
[s7]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/eip8141.rs#L27-L59
[s8]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/transaction/eip8141.rs#L83-L111
[s9]: https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/nonce.rs#L9-L51
[s10]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L264-L280
[s11]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/context.rs#L955-L1027
[s12]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/eip7906.rs#L5-L94
[s13]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L249-L262
[s14]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L518-L566
[s15]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8250.md#L58-L102
[s16]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8250.md#L132-L179
[s17]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-7906.md#L32-L118
[s18]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L1150-L1156
[s19]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L137-L223
[s20]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/registry.rs#L187-L236
[s21]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/mod.rs#L969-L1017
[s22]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/config.rs#L22-L55
[s23]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/spec_id.rs#L62-L99
[s24]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/features.rs
[s25]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/Cargo.toml#L61-L80
[s26]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs
[s27]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L54-L229
[s28]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L249-L322
[s29]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L326-L614
[s30]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L617-L823
[s31]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L827-L900
[s32]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L903-L1181
[s33]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/context.rs#L533-L806
[s34]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L431-L461
[s35]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/handler.rs#L41-L60
[s36]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/local.rs#L10-L152
[s37]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/instructions/frame.rs#L23-L57
[s38]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L723-L775
[s39]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/journal.rs#L7-L83
[s40]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/mod.rs#L773-L843
[s41]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/gas.rs#L327-L339
[s42]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/gas.rs#L62-L75
[s43]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/ethereum/mod.rs#L501-L532
[s44]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/tx.rs#L91-L118
[s45]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/interface/src/local.rs#L126-L172
[s46]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8141.md#L568-L697
[s47]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/opcode.rs
[s48]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/mod.rs
[s49]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/version/opcode_config.rs
[s50]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/instructions/mod.rs
[s51]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/host.rs#L114-L188
[s52]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/mod.rs#L1093-L1187
[s53]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/env.rs#L12-L26
[s54]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/tx.rs#L10-L49
[s55]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/inspector.rs#L14-L119
[s56]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/eip8141.rs#L123-L141
[s57]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/eip8141.rs#L1070-L1131
[s58]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L264-L283
[s59]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L1125-L1181
[s60]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/context.rs#L955-L1035
[s61]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal.rs#L193-L253
[s62]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L101-L115
[s63]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L196-L223
[s64]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8250.md#L34-L229
[s65]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8250.md#L260-L287
[s66]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/eip8272.rs#L14-L54
[s67]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_policy.rs#L103-L212
[s68]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs#L82-L103
[s69]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs#L178-L236
[s70]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L71-L106
[s71]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/env.rs#L61-L86
[s72]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/interpreter/instructions/block.rs#L83-L88
[s73]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/compiler/translate/mod.rs#L859-L864
[s74]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8272.md#L49-L57
[s75]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8272.md#L233-L307
[s76]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-8272.md#L392-L412
[s77]: https://github.com/ethereum/EIPs/blob/46005d745a5d921702f12d64a1b85e712e2b6051/EIPS/eip-7906.md#L47-L193
[s78]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/primitives/src/eip7906.rs
[s79]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal.rs#L275-L365
[s80]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal.rs#L670-L954
[s81]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/context.rs#L655-L727
[s82]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/interpreter/src/instructions/frame.rs#L164-L236
[s83]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/account.rs#L181-L204
[s84]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/tracked.rs#L9-L53
[s85]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/stream.rs#L9-L49
[s86]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/mod.rs#L940-L947
[s87]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/compiler/translate/mod.rs#L744-L864
[s88]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/builtins/src/lib.rs#L432-L472
[s89]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/builtins/src/lib.rs#L543-L871
[s90]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/context/src/lib.rs#L28-L69
[s91]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/builtins/src/gas.rs
[s92]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/compiler/mod.rs
[s93]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/runtime/src/evm2_evm.rs#L29-L67
[s94]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/runtime/src/runtime/storage.rs#L9-L31
[s95]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/jit/codegen/src/lib.rs#L19-L22
[s96]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/mod.rs
[s97]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/types.rs
[s98]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/builder/geth.rs
[s99]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/inspectors/src/tracing/builder/parity.rs
[s100]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/mod.rs#L296-L415
[s101]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/mod.rs#L563-L637
[s102]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/types.rs#L106-L114
[s103]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/builder/geth.rs#L266-L339
[s104]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/builder/parity.rs#L164-L225
[s105]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/evm.rs#L73-L108
[s106]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/mod.rs#L228-L250
[s107]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/tx.rs#L395-L445
[s108]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/traits.rs#L45-L70
[s109]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/env.rs#L54-L69
[s110]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/block.rs#L102-L124
[s111]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/block.rs#L291-L503
[s112]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/eth/receipt_builder.rs#L59-L88
[s113]: https://github.com/Soubhik-10/evm/blob/5c35db18a87dd6122b7d1c0ab690d275fe13fa3a/crates/evm/src/rpc/transaction.rs#L88-L171
[s114]: https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/frame.rs
[s115]: https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/signature.rs
[s116]: https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/receipt.rs
[s117]: https://github.com/Soubhik-10/eips/blob/921ddebb486507d74808239b51e180e7600196d1/crates/eip8141/src/nonce.rs
[s118]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/eip8141.rs#L1043-L1209
[s119]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/transaction/envelope.rs#L525-L527
[s120]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/receipt/envelope.rs
[s121]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/consensus/src/receipt/receipt2.rs
[s122]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/eips/src/eip8272.rs
[s123]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/rpc-types-eth/src/transaction/frame.rs#L6-L75
[s124]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/src/ethereum/builder.rs
[s125]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/provider/src/provider/trait.rs
[s126]: https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/transaction/eip8141.rs#L8-L35
[s127]: https://github.com/Soubhik-10/reth-core/blob/c42b71acb8e12de6b2b01238bbc1ce739510bdd3/crates/codecs/src/alloy/receipt.rs#L138-L163
[s128]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/frame_validation.rs#L24-L265
[s129]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/src/node.rs
[s130]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_policy.rs#L98-L227
[s131]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_inspector.rs#L36-L318
[s132]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/frame_state.rs#L9-L308
[s133]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/eth.rs#L506-L520
[s134]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/validate/eth.rs#L702-L789
[s135]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/ordering.rs#L87-L143
[s136]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/mod.rs#L555-L638
[s137]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/frame_revalidation.rs#L18-L77
[s138]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/txpool.rs#L191-L208
[s139]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/pool/txpool.rs#L1261-L1285
[s140]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/maintain.rs#L299-L305
[s141]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/identifier.rs
[s142]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/core.rs#L225-L265
[s143]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/state.rs#L55-L104
[s144]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/state.rs#L513-L545
[s145]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/call.rs#L308-L541
[s146]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/call.rs#L1060-L1210
[s147]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs#L79-L97
[s148]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs#L230-L238
[s149]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/estimate.rs#L351-L536
[s150]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/transaction.rs#L590-L640
[s151]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/simulate.rs#L625-L687
[s152]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/receipt.rs#L65-L185
[s153]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-types/src/error/mod.rs#L225-L235
[s154]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/rpc/rpc-eth-api/src/helpers/trace.rs
[s155]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/receipt.rs#L24-L153
[s156]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/receipt.rs#L234-L400
[s157]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/receipt.rs#L441-L446
[s158]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/trie/common/src/ordered_root.rs
[s159]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/receipt.rs#L15-L24
[s160]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/pooled.rs#L20-L47
[s161]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/primitives/src/pooled.rs#L128-L195
[s162]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/eth-wire-types/src/receipts.rs#L88-L101
[s163]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/transaction-pool/src/blobstore/tracker.rs#L110-L130
[s164]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/net/network/src/transactions/fetcher.rs
[s165]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/payload/src/lib.rs#L259-L270
[s166]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/payload/src/lib.rs#L370-L440
[s167]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/lib.rs#L139-L155
[s168]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/evm/src/lib.rs#L293-L306
[s169]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chainspec/src/spec.rs#L888-L905
[s170]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/chainspec/src/spec.rs#L1226-L1243
[s171]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/primitives/src/message.rs
[s172]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/payload/primitives/src/payload.rs
[s173]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/storage/provider/src/providers/database/provider.rs
[s174]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/era-utils/src/export/ere.rs
[s175]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/prune/prune/src/segments/user/receipts_by_logs.rs
[s176]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/execution-types/src/execution_outcome.rs
[s177]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/lib.rs
[s178]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/revm/src/database.rs
[s179]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/lib.rs
[s180]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/evm/evm/src/execute.rs
[s181]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/stream.rs#L106-L183
[s182]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/tx.rs#L244-L324
[s183]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/examples/bal_parallel.rs#L1-L93
[s184]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_validator.rs#L1124-L1143
[s185]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L1125-L1132
[s186]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/engine/tree/src/tree/payload_processor/bal/execute.rs#L1219-L1257
[s187]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/bal/bal_context.rs#L171-L202
[s188]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/state/pending.rs#L30-L110
[s189]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/evm2/src/evm/tx.rs#L264-L324
[s190]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L1450-L2028
[s191]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L2054-L3304
[s192]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/handler/src/eip8141.rs#L3305-L3895
[s193]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/context/src/journal.rs#L957-L1449
[s194]: https://github.com/Soubhik-10/revm/blob/b2109aeb7a69505617c629538e9014bd20148e5c/crates/inspector/src/handler.rs#L374-L496
[s195]: https://github.com/paradigmxyz/reth-oss/blob/7bde440574a2afef4f718f32c02b03320f8d57c1/crates/ethereum/node/tests/e2e/frame_transactions.rs#L171-L514
[s196]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/network/tests/eip8141_regressions.rs
[s197]: https://github.com/Rimeeeeee/alloy/blob/1ff4bf799bc78b0913561fa3e44bc65d0444c638/crates/provider/tests/frame_submission.rs
[s198]: https://github.com/Soubhik-10/revm-inspectors/blob/b736efa83f749ad15190ec3fbc3c95c6ca42d3cc/src/tracing/mod.rs#L1157-L1245
[s199]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/tx.rs#L77-L113
[s200]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/types.rs#L177-L211
[s201]: https://github.com/alloy-rs/evm2/blob/185be4b37d44f22cf81f71cf0568ea710b54a66d/crates/eest/src/forks.rs
