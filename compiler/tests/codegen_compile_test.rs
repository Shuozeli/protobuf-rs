//! Compile-check tests: verify that our codegen output actually compiles
//! with prost's derive macros.
//!
//! For each test proto, we:
//! 1. Parse with our analyzer
//! 2. Generate Rust with our codegen
//! 3. Write to a temporary Cargo project with `prost` as a dependency
//! 4. Run `cargo check` and assert it succeeds
//!
//! This catches annotation bugs that structural comparison would miss.

use std::process::Command;

/// Create a temporary Cargo project, write generated code, and `cargo check` it.
fn compile_check(proto_source: &str, test_name: &str) {
    let fds = protoc_rs_analyzer::analyze(proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: analyzer failed: {e}"));
    let files = protoc_rs_codegen::generate_rust(&fds)
        .unwrap_or_else(|e| panic!("{test_name}: codegen failed: {e}"));

    assert!(!files.is_empty(), "{test_name}: codegen produced no files");

    let tmp = tempfile::tempdir()
        .unwrap_or_else(|e| panic!("{test_name}: failed to create tempdir: {e}"));
    let project_dir = tmp.path().join(test_name);
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .unwrap_or_else(|e| panic!("{test_name}: failed to create src dir: {e}"));

    // Write Cargo.toml
    let cargo_toml = r#"[package]
name = "compile-check"
version = "0.1.0"
edition = "2021"

[dependencies]
prost = "0.14"
"#;
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write Cargo.toml: {e}"));

    // Write lib.rs that includes all generated modules
    let mut lib_rs = String::new();
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

// ---------------------------------------------------------------------------
// Test cases -- each verifies that generated code actually compiles with prost
// ---------------------------------------------------------------------------

#[test]
fn compile_all_scalar_types() {
    compile_check(
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
        "scalars",
    );
}

#[test]
fn compile_proto3_optional() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_optional;

            message WithOptional {
                optional int32 opt_int = 1;
                optional string opt_str = 2;
                optional double opt_dbl = 3;
                optional bool opt_bool = 4;
                optional bytes opt_bytes = 5;
            }
        "#,
        "optional",
    );
}

#[test]
fn compile_repeated_and_bytes() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_repeated;

            message WithRepeated {
                repeated int32 ids = 1;
                repeated string names = 2;
                repeated bytes blobs = 3;
                repeated double scores = 4;
            }
        "#,
        "repeated",
    );
}

#[test]
fn compile_enum() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_enum;

            enum Color {
                COLOR_UNKNOWN = 0;
                COLOR_RED = 1;
                COLOR_GREEN = 2;
                COLOR_BLUE = 3;
            }

            message WithEnum {
                Color color = 1;
                repeated Color favorites = 2;
                optional Color maybe_color = 3;
            }
        "#,
        "enums",
    );
}

#[test]
fn compile_oneof() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_oneof;

            message WithOneof {
                string common = 1;
                oneof payload {
                    string text = 2;
                    int32 number = 3;
                    bool flag = 4;
                    bytes data = 5;
                }
            }
        "#,
        "oneof",
    );
}

#[test]
fn compile_nested_message() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_nested;

            message Outer {
                message Inner {
                    int32 value = 1;
                    string label = 2;
                }
                Inner inner = 1;
                repeated Inner items = 2;
                optional Inner maybe = 3;
            }
        "#,
        "nested",
    );
}

#[test]
fn compile_map_fields() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_maps;

            message WithMaps {
                map<string, string> str_map = 1;
                map<int32, string> int_map = 2;
                map<string, int64> val_map = 3;
                map<string, bool> bool_map = 4;
            }
        "#,
        "maps",
    );
}

#[test]
fn compile_keyword_fields() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_keywords;

            message Keywords {
                string type = 1;
                int32 ref = 2;
                bool match = 3;
                string self = 4;
                int32 super = 5;
                string mod = 6;
            }
        "#,
        "keywords",
    );
}

#[test]
fn compile_proto2() {
    compile_check(
        r#"
            syntax = "proto2";
            package test_proto2;

            message Proto2Msg {
                optional int32 opt_field = 1;
                required string req_field = 2;
                repeated int32 rep_field = 3;
                optional bool opt_bool = 4;
            }
        "#,
        "proto2",
    );
}

#[test]
fn compile_complex_combined() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_complex;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
                STATUS_INACTIVE = 2;
            }

            message Person {
                string name = 1;
                int32 id = 2;
                string email = 3;
                Status status = 4;
                repeated string tags = 5;
                optional double score = 6;

                message Address {
                    string street = 1;
                    string city = 2;
                    int32 zip = 3;
                }

                Address home_address = 7;
                map<string, string> metadata = 8;

                oneof contact {
                    string phone = 9;
                    string fax = 10;
                }

                repeated Address other_addresses = 11;
            }
        "#,
        "complex",
    );
}

#[test]
fn compile_deeply_nested() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_deep;

            message Level1 {
                message Level2 {
                    message Level3 {
                        string value = 1;
                    }
                    Level3 child = 1;
                    string name = 2;
                }
                Level2 child = 1;
                int32 id = 2;
            }
        "#,
        "deep_nested",
    );
}

#[test]
fn compile_multiple_oneofs() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_multi_oneof;

            message MultiOneof {
                string common = 1;

                oneof first_choice {
                    string text_a = 2;
                    int32 num_a = 3;
                }

                oneof second_choice {
                    bool flag_b = 4;
                    double val_b = 5;
                }
            }
        "#,
        "multi_oneof",
    );
}

#[test]
fn compile_enum_with_alias() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_alias;

            enum AliasedEnum {
                option allow_alias = true;
                ALIAS_UNKNOWN = 0;
                ALIAS_FIRST = 1;
                ALIAS_SYNONYM = 1;
                ALIAS_SECOND = 2;
            }

            message WithAlias {
                AliasedEnum value = 1;
            }
        "#,
        "alias",
    );
}

#[test]
fn compile_message_only_maps_and_enums() {
    compile_check(
        r#"
            syntax = "proto3";
            package test_map_enum;

            enum Priority {
                PRIORITY_UNKNOWN = 0;
                PRIORITY_LOW = 1;
                PRIORITY_HIGH = 2;
            }

            message Config {
                map<string, int32> settings = 1;
                map<string, Priority> priorities = 2;
            }
        "#,
        "map_enum",
    );
}
