//! Pre-generated runtime-backed well-known protobuf types.
//!
//! This crate provides Rust implementations of the standard Google Protocol
//! Buffer well-known types, backed by `protoc-rs-runtime`. When your generated
//! code references types like `google.protobuf.Timestamp`, it can depend on
//! this crate instead of regenerating the WKT code.
//!
//! # Types included
//!
//! ## google.protobuf
//! - `Any`, `Api`, `Duration`, `Empty`, `FieldMask`
//! - `SourceContext`, `Struct`, `Value`, `ListValue`
//! - `Timestamp`, `Type`, `Field`, `Enum`, `EnumValue`, `Option`
//! - Wrapper types: `DoubleValue`, `FloatValue`, `Int64Value`, etc.
//!
//! ## google.rpc
//! - `Status`, `ErrorInfo`, `RetryInfo`, `DebugInfo`, `QuotaFailure`
//! - `PreconditionFailure`, `BadRequest`, `RequestInfo`, `ResourceInfo`
//! - `Help`, `LocalizedMessage`

#![allow(warnings)]

#[allow(clippy::all)]
pub mod google_protobuf;
#[allow(clippy::all)]
pub mod google_rpc;

/// Re-export google.protobuf types under a path matching the proto package.
pub mod google {
    pub mod protobuf {
        pub use crate::google_protobuf::*;
    }
    pub mod rpc {
        pub use crate::google_rpc::*;
    }
}
