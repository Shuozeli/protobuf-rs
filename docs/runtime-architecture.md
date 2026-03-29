# Runtime Architecture

## Overview

protobuf-rs has a dual-backend architecture: the original **prost backend** (`--rust_out`) and the new **runtime backend** (`--runtime_out`). The runtime backend owns the entire stack -- wire format, message traits, type-safe enums, zero-copy views, and serialization -- instead of depending on prost.

## Crate Map

```
protobuf-rs workspace
├── schema/          # IR types (FieldDescriptorProto, etc.) -- shared by both backends
├── parser/          # Hand-written recursive descent .proto parser
├── analyzer/        # Import resolution, type resolution, validation, WKT embedding
├── codegen/         # Code generation
│   ├── rust_gen.rs           # Prost-compatible backend (--rust_out)
│   ├── rust_gen_runtime.rs   # Runtime backend (--runtime_out)
│   └── features.rs           # Editions feature resolution (proto2/proto3 as presets)
├── compiler/        # CLI binary (protoc-rs)
├── runtime/         # protoc-rs-runtime: wire format, Message trait, views, serde
├── wkt/             # protoc-rs-wkt: pre-generated WKTs (Timestamp, Any, Status, etc.)
├── annotator/       # Binary protobuf walker (annotates wire data)
├── proto-gen/       # Random .proto schema + data generator (testing)
├── conformance/     # Real-world .proto conformance tests
├── fuzz/            # Fuzzing targets (4 targets, requires nightly)
├── wasm-api/        # WASM bindings for browser compilation
└── test-utils/      # Shared test helpers
```

## Runtime Crate Modules

```
protoc-rs-runtime/
├── wire.rs            # Varint (single-byte fast path), fixed32/64, tags, zigzag,
│                      # field skipping, packed encode/decode helpers
├── message.rs         # Message trait: compute_size, write_to, merge_field, clear
│                      # Free functions: encode(), decode(), decode_with_options()
│                      # DecodeOptions: recursion_limit, max_message_size
├── cached_size.rs     # CachedSize (AtomicU32 Relaxed) for two-pass O(n) serialization
├── enumeration.rs     # Enumeration trait + EnumValue<E> (Known/Unknown)
│                      # Serde: Known -> string name, Unknown -> i32
├── message_field.rs   # MessageField<T> wrapping Option<Box<T>>
│                      # Deref to static default instance when unset (via DefaultInstance)
├── unknown_fields.rs  # UnknownFields: preserves all 5 wire types including nested groups
├── view.rs            # MessageView<'a> trait for zero-copy decode
│                      # decode_view(), decode_sub_view()
├── error.rs           # DecodeError enum
└── lib.rs             # Feature flags: std (default), json (serde)
```

## Generated Code Shape

For a message like:
```protobuf
message Person {
  string name = 1;
  int32 id = 2;
  Status status = 3;
  Inner inner = 4;
}
```

The runtime codegen produces:

### Owned type
```rust
#[derive(Clone, PartialEq)]
pub struct Person {
    pub name: ::std::string::String,
    pub id: i32,
    pub status: protoc_rs_runtime::EnumValue<Status>,
    pub inner: protoc_rs_runtime::MessageField<Inner>,
    pub _cached_size: protoc_rs_runtime::CachedSize,
    pub _unknown_fields: protoc_rs_runtime::UnknownFields,
}
impl protoc_rs_runtime::Message for Person { ... }
unsafe impl protoc_rs_runtime::DefaultInstance for Person { ... }
```

### View type (zero-copy)
```rust
pub struct PersonView<'a> {
    pub name: &'a str,          // borrows from wire buffer
    pub id: i32,
    pub status: protoc_rs_runtime::EnumValue<Status>,
    pub inner: ::core::option::Option<InnerView<'a>>,
    pub _phantom: ::std::marker::PhantomData<&'a ()>,
}
impl<'a> protoc_rs_runtime::view::MessageView<'a> for PersonView<'a> { ... }
```

## Key Design Decisions

### Fully-qualified paths in generated code
All generated code uses `::core::option::Option`, `::std::string::String`,
`protoc_rs_runtime::EnumValue`, etc. instead of `use protoc_rs_runtime::*`.
This prevents name collisions when proto types are named `Option`, `EnumValue`,
`String`, etc. (as in `google.protobuf.Option`).

### Two-pass serialization
`compute_size()` caches sizes in `CachedSize` (AtomicU32), then `write_to()`
reads cached sizes for length prefixes. Both passes are O(n). Without caching,
nested messages cause O(depth^2) size computation.

### Packed field decode accepts both formats
Per proto3 spec, decoders must accept both packed (LengthDelimited) and unpacked
(individual tags) encoding for repeated scalar fields. The codegen emits a
`wire_type == WireType::LengthDelimited` check that branches to packed decode.

### Editions as feature presets
`features.rs` defines `ResolvedFeatures` with `field_presence` (Explicit/Implicit),
`enum_type` (Open/Closed), and `packed_by_default`. Proto2 and proto3 are just
preset configurations. The codegen uses `ResolvedFeatures` instead of matching
on `Syntax` directly.

## Performance

Benchmark results (from `cargo bench -p protoc-rs-runtime`):

| Operation | Time |
|---|---|
| varint decode (1 byte) | 1.6 ns |
| varint decode (10 bytes) | 7.5 ns |
| message encode (~350 bytes) | 77 ns |
| message decode (owned) | 184 ns |
| message decode (view, zero-copy) | 83 ns (2.2x faster) |
| compute_size | 12 ns |

## Testing

- 55+ runtime unit tests (wire, message, cached_size, enum, message_field, unknown_fields, view)
- 6 serde JSON tests (behind `json` feature)
- 9 codegen unit tests
- 6 compile-check integration tests (generates code, compiles in temp project)
- 4 end-to-end tests (parse proto -> generate -> compile -> encode -> decode -> verify)
- 4 fuzz targets (decode, encode-decode roundtrip, varint, JSON enum)
- All existing 670+ parser/analyzer/compiler tests continue passing
