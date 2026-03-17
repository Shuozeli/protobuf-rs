# Code Quality Findings

## 1. Duplicate Functions

### `rust_field_type` and `rust_map_value_type` are identical
- `codegen/src/rust_gen.rs:574-583` (`rust_field_type`)
- `codegen/src/rust_gen.rs:587-597` (`rust_map_value_type`)
- These two functions have identical logic. Deduplicate into one.

## 2. Duplicate Test Helpers Across Crates

### `find_msg`, `find_field`, `find_enum` duplicated
- `analyzer/tests/analyzer_test.rs:43-62`
- `parser/tests/parser_test.rs:27-80`
- Identical helper functions copy-pasted. Should live in `test-utils` crate.

### `test-utils` crate is empty
- `test-utils/src/lib.rs:1-2` -- contains only `// TODO: Phase 1`
- The crate exists in the workspace and is declared as a dependency but provides nothing.

## 3. Hand-Rolled Enum Conversions Instead of Macros

### Manual `from_i32` / `from_u32` match arms
- `schema/src/descriptor.rs:151-173` (`FieldType::from_i32` -- 18 manual arms)
- `schema/src/descriptor.rs:249-257` (`FieldLabel::from_i32` -- 3 manual arms)
- `schema/src/descriptor.rs:271-281` (`WireType::from_u32` -- 6 manual arms)
- Should use a derive macro (e.g. `num_enum::TryFromPrimitive`) or a `macro_rules!`.

### Six identical `From<T> for i32` impls
- `schema/src/descriptor.rs:494-528`
- All six impls are just `v as i32`. Should be a single `macro_rules!` invocation.

## 4. Copy-Paste Validation Logic (No Helper Extracted)

### Three nearly identical range-overlap checks
- `analyzer/src/validate.rs:288-308` (reserved range overlap check)
- `analyzer/src/validate.rs:311-331` (extension range overlap check)
- `analyzer/src/validate.rs:334-354` (extension vs reserved range overlap check)
- Same structure, same overlap math, different error messages. Extract a `check_range_overlap` helper.

### Repetitive `AnalyzeError` construction
- Throughout `analyzer/src/validate.rs`, every error site manually builds `AnalyzeError { message: format!(...), file: Some(file_name.to_string()), span: ... }`.
- A helper like `fn err(msg: String, file: &str, span: Option<Span>) -> AnalyzeError` or a macro would reduce noise.

## 5. No-Op Code

### `| 0` has no effect
- `annotator/src/walker.rs:976` -- `(99 << 3) | 0`
- `annotator/src/walker.rs:980` -- `(10 << 3) | 0`
- The `| 0` is a no-op. Remove it or use the wire type constant name for clarity.

## 6. Unused Imports and Dead Code

### Unused import `AnalyzeError`
- `analyzer/tests/analyzer_test.rs:10` -- `AnalyzeError` imported but never used.

### Unused import `parse`
- `analyzer/tests/analyzer_test.rs:11` -- `parse` imported but never used.

### Unused function `find_enum`
- `analyzer/tests/analyzer_test.rs:57` -- defined but never called.

## 7. Empty / Placeholder Crates

### `test-utils/src/lib.rs`
- Lines 1-2: `// Shared test infrastructure for protobuf-rs.` / `// TODO: Phase 1`
- Either populate with shared test helpers or remove from workspace.

### `conformance/src/lib.rs`
- Lines 1-2: `// Conformance test library -- intentionally empty.`
- Crate only has integration tests. The empty `lib.rs` is unnecessary -- can use a test-only crate without a lib target.

## 8. Excessive Section Divider Comments

138 instances of `// ----...` across 18 files. Particularly dense in:
- `schema/src/descriptor.rs` -- 20 dividers in 528 lines
- `compiler/src/descriptor_set.rs` -- 26 dividers in 760 lines
- `codegen/src/rust_gen.rs` -- 16 dividers in 1130 lines
- These add visual noise without information. Use module structure or `impl` blocks to organize instead.

## 9. Clippy Warnings

### `Vec::new()` followed by `push` instead of `vec![]`
- `annotator/src/walker.rs:1022-1024`
- `annotator/tests/annotator_test.rs:164-166`

### `map_or(false, ...)` instead of `is_some_and(...)`
- `conformance/tests/conformance_test.rs:28`

### Complex type as local variable (should use type alias)
- `conformance/tests/conformance_test.rs:197-200`

## 10. Map Key Validation Uses Chained Match Arms Instead of a Set

### Verbose match on disallowed key types
- `analyzer/src/validate.rs:459-498`
- Four separate match arms (`Float|Double`, `Bytes`, `Message|Group`, `Enum`) each returning nearly identical errors. Could be a single check against a disallowed set with a shared error message template.

## 11. Silent Failure in `proto-gen`

### Returns empty Vec instead of propagating error
- `proto-gen/src/data_gen.rs:10-18`
- When `root_message` is not found in schema, silently returns `Vec::new()`. Caller cannot distinguish between valid empty data and a lookup failure. Should return `Result` or `Option`.

## 12. Inconsistent Error Handling in Tests

### Excessive `unwrap_or_else(|e| panic!(...))` chains
- `compiler/tests/codegen_compile_test.rs:17,19,24,28,40,48,52,59`
- `proto-gen/src/tests.rs:26,40,62`
- In tests, plain `.unwrap()` or `.expect("msg")` is idiomatic. The verbose `unwrap_or_else` pattern adds noise.

## 13. `descriptor_set.rs` -- Inline Comments on Every Field Number

- `compiler/src/descriptor_set.rs:114-169` (and throughout the entire 760-line file)
- Every single encode call has a `// N: field_name` comment. These directly mirror the struct field names and add no information. Remove or reduce to section-level comments only.
