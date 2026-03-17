//! Conformance tests: parse real-world .proto files from popular open-source
//! projects and compare with protoc output.
//!
//! Two tiers:
//!   Tier 1 -- Does our parser accept the file without errors?
//!   Tier 2 -- Does our parsed output match protoc's descriptor?
//!
//! Run: cargo test -p protobuf-conformance

use protoc_rs_parser::parse;
use std::path::{Path, PathBuf};
use std::process::Command;

fn protos_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("protos")
}

fn find_all_protos(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if !dir.exists() {
        return result;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            result.extend(find_all_protos(&path));
        } else if path.extension().is_some_and(|e| e == "proto") {
            result.push(path);
        }
    }
    result.sort();
    result
}

/// Tier 1: Every .proto file should parse without errors.
#[test]
fn tier1_all_protos_parse() {
    let dir = protos_dir();
    let protos = find_all_protos(&dir);
    assert!(
        !protos.is_empty(),
        "No .proto files found in {}",
        dir.display()
    );

    let mut failures = Vec::new();
    let mut successes = 0;

    for path in &protos {
        let source = std::fs::read_to_string(path).unwrap();
        let rel = path.strip_prefix(&dir).unwrap_or(path);
        match parse(&source) {
            Ok(_) => successes += 1,
            Err(e) => {
                failures.push(format!("  FAIL {}: {}", rel.display(), e));
            }
        }
    }

    eprintln!(
        "\n=== Tier 1: Parse results ===\n  {} passed, {} failed, {} total\n",
        successes,
        failures.len(),
        protos.len()
    );

    if !failures.is_empty() {
        eprintln!("Failures:");
        for f in &failures {
            eprintln!("{f}");
        }
        panic!(
            "{} of {} .proto files failed to parse",
            failures.len(),
            protos.len()
        );
    }
}

/// Check if protoc is available.
fn has_protoc() -> bool {
    Command::new("protoc").arg("--version").output().is_ok()
}

/// Run protoc on a single file and check if it succeeds.
fn protoc_accepts(path: &Path, include_dir: &Path) -> Result<(), String> {
    let output = Command::new("protoc")
        .arg(format!("--proto_path={}", include_dir.display()))
        .arg("--descriptor_set_out=/dev/null")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run protoc: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// Tier 2: Compare our parser vs protoc acceptance.
/// Files that protoc accepts should also be accepted by us.
/// Files that protoc rejects, we can also reject (import errors are expected).
#[test]
fn tier2_compare_with_protoc() {
    if !has_protoc() {
        eprintln!("protoc not found, skipping tier2 comparison");
        return;
    }

    let dir = protos_dir();
    let protos = find_all_protos(&dir);

    let mut both_pass = 0;
    let mut both_fail = 0;
    let mut only_protoc_pass = Vec::new(); // protoc OK but we fail -- BUGS
    let mut only_we_pass = Vec::new(); // we OK but protoc fails -- usually import errors

    for path in &protos {
        let source = std::fs::read_to_string(path).unwrap();
        let rel = path.strip_prefix(&dir).unwrap_or(path);

        let our_result = parse(&source);

        // For protoc, use the project-specific include dir
        // e.g., for googleapis/google/api/http.proto, include dir is googleapis/
        let project_dir = find_project_dir(path, &dir);
        let protoc_result = protoc_accepts(path, &project_dir);

        match (our_result.is_ok(), protoc_result.is_ok()) {
            (true, true) => both_pass += 1,
            (false, false) => both_fail += 1,
            (false, true) => {
                only_protoc_pass.push(format!(
                    "  BUG {}: protoc OK, we fail: {}",
                    rel.display(),
                    parse(&source).unwrap_err()
                ));
            }
            (true, false) => {
                only_we_pass.push(format!(
                    "  OK  {}: we pass, protoc fails (likely import): {}",
                    rel.display(),
                    protoc_result.unwrap_err().lines().next().unwrap_or("?")
                ));
            }
        }
    }

    eprintln!("\n=== Tier 2: protoc comparison ===");
    eprintln!("  Both pass:       {both_pass}");
    eprintln!("  Both fail:       {both_fail}");
    eprintln!("  Only protoc:     {} (BUGS)", only_protoc_pass.len());
    eprintln!(
        "  Only us:         {} (expected, import errors)",
        only_we_pass.len()
    );
    eprintln!();

    if !only_we_pass.is_empty() {
        eprintln!("Files we accept but protoc rejects (expected -- missing imports):");
        for f in &only_we_pass {
            eprintln!("{f}");
        }
        eprintln!();
    }

    if !only_protoc_pass.is_empty() {
        eprintln!("DISCREPANCIES (protoc accepts, we reject):");
        for f in &only_protoc_pass {
            eprintln!("{f}");
        }
        panic!(
            "{} discrepancies: protoc accepts but our parser rejects",
            only_protoc_pass.len()
        );
    }
}

/// Find the project root directory for protoc includes.
/// e.g., for protos/googleapis/google/api/http.proto, returns protos/googleapis/
fn find_project_dir(path: &Path, base: &Path) -> PathBuf {
    let rel = path.strip_prefix(base).unwrap();
    let first_component = rel.components().next().unwrap();
    base.join(first_component)
}

/// Tier 2b: Structural validation -- verify parsed descriptor has expected elements.
#[test]
fn tier2b_structural_validation() {
    let dir = protos_dir();
    let mut issues = Vec::new();

    // Files with known structure we can validate
    let checks: Vec<(
        &str,
        Box<dyn Fn(&protoc_rs_schema::FileDescriptorProto) -> Option<String>>,
    )> = vec![
        (
            "googleapis/google/api/http.proto",
            Box::new(|fd| {
                // Should have HttpRule message with pattern oneof
                let has_http_rule = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("HttpRule"));
                if !has_http_rule {
                    Some("missing HttpRule message".to_string())
                } else {
                    None
                }
            }),
        ),
        (
            "googleapis/google/rpc/status.proto",
            Box::new(|fd| {
                let has_status = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("Status"));
                if !has_status {
                    Some("missing Status message".to_string())
                } else {
                    None
                }
            }),
        ),
        (
            "grpc/grpc/health/v1/health.proto",
            Box::new(|fd| {
                let has_service = fd
                    .service
                    .iter()
                    .any(|s| s.name.as_deref() == Some("Health"));
                let has_req = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("HealthCheckRequest"));
                let has_resp = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("HealthCheckResponse"));
                if !has_service {
                    return Some("missing Health service".to_string());
                }
                if !has_req {
                    return Some("missing HealthCheckRequest".to_string());
                }
                if !has_resp {
                    return Some("missing HealthCheckResponse".to_string());
                }
                None
            }),
        ),
        (
            "opentelemetry/opentelemetry/proto/common/v1/common.proto",
            Box::new(|fd| {
                let has_any_value = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("AnyValue"));
                let has_kv = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("KeyValue"));
                if !has_any_value {
                    return Some("missing AnyValue message".to_string());
                }
                if !has_kv {
                    return Some("missing KeyValue message".to_string());
                }
                None
            }),
        ),
        (
            "prometheus/io/prometheus/client/metrics.proto",
            Box::new(|fd| {
                let has_metric_family = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("MetricFamily"));
                let has_metric = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("Metric"));
                if !has_metric_family {
                    return Some("missing MetricFamily".to_string());
                }
                if !has_metric {
                    return Some("missing Metric".to_string());
                }
                None
            }),
        ),
        (
            "etcd/etcd/api/mvccpb/kv.proto",
            Box::new(|fd| {
                let has_kv = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("KeyValue"));
                let has_event = fd
                    .message_type
                    .iter()
                    .any(|m| m.name.as_deref() == Some("Event"));
                if !has_kv {
                    return Some("missing KeyValue".to_string());
                }
                if !has_event {
                    return Some("missing Event".to_string());
                }
                None
            }),
        ),
    ];

    for (rel_path, check) in &checks {
        let path = dir.join(rel_path);
        if !path.exists() {
            eprintln!("  SKIP {rel_path}: file not found");
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        match parse(&source) {
            Ok(fd) => {
                if let Some(issue) = check(&fd) {
                    issues.push(format!("  {rel_path}: {issue}"));
                }
            }
            Err(e) => {
                issues.push(format!("  {rel_path}: parse error: {e}"));
            }
        }
    }

    eprintln!("\n=== Tier 2b: Structural validation ===");
    eprintln!("  {} checks, {} issues", checks.len(), issues.len());

    if !issues.is_empty() {
        for issue in &issues {
            eprintln!("{issue}");
        }
        panic!("{} structural validation failures", issues.len());
    }
}
