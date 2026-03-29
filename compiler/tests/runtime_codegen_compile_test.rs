//! Compile-check tests for runtime-backed codegen.
//!
//! For each test proto, we:
//! 1. Parse with our analyzer
//! 2. Generate Rust with runtime-backed codegen
//! 3. Write to a temporary Cargo project with `protoc-rs-runtime` as a dependency
//! 4. Run `cargo check` and assert it succeeds

use std::process::Command;

/// Get the workspace root (parent of the compiler crate).
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Create a temporary Cargo project, write generated code, and `cargo check` it.
fn compile_check_runtime(proto_source: &str, test_name: &str) {
    let fds = protoc_rs_analyzer::analyze(proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: analyzer failed: {e}"));
    let files = protoc_rs_codegen::generate_rust_runtime(&fds)
        .unwrap_or_else(|e| panic!("{test_name}: codegen failed: {e}"));

    assert!(!files.is_empty(), "{test_name}: codegen produced no files");

    let tmp = tempfile::tempdir()
        .unwrap_or_else(|e| panic!("{test_name}: failed to create tempdir: {e}"));
    let project_dir = tmp.path().join(test_name);
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .unwrap_or_else(|e| panic!("{test_name}: failed to create src dir: {e}"));

    // Write Cargo.toml pointing to our runtime crate via path dependency
    let runtime_path = workspace_root().join("runtime");
    let cargo_toml = format!(
        r#"[package]
name = "runtime-compile-check"
version = "0.1.0"
edition = "2021"

[dependencies]
protoc-rs-runtime = {{ path = "{}" }}
"#,
        runtime_path.display()
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write Cargo.toml: {e}"));

    // Write lib.rs that includes all generated modules
    let mut lib_rs = String::from("#![allow(warnings)]\n");
    for (filename, source) in &files {
        let mod_name = filename.trim_end_matches(".rs");
        let mod_path = src_dir.join(filename);
        std::fs::write(&mod_path, source)
            .unwrap_or_else(|e| panic!("{test_name}: failed to write {filename}: {e}"));
        lib_rs.push_str(&format!("pub mod {mod_name};\n"));
    }
    std::fs::write(src_dir.join("lib.rs"), &lib_rs)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write lib.rs: {e}"));

    // Run cargo check
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&project_dir)
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: failed to run cargo check: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        eprintln!("=== {test_name}: cargo check FAILED ===");
        eprintln!("--- lib.rs ---");
        eprintln!("{lib_rs}");
        for (filename, source) in &files {
            eprintln!("--- {filename} ---");
            eprintln!("{source}");
        }
        eprintln!("--- stderr ---");
        eprintln!("{stderr}");
        if !stdout.is_empty() {
            eprintln!("--- stdout ---");
            eprintln!("{stdout}");
        }
        panic!("{test_name}: generated code failed to compile");
    }
}

#[test]
fn runtime_compile_all_scalar_types() {
    compile_check_runtime(
        r#"
            syntax = "proto3";
            package test_scalars;

            message AllScalars {
                double f_double = 1;
                float f_float = 2;
                int32 f_int32 = 3;
                int64 f_int64 = 4;
                uint32 f_uint32 = 5;
                uint64 f_uint64 = 6;
                sint32 f_sint32 = 7;
                sint64 f_sint64 = 8;
                fixed32 f_fixed32 = 9;
                fixed64 f_fixed64 = 10;
                sfixed32 f_sfixed32 = 11;
                sfixed64 f_sfixed64 = 12;
                bool f_bool = 13;
                string f_string = 14;
                bytes f_bytes = 15;
            }
        "#,
        "rt_scalars",
    );
}

#[test]
fn runtime_compile_enum_and_message() {
    compile_check_runtime(
        r#"
            syntax = "proto3";
            package test_enum;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
            }

            message Person {
                string name = 1;
                int32 id = 2;
                Status status = 3;
                repeated string tags = 4;
                optional double score = 5;
            }
        "#,
        "rt_enum_msg",
    );
}

#[test]
fn runtime_compile_nested_and_oneof() {
    compile_check_runtime(
        r#"
            syntax = "proto3";
            package test_nested;

            message Outer {
                message Inner {
                    int32 value = 1;
                }
                Inner inner = 1;
                repeated Inner items = 2;

                oneof payload {
                    string text = 3;
                    int32 number = 4;
                }
            }
        "#,
        "rt_nested_oneof",
    );
}

#[test]
fn runtime_compile_map_fields() {
    compile_check_runtime(
        r#"
            syntax = "proto3";
            package test_maps;

            message Config {
                map<string, string> str_map = 1;
                map<int32, string> int_map = 2;
                map<string, int64> val_map = 3;
            }
        "#,
        "rt_maps",
    );
}

#[test]
fn runtime_compile_complex() {
    compile_check_runtime(
        r#"
            syntax = "proto3";
            package test_complex;

            enum Priority {
                PRIORITY_UNKNOWN = 0;
                PRIORITY_LOW = 1;
                PRIORITY_HIGH = 2;
            }

            message Task {
                string title = 1;
                int32 id = 2;
                Priority priority = 3;
                repeated string labels = 4;
                optional bytes data = 5;
                map<string, string> metadata = 6;

                message SubTask {
                    string name = 1;
                    bool done = 2;
                }

                repeated SubTask sub_tasks = 7;
                SubTask current = 8;

                oneof contact {
                    string email = 9;
                    string phone = 10;
                }
            }
        "#,
        "rt_complex",
    );
}

/// Compile-check with serde JSON support enabled.
fn compile_check_runtime_serde(proto_source: &str, test_name: &str) {
    let fds = protoc_rs_analyzer::analyze(proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: analyzer failed: {e}"));
    let options = protoc_rs_codegen::RuntimeCodeGenOptions { emit_serde: true };
    let files = protoc_rs_codegen::generate_rust_runtime_with_options(&fds, &options)
        .unwrap_or_else(|e| panic!("{test_name}: codegen failed: {e}"));

    assert!(!files.is_empty(), "{test_name}: codegen produced no files");

    let tmp = tempfile::tempdir()
        .unwrap_or_else(|e| panic!("{test_name}: failed to create tempdir: {e}"));
    let project_dir = tmp.path().join(test_name);
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .unwrap_or_else(|e| panic!("{test_name}: failed to create src dir: {e}"));

    let runtime_path = workspace_root().join("runtime");
    let cargo_toml = format!(
        r#"[package]
name = "runtime-serde-check"
version = "0.1.0"
edition = "2021"

[dependencies]
protoc-rs-runtime = {{ path = "{}", features = ["json"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        runtime_path.display()
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write Cargo.toml: {e}"));

    let mut lib_rs = String::from("#![allow(warnings)]\n");
    for (filename, source) in &files {
        let mod_name = filename.trim_end_matches(".rs");
        let mod_path = src_dir.join(filename);
        std::fs::write(&mod_path, source)
            .unwrap_or_else(|e| panic!("{test_name}: failed to write {filename}: {e}"));
        lib_rs.push_str(&format!("pub mod {mod_name};\n"));
    }
    std::fs::write(src_dir.join("lib.rs"), &lib_rs)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write lib.rs: {e}"));

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&project_dir)
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: failed to run cargo check: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("=== {test_name}: cargo check FAILED ===");
        eprintln!("--- lib.rs ---");
        eprintln!("{lib_rs}");
        for (filename, source) in &files {
            eprintln!("--- {filename} ---");
            eprintln!("{source}");
        }
        eprintln!("--- stderr ---");
        eprintln!("{stderr}");
        panic!("{test_name}: generated code failed to compile");
    }
}

#[test]
fn runtime_compile_serde_json() {
    compile_check_runtime_serde(
        r#"
            syntax = "proto3";
            package test_serde;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
            }

            message Person {
                string name = 1;
                int32 id = 2;
                Status status = 3;
                repeated string tags = 4;
                optional bytes data = 5;

                message Address {
                    string city = 1;
                }

                Address home = 6;

                oneof contact {
                    string email = 7;
                    string phone = 8;
                }
            }
        "#,
        "rt_serde",
    );
}
