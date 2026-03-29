//! Wire compatibility test: encode with our runtime, decode with prost, and vice versa.
//!
//! This proves our wire format is compatible with the protoc/prost ecosystem.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Generate both prost and runtime code for the same proto, write a test binary
/// that cross-validates encoding between the two.
fn wire_compat_test(proto_source: &str, test_code: &str, test_name: &str) {
    // Generate runtime code
    let fds = protoc_rs_analyzer::analyze(proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: analyzer failed: {e}"));
    let runtime_files = protoc_rs_codegen::generate_rust_runtime(&fds)
        .unwrap_or_else(|e| panic!("{test_name}: runtime codegen failed: {e}"));

    // Generate prost code
    let prost_files = protoc_rs_codegen::generate_rust(&fds)
        .unwrap_or_else(|e| panic!("{test_name}: prost codegen failed: {e}"));

    let tmp = tempfile::tempdir()
        .unwrap_or_else(|e| panic!("{test_name}: tempdir failed: {e}"));
    let project_dir = tmp.path().join(test_name);
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let runtime_path = workspace_root().join("runtime");
    let cargo_toml = format!(
        r#"[package]
name = "wire-compat-test"
version = "0.1.0"
edition = "2021"

[dependencies]
protoc-rs-runtime = {{ path = "{}" }}
prost = "0.14"
"#,
        runtime_path.display()
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml).unwrap();

    // Write runtime modules under rt_ prefix
    let mut mod_decls = String::new();
    for (filename, source) in &runtime_files {
        let mod_name = format!("rt_{}", filename.trim_end_matches(".rs").replace('.', "_"));
        std::fs::write(src_dir.join(format!("{mod_name}.rs")), source).unwrap();
        mod_decls.push_str(&format!("#[allow(warnings)]\npub mod {mod_name};\n"));
    }

    // Write prost modules under prost_ prefix
    for (filename, source) in &prost_files {
        let mod_name = format!("prost_{}", filename.trim_end_matches(".rs").replace('.', "_"));
        std::fs::write(src_dir.join(format!("{mod_name}.rs")), source).unwrap();
        mod_decls.push_str(&format!("#[allow(warnings)]\npub mod {mod_name};\n"));
    }

    let main_rs = format!(
        r#"
{mod_decls}

use protoc_rs_runtime::*;

fn main() {{
    {test_code}
    println!("Wire compat test passed: {test_name}");
}}
"#
    );
    std::fs::write(src_dir.join("main.rs"), &main_rs).unwrap();

    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(&project_dir)
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: cargo run failed: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("=== {test_name}: FAILED ===");
        eprintln!("--- main.rs ---");
        eprintln!("{main_rs}");
        eprintln!("--- stderr ---");
        eprintln!("{stderr}");
        panic!("{test_name}: wire compat test failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Wire compat test passed"), "{test_name}: missing success message");
}

#[test]
fn wire_compat_scalars() {
    wire_compat_test(
        r#"
            syntax = "proto3";
            package compat;

            message Scalars {
                int32 i = 1;
                string s = 2;
                bool b = 3;
                double d = 4;
                bytes data = 5;
            }
        "#,
        r#"
    // --- Encode with our runtime, decode with prost ---
    {
        let mut rt_msg = rt_compat::Scalars::default();
        rt_msg.i = 42;
        rt_msg.s = "hello".to_string();
        rt_msg.b = true;
        rt_msg.d = 3.14;
        rt_msg.data = vec![1, 2, 3];

        let wire = encode(&rt_msg);

        // Decode with prost
        use prost::Message as _;
        let prost_msg = prost_compat::Scalars::decode(wire.as_slice())
            .expect("prost decode of runtime-encoded bytes failed");

        assert_eq!(prost_msg.i, 42);
        assert_eq!(prost_msg.s, "hello");
        assert_eq!(prost_msg.b, true);
        assert_eq!(prost_msg.d, 3.14);
        assert_eq!(prost_msg.data, vec![1, 2, 3]);
    }

    // --- Encode with prost, decode with our runtime ---
    {
        use prost::Message as _;
        let mut prost_msg = prost_compat::Scalars::default();
        prost_msg.i = 99;
        prost_msg.s = "world".to_string();
        prost_msg.b = false;
        prost_msg.d = 2.718;
        prost_msg.data = vec![4, 5, 6];

        let mut wire = Vec::new();
        prost_msg.encode(&mut wire).unwrap();

        let rt_msg: rt_compat::Scalars = decode(&wire)
            .expect("runtime decode of prost-encoded bytes failed");

        assert_eq!(rt_msg.i, 99);
        assert_eq!(rt_msg.s, "world");
        assert_eq!(rt_msg.b, false);
        assert_eq!(rt_msg.d, 2.718);
        assert_eq!(rt_msg.data, vec![4, 5, 6]);
    }
        "#,
        "compat_scalars",
    );
}

#[test]
fn wire_compat_nested_and_repeated() {
    wire_compat_test(
        r#"
            syntax = "proto3";
            package compat;

            message Inner {
                string value = 1;
            }

            message Outer {
                Inner child = 1;
                repeated int32 numbers = 2;
                repeated string tags = 3;
            }
        "#,
        r#"
    // --- Runtime -> Prost ---
    {
        let mut rt_msg = rt_compat::Outer::default();
        rt_msg.child.modify(|c| c.value = "nested".to_string());
        rt_msg.numbers = vec![1, 2, 3, 100];
        rt_msg.tags = vec!["a".to_string(), "b".to_string()];

        let wire = encode(&rt_msg);

        use prost::Message as _;
        let prost_msg = prost_compat::Outer::decode(wire.as_slice())
            .expect("prost decode failed");

        assert_eq!(prost_msg.child.unwrap().value, "nested");
        assert_eq!(prost_msg.numbers, vec![1, 2, 3, 100]);
        assert_eq!(prost_msg.tags, vec!["a", "b"]);
    }

    // --- Prost -> Runtime ---
    {
        use prost::Message as _;
        let prost_msg = prost_compat::Outer {
            child: Some(prost_compat::Inner { value: "from_prost".to_string() }),
            numbers: vec![10, 20],
            tags: vec!["x".to_string()],
        };
        let mut wire = Vec::new();
        prost_msg.encode(&mut wire).unwrap();

        let rt_msg: rt_compat::Outer = decode(&wire)
            .expect("runtime decode failed");

        assert!(rt_msg.child.is_set());
        assert_eq!(rt_msg.child.value, "from_prost");
        assert_eq!(rt_msg.numbers, vec![10, 20]);
        assert_eq!(rt_msg.tags, vec!["x"]);
    }
        "#,
        "compat_nested",
    );
}

#[test]
fn wire_compat_enum_and_oneof() {
    wire_compat_test(
        r#"
            syntax = "proto3";
            package compat;

            enum Color {
                COLOR_UNKNOWN = 0;
                COLOR_RED = 1;
                COLOR_BLUE = 2;
            }

            message Msg {
                Color color = 1;
                oneof payload {
                    string text = 2;
                    int32 number = 3;
                }
            }
        "#,
        r#"
    // --- Runtime -> Prost ---
    {
        let mut rt_msg = rt_compat::Msg::default();
        rt_msg.color = protoc_rs_runtime::EnumValue::Known(rt_compat::Color::Red);
        rt_msg.payload = Some(rt_compat::msg::Payload::Text("hello".to_string()));

        let wire = encode(&rt_msg);

        use prost::Message as _;
        let prost_msg = prost_compat::Msg::decode(wire.as_slice())
            .expect("prost decode failed");

        assert_eq!(prost_msg.color, 1); // RED
        match prost_msg.payload {
            Some(prost_compat::msg::Payload::Text(s)) => assert_eq!(s, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    // --- Prost -> Runtime ---
    {
        use prost::Message as _;
        let prost_msg = prost_compat::Msg {
            color: 2, // BLUE
            payload: Some(prost_compat::msg::Payload::Number(42)),
        };
        let mut wire = Vec::new();
        prost_msg.encode(&mut wire).unwrap();

        let rt_msg: rt_compat::Msg = decode(&wire)
            .expect("runtime decode failed");

        assert_eq!(rt_msg.color, protoc_rs_runtime::EnumValue::Known(rt_compat::Color::Blue));
        match rt_msg.payload {
            Some(rt_compat::msg::Payload::Number(n)) => assert_eq!(n, 42),
            other => panic!("expected Number, got {:?}", other),
        }
    }
        "#,
        "compat_enum_oneof",
    );
}

#[test]
fn wire_compat_map_fields() {
    wire_compat_test(
        r#"
            syntax = "proto3";
            package compat;

            message Config {
                map<string, int32> settings = 1;
            }
        "#,
        r#"
    // --- Runtime -> Prost ---
    {
        let mut rt_msg = rt_compat::Config::default();
        rt_msg.settings.insert("timeout".to_string(), 30);
        rt_msg.settings.insert("retries".to_string(), 3);

        let wire = encode(&rt_msg);

        use prost::Message as _;
        let prost_msg = prost_compat::Config::decode(wire.as_slice())
            .expect("prost decode failed");

        assert_eq!(prost_msg.settings.get("timeout"), Some(&30));
        assert_eq!(prost_msg.settings.get("retries"), Some(&3));
    }

    // --- Prost -> Runtime ---
    {
        use prost::Message as _;
        let mut prost_msg = prost_compat::Config::default();
        prost_msg.settings.insert("max".to_string(), 100);

        let mut wire = Vec::new();
        prost_msg.encode(&mut wire).unwrap();

        let rt_msg: rt_compat::Config = decode(&wire)
            .expect("runtime decode failed");

        assert_eq!(rt_msg.settings.get("max"), Some(&100));
    }
        "#,
        "compat_maps",
    );
}
