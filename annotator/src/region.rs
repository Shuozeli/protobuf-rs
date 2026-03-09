//! Annotated region types for schema-aware protobuf binary walking.

use std::ops::Range;

/// A byte-level annotation of a protobuf binary region.
///
/// Regions form a flat list with parent-child relationships via `children`
/// indices. This matches the protoviewer's `AnnotatedRegion` structure
/// for seamless integration.
#[derive(Debug, Clone)]
pub struct AnnotatedRegion {
    /// Byte range in the binary.
    pub byte_range: Range<usize>,
    /// What this region represents.
    pub kind: ProtoRegionKind,
    /// Human-readable label (e.g., "name: \"Alice\"").
    pub label: String,
    /// Breadcrumb path from root (e.g., ["Person", "name"]).
    pub field_path: Vec<String>,
    /// Decoded value for display.
    pub value_display: String,
    /// Indices into the parent `Vec<AnnotatedRegion>` for child regions.
    pub children: Vec<usize>,
    /// Nesting depth (0 = root message).
    pub depth: usize,
}

/// What kind of protobuf element a region represents.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtoRegionKind {
    /// Root or embedded message.
    Message { type_name: String },
    /// Tag varint (field_number + wire_type).
    Tag { field_number: u32, wire_type: u32 },
    /// Length prefix varint.
    LengthPrefix,
    /// Varint-encoded scalar (int32, int64, uint32, uint64, sint32, sint64, bool, enum).
    Varint { field_name: String },
    /// Fixed 32-bit value (fixed32, sfixed32, float).
    Fixed32 { field_name: String },
    /// Fixed 64-bit value (fixed64, sfixed64, double).
    Fixed64 { field_name: String },
    /// UTF-8 string payload.
    StringData { field_name: String },
    /// Raw bytes payload.
    BytesData { field_name: String },
    /// Packed repeated field.
    PackedRepeated {
        field_name: String,
        element_type: String,
    },
    /// Enum value (decoded to name).
    EnumValue {
        field_name: String,
        enum_name: String,
        value_name: String,
    },
    /// Field not found in schema.
    UnknownField { field_number: u32, wire_type: u32 },
}

impl ProtoRegionKind {
    /// Short identifier for this region kind.
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Message { .. } => "message",
            Self::Tag { .. } => "tag",
            Self::LengthPrefix => "length",
            Self::Varint { .. } => "varint",
            Self::Fixed32 { .. } => "fixed32",
            Self::Fixed64 { .. } => "fixed64",
            Self::StringData { .. } => "string",
            Self::BytesData { .. } => "bytes",
            Self::PackedRepeated { .. } => "packed",
            Self::EnumValue { .. } => "enum",
            Self::UnknownField { .. } => "unknown",
        }
    }

    /// RGB color for UI rendering.
    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            Self::Message { .. } => (59, 130, 246),        // blue
            Self::Tag { .. } => (100, 116, 139),           // slate
            Self::LengthPrefix => (148, 163, 184),         // light slate
            Self::Varint { .. } => (34, 197, 94),          // green
            Self::Fixed32 { .. } => (249, 115, 22),        // orange
            Self::Fixed64 { .. } => (249, 115, 22),        // orange
            Self::StringData { .. } => (218, 165, 32),     // goldenrod
            Self::BytesData { .. } => (218, 165, 32),      // goldenrod
            Self::PackedRepeated { .. } => (168, 85, 247), // purple
            Self::EnumValue { .. } => (34, 197, 94),       // green (like varint)
            Self::UnknownField { .. } => (156, 163, 175),  // gray
        }
    }
}
