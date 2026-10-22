//! Explicit additional EEST fixtures, using the normal harness and backend suffixes.
//!
//! Every JSON file under `EVM2_ADDITIONAL_TESTS` runs without ignore lists or smoke subsets.

use crate::{
    blockchaintest::{NoopHook, execute as block_execute},
    execute::{self, ExecuteConfig as StateConfig},
    execution::{ExecutionMode, ExecutionResources},
    filter::NameFilter,
    fixture_io,
    forks::{SKIP_FORKS_ENV, is_fork_skipped},
    harness::{TestRoot, TestSuite, descend_all},
    types::SpecName,
};
use evm2::evm::DbStatsCounts;
use libtest_mimic::Failed;
use std::{env, fs, io, path::PathBuf};

/// Builds the explicit corpus suites with the same backend suffixes as the normal harness.
pub(crate) fn suites(path: PathBuf) -> Vec<TestSuite> {
    vec![
        suite(path.clone(), "additional", run_file, run_files),
        suite(path.clone(), "additional::inspected", run_file_inspected, run_files_inspected),
        suite(path.clone(), "additional::revm", run_file_revm, run_files_revm),
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

fn run_file_revm(path: PathBuf) -> Result<(), Failed> {
    run_files_revm(vec![path])
}

fn run_files_revm(paths: Vec<PathBuf>) -> Result<(), Failed> {
    run_with_mode(paths, ExecutionMode::Revm)
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

const EXPECTED_FILES_ENV: &str = "EVM2_FRAME_EXPECTED_FILES";
const EXPECTED_UNITS_ENV: &str = "EVM2_FRAME_EXPECTED_UNITS";
const FRAME_EIPS: [u16; 4] = [8141, 8250, 8272, 7906];

/// Counts for an explicit corpus run; any failure or skipped case prevents qualification.
#[derive(Clone, Copy, Debug, Default)]
struct CorpusSummary {
    /// Number of selected JSON files.
    files: usize,
    /// Number of files with a parse, execution or backend qualification error.
    failed_files: usize,
    /// Executed cases in successful files.
    successful_file_units: usize,
    /// Skipped cases in successful files.
    skipped_units: usize,
}

/// Executes the selected state/blockchain files with shared compilation resources.
///
/// No normal name exclusions or compiled smoke subsets are applied. JIT/AOT reject
/// nonempty-code fallback. Per-file failures are printed and counted so later files still run.
/// Configured fork exclusions are rejected. Setting both `EVM2_FRAME_EXPECTED_FILES`
/// and `EVM2_FRAME_EXPECTED_UNITS` additionally requires the complete selected four-EIP
/// corpus with those pinned totals; otherwise arbitrary explicit folders are supported.
fn execute_paths(mut paths: Vec<PathBuf>, mode: ExecutionMode) -> io::Result<CorpusSummary> {
    validate_fork_selection(optional_env(SKIP_FORKS_ENV)?.as_deref())?;
    let expected = expected_counts(
        optional_env(EXPECTED_FILES_ENV)?.as_deref(),
        optional_env(EXPECTED_UNITS_ENV)?.as_deref(),
    )?;
    paths.sort();
    if paths.is_empty() {
        return Err(io::Error::other("no JSON fixtures selected"));
    }
    if let Some((expected_files, _)) = expected {
        validate_complete_selection(&paths, expected_files)?;
    }
    let resources = ExecutionResources::new(mode).map_err(io::Error::other)?;
    let mut summary = CorpusSummary { files: paths.len(), ..Default::default() };
    for path in paths {
        let result = (|| -> Result<(usize, usize), String> {
            let input = fs::read_to_string(&path).map_err(|error| error.to_string())?;
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
                validate_state_posts(&json)?;
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
    if let Some((_, expected_units)) = expected
        && summary.successful_file_units != expected_units
    {
        return Err(io::Error::other(format!(
            "fixture unit count mismatch: expected {expected_units}, executed {}",
            summary.successful_file_units
        )));
    }
    Ok(summary)
}

fn optional_env(name: &str) -> io::Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(io::Error::other(error)),
    }
}

fn expected_counts(files: Option<&str>, units: Option<&str>) -> io::Result<Option<(usize, usize)>> {
    match (files, units) {
        (None, None) => Ok(None),
        (Some(files), Some(units)) => {
            let files = files.parse::<usize>().map_err(io::Error::other)?;
            let units = units.parse::<usize>().map_err(io::Error::other)?;
            if files == 0 || units == 0 {
                return Err(io::Error::other("expected corpus counts must be positive"));
            }
            Ok(Some((files, units)))
        }
        _ => Err(io::Error::other(format!(
            "{EXPECTED_FILES_ENV} and {EXPECTED_UNITS_ENV} must be supplied together"
        ))),
    }
}

fn validate_fork_selection(configured: Option<&str>) -> io::Result<()> {
    if configured.is_some_and(|value| !value.trim().is_empty()) {
        return Err(io::Error::other(format!(
            "{SKIP_FORKS_ENV} cannot exclude forks during explicit corpus execution"
        )));
    }
    Ok(())
}

fn validate_complete_selection(paths: &[PathBuf], expected_files: usize) -> io::Result<()> {
    if paths.len() != expected_files {
        return Err(io::Error::other(format!(
            "fixture file count mismatch: expected {expected_files}, selected {}",
            paths.len()
        )));
    }
    for eip in FRAME_EIPS {
        let prefix = format!("eip{eip}_");
        if !paths.iter().any(|path| {
            path.components()
                .any(|part| part.as_os_str().to_str().is_some_and(|part| part.starts_with(&prefix)))
        }) {
            return Err(io::Error::other(format!("no fixtures selected for EIP-{eip}")));
        }
    }
    Ok(())
}

fn validate_state_posts(json: &serde_json::Value) -> Result<(), String> {
    let suite = json.as_object().ok_or("fixture is not an object")?;
    for (name, unit) in suite {
        let post = unit.get("post").and_then(|post| post.as_object()).ok_or("missing post map")?;
        if post.is_empty() {
            return Err(format!("state fixture {name} has no post cases"));
        }
        for (fork, posts) in post {
            let spec: SpecName = serde_json::from_value(serde_json::Value::String(fork.clone()))
                .map_err(|error| error.to_string())?;
            let spec = spec.to_spec_id().ok_or_else(|| format!("unsupported state fork {fork}"))?;
            if is_fork_skipped(spec) {
                return Err(format!("state fork {fork} is excluded from execution"));
            }
            if posts.as_array().is_none_or(Vec::is_empty) {
                return Err(format!("state fixture {name} has no post cases for {fork}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_counts_cannot_be_half_configured_or_empty() {
        assert_eq!(expected_counts(None, None).unwrap(), None);
        assert_eq!(expected_counts(Some("419"), Some("1481")).unwrap(), Some((419, 1481)));
        for (files, units) in [
            (Some("419"), None),
            (None, Some("1481")),
            (Some("0"), Some("1481")),
            (Some("419"), Some("0")),
            (Some("invalid"), Some("1481")),
        ] {
            assert!(expected_counts(files, units).is_err());
        }
    }

    #[test]
    fn strict_selection_requires_the_total_and_every_frame_eip() {
        let paths =
            FRAME_EIPS.map(|eip| PathBuf::from(format!("state_tests/eip{eip}_test/a.json")));
        validate_complete_selection(&paths, 4).unwrap();
        assert!(validate_complete_selection(&paths, 5).is_err());
        assert!(validate_complete_selection(&paths[..3], 3).is_err());
    }

    #[test]
    fn state_units_with_no_executable_posts_cannot_report_success() {
        for invalid in [
            json!({"case": {"post": {}}}),
            json!({"case": {"post": {"Bogota": []}}}),
            json!({"case": {"post": {"Constantinople": [{}]}}}),
            json!({"case": {"post": {"unknown": [{}]}}}),
            json!({"case": {"post": {"Bogota": [{}]}}, "empty": {"post": {}}}),
        ] {
            assert!(validate_state_posts(&invalid).is_err());
        }
        validate_state_posts(&json!({"case": {"post": {"Bogota": [{}]}}})).unwrap();
    }

    #[test]
    fn configured_fork_exclusions_cannot_silently_skip_state_posts() {
        for configured in [None, Some(""), Some("   ")] {
            validate_fork_selection(configured).unwrap();
        }
        for configured in [Some("Bogota"), Some("Amsterdam,Bogota")] {
            assert!(validate_fork_selection(configured).is_err());
        }
    }
}
