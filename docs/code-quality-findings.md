# Code Quality Findings

Last updated: 2026-03-19

## Resolved

The following issues from prior reviews have been addressed:

- **Duplicate `rust_field_type`/`rust_map_value_type`** -- Deduplicated; only `rust_field_type` remains in `codegen/src/rust_gen.rs:553`.
- **Duplicate test helpers across crates** -- Moved to `test-utils` crate, which now exports 8 helpers (find_msg, find_field, etc.). Both parser and analyzer tests use `protoc_rs_test_utils::*`.
- **Empty `test-utils` crate** -- Now populated with shared helpers.
- **Hand-rolled enum conversions** -- Replaced with `impl_from_int!` and `impl_into_i32!` macros in `schema/src/descriptor.rs`.
- **Copy-paste validation logic** -- Extracted into `check_exclusive_range_overlaps` and `check_cross_range_overlaps` generic functions in `analyzer/src/validate.rs`.
- **`AnalyzeError` construction boilerplate** -- `make_err` helper added in `analyzer/src/validate.rs:10`.
- **No-op `| 0` in annotator** -- Removed from `annotator/src/walker.rs`.

## Open

### 1. Empty `conformance/src/lib.rs`
- Lines 1-2: `// Conformance test library -- intentionally empty.`
- Crate only has integration tests. The empty `lib.rs` is unnecessary -- can use a test-only crate without a lib target.

### 2. Excessive Section Divider Comments
- `compiler/src/descriptor_set.rs` and `codegen/src/rust_gen.rs` still have dense inline `// N: field_name` comments on every encode call.
- These directly mirror the struct field names and add no information. Consider reducing to section-level comments.

### 3. Silent Failure in `proto-gen`
- `proto-gen/src/data_gen.rs:10` -- `generate_data` panics via `.expect()` when `root_message` is not found.
- While this is better than silent failure, returning `Result` would be more idiomatic for a library function.

### 4. Map Key Validation Verbosity
- `analyzer/src/validate.rs:494-510` -- Four separate match arms for disallowed map key types, each returning nearly identical errors. Could use a single check against a disallowed set.

### 5. Unused Imports (verify with clippy)
- CI runs `cargo clippy --workspace -- -D warnings`, so any unused imports should be caught. If clippy passes, these are already resolved.
