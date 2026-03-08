# protobuf-rs

A pure Rust implementation of the [Protocol Buffers](https://protobuf.dev/) compiler.

## Features

- Hand-written recursive descent parser for `.proto` files (proto2, proto3, editions)
- Semantic analysis: import resolution, type resolution, validation
- 10 well-known types embedded (Any, Timestamp, Duration, etc.)
- Editions support (2023, 2024)
- Random schema and binary data generation (`proto-gen` crate)
- 500+ tests including upstream C++ test parity

## Crate Structure

| Crate | Description |
|-------|-------------|
| `schema` | IR types mirroring `descriptor.proto` |
| `parser` | `.proto` -> unresolved `FileDescriptorProto` |
| `analyzer` | Import resolution, type resolution, validation |
| `annotator` | Source annotation utilities |
| `codegen` | Code generation (placeholder) |
| `compiler` | CLI entry point |
| `proto-gen` | Random `.proto` schema + binary data generator |
| `test-utils` | Shared test framework |

## Quick Start

```bash
# Build
cargo build --workspace

# Run all tests
cargo test --workspace

# Parse a .proto file
cargo run -- file.proto
```

## License

BSD 3-Clause -- same as the original Protocol Buffers project. See [LICENSE](LICENSE).
