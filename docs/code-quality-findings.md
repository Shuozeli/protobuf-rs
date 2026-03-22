# Code Quality Findings

Last updated: 2026-03-21

## Resolved (Prior Reviews)

- **Duplicate `rust_field_type`/`rust_map_value_type`** -- Deduplicated; only `rust_field_type` remains in `codegen/src/rust_gen.rs:553`.
- **Duplicate test helpers across crates** -- Moved to `test-utils` crate, which now exports 8 helpers (find_msg, find_field, etc.).
- **Empty `test-utils` crate** -- Now populated with shared helpers.
- **Hand-rolled enum conversions** -- Replaced with `impl_from_int!` and `impl_into_i32!` macros in `schema/src/descriptor.rs`.
- **Copy-paste validation logic** -- Extracted into `check_exclusive_range_overlaps` and `check_cross_range_overlaps` generic functions in `analyzer/src/validate.rs`.
- **`AnalyzeError` construction boilerplate** -- `make_err` helper added in `analyzer/src/validate.rs:10`.
- **No-op `| 0` in annotator** -- Removed from `annotator/src/walker.rs`.
- **F1: Duplicate `to_camel_case` / `default_json_name`** -- Consolidated to one implementation in `parser/src/parser/parse_helpers.rs:104`, re-exported from parser crate. `analyzer/src/validate.rs` now delegates to it.
- **F4: Dead code -- 20+ unused `field_num` constants** -- Removed from `parser/src/source_info_builder.rs`.
- **F5: Silent failures in option application** -- `parser/src/parser/options.rs` bool options now use `expect_bool_option` which returns errors for non-bool values.
- **F6: Redundant `from_i32` wrappers** -- Removed from `schema/src/descriptor.rs`. Callers use `from_int` directly.
- **F7: Duplicate `parse_reserved` / `parse_enum_reserved`** -- Extracted shared logic into a helper in `parser/src/parser/mod.rs`.

## Resolved (Current Review)

- [DONE] **F2: Duplicate `WireType` enum** -- `schema/src/descriptor.rs` and `annotator/src/wire.rs` defined identical `WireType` enums. Removed the annotator's copy; `annotator/src/wire.rs` now re-exports `protoc_rs_schema::WireType`. Adapted `decode_tag` to use `Option`-returning `from_u32` (`.ok_or(...)` instead of `.map_err(...)`).
- [DONE] **F3: Duplicate `is_packable_type`** -- `codegen/src/rust_gen.rs:451` and `annotator/src/walker.rs:805` had near-identical match arms listing all packable field types. Added `FieldType::is_packable()` method to `schema/src/descriptor.rs`. Both call sites now delegate to it.
- [DONE] **F8: `truncate_string` byte-index panic on multi-byte UTF-8** -- `annotator/src/walker.rs:842` used `&s[..max_len]` which panics if `max_len` falls in the middle of a multi-byte UTF-8 character. Replaced with `s.chars().take(max_len).collect()`.

## Open (Low Priority)

### 1. Empty `conformance/src/lib.rs`
- Lines 1-2: `// Conformance test library -- intentionally empty.`
- Crate only has integration tests. The empty `lib.rs` is unnecessary but harmless.

### 2. `proto-gen/src/data_gen.rs:10` -- `generate_data` panics via `.expect()`
- Returning `Result` would be more idiomatic for a library function.

### 3. Noise comments in `compiler/src/descriptor_set.rs`
- Dense `// N: field_name` comments on every encode call mirror the struct field names. Harmless but noisy.

### 4. Duplicate varint/tag encoding across crates
- `proto-gen/src/data_gen.rs:166-181` (`write_varint`, `write_tag`) and `compiler/src/descriptor_set.rs:20-34` (`encode_varint`, `encode_tag`) implement the same logic. Not trivially consolidatable because `proto-gen` does not depend on `schema` and uses its own type system.
