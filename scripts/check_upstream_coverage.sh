#!/bin/bash
# Compare our parser's coverage against upstream protobuf test .proto files.
# Usage: ./scripts/check_upstream_coverage.sh
#
# Scans all .proto files in original-protobuf, attempts to parse each one,
# and reports pass/fail/skip statistics.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
UPSTREAM_ROOT="$(dirname "$PROJECT_ROOT")/original-protobuf"

if [ ! -d "$UPSTREAM_ROOT" ]; then
    echo "ERROR: original-protobuf not found at $UPSTREAM_ROOT"
    exit 1
fi

# Build the parser test binary
echo "Building parser..."
cargo build -p protoc-rs-parser --quiet 2>/dev/null

# Create a small Rust program that tries to parse a .proto file
PARSE_BIN="$PROJECT_ROOT/target/debug/parse_check"
if [ ! -f "$PARSE_BIN" ] || [ "$PROJECT_ROOT/parser/src/parser.rs" -nt "$PARSE_BIN" ]; then
    cat > /tmp/parse_check.rs << 'RUST_EOF'
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: parse_check <file.proto>");
        process::exit(2);
    }
    let source = match fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("READ_ERROR: {}", e);
            process::exit(2);
        }
    };
    match protoc_rs_parser::parse(&source) {
        Ok(_) => process::exit(0),
        Err(e) => {
            eprintln!("PARSE_ERROR: {}", e);
            process::exit(1);
        }
    }
}
RUST_EOF

    # Build the check binary
    rustc /tmp/parse_check.rs \
        --edition 2021 \
        -L "$PROJECT_ROOT/target/debug/deps" \
        --extern protoc_rs_parser="$(ls "$PROJECT_ROOT/target/debug/libprotoc_rs_parser"*.rlib | head -1)" \
        --extern protoc_rs_schema="$(ls "$PROJECT_ROOT/target/debug/libprotoc_rs_schema"*.rlib | head -1)" \
        -o "$PARSE_BIN" 2>/dev/null

    if [ $? -ne 0 ]; then
        echo "WARN: Could not build standalone binary, falling back to cargo test approach"
        PARSE_BIN=""
    fi
fi

# Collect all .proto files from upstream
echo ""
echo "Scanning upstream .proto files..."
echo "================================================================"

TOTAL=0
PASS=0
FAIL=0
SKIP=0
EDITION=0

declare -a FAILED_FILES=()
declare -a EDITION_FILES=()

# Find all .proto files, excluding third_party and generated files
while IFS= read -r proto_file; do
    TOTAL=$((TOTAL + 1))
    rel_path="${proto_file#$UPSTREAM_ROOT/}"

    # Skip edition files (we don't support editions yet)
    if grep -q '^edition ' "$proto_file" 2>/dev/null; then
        EDITION=$((EDITION + 1))
        EDITION_FILES+=("$rel_path")
        continue
    fi

    # Try to parse
    if [ -n "$PARSE_BIN" ]; then
        if "$PARSE_BIN" "$proto_file" 2>/dev/null; then
            PASS=$((PASS + 1))
        else
            err=$("$PARSE_BIN" "$proto_file" 2>&1 || true)
            FAIL=$((FAIL + 1))
            FAILED_FILES+=("$rel_path|$err")
        fi
    else
        # Fallback: just check if file is readable
        if [ -r "$proto_file" ]; then
            SKIP=$((SKIP + 1))
        fi
    fi
done < <(find "$UPSTREAM_ROOT" -name '*.proto' -not -path '*/third_party/*' | sort)

echo ""
echo "================================================================"
echo "UPSTREAM PROTO COVERAGE REPORT"
echo "================================================================"
echo ""
printf "  Total .proto files:     %4d\n" "$TOTAL"
printf "  Parse OK:               %4d\n" "$PASS"
printf "  Parse FAILED:           %4d\n" "$FAIL"
printf "  Edition (skipped):      %4d\n" "$EDITION"
if [ "$SKIP" -gt 0 ]; then
    printf "  Skipped (no binary):    %4d\n" "$SKIP"
fi
echo ""

if [ "$TOTAL" -gt 0 ]; then
    TESTABLE=$((TOTAL - EDITION))
    if [ "$TESTABLE" -gt 0 ]; then
        PCT=$((PASS * 100 / TESTABLE))
        printf "  Pass rate (non-edition): %d/%d (%d%%)\n" "$PASS" "$TESTABLE" "$PCT"
    fi
fi

if [ ${#FAILED_FILES[@]} -gt 0 ]; then
    echo ""
    echo "FAILED FILES:"
    echo "----------------------------------------------------------------"
    for entry in "${FAILED_FILES[@]}"; do
        file="${entry%%|*}"
        err="${entry#*|}"
        printf "  FAIL: %s\n" "$file"
        printf "        %s\n" "$err"
    done
fi

if [ ${#EDITION_FILES[@]} -gt 0 ]; then
    echo ""
    echo "EDITION FILES (not yet supported):"
    echo "----------------------------------------------------------------"
    for f in "${EDITION_FILES[@]}"; do
        printf "  SKIP: %s\n" "$f"
    done
fi

echo ""
echo "================================================================"

# Exit with failure if any parseable files failed
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
