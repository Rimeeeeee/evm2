#![allow(missing_docs)]

mod build_support;

fn main() {
    println!("cargo:rerun-if-changed=cpp/lib.cpp");
    println!("cargo:rerun-if-changed=build_support.rs");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PATH");

    let candidates = build_support::config_candidates(
        std::env::var_os("LLVM_SYS_231_PREFIX"),
        std::env::var_os("LLVM_CONFIG_PATH"),
    );
    let config = build_support::detect_config(&candidates, query)
        .unwrap_or_else(|error| panic!("cannot select LLVM 23.1 C++ headers:\n{error}"));
    println!("cargo:warning=LLVM C++ shim uses {} ({})", config.path.display(), config.version);

    cc::Build::new()
        .cpp(true)
        .flags(config.cxxflags.split_whitespace())
        .flag("-w")
        .file("cpp/lib.cpp")
        .compile("evm2_jit_llvm_cpp");
}

fn query(llvm_config: &std::path::Path, argument: &str) -> Result<String, String> {
    let output = std::process::Command::new(llvm_config)
        .arg(argument)
        .output()
        .map_err(|error| format!("failed to run {argument}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{argument} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{argument} is not valid UTF-8: {error}"))
}
