# protobuf-rs

A pure Rust implementation of the [Protocol Buffers](https://protobuf.dev/) compiler.

## Features

- Recursive descent parser for `.proto` files (proto2, proto3, editions)
- Semantic analysis: import resolution, type resolution, validation
- CLI with `--descriptor_set_out` producing byte-for-byte identical output to `protoc`
- 10 well-known types embedded (Any, Timestamp, Duration, etc.)
- Editions support (2023, 2024)
- Real-world conformance testing against 29 open-source `.proto` files
- Random schema and binary data generation (`proto-gen` crate)
- 660+ tests including upstream C++ test parity and cross-compat with `protoc`

## Crate Structure

| Crate | Description |
|-------|-------------|
| `schema` | IR types mirroring `descriptor.proto` |
| `parser` | `.proto` -> unresolved `FileDescriptorProto` |
| `analyzer` | Import resolution, type resolution, validation |
| `compiler` | CLI entry point + `FileDescriptorSet` serializer |
| `annotator` | Binary walker with byte-level annotation |
| `codegen` | Rust code generation (prost-compatible structs). Optional `grpc` feature generates gRPC service stubs via [pure-grpc-rs](https://github.com/shuozeli/pure-grpc-rs) |
| `proto-gen` | Random `.proto` schema + binary data generator |
| `conformance` | Real-world `.proto` conformance tests |
| `test-utils` | Shared test helpers (find_msg, find_field, find_enum, etc.) |
| `wasm-api` | WASM bindings for browser-based protobuf compilation |

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

# Include imported files in output
cargo run -- -I src/ -o out.pb --include_imports file.proto

# Generate prost-compatible Rust code
cargo run -- -I src/ --rust_out out/ file.proto

# Include source code info in output
cargo run -- -I src/ -o out.pb --include_source_info file.proto

# Dump parsed schema (debug)
cargo run -- --dump-schema -I src/ file.proto
```

## License

BSD 3-Clause -- same as the original Protocol Buffers project. See [LICENSE](LICENSE).
