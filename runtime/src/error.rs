//! Error types for protobuf encoding and decoding.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use thiserror::Error;

/// Errors that can occur during protobuf decoding.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DecodeError {
    #[error("unexpected end of input")]
    UnexpectedEof,

    #[error("varint exceeds 64 bits")]
    VarintTooLong,

    #[error("unknown wire type: {0}")]
    UnknownWireType(u32),

    #[error("field number must be >= 1")]
    InvalidFieldNumber,

    #[error("wire type mismatch: expected {expected:?}, got {actual:?}")]
    WireTypeMismatch {
        expected: crate::wire::WireType,
        actual: crate::wire::WireType,
    },

    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,

    #[error("invalid UTF-8 in string field")]
    InvalidUtf8,

    #[error("message size {size} exceeds limit {limit}")]
    MessageTooLarge { size: usize, limit: usize },

    #[error("{0}")]
    Custom(String),
}
