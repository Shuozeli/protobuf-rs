//! Integration tests for descriptor set serialization.
//!
//! Tests verify that our serialized FileDescriptorSet is byte-for-byte
//! identical to protoc's output for the same .proto input.

use protoc_rs_analyzer::{analyze, analyze_files, FileResolver};
use protoc_rs_compiler::descriptor_set::serialize_descriptor_set;
use protoc_rs_compiler::resolver::FsResolver;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Unit tests (no protoc dependency)
// ---------------------------------------------------------------------------

#[test]
fn empty_descriptor_set_produces_empty_bytes() {
    let fds = protoc_rs_schema::FileDescriptorSet { file: vec![] };
    let bytes = serialize_descriptor_set(&fds);
    assert!(bytes.is_empty());
}

#[test]
fn single_file_round_trips_through_analyze() {
    let source = r#"
        syntax = "proto3";
        package test;
        message Foo {
            string name = 1;
            int32 id = 2;
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
    // Should start with field 1 (file), wire type 2 (LEN)
    assert_eq!(bytes[0], 0x0A);
}

#[test]
fn enum_fields_are_serialized() {
    let source = r#"
        syntax = "proto3";
        package test;
        enum Color { RED = 0; GREEN = 1; BLUE = 2; }
        message Paint { Color color = 1; }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn nested_message_serialized() {
    let source = r#"
        syntax = "proto3";
        message Outer {
            message Inner { int32 x = 1; }
            Inner inner = 1;
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn proto2_with_required_and_defaults() {
    let source = r#"
        syntax = "proto2";
        message Config {
            required string name = 1;
            optional int32 retries = 2 [default = 3];
            repeated string hosts = 3;
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn service_with_streaming() {
    let source = r#"
        syntax = "proto3";
        package svc;
        message Req { string q = 1; }
        message Resp { string a = 1; }
        service Search {
            rpc Query(Req) returns (Resp);
            rpc StreamResults(Req) returns (stream Resp);
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn oneof_serialized() {
    let source = r#"
        syntax = "proto3";
        message Event {
            oneof payload {
                string text = 1;
                bytes data = 2;
                int32 code = 3;
            }
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn map_field_serialized() {
    let source = r#"
        syntax = "proto3";
        message Config {
            map<string, string> settings = 1;
            map<int32, bool> flags = 2;
        }
    "#;
    let fds = analyze(source).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
}

#[test]
fn multi_file_with_imports() {
    struct TestResolver {
        files: HashMap<String, String>,
    }
    impl FileResolver for TestResolver {
        fn resolve(&self, name: &str) -> Option<String> {
            self.files.get(name).cloned()
        }
    }

    let mut files = HashMap::new();
    files.insert(
        "base.proto".to_string(),
        r#"
        syntax = "proto3";
        package base;
        message Base { string id = 1; }
        "#
        .to_string(),
    );
    files.insert(
        "derived.proto".to_string(),
        r#"
        syntax = "proto3";
        package derived;
        import "base.proto";
        message Derived { base.Base parent = 1; string name = 2; }
        "#
        .to_string(),
    );

    let resolver = TestResolver { files };
    let fds = analyze_files(&["derived.proto"], &resolver).unwrap();
    let bytes = serialize_descriptor_set(&fds);
    assert!(!bytes.is_empty());
    // Should have 2 files (base.proto imported + derived.proto)
    assert_eq!(fds.file.len(), 2);
}

// ---------------------------------------------------------------------------
// Cross-compatibility tests (require protoc)
// ---------------------------------------------------------------------------

fn has_protoc() -> bool {
    Command::new("protoc").arg("--version").output().is_ok()
}

fn protoc_descriptor_set(proto_content: &str, include_paths: &[&str]) -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let proto_path = tmp.path().join("test.proto");
    std::fs::write(&proto_path, proto_content).unwrap();

    let out_path = tmp.path().join("out.pb");
    let mut cmd = Command::new("protoc");
    cmd.arg(format!("--proto_path={}", tmp.path().display()));
    for ip in include_paths {
        cmd.arg(format!("--proto_path={}", ip));
    }
    cmd.arg(format!("--descriptor_set_out={}", out_path.display()));
    cmd.arg(&proto_path);

    let output = cmd.output().expect("failed to run protoc");
    if !output.status.success() {
        panic!("protoc failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    std::fs::read(&out_path).unwrap()
}

fn our_descriptor_set(proto_content: &str, include_paths: &[PathBuf]) -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let proto_path = tmp.path().join("test.proto");
    std::fs::write(&proto_path, proto_content).unwrap();

    let mut all_paths = vec![tmp.path().to_path_buf()];
    all_paths.extend(include_paths.iter().cloned());

    let resolver = FsResolver::new(all_paths);
    let fds = analyze_files(&["test.proto"], &resolver).unwrap();

    // Strip source_code_info (protoc doesn't include it by default)
    let mut fds = fds;
    for file in &mut fds.file {
        file.source_code_info = None;
    }
    // Only include root file (no --include_imports)
    fds.file.retain(|f| f.name.as_deref() == Some("test.proto"));

    serialize_descriptor_set(&fds)
}

#[test]
fn cross_compat_simple_proto3() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
message Msg {
  string name = 1;
  int32 id = 2;
  repeated string tags = 3;
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "proto3 simple message: output differs from protoc"
    );
}

#[test]
fn cross_compat_proto2_with_defaults() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto2";
package test;
message Config {
  required string name = 1;
  optional int32 retries = 2 [default = 3];
  repeated string hosts = 3;
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "proto2 with defaults: output differs from protoc"
    );
}

#[test]
fn cross_compat_enum_and_nested() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
enum Status {
  UNKNOWN = 0;
  ACTIVE = 1;
}
message Container {
  message Item {
    string key = 1;
    Status status = 2;
  }
  repeated Item items = 1;
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "enum + nested message: output differs from protoc"
    );
}

#[test]
fn cross_compat_oneof() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
message Event {
  string id = 1;
  oneof payload {
    string text = 2;
    bytes data = 3;
    int32 code = 4;
  }
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(protoc_bytes, our_bytes, "oneof: output differs from protoc");
}

#[test]
fn cross_compat_map_fields() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
message Config {
  map<string, string> settings = 1;
  map<int32, bool> flags = 2;
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "map fields: output differs from protoc"
    );
}

#[test]
fn cross_compat_service() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
message Req { string q = 1; }
message Resp { string a = 1; }
service Search {
  rpc Query(Req) returns (Resp);
  rpc StreamResults(Req) returns (stream Resp);
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "service with streaming: output differs from protoc"
    );
}

#[test]
fn cross_compat_proto2_extensions() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto2";
package test;
message Base {
  required string name = 1;
  extensions 100 to 200;
}
extend Base {
  optional string nickname = 100;
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "proto2 extensions: output differs from protoc"
    );
}

#[test]
fn cross_compat_reserved_fields() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
message Evolving {
  string current_field = 1;
  reserved 2, 3, 10 to 20;
  reserved "old_name", "legacy_field";
}
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "reserved fields: output differs from protoc"
    );
}

#[test]
fn cross_compat_file_options() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping cross-compat test");
        return;
    }
    let proto = r#"syntax = "proto3";
package test;
option java_package = "com.example.test";
option go_package = "example.com/test";
option optimize_for = SPEED;
message Msg { string name = 1; }
"#;
    let protoc_bytes = protoc_descriptor_set(proto, &[]);
    let our_bytes = our_descriptor_set(proto, &[]);
    assert_eq!(
        protoc_bytes, our_bytes,
        "file options: output differs from protoc"
    );
}
