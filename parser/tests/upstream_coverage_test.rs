use std::path::{Path, PathBuf};

use protoc_rs_parser::parse;

fn upstream_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("original-protobuf")
}

fn collect_proto_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.exists() {
        return files;
    }
    for entry in walkdir(dir) {
        if entry.extension().is_some_and(|e| e == "proto") {
            files.push(entry);
        }
    }
    files.sort();
    files
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip third_party
                if path.file_name().is_some_and(|n| n == "third_party") {
                    continue;
                }
                result.extend(walkdir(&path));
            } else {
                result.push(path);
            }
        }
    }
    result
}

#[derive(Debug)]
struct ParseResult {
    file: String,
    status: Status,
}

#[derive(Debug)]
enum Status {
    Pass,
    Fail(String),
}

fn classify_and_parse(path: &Path, root: &Path) -> ParseResult {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let file = rel.display().to_string();

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return ParseResult {
                file,
                status: Status::Fail(format!("read error: {}", e)),
            };
        }
    };

    match parse(&source) {
        Ok(_) => ParseResult {
            file,
            status: Status::Pass,
        },
        Err(e) => ParseResult {
            file,
            status: Status::Fail(e.to_string()),
        },
    }
}

#[test]
fn upstream_coverage_report() {
    let root = upstream_root();
    if !root.exists() {
        eprintln!("SKIP: original-protobuf not found at {}", root.display());
        return;
    }

    let files = collect_proto_files(&root);
    assert!(
        !files.is_empty(),
        "no .proto files found in {}",
        root.display()
    );

    let results: Vec<ParseResult> = files.iter().map(|f| classify_and_parse(f, &root)).collect();

    let total = results.len();
    let pass = results
        .iter()
        .filter(|r| matches!(r.status, Status::Pass))
        .count();
    let fail: Vec<_> = results
        .iter()
        .filter(|r| matches!(r.status, Status::Fail(_)))
        .collect();

    // Print report
    eprintln!();
    eprintln!("================================================================");
    eprintln!("UPSTREAM PROTO COVERAGE REPORT");
    eprintln!("================================================================");
    eprintln!();
    eprintln!("  Total .proto files:      {:>4}", total);
    eprintln!("  Parse OK:                {:>4}", pass);
    eprintln!("  Parse FAILED:            {:>4}", fail.len());
    eprintln!();
    if total > 0 {
        let pct = pass * 100 / total;
        eprintln!("  Pass rate: {}/{} ({}%)", pass, total, pct);
    }

    if !fail.is_empty() {
        eprintln!();
        eprintln!("FAILED FILES:");
        eprintln!("----------------------------------------------------------------");
        for r in &fail {
            if let Status::Fail(ref err) = r.status {
                eprintln!("  FAIL: {}", r.file);
                // Truncate long errors
                let short = if err.len() > 120 { &err[..120] } else { err };
                eprintln!("        {}", short);
            }
        }
    }

    eprintln!();
    eprintln!("================================================================");

    // The test passes as long as we report. We track regressions by checking
    // the pass rate doesn't go down.
    if total > 0 {
        let pct = pass * 100 / total;
        assert!(
            pct >= 80,
            "pass rate {}% is below 80% threshold ({}/{})",
            pct,
            pass,
            total
        );
    }
}
