#!/usr/bin/env python3
"""
Verify test parity between C++ protoc parser_unittest.cc and our Rust tests.

Extracts all TEST_F(Fixture, TestName) from the C++ file, extracts all #[test]
functions from our Rust test files, and cross-references them against a mapping
file (test_mapping.jsonl).

Usage:
    python3 scripts/verify_test_parity.py                  # verify parity
    python3 scripts/verify_test_parity.py --init-mapping   # auto-generate mapping
    python3 scripts/verify_test_parity.py --update-mapping # interactive update for gaps

Exit code:
    0 if no unmapped C++ tests remain (excluding deferred/excluded)
    1 if there are gaps
"""

import json
import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CPP_TEST_DIR = (
    REPO_ROOT.parent / "original-protobuf" / "src" / "google" / "protobuf"
    / "compiler"
)

CPP_TEST_FILES = [
    CPP_TEST_DIR / "parser_unittest.cc",
    CPP_TEST_DIR / "importer_unittest.cc",
]

RUST_TEST_FILES = [
    REPO_ROOT / "parser" / "tests" / "parser_test.rs",
    REPO_ROOT / "parser" / "src" / "parser.rs",  # unit tests with source_info_*
    REPO_ROOT / "analyzer" / "tests" / "analyzer_test.rs",
]

MAPPING_FILE = REPO_ROOT / "scripts" / "test_mapping.jsonl"
OVERRIDES_FILE = REPO_ROOT / "scripts" / "test_mapping_overrides.jsonl"

# Fixtures we explicitly exclude from parity tracking
EXCLUDED_FIXTURES = {
    "ParseDescriptorDebugTest",  # Debug string output, different scope
    "ParserTest",  # Internal parser plumbing tests
    "DiskSourceTreeTest",  # C++ DiskSourceTree path mapping, no Rust equivalent
    "SourceTreeDescriptorDatabaseTest",  # C++-specific extension declarations from .txtpb
}


def extract_cpp_tests(path: Path) -> list[tuple[str, str]]:
    """Extract (fixture, test_name) pairs from C++ TEST_F macros."""
    pattern = re.compile(r"TEST_F\(\s*(\w+)\s*,\s*(\w+)\s*\)")
    tests = []
    with open(path, "r") as f:
        for line in f:
            m = pattern.search(line)
            if m:
                tests.append((m.group(1), m.group(2)))
    return tests


def extract_all_cpp_tests() -> list[tuple[str, str]]:
    """Extract tests from all configured C++ test files."""
    all_tests = []
    for path in CPP_TEST_FILES:
        if path.exists():
            all_tests.extend(extract_cpp_tests(path))
    return all_tests


def extract_rust_tests(path: Path) -> set[str]:
    """Extract test function names from a Rust test file.

    Handles both direct #[test] fn declarations and macro-generated tests
    like expect_error!(test_name, ...).
    """
    if not path.exists():
        return set()
    content = path.read_text()

    tests: set[str] = set()
    # Direct #[test] fn name()
    tests.update(re.findall(r"#\[test\]\s*\n\s*fn\s+(\w+)", content))
    # Macro-generated: expect_error!(name, ...) etc.
    tests.update(re.findall(r"expect_error!\(\s*\n?\s*(\w+)", content))
    return tests


def camel_to_snake(name: str) -> str:
    """Convert CamelCase to snake_case."""
    s1 = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    return re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s1).lower()


def load_mapping(path: Path) -> dict[str, dict]:
    """Load the mapping file. Returns {cpp_key: entry_dict}."""
    mapping: dict[str, dict] = {}
    if not path.exists():
        return mapping
    with open(path, "r") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            entry = json.loads(line)
            key = f"{entry['fixture']}::{entry['cpp_test']}"
            mapping[key] = entry
    return mapping


def save_mapping(path: Path, mapping: dict[str, dict]) -> None:
    """Save the mapping file, sorted by key."""
    with open(path, "w") as f:
        for key in sorted(mapping.keys()):
            f.write(json.dumps(mapping[key], separators=(",", ":")) + "\n")


def find_rust_matches(
    cpp_fixture: str, cpp_test: str, all_rust: set[str]
) -> list[str]:
    """Try to find Rust test(s) matching a C++ test by naming conventions."""
    snake = camel_to_snake(cpp_test)
    matches = []

    # Direct snake_case match with common prefixes
    prefixes = ["cpp_", "err_", "source_info_", ""]
    for prefix in prefixes:
        candidate = prefix + snake
        if candidate in all_rust:
            matches.append(candidate)

    # Fixture-specific prefix patterns
    fixture_prefixes: dict[str, list[str]] = {
        "ParseErrorTest": ["cpp_", "err_"],
        "ParserValidationErrorTest": ["cpp_", "err_"],
        "ParseEditionsTest": ["cpp_editions_"],
        "ParseVisibilityTest": ["cpp_visibility_"],
        "SourceInfoTest": ["source_info_"],
        "ParseMessageTest": ["parses_", "test_parse_", "cpp_editions_"],
        "ParseImportTest": ["cpp_import_", "parses_"],
        "ParseEnumTest": ["parses_", "test_parse_"],
        "ParseServiceTest": ["parses_", "test_parse_"],
        "ParseMiscTest": ["cpp_", "test_parse_"],
        "ImporterTest": ["cpp_importer_"],
    }

    for prefix in fixture_prefixes.get(cpp_fixture, []):
        candidate = prefix + snake
        if candidate in all_rust and candidate not in matches:
            matches.append(candidate)

    # Fuzzy: find any rust test whose name contains the snake_case form
    if not matches:
        for rt in all_rust:
            if snake in rt and rt not in matches:
                matches.append(rt)

    # Even fuzzier: substring matching with key parts of the snake name
    # e.g. "missing_syntax_identifier" -> find tests containing "missing_syntax" or "invalid_syntax"
    if not matches:
        # Try matching on significant word combos (2+ words)
        words = snake.split("_")
        if len(words) >= 2:
            for i in range(len(words) - 1):
                bigram = f"{words[i]}_{words[i+1]}"
                for rt in all_rust:
                    if bigram in rt and rt not in matches:
                        matches.append(rt)

    return matches


def init_mapping() -> None:
    """Auto-generate mapping by matching C++ tests to Rust tests."""
    cpp_tests = extract_all_cpp_tests()
    if not cpp_tests:
        print("ERROR: No C++ test files found")
        sys.exit(1)
    all_rust: set[str] = set()
    for path in RUST_TEST_FILES:
        all_rust |= extract_rust_tests(path)

    entries: dict[str, dict] = {}
    for fixture, test_name in cpp_tests:
        key = f"{fixture}::{test_name}"

        if fixture in EXCLUDED_FIXTURES:
            entries[key] = {
                "fixture": fixture,
                "cpp_test": test_name,
                "rust_tests": [],
                "status": "excluded",
            }
            continue

        rust_matches = find_rust_matches(fixture, test_name, all_rust)
        status = "covered" if rust_matches else "unmapped"
        entries[key] = {
            "fixture": fixture,
            "cpp_test": test_name,
            "rust_tests": rust_matches,
            "status": status,
        }

    # Apply overrides from hand-curated file
    overrides = load_mapping(OVERRIDES_FILE)
    for key, override_entry in overrides.items():
        entries[key] = override_entry

    save_mapping(MAPPING_FILE, entries)

    covered = sum(1 for e in entries.values() if e["status"] == "covered")
    excluded = sum(1 for e in entries.values() if e["status"] == "excluded")
    unmapped = sum(1 for e in entries.values() if e["status"] == "unmapped")
    deferred = sum(1 for e in entries.values() if e["status"] == "deferred")

    print(f"Generated {MAPPING_FILE}")
    print(f"  Total C++ tests: {len(entries)}")
    print(f"  Auto-matched + overrides: {covered}")
    print(f"  Excluded:                 {excluded}")
    print(f"  Deferred:                 {deferred}")
    print(f"  Unmapped:                 {unmapped}")

    if unmapped > 0:
        print(f"\nUnmapped tests ({unmapped}):")
        for key, entry in sorted(entries.items()):
            if entry["status"] == "unmapped":
                snake = camel_to_snake(entry["cpp_test"])
                print(f"  {key}  (snake: {snake})")
        print("\nAdd mappings to test_mapping_overrides.jsonl or run --update-mapping.")


def verify() -> bool:
    """Main verification. Returns True if no gaps."""
    cpp_tests = extract_all_cpp_tests()
    if not cpp_tests:
        print("SKIP: No C++ test files found")
        return True
    all_rust: set[str] = set()
    for path in RUST_TEST_FILES:
        all_rust |= extract_rust_tests(path)

    mapping = load_mapping(MAPPING_FILE)

    # Check for new C++ tests not in mapping
    new_cpp: list[tuple[str, str]] = []
    for fixture, test_name in cpp_tests:
        key = f"{fixture}::{test_name}"
        if key not in mapping and fixture not in EXCLUDED_FIXTURES:
            new_cpp.append((fixture, test_name))

    # Group C++ tests by fixture
    fixtures: dict[str, list[str]] = {}
    for fixture, test_name in cpp_tests:
        fixtures.setdefault(fixture, []).append(test_name)

    covered = 0
    excluded = 0
    deferred = 0
    unmapped_tests: list[tuple[str, str]] = []
    broken_mappings: list[tuple[str, str, str]] = []

    for fixture, test_name in cpp_tests:
        key = f"{fixture}::{test_name}"

        if fixture in EXCLUDED_FIXTURES:
            excluded += 1
            continue

        entry = mapping.get(key)
        if entry is None:
            unmapped_tests.append((fixture, test_name))
            continue

        if entry.get("status") == "deferred":
            deferred += 1
            continue

        rust_tests = entry.get("rust_tests", [])
        if rust_tests:
            covered += 1
            # Check if mapped rust tests still exist
            for rt in rust_tests:
                if rt and rt not in all_rust:
                    broken_mappings.append((fixture, test_name, rt))
        else:
            unmapped_tests.append((fixture, test_name))

    total = len(cpp_tests) - excluded

    # Print report
    print("=" * 70)
    print("PROTOC TEST PARITY VERIFICATION")
    print("=" * 70)
    print()
    print(f"  C++ parser_unittest.cc tests:  {len(cpp_tests)}")
    print(f"  Excluded fixtures:             {excluded}")
    print(f"  In-scope tests:                {total}")
    print(f"  Covered (mapped to Rust):      {covered}")
    print(f"  Deferred:                      {deferred}")
    print(f"  Unmapped:                      {len(unmapped_tests)}")
    if total > 0:
        pct = covered * 100 // total
        print(f"  Coverage:                      {pct}%")
    print()

    # Per-fixture summary
    print("Per-fixture breakdown:")
    print(f"  {'Fixture':<35} {'Total':>5} {'Mapped':>6} {'Gap':>4}")
    print(f"  {'-'*35} {'-'*5} {'-'*6} {'-'*4}")
    for fixture in sorted(fixtures.keys()):
        if fixture in EXCLUDED_FIXTURES:
            continue
        tests = fixtures[fixture]
        mapped = 0
        for t in tests:
            key = f"{fixture}::{t}"
            entry = mapping.get(key, {})
            if entry.get("rust_tests") or entry.get("status") == "deferred":
                mapped += 1
        gap = len(tests) - mapped
        marker = " *" if gap > 0 else ""
        print(f"  {fixture:<35} {len(tests):>5} {mapped:>6} {gap:>4}{marker}")
    print()

    # Rust cpp_* tests not referenced in any mapping (informational)
    mapped_rust: set[str] = set()
    for entry in mapping.values():
        mapped_rust.update(entry.get("rust_tests", []))
    cpp_prefixed_rust = {t for t in all_rust if t.startswith("cpp_")}
    orphan_rust = cpp_prefixed_rust - mapped_rust
    if orphan_rust:
        print(f"INFO: Rust cpp_* tests not in mapping ({len(orphan_rust)}):")
        print("  (These may cover other test suites like importer_unittest.cc,")
        print("   or be duplicates of auto-matched tests under different names.)")
        for t in sorted(orphan_rust):
            print(f"  {t}")
        print()

    if broken_mappings:
        print(f"BROKEN mappings (Rust test no longer exists) ({len(broken_mappings)}):")
        for fixture, cpp, missing in broken_mappings:
            print(f"  {fixture}::{cpp} -> {missing} (MISSING)")
        print()

    if new_cpp:
        print(f"NEW C++ tests not in mapping ({len(new_cpp)}):")
        for fixture, test_name in new_cpp:
            print(f"  {fixture}::{test_name}")
        print()

    if unmapped_tests:
        print(f"UNMAPPED C++ tests ({len(unmapped_tests)}):")
        for fixture, test_name in unmapped_tests:
            print(f"  {fixture}::{test_name}")
        print()

    if not unmapped_tests and not broken_mappings and not new_cpp:
        print("ALL in-scope C++ tests are mapped to Rust equivalents.")
        print()

    print("=" * 70)

    return len(unmapped_tests) == 0 and len(broken_mappings) == 0 and len(new_cpp) == 0


def update_mapping() -> None:
    """Interactive helper to update mappings for unmapped tests."""
    cpp_tests = extract_all_cpp_tests()
    if not cpp_tests:
        print("ERROR: No C++ test files found")
        sys.exit(1)
    all_rust: set[str] = set()
    for path in RUST_TEST_FILES:
        all_rust |= extract_rust_tests(path)
    all_rust_sorted = sorted(all_rust)

    mapping = load_mapping(MAPPING_FILE)

    unmapped = []
    for fixture, test_name in cpp_tests:
        key = f"{fixture}::{test_name}"
        if fixture in EXCLUDED_FIXTURES:
            continue
        entry = mapping.get(key, {})
        if not entry.get("rust_tests") and entry.get("status") != "deferred":
            unmapped.append((fixture, test_name, key))

    if not unmapped:
        print("No unmapped C++ tests found.")
        return

    print(f"Found {len(unmapped)} unmapped C++ tests.")
    print("Enter comma-separated Rust test names, 'defer' to skip, or empty to leave unmapped.\n")

    for fixture, test_name, key in unmapped:
        snake = camel_to_snake(test_name)
        candidates = [t for t in all_rust_sorted if snake in t][:5]

        prompt = f"  {key}"
        if candidates:
            prompt += f"\n    candidates: {', '.join(candidates)}"
        prompt += "\n    > "

        try:
            answer = input(prompt).strip()
        except (EOFError, KeyboardInterrupt):
            print("\nAborted. Saving progress...")
            break

        if answer == "defer":
            mapping[key] = {
                "fixture": fixture,
                "cpp_test": test_name,
                "rust_tests": [],
                "status": "deferred",
            }
        elif answer:
            rust_tests = [t.strip() for t in answer.split(",") if t.strip()]
            mapping[key] = {
                "fixture": fixture,
                "cpp_test": test_name,
                "rust_tests": rust_tests,
                "status": "covered",
            }
        # else: leave as-is

    save_mapping(MAPPING_FILE, mapping)
    print(f"\nSaved {MAPPING_FILE}")


def main() -> None:
    os.chdir(REPO_ROOT)

    if "--init-mapping" in sys.argv:
        init_mapping()
        return

    if "--update-mapping" in sys.argv:
        update_mapping()
        return

    if not MAPPING_FILE.exists():
        print(f"No mapping file found at {MAPPING_FILE}")
        print("Run with --init-mapping to auto-generate from test names.")
        sys.exit(1)

    ok = verify()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
