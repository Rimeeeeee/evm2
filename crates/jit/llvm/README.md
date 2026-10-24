# evm2-jit-llvm

EVM bytecode compiler [LLVM] backend.

Building requires LLVM 23.1. Set `LLVM_SYS_231_PREFIX` to its installation directory so
the linked libraries and C++ shim headers come from the same installation. Without
that prefix, the shim uses `LLVM_CONFIG_PATH`, `llvm-config-23`, or a matching
`llvm-config` from `PATH`; incompatible versions fail at build time. The selected
configuration and version appear in the build output. AOT linking separately needs
the `lld` executable on `PATH`.

[LLVM]: https://llvm.org/
