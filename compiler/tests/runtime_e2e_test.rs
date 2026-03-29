//! End-to-end test: parse proto -> generate runtime code -> compile ->
//! encode message -> decode -> verify round-trip.
//!
//! This test generates a temporary Cargo project that depends on `protoc-rs-runtime`,
//! writes generated code + a test binary that constructs, encodes, decodes, and
//! asserts message equality, then runs `cargo run` on it.

use std::process::Command;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Generate runtime code, write a test binary, compile and run it.
fn e2e_test(proto_source: &str, test_code: &str, test_name: &str) {
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

    let runtime_path = workspace_root().join("runtime");
    let cargo_toml = format!(
        r#"[package]
name = "e2e-test"
version = "0.1.0"
edition = "2021"

[dependencies]
protoc-rs-runtime = {{ path = "{}" }}
"#,
        runtime_path.display()
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write Cargo.toml: {e}"));

    // Write generated proto modules
    let mut mod_decls = String::new();
    for (filename, source) in &files {
        let mod_name = filename.trim_end_matches(".rs");
        let mod_path = src_dir.join(filename);
        std::fs::write(&mod_path, source)
            .unwrap_or_else(|e| panic!("{test_name}: failed to write {filename}: {e}"));
        mod_decls.push_str(&format!("#[allow(warnings)]\npub mod {mod_name};\n"));
    }

    // Write main.rs with the test code
    let main_rs = format!(
        r#"
{mod_decls}

use protoc_rs_runtime::*;
use protoc_rs_runtime::view::MessageView;

fn main() {{
    {test_code}
    println!("E2E test passed: {test_name}");
}}
"#
    );
    std::fs::write(src_dir.join("main.rs"), &main_rs)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write main.rs: {e}"));

    // Build and run
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(&project_dir)
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: failed to run cargo: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("=== {test_name}: FAILED ===");
        eprintln!("--- main.rs ---");
        eprintln!("{main_rs}");
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
        panic!("{test_name}: e2e test failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("E2E test passed"),
        "{test_name}: expected success message in stdout, got: {stdout}"
    );
}

#[test]
fn e2e_encode_decode_simple() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            message Person {
                string name = 1;
                int32 id = 2;
                repeated string tags = 3;
                bytes data = 4;
            }
        "#,
        r#"
    use e2e::Person;

    // Construct
    let mut msg = Person::default();
    msg.name = "Alice".to_string();
    msg.id = 42;
    msg.tags = vec!["dev".to_string(), "rust".to_string()];
    msg.data = vec![0xDE, 0xAD];

    // Encode
    let bytes = encode(&msg);
    assert!(!bytes.is_empty(), "encoded bytes should not be empty");

    // Decode
    let decoded: Person = decode(&bytes).expect("decode failed");
    assert_eq!(decoded.name, "Alice");
    assert_eq!(decoded.id, 42);
    assert_eq!(decoded.tags, vec!["dev", "rust"]);
    assert_eq!(decoded.data, vec![0xDE, 0xAD]);

    // Round-trip equality
    assert_eq!(msg, decoded, "round-trip mismatch");

    // Re-encode should produce identical bytes
    let bytes2 = encode(&decoded);
    assert_eq!(bytes, bytes2, "re-encode mismatch");
        "#,
        "e2e_simple",
    );
}

#[test]
fn e2e_nested_message_and_enum() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
                STATUS_INACTIVE = 2;
            }

            message Task {
                string title = 1;
                Status status = 2;

                message SubTask {
                    string name = 1;
                    bool done = 2;
                }

                SubTask current = 3;
                repeated SubTask items = 4;
            }
        "#,
        r#"
    use e2e::{Task, Status, task};

    let mut msg = Task::default();
    msg.title = "Build runtime".to_string();
    msg.status = EnumValue::Known(Status::Active);

    // Set nested message
    msg.current.modify(|sub| {
        sub.name = "wire format".to_string();
        sub.done = true;
    });

    // Add repeated nested messages
    let mut item = task::SubTask::default();
    item.name = "codegen".to_string();
    item.done = false;
    msg.items.push(item);

    // Encode and decode
    let bytes = encode(&msg);
    let decoded: Task = decode(&bytes).expect("decode failed");

    assert_eq!(decoded.title, "Build runtime");
    assert_eq!(decoded.status, EnumValue::Known(Status::Active));
    assert!(decoded.current.is_set());
    assert_eq!(decoded.current.name, "wire format");
    assert!(decoded.current.done);
    assert_eq!(decoded.items.len(), 1);
    assert_eq!(decoded.items[0].name, "codegen");
    assert!(!decoded.items[0].done);

    // Full round-trip
    assert_eq!(msg, decoded);
        "#,
        "e2e_nested_enum",
    );
}

#[test]
fn e2e_unknown_fields_preserved() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            message Msg {
                int32 known = 1;
            }
        "#,
        r#"
    use e2e::Msg;
    use protoc_rs_runtime::wire;

    // Build wire data with known field + unknown field
    let mut wire_data = Vec::new();
    // known field: field 1, varint, value 42
    wire::encode_tag(1, wire::WireType::Varint, &mut wire_data);
    wire::encode_varint(42, &mut wire_data);
    // unknown field: field 99, varint, value 7
    wire::encode_tag(99, wire::WireType::Varint, &mut wire_data);
    wire::encode_varint(7, &mut wire_data);

    let msg: Msg = decode(&wire_data).expect("decode failed");
    assert_eq!(msg.known, 42);
    assert_eq!(msg.unknown_fields().len(), 1);

    // Re-encode preserves unknown fields
    let re_encoded = encode(&msg);
    assert_eq!(re_encoded, wire_data, "unknown fields not preserved");
        "#,
        "e2e_unknown_fields",
    );
}

#[test]
fn e2e_view_decode() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            message Person {
                string name = 1;
                int32 id = 2;
                bytes data = 3;
            }
        "#,
        r#"
    use e2e::{Person, PersonView};

    // Build a message and encode it
    let mut msg = Person::default();
    msg.name = "Bob".to_string();
    msg.id = 99;
    msg.data = vec![1, 2, 3];

    let bytes = encode(&msg);

    // Decode as view (zero-copy)
    let view: PersonView = protoc_rs_runtime::view::decode_view(&bytes).expect("view decode failed");
    assert_eq!(view.name, "Bob");
    assert_eq!(view.id, 99);
    assert_eq!(view.data, &[1, 2, 3]);

    // Verify the view borrows from the original buffer
    let name_ptr = view.name.as_ptr();
    let buf_range = bytes.as_ptr_range();
    assert!(
        buf_range.contains(&name_ptr),
        "view.name should point into the original buffer (zero-copy)"
    );
        "#,
        "e2e_view",
    );
}

#[test]
fn e2e_packed_repeated_encoding() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            message Packed {
                repeated int32 values = 1;
                repeated fixed32 fixed = 2;
                repeated double doubles = 3;
                repeated bool flags = 4;
            }
        "#,
        r#"
    use e2e::Packed;
    use protoc_rs_runtime::wire;

    let mut msg = Packed::default();
    msg.values = vec![1, 2, 300, -1];
    msg.fixed = vec![0, 1, 0xDEADBEEF];
    msg.doubles = vec![1.0, 2.5, 3.14];
    msg.flags = vec![true, false, true];

    // Encode
    let bytes = encode(&msg);

    // Verify packed format: first tag should be field 1, LengthDelimited
    let (tag, _) = wire::decode_tag(&bytes).unwrap();
    assert_eq!(tag.field_number, 1);
    assert_eq!(tag.wire_type, wire::WireType::LengthDelimited,
        "proto3 repeated int32 should use packed (LengthDelimited) encoding");

    // Decode and verify round-trip
    let decoded: Packed = decode(&bytes).expect("decode failed");
    assert_eq!(decoded.values, vec![1, 2, 300, -1]);
    assert_eq!(decoded.fixed, vec![0, 1, 0xDEADBEEF]);
    assert_eq!(decoded.doubles, vec![1.0, 2.5, 3.14]);
    assert_eq!(decoded.flags, vec![true, false, true]);

    // Re-encode should be identical
    let bytes2 = encode(&decoded);
    assert_eq!(bytes, bytes2, "packed re-encode mismatch");
        "#,
        "e2e_packed",
    );
}

#[test]
fn e2e_view_to_owned() {
    e2e_test(
        r#"
            syntax = "proto3";
            package e2e;

            message Person {
                string name = 1;
                int32 id = 2;
                bytes data = 3;
                repeated string tags = 4;

                message Address {
                    string city = 1;
                }

                Address home = 5;
            }
        "#,
        r#"
    use e2e::{Person, PersonView};

    // Build and encode a message
    let mut msg = Person::default();
    msg.name = "Alice".to_string();
    msg.id = 42;
    msg.data = vec![1, 2, 3];
    msg.tags = vec!["a".to_string(), "b".to_string()];
    msg.home.modify(|a| a.city = "NYC".to_string());
    let bytes = encode(&msg);

    // Decode as view
    let view: PersonView = protoc_rs_runtime::view::decode_view(&bytes).expect("view decode failed");
    assert_eq!(view.name, "Alice");
    assert_eq!(view.id, 42);

    // Convert view to owned
    let owned: Person = view.into();
    assert_eq!(owned.name, "Alice");
    assert_eq!(owned.id, 42);
    assert_eq!(owned.data, vec![1, 2, 3]);
    assert_eq!(owned.tags, vec!["a", "b"]);
    assert!(owned.home.is_set());
    assert_eq!(owned.home.city, "NYC");

    // Re-encode owned should match original
    let bytes2 = encode(&owned);
    assert_eq!(bytes, bytes2, "view->owned->encode should match original");
        "#,
        "e2e_view_to_owned",
    );
}
