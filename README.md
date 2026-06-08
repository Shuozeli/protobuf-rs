# protobuf-rs

A pure Rust implementation of the [Protocol Buffers](https://protobuf.dev/) compiler and runtime.

Zero dependency on `protoc` or any C++ code -- parses `.proto` files, resolves types, and generates Rust code entirely in Rust.

## Features

**Compiler**
- Recursive descent parser for `.proto` files (proto2, proto3, editions 2023/2024)
- Semantic analysis: import resolution, type resolution, validation
- CLI producing byte-for-byte identical `FileDescriptorSet` output to `protoc`
- 12 well-known types embedded (Timestamp, Duration, Any, Status, etc.)
- 670+ tests including upstream C++ test parity

**Runtime Backend** (`--runtime_out`)
- Own wire format encode/decode (no prost dependency)
- Zero-copy view types (`FooView<'a>`) -- 2.2x faster decode by borrowing from wire buffer
- Type-safe enums (`EnumValue<E>` with `Known`/`Unknown` preservation)
- Ergonomic message fields (`MessageField<T>` with static default deref)
- Unknown field preservation for round-trip fidelity
- Two-pass O(n) serialization via `CachedSize`
- Proto3 JSON mapping via serde (optional `json` feature)
- `no_std + alloc` support
- Deterministic map encoding (`BTreeMap`)
- Extension field typed accessors
- Wire-compatible with prost/protoc (cross-validated)

**Prost Backend** (`--rust_out`)
- Generates `prost`-compatible Rust structs with derive macros
- Optional gRPC service stubs via [pure-grpc-rs](https://github.com/shuozeli/pure-grpc-rs)

**TypeScript/Node.js Backend** (`--ts_out`, `--nodejs_out`)
- Generates dependency-free TypeScript model interfaces and enums
- Preserves proto2/proto3 optionality, repeated fields, maps, nested messages, oneofs, enum aliases, and keyword-safe identifiers

**Python Backend** (`--python_out`)
- Generates dependency-free Python dataclass models and `IntEnum` enums
- Preserves proto2/proto3 optionality, repeated fields, maps, nested messages, oneofs, enum aliases, and keyword-safe identifiers

## Crate Structure

| Crate | Description |
|-------|-------------|
| `schema` | IR types mirroring `descriptor.proto` |
| `parser` | `.proto` -> unresolved `FileDescriptorProto` |
| `analyzer` | Import resolution, type resolution, validation |
| `compiler` | CLI entry point + `FileDescriptorSet` serializer |
| `runtime` | Wire format, `Message` trait, views, serde, extensions |
| `wkt` | Pre-generated well-known types for the runtime backend |
| `build` | `build.rs` helper for compile-time code generation |
| `codegen` | Rust, TypeScript/Node.js, and Python code generation |
| `annotator` | Binary walker with byte-level annotation |
| `proto-gen` | Random `.proto` schema + binary data generator |
| `conformance` | Real-world `.proto` conformance tests |
| `test-utils` | Shared test helpers |
| `wasm-api` | WASM bindings for browser-based compilation |

## Quick Start

```bash
# Build
cargo build --workspace

# Run all tests
cargo test --workspace

# Parse and validate a .proto file
cargo run -- -I src/ file.proto

# Output FileDescriptorSet binary (like protoc --descriptor_set_out)
cargo run -- -I src/ -o out.pb file.proto

# Generate runtime-backed Rust code (recommended)
cargo run -- -I src/ --runtime_out out/ file.proto

# Generate runtime code with serde JSON support
cargo run -- -I src/ --runtime_out out/ --runtime_out_json file.proto

# Generate prost-compatible Rust code
cargo run -- -I src/ --rust_out out/ file.proto

# Generate TypeScript/Node.js model code
cargo run -- -I src/ --ts_out out/ file.proto

# Same TypeScript backend, named for Node.js build pipelines
cargo run -- -I src/ --nodejs_out out/ file.proto

# Generate Python dataclass model code
cargo run -- -I src/ --python_out out/ file.proto

# Generate multiple language outputs in one invocation
cargo run -- -I src/ --runtime_out out/rust --ts_out out/ts --python_out out/py file.proto
```

## Language Codegen Usage

`protoc-rs` can emit several language targets from the same analyzed descriptor set.

| Flag | Output | Runtime dependency | Notes |
|------|--------|--------------------|-------|
| `--runtime_out <DIR>` | `package.rs` | `protoc-rs-runtime` | Full Rust wire encode/decode backend with owned and zero-copy view types |
| `--rust_out <DIR>` | `package.rs` | `prost` | Prost-compatible Rust structs and enums |
| `--ts_out <DIR>` | `package.ts` | none | TypeScript model interfaces, enums, oneof unions, maps, repeated fields, and keyword-safe names |
| `--nodejs_out <DIR>` | `package.ts` | none | Alias for `--ts_out`; useful when build scripts name the Node.js target explicitly |
| `--python_out <DIR>` | `package.py` | Python standard library | Python `dataclass(slots=True)` models and `IntEnum` enums |

For package-less `.proto` files, the output name is derived from the proto path. For example, `api/user.proto` emits `api.user.ts`, `api.user.py`, or `api.user.rs`.

The TypeScript/Node.js and Python backends generate typed model code only. They preserve schema shape and defaults for application code and tooling, but they do not include protobuf wire encode/decode runtimes.

## Using the Runtime Backend

### In a build script

```toml
# Cargo.toml
[dependencies]
protoc-rs-runtime = { path = "path/to/runtime" }

[build-dependencies]
protoc-rs-build = { path = "path/to/build" }
```

```rust
// build.rs
fn main() {
    protoc_rs_build::Config::new()
        .include("proto/")
        .compile(&["proto/my_service.proto"])
        .expect("protobuf codegen failed");
}
```

```rust
// src/lib.rs
include!(concat!(env!("OUT_DIR"), "/my.package.rs"));
```

### Generated code example

For a proto message:
```protobuf
message Person {
    string name = 1;
    int32 id = 2;
    repeated string tags = 3;
}
```

The runtime generates both owned and zero-copy view types:

```rust
// Owned type -- full encode/decode/modify support
let mut msg = Person::default();
msg.name = "Alice".to_string();
msg.id = 42;

let bytes = protoc_rs_runtime::encode(&msg);
let decoded: Person = protoc_rs_runtime::decode(&bytes).unwrap();

// View type -- zero-copy, borrows from wire buffer
let view: PersonView = protoc_rs_runtime::decode_view(&bytes).unwrap();
assert_eq!(view.name, "Alice"); // &str pointing into `bytes`

// Convert view to owned when needed
let owned: Person = view.into();
```

## Performance

Benchmarks on a message with string, int, float, repeated, and bytes fields (~350 bytes encoded):

| Operation | Time |
|---|---|
| varint decode (1 byte) | 1.6 ns |
| varint decode (10 bytes) | 7.5 ns |
| message encode | 77 ns |
| message decode (owned) | 184 ns |
| **message decode (view)** | **83 ns** (2.2x faster) |
| compute_size | 12 ns |

Run benchmarks: `cargo bench -p protoc-rs-runtime`

## Architecture

See [docs/runtime-architecture.md](docs/runtime-architecture.md) for the full crate map, module breakdown, and design decisions.

See [docs/runtime-roadmap.md](docs/runtime-roadmap.md) for implementation history and future work.

## License

BSD 3-Clause -- same as the original Protocol Buffers project. See [LICENSE](LICENSE).
