# Test Gap Analysis: protobuf-rs vs C++ protoc

## Automated Verification

Run `python3 scripts/verify_test_parity.py` to check parity. The script scans both
C++ test files, auto-matches to Rust tests, and merges hand-curated overrides from
`scripts/test_mapping_overrides.jsonl`.

```bash
python3 scripts/verify_test_parity.py              # verify parity
python3 scripts/verify_test_parity.py --init-mapping   # regenerate mapping
```

## Coverage Summary

| C++ Test File | Fixtures | In-Scope Tests | Covered | Coverage |
|---|---|---|---|---|
| `parser_unittest.cc` | 11 | 257 | 257 | **100%** |
| `importer_unittest.cc` | 3 | 7 | 7 | **100%** |
| **Total** | **14** | **264** | **264** | **100%** |

### Per-Fixture Breakdown

| C++ Fixture | Source File | C++ Count | Mapped | Status |
|---|---|---|---|---|
| ParseErrorTest | parser_unittest.cc | 68 | 68 | **Complete** |
| ParseMessageTest | parser_unittest.cc | 67 | 67 | **Complete** |
| ParserValidationErrorTest | parser_unittest.cc | 47 | 47 | **Complete** |
| SourceInfoTest | parser_unittest.cc | 28 | 28 | **Complete** |
| ParseEditionsTest | parser_unittest.cc | 18 | 18 | **Complete** |
| ParseVisibilityTest | parser_unittest.cc | 9 | 9 | **Complete** |
| ParseImportTest | parser_unittest.cc | 8 | 8 | **Complete** |
| ParseEnumTest | parser_unittest.cc | 6 | 6 | **Complete** |
| ParseMiscTest | parser_unittest.cc | 4 | 4 | **Complete** |
| ParseServiceTest | parser_unittest.cc | 2 | 2 | **Complete** |
| ImporterTest | importer_unittest.cc | 7 | 7 | **Complete** |

### Excluded Fixtures (not in scope)

| C++ Fixture | Source File | Count | Reason |
|---|---|---|---|
| ParseDescriptorDebugTest | parser_unittest.cc | 5 | Debug string output, different scope |
| ParserTest | parser_unittest.cc | 2 | Internal parser plumbing tests |
| DiskSourceTreeTest | importer_unittest.cc | 11 | C++ DiskSourceTree path mapping; no Rust equivalent (we use `FileResolver` trait) |
| SourceTreeDescriptorDatabaseTest | importer_unittest.cc | 6 | C++-specific extension declarations from `.txtpb` files |

## ImporterTest Details (7/7)

| C++ Test | Rust Test | Description |
|---|---|---|
| Import | cpp_importer_import | Basic import + deduplication |
| ImportNested | cpp_importer_import_nested | Transitive import, cross-linking |
| FileNotFound | cpp_importer_file_not_found | Root file not found |
| ImportNotFound | cpp_importer_import_not_found | Dependency not found |
| RecursiveImport | cpp_importer_recursive_import | A imports B imports A (cycle) |
| RecursiveImportSelf | cpp_importer_recursive_import_self | A imports itself |
| LiteRuntimeImport | cpp_importer_lite_runtime_import | Non-lite cannot import lite |

## ParseErrorTest Details (68/68)

All 68 C++ `ParseErrorTest` cases covered. Key categories:
- Syntax errors (missing names, bodies, field numbers)
- Default value validation (type mismatches, out-of-range, missing)
- Label errors (proto2 missing label, oneof labels, map labels)
- Reserved/extension errors (ranges, names, mixing)
- Edition-specific errors (features without editions, option imports)
- Error recovery (multiple parse errors in one file)

## ParserValidationErrorTest Details (47/47)

All 47 C++ `ParserValidationErrorTest` cases covered. Key categories:
- Duplicate name checks (message, field, enum, service, method)
- Type resolution errors (field, extendee, method input/output)
- Import validation (unloaded, duplicate, lite runtime)
- Field number / range validation
- Proto3 restrictions (required, default, extensions, message_set)
- JSON name conflict detection (proto2 vs proto3, custom vs default)
- File/field option validation (unknown names, wrong types)
- Edition-specific (import options, extension option resolution)

## SourceInfoTest Details (28/28)

All spans and doc comment attachment verified.

## ParseEditionsTest Details (18/18)

Covers basic editions parsing, edition validation errors, mixed syntax/edition,
label restrictions, feature validation (features without editions, implicit
presence + default conflict, UNKNOWN feature values).

## ParseVisibilityTest Details (9/9)

Edition 2023/2024 visibility keywords (export, local) for messages, enums,
services, extends. Nested keyword-as-name tests.

## ParseMessageTest Notable Coverage

- UNSTABLE edition type system: varint32/64, uvarint32/64, zigzag32/64
- UNSTABLE type remapping: int32->SFIXED32, uint32->FIXED32, etc.
- UNSTABLE obsolete types: fixed32/sfixed32/sint32 banned
- Edition 2024 scalar types (10 tests)
- Maps, extensions, reserved ranges, message_set

## Future Coverage Targets

- `descriptor_unittest.cc` (542 tests) -- comprehensive descriptor validation
