//! Conformance tests: generate runtime code from real-world .proto files
//! and verify it compiles.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Parse proto sources with the analyzer, generate runtime code, compile in a temp project.
fn conformance_check(proto_source: &str, test_name: &str) {
    let fds = protoc_rs_analyzer::analyze(proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: analyzer failed: {e}"));
    let files = protoc_rs_codegen::generate_rust_runtime(&fds)
        .unwrap_or_else(|e| panic!("{test_name}: codegen failed: {e}"));

    if files.is_empty() {
        return; // No output (e.g., file has only imports)
    }

    let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{test_name}: tempdir failed: {e}"));
    let project_dir = tmp.path().join(test_name);
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap_or_else(|e| panic!("{test_name}: mkdir failed: {e}"));

    let runtime_path = workspace_root().join("runtime");
    let cargo_toml = format!(
        r#"[package]
name = "conformance-check"
version = "0.1.0"
edition = "2021"

[dependencies]
protoc-rs-runtime = {{ path = "{}" }}
"#,
        runtime_path.display()
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml).unwrap();

    let mut lib_rs = String::from("#![allow(warnings)]\n");
    for (filename, source) in &files {
        let mod_name = filename.trim_end_matches(".rs").replace('.', "_");
        let mod_path = src_dir.join(format!("{mod_name}.rs"));
        std::fs::write(&mod_path, source).unwrap();
        lib_rs.push_str(&format!("pub mod {mod_name};\n"));
    }
    std::fs::write(src_dir.join("lib.rs"), &lib_rs).unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&project_dir)
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: cargo check failed: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("=== {test_name}: FAILED ===");
        eprintln!("--- lib.rs ---");
        eprintln!("{lib_rs}");
        for (filename, source) in &files {
            eprintln!("--- {filename} ---");
            eprintln!("{source}");
        }
        eprintln!("--- stderr ---");
        eprintln!("{stderr}");
        panic!("{test_name}: conformance check failed");
    }
}

// ---------------------------------------------------------------------------
// Testdata protos
// ---------------------------------------------------------------------------

#[test]
fn conformance_simple() {
    conformance_check(
        &std::fs::read_to_string(workspace_root().join("testdata/simple.proto")).unwrap(),
        "simple",
    );
}

// complex.proto references types from other files (WatchUsersRequest etc.)
// that aren't self-contained. Skipped for single-file conformance test.
// TODO: Use analyze_files() with proper file resolver for multi-file tests.

// proto2_features.proto uses Group fields which aren't supported in runtime codegen yet.
// TODO: Add group field support to runtime codegen.

// ---------------------------------------------------------------------------
// Upstream protos (parser compatibility tests)
// ---------------------------------------------------------------------------

#[test]
fn conformance_upstream_enums() {
    conformance_check(
        &std::fs::read_to_string(workspace_root().join("testdata/upstream/enums.proto")).unwrap(),
        "upstream_enums",
    );
}

#[test]
fn conformance_upstream_nested() {
    conformance_check(
        &std::fs::read_to_string(workspace_root().join("testdata/upstream/nested.proto")).unwrap(),
        "upstream_nested",
    );
}

#[test]
fn conformance_upstream_parent() {
    conformance_check(
        &std::fs::read_to_string(workspace_root().join("testdata/upstream/parent.proto")).unwrap(),
        "upstream_parent",
    );
}
