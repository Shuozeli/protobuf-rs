# Code Quality Findings

Last updated: 2026-03-26 (Phase 3 fixes applied)

## Resolved (Prior Reviews)

- **Duplicate `rust_field_type`/`rust_map_value_type`** -- Deduplicated; only `rust_field_type` remains in `codegen/src/rust_gen.rs:553`.
- **Duplicate test helpers across crates** -- Moved to `test-utils` crate, which now exports 8 helpers (find_msg, find_field, etc.).
- **Empty `test-utils` crate** -- Now populated with shared helpers.
- **Hand-rolled enum conversions** -- Replaced with `impl_from_int!` and `impl_into_i32!` macros in `schema/src/descriptor.rs`.
- **Copy-paste validation logic** -- Extracted into `check_exclusive_range_overlaps` and `check_cross_range_overlaps` generic functions in `analyzer/src/validate.rs`.
- **`AnalyzeError` construction boilerplate** -- `make_err` helper added in `analyzer/src/validate.rs:10`.
- **No-op `| 0` in annotator** -- Removed from `annotator/src/walker.rs`.
- **F1: Duplicate `to_camel_case` / `default_json_name`** -- Consolidated to one implementation in `parser/src/parser/parse_helpers.rs:104`, re-exported from parser crate. `analyzer/src/validate.rs` now delegates to it.
- **F2: Duplicate `WireType` enum** -- `schema/src/descriptor.rs` and `annotator/src/wire.rs` defined identical `WireType` enums. Removed the annotator's copy; `annotator/src/wire.rs` now re-exports `protoc_rs_schema::WireType`.
- **F3: Duplicate `is_packable_type`** -- Added `FieldType::is_packable()` method to `schema/src/descriptor.rs`. Both call sites now delegate to it.
- **F4: Dead code -- 20+ unused `field_num` constants** -- Removed from `parser/src/source_info_builder.rs`.
- **F5: Silent failures in option application** -- `parser/src/parser/options.rs` bool options now use `expect_bool_option` which returns errors for non-bool values.
- **F6: Redundant `from_i32` wrappers** -- Removed from `schema/src/descriptor.rs`. Callers use `from_int` directly.
- **F7: Duplicate `parse_reserved` / `parse_enum_reserved`** -- Extracted shared logic into a helper in `parser/src/parser/mod.rs`.
- **F8: `truncate_string` byte-index panic on multi-byte UTF-8** -- `annotator/src/walker.rs:842` used `&s[..max_len]` which panics on multi-byte chars. Replaced with `s.chars().take(max_len).collect()`.

## Resolved -- High Priority

- **H1: Silent failures: unrecognized enum option values** -- Fixed. `optimize_for`, `jstype`, and `ctype` wildcard arms now return `ParseError` with descriptive messages instead of `None`.
- **H2: Silent failure: unrecognized idempotency_level** -- Fixed. Now uses `expect_enum_option` and returns `ParseError` for unrecognized values, consistent with H1.
- **H3: Silent failure: double_value parse failure** -- Fixed. Used `unwrap_or(f64::NAN)` fallback so the field is always present in serialized output.
- **H4: unwrap() calls in AnalyzeContext (7 sites)** -- Fixed. Added `get_file()` helper returning `Result<&FileDescriptorProto, AnalyzeError>`. All 7 `.unwrap()` sites replaced with `self.get_file(name)?`.
- **H5: unwrap() on source_span in parser** -- Fixed. Replaced `.unwrap()` with `.unwrap_or_default()` at `parser/src/parser/mod.rs`.

## Resolved -- Medium Priority

- **M1: Repeated prefix-building logic (5 sites)** -- Fixed. Added `make_fqn_prefix(pkg: &str) -> String` function. All 5 sites in `analyzer/src/lib.rs` now use it.
- **M6: Unused `_file_pkg` parameter** -- Fixed. Removed from `resolve::resolve_type_name` and its wrapper `AnalyzeContext::resolve_type_name`. Also cleaned up `resolve_field_type` which no longer needed it. All call sites updated.
- **M7: Misleading underscore prefix on used field** -- Fixed. Renamed `_include_source_info` to `include_source_info` in `compiler/src/main.rs`.

## Resolved -- This Review (2026-03-26)

- **R1: `unwrap()` in `is_valid_identifier`** -- `parser/src/parser/parse_helpers.rs:96`. Replaced `chars.next().unwrap()` with `let Some(first) = chars.next() else { return false; }` pattern. Removes unwrap from non-test code.
- **R2: `unwrap()` in group json_name** -- `parser/src/parser/mod.rs:1203`. Replaced `field.name.as_ref().unwrap()` with `if let Some(ref name) = field.name` guard. Removes unwrap from non-test code.
- **R3: `expect()` in `generate_data`** -- `proto-gen/src/data_gen.rs:15`. Replaced `.expect("root message not found")` with `let Some(root) = ... else { return Vec::new(); }`. Library code no longer panics.
- **R4: Unnecessary `syntax.clone()`** -- `codegen/src/rust_gen.rs:52`. Removed clone; `syntax` is now moved into the struct directly since `syntax_enum()` returns an owned value.
- **R5: Clippy `type_complexity` warning** -- `conformance/tests/conformance_test.rs:197`. Extracted `type CheckFn` alias to silence clippy warning. Zero clippy warnings now.

## Skipped -- Medium Priority

### M2. Duplicate collect_symbols_from_prefix calls
- **Skipped:** Partially addressed by M1 (prefix-building was the main duplication). The remaining 3 call sites operate in different contexts (own file, direct imports, public transitive imports) with different surrounding logic. Extracting a common helper would hide important context differences. Not worth the abstraction.

### M3. Expensive .clone() on FileDescriptorProto during analysis
- **Skipped:** Requires architectural change (interior mutability or two-pass resolution pattern). The clone-modify-reinsert pattern, while inefficient, is correct and clear. This is a performance optimization that should be done separately with benchmarks to measure impact.

### M4. Inconsistent apply_*_option function signatures
- **Skipped:** Unifying to borrowed references (`&str, &OptionValue`) requires updating all internal helper calls (which pass `&name` / `&value` that would become double-references). The change cascades further than expected for the readability benefit. The inconsistency is minor and documented.

### M5. Stringly-typed Syntax field on FileDescriptorProto
- **Skipped:** Changing the schema's `syntax` field touches the schema crate and all downstream consumers (parser, analyzer, codegen, compiler). This is a cross-crate refactor that should be its own focused change.

### M8. Inconsistent error type patterns across crates
- **Skipped:** Adding `thiserror` to all crates and converting manual `Display`/`Error` impls is a mechanical but broad change across 4 crates. Low risk but low impact -- the current manual impls work correctly. Better done as a standalone cleanup.

## Open -- Low Priority

### L1. Empty `conformance/src/lib.rs`
- **Location:** `conformance/src/lib.rs:1-2`
- **Problem:** Contains only a comment. The crate only has integration tests; the empty `lib.rs` is unnecessary but harmless.
- **Status:** Skipped -- harmless, no functional impact.

### L2. Duplicate varint/tag encoding across crates
- **Location:** `compiler/src/descriptor_set.rs:20-34` and `proto-gen/src/data_gen.rs:166-182`
- **Problem:** Identical varint and tag encoding logic implemented independently. Not trivially consolidatable because `proto-gen` deliberately does not depend on `schema`.
- **Status:** Skipped -- accepted as cost of crate independence.

### L3. Banner/divider comments in proto-gen
- **Status:** Skipped -- cosmetic only.

### L4. FieldTypeDef duplicates FieldType from schema crate
- **Status:** Skipped -- intentional design choice for crate independence.

## Resolved -- Low Priority

- **L5: fqn_parts.last().unwrap() in codegen** -- Fixed. Replaced with `.unwrap_or(&"Unknown")` at `codegen/src/rust_gen.rs`.
