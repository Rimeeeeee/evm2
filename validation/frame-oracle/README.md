# Bogota frame differential runner

This standalone workspace compares native evm2 execution with the immutable Bogota
revm revision in [the protocol manifest](../../docs/frame-transactions-protocol.toml).
Its own `Cargo.lock` pins the EIP primitives as well as revm and Alloy. Always use
`--locked`; the upstream Alloy manifest references a moving EIP branch.

```sh
cargo +nightly run --locked --manifest-path validation/frame-oracle/Cargo.toml
```

The 89 deterministic cases cover payment, keyed/legacy nonce selection, entry gas,
state exhaustion, every Bogota assertion selector including invalid selectors, and
32 five-frame charge/restore sequences. Both sides start with the same deployed
nonce-manager account. Valid outcomes compare status, payer, execution/state/floor/
refund/fee gas, ordered frame receipts and outputs, canonical logs, and surviving
account/storage values. Invalid cases compare rejection, and print both errors;
they do **not** establish identical error precedence or error taxonomy. This tool
does not import Reth, compute trie roots, or qualify RPC/network/pool behavior.

The official fixture runner independently checks state, logs, receipt roots, block
gas, and BAL commitments against the pinned execution-specs corpus:

```sh
python3 scripts/extract_bogota_fixtures.py fixtures_bogota-devnet.tar.gz test-fixtures/bogota
cargo run --locked -p evm2-eest --bin frame-conformance -- test-fixtures/bogota interpreter
cargo run --locked -p evm2-eest --features jit --bin frame-conformance -- test-fixtures/bogota jit
cargo run --locked -p evm2-eest --features jit --bin frame-conformance -- test-fixtures/bogota aot
```

The extractor verifies the archive SHA-256 before extracting the four EIP families
in state/blockchain format, including fork-transition tests. Engine API and
execute-only fixture formats are outside this runner. Every selected JSON file
runs without the usual EEST exclusions or compiled smoke subsets; failures and
skips fail the process. Compiled runs also reject compilation failures and fallback
for nonempty bytecode. Empty code takes its implicit `STOP` directly. Native
precompiles/default verification remain host operations.

On Windows use portable engine features (`--no-default-features`) where the GMP
backend cannot build with MSVC. JIT/AOT qualification requires LLVM 23 and an AOT
linker/export setup; the local compiled evidence uses Linux in WSL. Ordinary CLI
revm comparisons still use upstream revm and explicitly reject Bogota fixtures;
this workspace supplies the pinned frame reference separately.
