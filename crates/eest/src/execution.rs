use evm2::{BaseEvmTypes, Evm, NoopInspector};
#[cfg(feature = "jit")]
use evm2_jit_runtime::{evm2_evm::run_interpreter, runtime::JitBackend};
use std::{error::Error, fmt};

#[cfg(feature = "jit")]
use evm2::{
    InterpreterRunner,
    interpreter::{InstrStop, Interpreter},
};
#[cfg(feature = "jit")]
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// EEST execution backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Run through the evm2 interpreter.
    #[default]
    Interpreter,
    /// Run through inspected instruction dispatch without producing debug output.
    InspectedInterpreter,
    /// Compare every native transaction directly with the pinned Bogota revm implementation.
    Revm,
    /// Run through the evm2 JIT runtime, failing qualification if any code falls back.
    #[cfg(feature = "jit")]
    Jit,
    /// Run through the evm2 AOT runtime, failing qualification if any code falls back.
    #[cfg(feature = "jit")]
    Aot,
}

#[cfg(feature = "jit")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompiledMode {
    Jit,
    Aot,
}

#[cfg(feature = "jit")]
impl CompiledMode {
    #[inline]
    pub(crate) const fn execution_mode(self) -> ExecutionMode {
        match self {
            Self::Jit => ExecutionMode::Jit,
            Self::Aot => ExecutionMode::Aot,
        }
    }

    #[inline]
    pub(crate) const fn suffix(self) -> &'static str {
        match self {
            Self::Jit => "jit",
            Self::Aot => "aot",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExecutionResourceError(String);

impl fmt::Display for ExecutionResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "jit runtime error: {}", self.0)
    }
}

impl Error for ExecutionResourceError {}

#[derive(Clone, Debug)]
pub(crate) struct ExecutionResources {
    inspected: bool,
    differential: bool,
    #[cfg(feature = "jit")]
    backend: Option<JitBackend>,
    #[cfg(feature = "jit")]
    fallbacks: Arc<AtomicUsize>,
}

impl ExecutionResources {
    #[cfg(feature = "jit")]
    pub(crate) fn new(mode: ExecutionMode) -> Result<Self, ExecutionResourceError> {
        let backend = match mode {
            ExecutionMode::Interpreter
            | ExecutionMode::InspectedInterpreter
            | ExecutionMode::Revm => None,
            ExecutionMode::Jit | ExecutionMode::Aot => {
                let aot = mode == ExecutionMode::Aot;
                Some(crate::jit::make_backend(aot).map_err(ExecutionResourceError)?)
            }
        };
        Ok(Self {
            inspected: mode == ExecutionMode::InspectedInterpreter,
            differential: mode == ExecutionMode::Revm,
            backend,
            fallbacks: Arc::default(),
        })
    }

    #[cfg(not(feature = "jit"))]
    pub(crate) const fn new(mode: ExecutionMode) -> Result<Self, ExecutionResourceError> {
        Ok(Self {
            inspected: matches!(mode, ExecutionMode::InspectedInterpreter),
            differential: matches!(mode, ExecutionMode::Revm),
        })
    }

    pub(crate) const fn differential(&self) -> bool {
        self.differential
    }

    #[cfg(feature = "jit")]
    #[inline]
    pub(crate) fn configure_evm(&self, _evm: &mut Evm<'_, BaseEvmTypes>) {
        if let Some(backend) = &self.backend {
            _evm.set_interpreter_runner(ConformanceRunner {
                backend: backend.clone(),
                fallbacks: self.fallbacks.clone(),
            });
        }
        if self.inspected {
            _evm.set_inspector(NoopInspector::default());
        }
    }

    #[cfg(feature = "jit")]
    pub(crate) fn print_runtime_stats(&self) {
        if let Some(backend) = &self.backend {
            let stats = backend.stats();
            println!(
                "Runtime backend: {} hits, {} misses, {} resident",
                stats.lookup_hits, stats.lookup_misses, stats.resident_entries,
            );
        }
    }

    #[cfg(not(feature = "jit"))]
    #[inline]
    pub(crate) fn configure_evm(&self, _evm: &mut Evm<'_, BaseEvmTypes>) {
        if self.inspected {
            _evm.set_inspector(NoopInspector::default());
        }
    }

    #[cfg(feature = "jit")]
    pub(crate) fn verify_backend(&self) -> Result<(), ExecutionResourceError> {
        if let Some(backend) = &self.backend {
            let fallbacks = self.fallbacks.load(Ordering::Relaxed);
            let failures = backend.stats().compilations_failed;
            if fallbacks != 0 || failures != 0 {
                return Err(ExecutionResourceError(format!(
                    "compiled conformance requires native execution: {fallbacks} interpreter fallbacks, {failures} compilation failures"
                )));
            }
        }
        Ok(())
    }
    #[cfg(not(feature = "jit"))]
    pub(crate) const fn verify_backend(&self) -> Result<(), ExecutionResourceError> {
        Ok(())
    }
}

#[cfg(feature = "jit")]
#[derive(Debug)]
struct ConformanceRunner {
    backend: JitBackend,
    fallbacks: Arc<AtomicUsize>,
}

#[cfg(feature = "jit")]
impl InterpreterRunner<BaseEvmTypes> for ConformanceRunner {
    fn run<'frame, 'host>(
        &self,
        interpreter: &mut Interpreter<'frame, 'host, BaseEvmTypes>,
        host: &mut Evm<'host, BaseEvmTypes>,
    ) -> Option<InstrStop> {
        // The runtime intentionally does not compile empty code. There are no EVM
        // instructions to interpret: perform its implicit STOP directly instead.
        if interpreter.original_bytecode_slice().is_empty() {
            interpreter.prepare_run(host);
            return Some(InstrStop::Stop);
        }
        let result = run_interpreter(&self.backend, interpreter, host);
        if result.is_none() {
            self.fallbacks.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}
