//! Keep the C++ shim's LLVM headers aligned with the llvm-sys 231 library contract.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) struct LlvmConfig {
    pub(crate) path: PathBuf,
    pub(crate) version: String,
    pub(crate) cxxflags: String,
}

pub(crate) fn config_candidates(
    prefix: Option<OsString>,
    explicit: Option<OsString>,
) -> Vec<PathBuf> {
    // llvm-sys selects this prefix for the linked libraries. Never take C++ headers from
    // another installation just because it occurs earlier in PATH or an override is stale.
    if let Some(prefix) = prefix {
        return vec![PathBuf::from(prefix).join("bin").join("llvm-config")];
    }
    if let Some(explicit) = explicit {
        return vec![PathBuf::from(explicit)];
    }
    vec!["llvm-config-23".into(), "llvm-config".into()]
}

pub(crate) fn detect_config(
    candidates: &[PathBuf],
    mut query: impl FnMut(&Path, &str) -> Result<String, String>,
) -> Result<LlvmConfig, String> {
    let mut diagnostics = Vec::new();
    for candidate in candidates {
        let attempt = (|| {
            let version = query(candidate, "--version")?;
            let version = version.trim();
            let mut numbers = version.split('.');
            if numbers.next().and_then(|n| n.parse::<u32>().ok()) != Some(23)
                || numbers.next().and_then(|n| n.parse::<u32>().ok()) != Some(1)
            {
                return Err(format!("reported LLVM {version}; llvm-sys 231 requires LLVM 23.1"));
            }
            let cxxflags = query(candidate, "--cxxflags")?;
            Ok(LlvmConfig { path: candidate.clone(), version: version.to_owned(), cxxflags })
        })();
        match attempt {
            Ok(config) => return Ok(config),
            Err(error) => diagnostics.push(format!("{}: {error}", candidate.display())),
        }
    }
    Err(diagnostics.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_library_prefix_takes_precedence_over_path_and_explicit_override() {
        assert_eq!(
            config_candidates(Some("/llvm23".into()), Some("/llvm22/llvm-config".into())),
            vec![PathBuf::from("/llvm23/bin/llvm-config")]
        );
        assert_eq!(
            config_candidates(None, Some("/custom/llvm-config".into())),
            vec![PathBuf::from("/custom/llvm-config")]
        );
        assert_eq!(
            config_candidates(None, None),
            vec![PathBuf::from("llvm-config-23"), PathBuf::from("llvm-config")]
        );
    }

    #[test]
    fn mismatched_default_headers_never_reach_the_cpp_compiler() {
        let result = detect_config(&["llvm-config".into()], |_, argument| {
            assert_eq!(argument, "--version");
            Ok("22.1.8\n".into())
        });
        assert_eq!(
            result.err().unwrap(),
            "llvm-config: reported LLVM 22.1.8; llvm-sys 231 requires LLVM 23.1"
        );
    }

    #[test]
    fn broken_candidate_does_not_shadow_a_matching_version() {
        let config = detect_config(&["broken".into(), "good".into()], |path, argument| {
            if path == Path::new("broken") {
                return Err("controlled process failure".into());
            }
            Ok(if argument == "--version" { "23.1.4\n" } else { "-I/llvm23/include\n" }.into())
        })
        .unwrap();
        assert_eq!(config.path, Path::new("good"));
        assert_eq!(config.version, "23.1.4");
        assert_eq!(config.cxxflags, "-I/llvm23/include\n");
    }

    #[test]
    fn failed_cxxflags_does_not_shadow_a_usable_candidate() {
        let config = detect_config(&["broken".into(), "good".into()], |path, argument| {
            if argument == "--version" {
                return Ok("23.1.4".into());
            }
            if path == Path::new("broken") {
                return Err("--cxxflags exited 1".into());
            }
            Ok("-I/llvm23/include".into())
        })
        .unwrap();
        assert_eq!(config.path, Path::new("good"));
    }
}
