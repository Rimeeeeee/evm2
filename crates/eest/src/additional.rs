//! Explicit additional EEST fixtures, using the normal harness and backend suffixes.
//!
//! Every JSON file under `EVM2_ADDITIONAL_TESTS` runs without ignore lists or smoke subsets.

use crate::{
    blockchaintest::{NoopHook, execute as block_execute},
    execute::{self, ExecuteConfig as StateConfig},
    execution::{ExecutionMode, ExecutionResources},
    filter::NameFilter,
    fixture_io,
    harness::{TestRoot, TestSuite, descend_all},
};
use evm2::evm::DbStatsCounts;
use libtest_mimic::Failed;
use std::{io, path::PathBuf};

/// Builds the explicit corpus suites with the same backend suffixes as the normal harness.
pub(crate) fn suites(path: PathBuf) -> Vec<TestSuite> {
    vec![
        suite(path.clone(), "additional", run_file, run_files),
        suite(path.clone(), "additional::inspected", run_file_inspected, run_files_inspected),
        #[cfg(feature = "jit")]
        suite(path.clone(), "additional::jit", run_file_jit, run_files_jit),
        #[cfg(feature = "jit")]
        suite(path, "additional::aot", run_file_aot, run_files_aot),
    ]
}

fn suite(
    path: PathBuf,
    name: &'static str,
    run_file: fn(PathBuf) -> Result<(), Failed>,
    run_files: fn(Vec<PathBuf>) -> Result<(), Failed>,
) -> TestSuite {
    TestSuite {
        name,
        roots: vec![TestRoot { name: name.to_string(), label: "additional tests", path }],
        should_descend: descend_all,
        should_ignore: |_| false,
        run_file,
        run_files: Some(run_files),
        exhaustive: true,
    }
}

fn run_file(path: PathBuf) -> Result<(), Failed> {
    run_files(vec![path])
}

fn run_files(paths: Vec<PathBuf>) -> Result<(), Failed> {
    run_with_mode(paths, ExecutionMode::Interpreter)
}

fn run_file_inspected(path: PathBuf) -> Result<(), Failed> {
    run_files_inspected(vec![path])
}

fn run_files_inspected(paths: Vec<PathBuf>) -> Result<(), Failed> {
    run_with_mode(paths, ExecutionMode::InspectedInterpreter)
}

#[cfg(feature = "jit")]
fn run_file_jit(path: PathBuf) -> Result<(), Failed> {
    run_files_jit(vec![path])
}

#[cfg(feature = "jit")]
fn run_files_jit(paths: Vec<PathBuf>) -> Result<(), Failed> {
    run_with_mode(paths, ExecutionMode::Jit)
}

#[cfg(feature = "jit")]
fn run_file_aot(path: PathBuf) -> Result<(), Failed> {
    run_files_aot(vec![path])
}

#[cfg(feature = "jit")]
fn run_files_aot(paths: Vec<PathBuf>) -> Result<(), Failed> {
    run_with_mode(paths, ExecutionMode::Aot)
}

fn run_with_mode(paths: Vec<PathBuf>, mode: ExecutionMode) -> Result<(), Failed> {
    let summary = execute_paths(paths, mode).map_err(|error| error.to_string())?;
    println!(
        "mode={mode:?}; files={}; failed_files={}; successful_file_units={}; skipped_units={}",
        summary.files, summary.failed_files, summary.successful_file_units, summary.skipped_units
    );
    if summary.failed_files != 0 || summary.skipped_units != 0 {
        return Err("explicit corpus execution failed or skipped fixture units".into());
    }
    Ok(())
}

/// Counts for an explicit additional-fixture run.
#[derive(Clone, Copy, Debug, Default)]
struct CorpusSummary {
    /// Number of selected JSON files.
    files: usize,
    /// Number of files with a parse or execution error.
    failed_files: usize,
    /// Executed cases in successful files.
    successful_file_units: usize,
    /// Skipped cases in successful files.
    skipped_units: usize,
}

/// Executes the selected state/blockchain files with shared compilation resources.
///
/// No normal name exclusions or compiled smoke subsets are applied.
/// Per-file failures are printed and counted so later files still run.
fn execute_paths(mut paths: Vec<PathBuf>, mode: ExecutionMode) -> io::Result<CorpusSummary> {
    paths.sort();
    if paths.is_empty() {
        return Err(io::Error::other("no JSON fixtures selected"));
    }
    let resources = ExecutionResources::new(mode).map_err(io::Error::other)?;
    let mut summary = CorpusSummary { files: paths.len(), ..Default::default() };
    for path in paths {
        let result = (|| -> Result<(usize, usize), String> {
            let input = fixture_io::read_to_string(&path).map_err(|error| error.to_string())?;
            let json: serde_json::Value =
                serde_json::from_str(&input).map_err(|error| error.to_string())?;
            let first =
                json.as_object().and_then(|map| map.values().next()).ok_or("empty fixture")?;
            if first.get("blocks").is_some() {
                let suite = fixture_io::parse_json(&input).map_err(|error| error.to_string())?;
                let result = block_execute::execute_suite_with_resources(
                    &path,
                    &suite,
                    block_execute::ExecuteConfig {
                        mode,
                        compare_state_root: true,
                        compare_receipt_root: true,
                        ..Default::default()
                    },
                    &NameFilter::default(),
                    &mut NoopHook,
                    &resources,
                )
                .map_err(|error| error.to_string())?;
                Ok((result.executed, result.skipped))
            } else if first.get("post").is_some() {
                let result = execute::execute_str_with_resources(
                    &path,
                    &input,
                    StateConfig { mode, ..Default::default() },
                    &NameFilter::default(),
                    &resources,
                    &mut DbStatsCounts::default(),
                )
                .map_err(|error| error.to_string())?;
                Ok((result.executed, result.skipped))
            } else {
                Err("unsupported fixture format".into())
            }
        })();
        match result {
            Ok((0, _)) => {
                summary.failed_files += 1;
                eprintln!("FAIL {}: no fixture units executed", path.display());
            }
            Ok((ran, skipped)) => {
                summary.successful_file_units += ran;
                summary.skipped_units += skipped;
                println!("PASS {} ({ran} units, {skipped} skipped)", path.display());
            }
            Err(error) => {
                summary.failed_files += 1;
                eprintln!("FAIL {}: {error}", path.display());
            }
        }
    }
    Ok(summary)
}
