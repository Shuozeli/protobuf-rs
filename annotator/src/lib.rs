//! Schema-aware protobuf binary walker with byte-level annotation.
//!
//! Given a protobuf binary and a `FileDescriptorSet`, produces a flat list of
//! `AnnotatedRegion`s that map every byte to its schema meaning (field name,
//! type, decoded value).

pub mod region;
pub mod walker;
pub mod wire;

pub use region::{AnnotatedRegion, ProtoRegionKind};
pub use walker::{walk_protobuf, WalkError};
pub use wire::WireType;
