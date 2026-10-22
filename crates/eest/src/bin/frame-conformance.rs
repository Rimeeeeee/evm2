//! Run the complete manifest-pinned frame corpus with one shared compilation cache.

use evm2_eest::{StateTestExecutionMode as ExecutionMode, conformance::execute_frame_corpus};
use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!(
            "usage: frame-conformance <fixture-root-or-file> [interpreter|inspected|jit|aot]"
        );
        return ExitCode::FAILURE;
    };
    let backend = args.next().unwrap_or_else(|| "interpreter".into());
    let mode = match backend.as_str() {
        "interpreter" => ExecutionMode::Interpreter,
        "inspected" => ExecutionMode::InspectedInterpreter,
        #[cfg(feature = "jit")]
        "jit" => ExecutionMode::Jit,
        #[cfg(feature = "jit")]
        "aot" => ExecutionMode::Aot,
        _ => {
            eprintln!("unsupported backend {backend:?}; compiled modes require --features jit");
            return ExitCode::FAILURE;
        }
    };
    match execute_frame_corpus(Path::new(&root), mode) {
        Ok(summary) => {
            println!(
                "backend={backend}; files={}; failed_files={}; successful_file_units={}; skipped_units={}",
                summary.files,
                summary.failed_files,
                summary.successful_file_units,
                summary.skipped_units
            );
            if summary.failed_files == 0 && summary.skipped_units == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{root}: {error}");
            ExitCode::FAILURE
        }
    }
}
