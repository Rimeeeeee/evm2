//! Full explicit-corpus qualification with shared compiled resources and no smoke subsets.

use crate::{
    blockchaintest::{NoopHook, execute as block_execute},
    execute::{self, ExecuteConfig as StateConfig},
    execution::{ExecutionMode, ExecutionResources},
    filter::NameFilter,
    fixture_io,
};
use evm2::evm::DbStatsCounts;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Counts for an explicit corpus run; any failure or skipped case prevents qualification.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConformanceSummary {
    /// Number of selected JSON files.
    pub files: usize,
    /// Number of files with a parse, execution or backend qualification error.
    pub failed_files: usize,
    /// Executed cases in successful files.
    pub successful_file_units: usize,
    /// Skipped cases in successful files.
    pub skipped_units: usize,
}

/// Executes every state/blockchain JSON file below `root`, sharing the compilation cache.
///
/// No normal name exclusions or compiled smoke subsets are applied. JIT/AOT reject
/// nonempty-code fallback. Per-file failures are printed and counted so later files still run.
pub fn execute_frame_corpus(root: &Path, mode: ExecutionMode) -> io::Result<ConformanceSummary> {
    let mut paths = Vec::new();
    files(root, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(io::Error::other("no JSON fixtures selected"));
    }
    let resources = ExecutionResources::new(mode).map_err(io::Error::other)?;
    let mut summary = ConformanceSummary { files: paths.len(), ..Default::default() };
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

fn files(path: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            files(&entry?.path(), output)?;
        }
    } else if path.extension().is_some_and(|extension| extension == "json") {
        output.push(path.to_owned());
    }
    Ok(())
}
