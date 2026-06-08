use std::process::Command;

fn run_codegen_flags(proto_source: &str, test_name: &str, flags: &[&str], expected_files: &[&str]) {
    // Arrange
    let tmp = tempfile::tempdir()
        .unwrap_or_else(|e| panic!("{test_name}: failed to create tempdir: {e}"));
    let proto_dir = tmp.path().join("proto");
    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&proto_dir)
        .unwrap_or_else(|e| panic!("{test_name}: failed to create proto dir: {e}"));
    let proto_path = proto_dir.join("input.proto");
    std::fs::write(&proto_path, proto_source)
        .unwrap_or_else(|e| panic!("{test_name}: failed to write proto: {e}"));

    // Act
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_protoc-rs"));
    cmd.arg("-I")
        .arg(&proto_dir)
        .arg(&proto_path)
        .current_dir(tmp.path());
    for flag in flags {
        cmd.arg(flag).arg(&out_dir);
    }
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("{test_name}: failed to run protoc-rs: {e}"));

    // Assert
    if !output.status.success() {
        panic!(
            "{test_name}: protoc-rs failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for expected in expected_files {
        let path = out_dir.join(expected);
        assert!(
            path.exists(),
            "{test_name}: expected generated file {}",
            path.display()
        );
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{test_name}: failed to read {}: {e}", path.display()));
        assert!(
            !content.trim().is_empty(),
            "{test_name}: generated file {} is empty",
            path.display()
        );
    }
}

#[test]
fn cli_generates_typescript_and_python() {
    run_codegen_flags(
        r#"
            syntax = "proto3";
            package cli_codegen;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
            }

            message Person {
                string name = 1;
                optional int32 age = 2;
                repeated string tags = 3;
                map<string, Status> statuses = 4;
                oneof contact {
                    string email = 5;
                    bytes phone = 6;
                }
            }
        "#,
        "cli_ts_python",
        &["--ts_out", "--python_out"],
        &["cli_codegen.ts", "cli_codegen.py"],
    );
}

#[test]
fn cli_nodejs_out_is_typescript_alias() {
    run_codegen_flags(
        r#"
            syntax = "proto3";
            package cli_nodejs;

            message Event {
                string id = 1;
                int64 created_at = 2;
            }
        "#,
        "cli_nodejs_alias",
        &["--nodejs_out"],
        &["cli_nodejs.ts"],
    );
}
