# Runtime Roadmap

## Completed (Phases 1-5)

### Phase 1: Runtime Foundation
- [x] Wire format encode/decode (varint fast path, fixed, tags, zigzag, groups)
- [x] `Message` trait (compute_size, write_to, merge_field, clear)
- [x] `CachedSize` (AtomicU32 Relaxed, transparent eq/hash)
- [x] `EnumValue<E>` (Known/Unknown, serde string/int)
- [x] `MessageField<T>` (Option<Box<T>> with static default deref)
- [x] `UnknownFields` (round-trip for all 5 wire types)

### Phase 2: Codegen Integration
- [x] Runtime-backed codegen (`rust_gen_runtime.rs`)
- [x] Zero-copy view types (`FooView<'a>` with `&str`/`&[u8]`)

### Phase 3: Features
- [x] Proto3 JSON via serde (optional `json` feature)
- [x] Editions feature resolution (proto2/proto3/2023/2024 as presets)

### Phase 4: Hardening
- [x] 4 fuzz targets
- [x] `no_std + alloc` support
- [x] Criterion benchmarks (2.2x view decode speedup)

### Phase 5: End-to-End
- [x] `--runtime_out` CLI flag
- [x] Packed repeated field decode
- [x] 4 end-to-end integration tests
- [x] `protoc-rs-wkt` crate (12 well-known types)
- [x] Name collision fix (fully-qualified paths in generated code)

## Phase 6: Production Readiness

### Completed

- [x] Packed repeated encoding in codegen (proto3 default, gated on `features.packed_by_default`)
- [x] `From<FooView<'a>> for Foo` conversion (view-to-owned, handles boxed nested messages)
- [x] Conformance tests (simple, upstream_enums, upstream_nested, upstream_parent)
- [x] Boxed view message fields (`Option<Box<FooView<'a>>>`) to handle recursive types
- [x] Numeric enum variant names prefixed with `_` when starting with digit
- [x] Fully-qualified `Result`/`Ok`/`Err` in generated code (prevents `google.protobuf.Result`-style collisions)
- [x] Deterministic map encoding (`BTreeMap` instead of `HashMap`)
- [x] Proto2 custom default values (`[default = 42]`)
- [x] `protoc-rs-build` crate (build.rs helper for compile-time codegen)
- [x] Extension support (ExtensionDescriptor + MessageExtensionDescriptor with typed accessors)
- [x] gRPC service generation (server traits + client stubs with runtime Message types)

### High Priority (Original tasks, now done)

#### Packed repeated ENCODING in codegen
- **What**: Codegen emits unpacked encoding for repeated scalars. Proto3 default is packed.
- **Why**: Wire output won't match protoc. Other decoders expect packed for proto3.
- **Where**: `codegen/src/rust_gen_runtime.rs` -> `gen_field_write_to` and `gen_field_compute_size`
  for `FieldLabel::Repeated` with packable types.
- **How**: Use `wire::encode_packed_varints` / `wire::encode_packed_fixed32` etc. from runtime.
  Gate on `features.packed_by_default` (true for proto3/editions, false for proto2).
- **Test**: Add e2e test that encodes with our runtime, decodes with prost, and verifies equality.

#### Into<Owned> conversion: FooView<'a> -> Foo
- **What**: Generate `impl<'a> From<FooView<'a>> for Foo` on every message.
- **Why**: Views are read-only. Users need to promote to owned for storage/mutation.
- **Where**: `codegen/src/rust_gen_runtime.rs` -> add `gen_view_into_owned` after view struct.
- **How**: For each field: `&str -> String` (`.to_string()`), `&[u8] -> Vec<u8>` (`.to_vec()`),
  scalars copy, nested views recurse. `MessageField::some(inner.into())` for nested.
- **Test**: e2e test that decodes as view, converts to owned, re-encodes, verifies match.

#### Conformance test: 29 real-world protos through runtime codegen
- **What**: Run all protos in `testdata/` through `--runtime_out`, compile the output.
- **Why**: Will flush out edge cases in real schemas (Kubernetes, gRPC, Envoy, etc.).
- **Where**: New test file `compiler/tests/runtime_conformance_test.rs`.
- **How**: Similar to `codegen_compile_test.rs` but using `generate_rust_runtime`.
  For each proto in testdata, generate runtime code, write to temp project, `cargo check`.
- **Expected issues**: Cross-package references, deeply nested types, group fields,
  proto2-specific features, keyword field names.

### Medium Priority

#### Map field deterministic encoding
- **What**: HashMap iteration order is non-deterministic. Re-encoded messages aren't byte-stable.
- **Why**: Some systems (caching, dedup) rely on deterministic encoding.
- **Where**: `codegen/src/rust_gen_runtime.rs` -> `gen_field_write_to` for map fields.
- **How**: Sort map entries by key before writing. Use `BTreeMap` instead of `HashMap`,
  or sort in `write_to`. Prefer BTreeMap in generated structs for determinism.
- **Note**: This is a breaking API change if users depend on HashMap. Consider a codegen option.

#### Extension support in runtime codegen
- **What**: Parser handles `extend` blocks but runtime codegen ignores them.
- **Where**: `codegen/src/rust_gen_runtime.rs` -- need new `gen_extension` method.
- **How**: Extensions store values in `UnknownFields`. Generate typed accessor methods
  that decode from unknown fields on access (lazy, like buffa).
- **Reference**: See buffa's extension design in `buffa/src/extension.rs`.

#### gRPC integration with runtime types
- **What**: `pure-grpc-rs` currently works with prost types. Update for runtime types.
- **Where**: `codegen/src/service_gen.rs` (behind `grpc` feature).
- **How**: Generated service traits need `Message` bound instead of `prost::Message`.
  The `tonic`-compatible layer needs adaptation.

## Phase 7: Polish

#### build.rs helper crate
- **What**: `protoc-rs-build` crate for compile-time code generation from `build.rs`.
- **Why**: Standard pattern for protobuf in Rust (like `prost-build`).
- **How**: Wraps `protoc_rs_analyzer::analyze_files` + `protoc_rs_codegen::generate_rust_runtime`.
  Writes output to `OUT_DIR`, emits `include!()` instructions.

#### Proto2 custom default values
- **What**: Proto2 fields can have `[default = 42]`. Currently ignored.
- **Where**: `codegen/src/rust_gen_runtime.rs` -> `field_default_value` and `gen_default_impl`.
- **How**: Read `FieldDescriptorProto.default_value` string, parse to Rust literal.
  Affects Default impl and MessageField deref behavior.

## Architecture Notes for Future Work

### Codegen string generation pattern
The codegen uses `push_line()` to emit Rust source as strings. All runtime type
references must be fully-qualified (`protoc_rs_runtime::CachedSize`, not `CachedSize`)
to prevent name collisions with proto types. See `docs/runtime-architecture.md`.

### Testing pattern
- **Unit tests**: In each module's `#[cfg(test)]` block
- **Compile-check tests**: `compiler/tests/runtime_codegen_compile_test.rs` -- generates code
  in temp Cargo project, runs `cargo check`
- **E2E tests**: `compiler/tests/runtime_e2e_test.rs` -- generates code, compiles, runs
  assertions in generated binary via `cargo run`
- **Serde tests**: Gated behind `#[cfg(feature = "json")]`

### WKT regeneration
The WKT crate (`wkt/`) contains checked-in generated code. To regenerate:
```bash
cargo run -p protoc-rs-compiler -- --runtime_out /tmp/wkt_gen \
  analyzer/src/well_known_protos/google/protobuf/*.proto \
  analyzer/src/well_known_protos/google/rpc/*.proto \
  -I analyzer/src/well_known_protos/
```
Then copy, remove `#![allow(...)]` inner attributes, deduplicate imports,
and fix `google::protobuf::` -> `crate::google_protobuf::` in google_rpc.rs.

### Key files to read first
1. `runtime/src/message.rs` -- Message trait definition
2. `runtime/src/wire.rs` -- wire format primitives
3. `codegen/src/rust_gen_runtime.rs` -- the codegen (2300+ lines)
4. `codegen/src/features.rs` -- editions feature resolution
5. `compiler/tests/runtime_e2e_test.rs` -- how e2e tests work
