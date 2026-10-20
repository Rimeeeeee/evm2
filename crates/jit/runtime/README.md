# evm2-jit-runtime

Runtime JIT/AOT backend for evm2-jit.

This crate owns the runtime compilation infrastructure: compiled-program lookup, resident-code management, artifact storage, background worker scheduling, AOT preloading, and evm2 execution wrappers.

It builds on `evm2-jit-codegen` for compilation and is re-exported by the umbrella `evm2-jit` crate.

## Custom hosts

Set `RuntimeConfig::host` to `HostConfig::for_types::<T>()` before creating a backend for a custom `EvmTypesHost`. This selects the concrete message layout and monomorphized host builtins. A runner checks the host type and active gas schedule before executing compiled code; a mismatch falls back to the interpreter.

Chains with custom opcode semantics can replace the builtin resolver with unsafe `HostConfig::with_resolver`. The resolver must preserve each builtin's C ABI and use the same host type as the configuration. Configure matching gas parameters as well. Custom hosts currently support in-process JIT only, without persistent artifact storage or AOT: function pointers and custom layouts are process-local.
