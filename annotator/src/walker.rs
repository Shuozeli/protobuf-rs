//! Schema-aware protobuf binary walker.
//!
//! Recursively walks a protobuf binary using a `FileDescriptorSet` to
//! produce `AnnotatedRegion`s with field names, types, and decoded values.

use crate::region::{AnnotatedRegion, ProtoRegionKind};
use crate::wire::{self, WireType};
use protoc_rs_schema::*;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WalkError {
    #[error("message type not found: {0}")]
    MessageNotFound(String),
    #[error("wire format error: {0}")]
    WireError(#[from] crate::wire::WireError),
    #[error("max depth exceeded ({0})")]
    MaxDepthExceeded(usize),
    #[error("invalid byte slice at offset {0}: expected {1} bytes")]
    InvalidSliceLength(usize, usize),
}

/// Shared context passed through walker functions to avoid long parameter lists.
struct WalkCtx<'a> {
    data: &'a [u8],
    index: &'a SchemaIndex<'a>,
    regions: Vec<AnnotatedRegion>,
}

/// Per-field decoding context for the current field being decoded.
struct FieldCtx<'a> {
    field_info: Option<&'a FieldDescriptorProto>,
    field_name: &'a str,
    parent_path: &'a [String],
    depth: usize,
}

const MAX_DEPTH: usize = 64;

/// Walk a protobuf binary with schema information.
///
/// Returns a flat list of `AnnotatedRegion`s. The first region is the root
/// message, with `children` indices pointing to its fields.
pub fn walk_protobuf(
    data: &[u8],
    schema: &FileDescriptorSet,
    root_message: &str,
) -> Result<Vec<AnnotatedRegion>, WalkError> {
    let index = SchemaIndex::build(schema);
    let msg = index
        .find_message(root_message)
        .ok_or_else(|| WalkError::MessageNotFound(root_message.to_string()))?;

    let mut ctx = WalkCtx {
        data,
        index: &index,
        regions: Vec::new(),
    };
    let type_name = root_message.to_string();

    // Root message region (placeholder, children filled in below)
    let root_idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: 0..data.len(),
        kind: ProtoRegionKind::Message {
            type_name: type_name.clone(),
        },
        label: short_name(&type_name),
        field_path: vec![short_name(&type_name)],
        value_display: format!("{} bytes", data.len()),
        children: Vec::new(),
        depth: 0,
    });

    let children = walk_message_fields(&mut ctx, 0, data.len(), msg, &[short_name(&type_name)], 1)?;
    ctx.regions[root_idx].children = children;

    Ok(ctx.regions)
}

/// Walk the fields of a single message, returning child indices.
fn walk_message_fields(
    ctx: &mut WalkCtx,
    start: usize,
    end: usize,
    msg: &DescriptorProto,
    parent_path: &[String],
    depth: usize,
) -> Result<Vec<usize>, WalkError> {
    if depth > MAX_DEPTH {
        return Err(WalkError::MaxDepthExceeded(depth));
    }

    let field_map = build_field_map(msg);
    let mut children = Vec::new();
    let mut pos = start;

    while pos < end {
        let (tag, after_tag) = wire::decode_tag(ctx.data, pos)?;

        // Find field in schema
        let field_info = field_map.get(&tag.field_number);
        let field_name = field_info
            .map(|f| f.name.as_deref().unwrap_or("?"))
            .unwrap_or("?");

        // Tag region
        let tag_idx = ctx.regions.len();
        ctx.regions.push(AnnotatedRegion {
            byte_range: tag.byte_range.clone(),
            kind: ProtoRegionKind::Tag {
                field_number: tag.field_number,
                wire_type: tag.wire_type as u32,
            },
            label: format!(
                "tag: field {}, wire type {}",
                tag.field_number, tag.wire_type as u32
            ),
            field_path: parent_path.to_vec(),
            value_display: format!("field={} wire={}", tag.field_number, tag.wire_type as u32),
            children: Vec::new(),
            depth,
        });

        // Decode field value
        let fctx = FieldCtx {
            field_info: field_info.copied(),
            field_name,
            parent_path,
            depth,
        };
        let (value_indices, next_pos) = decode_field_value(ctx, after_tag, end, &tag, &fctx, msg)?;

        // Group region: tag + value(s)
        let mut group_children = vec![tag_idx];
        group_children.extend(&value_indices);

        let group_idx = ctx.regions.len();
        let mut path = parent_path.to_vec();
        path.push(field_name.to_string());

        let group_label = if field_info.is_some() {
            format!("{} (field {})", field_name, tag.field_number)
        } else {
            format!("unknown field {}", tag.field_number)
        };

        ctx.regions.push(AnnotatedRegion {
            byte_range: pos..next_pos,
            kind: if field_info.is_some() {
                ProtoRegionKind::Message {
                    type_name: field_name.to_string(),
                }
            } else {
                ProtoRegionKind::UnknownField {
                    field_number: tag.field_number,
                    wire_type: tag.wire_type as u32,
                }
            },
            label: group_label,
            field_path: path,
            value_display: String::new(),
            children: group_children,
            depth,
        });

        children.push(group_idx);
        pos = next_pos;
    }

    Ok(children)
}

/// Decode a single field value based on wire type and schema info.
/// Returns (value region indices, next position).
fn decode_field_value(
    ctx: &mut WalkCtx,
    offset: usize,
    msg_end: usize,
    tag: &wire::Tag,
    fctx: &FieldCtx,
    parent_msg: &DescriptorProto,
) -> Result<(Vec<usize>, usize), WalkError> {
    match tag.wire_type {
        WireType::Varint => decode_varint_field(ctx, offset, fctx),
        WireType::Fixed64 => decode_fixed_field(ctx, offset, fctx, FixedWidth::W64),
        WireType::Fixed32 => decode_fixed_field(ctx, offset, fctx, FixedWidth::W32),
        WireType::LengthDelimited => {
            decode_length_delimited_field(ctx, offset, msg_end, tag, fctx, parent_msg)
        }
        WireType::StartGroup => {
            // Skip group (deprecated)
            let range = wire::skip_field(ctx.data, offset, WireType::StartGroup)?;
            let idx = ctx.regions.len();
            ctx.regions.push(AnnotatedRegion {
                byte_range: range.clone(),
                kind: ProtoRegionKind::UnknownField {
                    field_number: tag.field_number,
                    wire_type: 3,
                },
                label: format!("{}: <group>", fctx.field_name),
                field_path: fctx.parent_path.to_vec(),
                value_display: "<group>".to_string(),
                children: Vec::new(),
                depth: fctx.depth,
            });
            Ok((vec![idx], range.end))
        }
        WireType::EndGroup => Ok((Vec::new(), offset)),
    }
}

fn decode_varint_field(
    ctx: &mut WalkCtx,
    offset: usize,
    fctx: &FieldCtx,
) -> Result<(Vec<usize>, usize), WalkError> {
    let (raw_value, range) = wire::decode_varint(ctx.data, offset)?;

    let field_type = fctx.field_info.and_then(|f| f.r#type);
    let field_name = fctx.field_name;

    // Try to decode as enum
    if let Some(FieldType::Enum) = field_type {
        if let Some(field) = fctx.field_info {
            if let Some(ref type_name) = field.type_name {
                if let Some(enum_desc) = ctx.index.find_enum(type_name) {
                    let value_name = enum_desc
                        .value
                        .iter()
                        .find(|v| v.number == Some(raw_value as i32))
                        .and_then(|v| v.name.as_deref())
                        .unwrap_or("?");
                    let enum_name = enum_desc.name.as_deref().unwrap_or("?");

                    let idx = ctx.regions.len();
                    ctx.regions.push(AnnotatedRegion {
                        byte_range: range.clone(),
                        kind: ProtoRegionKind::EnumValue {
                            field_name: field_name.to_string(),
                            enum_name: enum_name.to_string(),
                            value_name: value_name.to_string(),
                        },
                        label: format!("{}: {}", field_name, value_name),
                        field_path: fctx.parent_path.to_vec(),
                        value_display: format!("{} ({})", value_name, raw_value),
                        children: Vec::new(),
                        depth: fctx.depth,
                    });
                    return Ok((vec![idx], range.end));
                }
            }
        }
    }

    // Format based on field type
    let (display, decoded_label) = match field_type {
        Some(FieldType::Bool) => {
            let v = raw_value != 0;
            (format!("{}", v), format!("{}: {}", field_name, v))
        }
        Some(FieldType::Sint32) => {
            let v = wire::zigzag_decode(raw_value) as i32;
            (format!("{}", v), format!("{}: {}", field_name, v))
        }
        Some(FieldType::Sint64) => {
            let v = wire::zigzag_decode(raw_value);
            (format!("{}", v), format!("{}: {}", field_name, v))
        }
        Some(FieldType::Int32) => {
            let v = raw_value as i32;
            (format!("{}", v), format!("{}: {}", field_name, v))
        }
        Some(FieldType::Int64) => {
            let v = raw_value as i64;
            (format!("{}", v), format!("{}: {}", field_name, v))
        }
        _ => {
            // uint32, uint64, or unknown
            (
                format!("{}", raw_value),
                format!("{}: {}", field_name, raw_value),
            )
        }
    };

    let idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: range.clone(),
        kind: ProtoRegionKind::Varint {
            field_name: field_name.to_string(),
        },
        label: decoded_label,
        field_path: fctx.parent_path.to_vec(),
        value_display: display,
        children: Vec::new(),
        depth: fctx.depth,
    });
    Ok((vec![idx], range.end))
}

/// Whether to decode as 32-bit or 64-bit fixed-width.
enum FixedWidth {
    W32,
    W64,
}

/// Unified decoder for fixed32 and fixed64 wire types.
fn decode_fixed_field(
    ctx: &mut WalkCtx,
    offset: usize,
    fctx: &FieldCtx,
    width: FixedWidth,
) -> Result<(Vec<usize>, usize), WalkError> {
    let field_name = fctx.field_name;
    let byte_len = match width {
        FixedWidth::W32 => 4,
        FixedWidth::W64 => 8,
    };
    let end = offset + byte_len;
    if end > ctx.data.len() {
        return Err(wire::WireError::UnexpectedEof(offset).into());
    }
    let bytes = &ctx.data[offset..end];

    let display = match width {
        FixedWidth::W32 => {
            let int_bytes: [u8; 4] = bytes
                .try_into()
                .map_err(|_| WalkError::InvalidSliceLength(offset, 4))?;
            let val_u = u32::from_le_bytes(int_bytes);
            let val_f = f32::from_le_bytes(int_bytes);
            if val_f.is_finite() && val_f != 0.0 && val_f.abs() < 1e10 && val_f.abs() > 1e-6 {
                format!("{}", val_f)
            } else {
                format!("{}", val_u)
            }
        }
        FixedWidth::W64 => {
            let int_bytes: [u8; 8] = bytes
                .try_into()
                .map_err(|_| WalkError::InvalidSliceLength(offset, 8))?;
            let val_u = u64::from_le_bytes(int_bytes);
            let val_f = f64::from_le_bytes(int_bytes);
            if val_f.is_finite() && val_f != 0.0 && val_f.abs() < 1e15 && val_f.abs() > 1e-10 {
                format!("{}", val_f)
            } else {
                format!("{}", val_u)
            }
        }
    };

    let kind = match width {
        FixedWidth::W32 => ProtoRegionKind::Fixed32 {
            field_name: field_name.to_string(),
        },
        FixedWidth::W64 => ProtoRegionKind::Fixed64 {
            field_name: field_name.to_string(),
        },
    };

    let idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: offset..end,
        kind,
        label: format!("{}: {}", field_name, display),
        field_path: fctx.parent_path.to_vec(),
        value_display: display,
        children: Vec::new(),
        depth: fctx.depth,
    });
    Ok((vec![idx], end))
}

fn decode_length_delimited_field(
    ctx: &mut WalkCtx,
    offset: usize,
    _msg_end: usize,
    tag: &wire::Tag,
    fctx: &FieldCtx,
    parent_msg: &DescriptorProto,
) -> Result<(Vec<usize>, usize), WalkError> {
    let field_info = fctx.field_info;
    let field_name = fctx.field_name;
    let parent_path = fctx.parent_path;
    let depth = fctx.depth;
    let (length, len_range) = wire::decode_varint(ctx.data, offset)?;
    let data_start = len_range.end;
    let data_end = data_start + length as usize;

    if data_end > ctx.data.len() {
        return Err(wire::WireError::UnexpectedEof(data_start).into());
    }

    // Length prefix region
    let len_idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: len_range.clone(),
        kind: ProtoRegionKind::LengthPrefix,
        label: format!("length: {}", length),
        field_path: parent_path.to_vec(),
        value_display: format!("{}", length),
        children: Vec::new(),
        depth,
    });

    let field_type = field_info.and_then(|f| f.r#type);

    match field_type {
        Some(FieldType::Message) => {
            // Embedded message -- look up the nested message type
            let type_name = field_info
                .and_then(|f| f.type_name.as_deref())
                .unwrap_or("?");

            // Check if this is a map entry
            let nested_msg = find_nested_message_by_type(type_name, parent_msg, ctx.index);
            let is_map = nested_msg
                .and_then(|m| m.options.as_ref())
                .and_then(|o| o.map_entry)
                .unwrap_or(false);

            if let Some(msg_desc) = nested_msg {
                let msg_idx = ctx.regions.len();
                let mut path = parent_path.to_vec();
                path.push(field_name.to_string());

                let kind_label = if is_map {
                    format!("{}: <map entry>", field_name)
                } else {
                    format!("{}: <{}>", field_name, short_name(type_name))
                };

                ctx.regions.push(AnnotatedRegion {
                    byte_range: data_start..data_end,
                    kind: ProtoRegionKind::Message {
                        type_name: type_name.to_string(),
                    },
                    label: kind_label,
                    field_path: path.clone(),
                    value_display: format!("{} bytes", length),
                    children: Vec::new(),
                    depth,
                });

                let msg_children =
                    walk_message_fields(ctx, data_start, data_end, msg_desc, &path, depth + 1)?;
                ctx.regions[msg_idx].children = msg_children;

                Ok((vec![len_idx, msg_idx], data_end))
            } else {
                // Message type not found in schema -- treat as raw bytes
                let bytes_idx = ctx.regions.len();
                ctx.regions.push(AnnotatedRegion {
                    byte_range: data_start..data_end,
                    kind: ProtoRegionKind::BytesData {
                        field_name: field_name.to_string(),
                    },
                    label: format!("{}: <unknown message {}>", field_name, type_name),
                    field_path: parent_path.to_vec(),
                    value_display: format!("{} bytes", length),
                    children: Vec::new(),
                    depth,
                });
                Ok((vec![len_idx, bytes_idx], data_end))
            }
        }
        Some(FieldType::String) => {
            let s = String::from_utf8_lossy(&ctx.data[data_start..data_end]);
            let str_idx = ctx.regions.len();
            ctx.regions.push(AnnotatedRegion {
                byte_range: data_start..data_end,
                kind: ProtoRegionKind::StringData {
                    field_name: field_name.to_string(),
                },
                label: format!("{}: \"{}\"", field_name, truncate_string(&s, 64)),
                field_path: parent_path.to_vec(),
                value_display: format!("\"{}\"", s),
                children: Vec::new(),
                depth,
            });
            Ok((vec![len_idx, str_idx], data_end))
        }
        Some(FieldType::Bytes) => {
            let bytes_idx = ctx.regions.len();
            ctx.regions.push(AnnotatedRegion {
                byte_range: data_start..data_end,
                kind: ProtoRegionKind::BytesData {
                    field_name: field_name.to_string(),
                },
                label: format!("{}: <{} bytes>", field_name, length),
                field_path: parent_path.to_vec(),
                value_display: format!("{} bytes", length),
                children: Vec::new(),
                depth,
            });
            Ok((vec![len_idx, bytes_idx], data_end))
        }
        _ if is_packable_type(field_info) => {
            // Packed repeated field
            decode_packed_field(ctx, data_start, data_end, fctx, len_idx)
        }
        _ if field_info.is_none() => {
            // Unknown field -- try to interpret
            decode_unknown_length_delimited(
                ctx,
                data_start,
                data_end,
                tag.field_number,
                parent_path,
                depth,
                len_idx,
            )
        }
        _ => {
            // Fallback: raw bytes
            let bytes_idx = ctx.regions.len();
            ctx.regions.push(AnnotatedRegion {
                byte_range: data_start..data_end,
                kind: ProtoRegionKind::BytesData {
                    field_name: field_name.to_string(),
                },
                label: format!("{}: <{} bytes>", field_name, length),
                field_path: parent_path.to_vec(),
                value_display: format!("{} bytes", length),
                children: Vec::new(),
                depth,
            });
            Ok((vec![len_idx, bytes_idx], data_end))
        }
    }
}

/// Decode a packed repeated field.
fn decode_packed_field(
    ctx: &mut WalkCtx,
    start: usize,
    end: usize,
    fctx: &FieldCtx,
    len_idx: usize,
) -> Result<(Vec<usize>, usize), WalkError> {
    let field_type = fctx.field_info.and_then(|f| f.r#type);
    let field_name = fctx.field_name;
    let parent_path = fctx.parent_path;
    let depth = fctx.depth;
    let element_type_name = field_type
        .map(|t| format!("{:?}", t))
        .unwrap_or_else(|| "?".to_string());

    let packed_idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: start..end,
        kind: ProtoRegionKind::PackedRepeated {
            field_name: field_name.to_string(),
            element_type: element_type_name.clone(),
        },
        label: format!("{}: packed {}", field_name, element_type_name),
        field_path: parent_path.to_vec(),
        value_display: String::new(),
        children: Vec::new(),
        depth,
    });

    // Decode individual elements
    let mut element_indices = Vec::new();
    let mut pos = start;
    while pos < end {
        match field_type {
            Some(FieldType::Fixed32 | FieldType::Sfixed32 | FieldType::Float) => {
                let elem_end = pos + 4;
                if elem_end > end {
                    break;
                }
                let idx = ctx.regions.len();
                let bytes: [u8; 4] = ctx.data[pos..elem_end]
                    .try_into()
                    .map_err(|_| WalkError::InvalidSliceLength(pos, 4))?;
                let val = u32::from_le_bytes(bytes);
                ctx.regions.push(AnnotatedRegion {
                    byte_range: pos..elem_end,
                    kind: ProtoRegionKind::Fixed32 {
                        field_name: field_name.to_string(),
                    },
                    label: format!("{}", val),
                    field_path: parent_path.to_vec(),
                    value_display: format!("{}", val),
                    children: Vec::new(),
                    depth: depth + 1,
                });
                element_indices.push(idx);
                pos = elem_end;
            }
            Some(FieldType::Fixed64 | FieldType::Sfixed64 | FieldType::Double) => {
                let elem_end = pos + 8;
                if elem_end > end {
                    break;
                }
                let idx = ctx.regions.len();
                let bytes: [u8; 8] = ctx.data[pos..elem_end]
                    .try_into()
                    .map_err(|_| WalkError::InvalidSliceLength(pos, 8))?;
                let val = u64::from_le_bytes(bytes);
                ctx.regions.push(AnnotatedRegion {
                    byte_range: pos..elem_end,
                    kind: ProtoRegionKind::Fixed64 {
                        field_name: field_name.to_string(),
                    },
                    label: format!("{}", val),
                    field_path: parent_path.to_vec(),
                    value_display: format!("{}", val),
                    children: Vec::new(),
                    depth: depth + 1,
                });
                element_indices.push(idx);
                pos = elem_end;
            }
            _ => {
                // Varint-encoded elements
                let (val, range) = wire::decode_varint(ctx.data, pos)?;
                let idx = ctx.regions.len();
                ctx.regions.push(AnnotatedRegion {
                    byte_range: range.clone(),
                    kind: ProtoRegionKind::Varint {
                        field_name: field_name.to_string(),
                    },
                    label: format!("{}", val),
                    field_path: parent_path.to_vec(),
                    value_display: format!("{}", val),
                    children: Vec::new(),
                    depth: depth + 1,
                });
                element_indices.push(idx);
                pos = range.end;
            }
        }
    }

    let values_str = format!("[{} elements]", element_indices.len());
    ctx.regions[packed_idx].children = element_indices;
    ctx.regions[packed_idx].value_display = values_str;

    Ok((vec![len_idx, packed_idx], end))
}

/// Decode an unknown length-delimited field (no schema info).
fn decode_unknown_length_delimited(
    ctx: &mut WalkCtx,
    start: usize,
    end: usize,
    field_number: u32,
    parent_path: &[String],
    depth: usize,
    len_idx: usize,
) -> Result<(Vec<usize>, usize), WalkError> {
    let payload = &ctx.data[start..end];

    // Try UTF-8 string
    if let Ok(s) = std::str::from_utf8(payload) {
        if s.chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\r' || c == '\t')
        {
            let idx = ctx.regions.len();
            ctx.regions.push(AnnotatedRegion {
                byte_range: start..end,
                kind: ProtoRegionKind::StringData {
                    field_name: format!("field_{}", field_number),
                },
                label: format!("field_{}: \"{}\"", field_number, truncate_string(s, 64)),
                field_path: parent_path.to_vec(),
                value_display: format!("\"{}\"", s),
                children: Vec::new(),
                depth,
            });
            return Ok((vec![len_idx, idx], end));
        }
    }

    // Fallback: raw bytes
    let idx = ctx.regions.len();
    ctx.regions.push(AnnotatedRegion {
        byte_range: start..end,
        kind: ProtoRegionKind::BytesData {
            field_name: format!("field_{}", field_number),
        },
        label: format!("field_{}: <{} bytes>", field_number, end - start),
        field_path: parent_path.to_vec(),
        value_display: format!("{} bytes", end - start),
        children: Vec::new(),
        depth,
    });
    Ok((vec![len_idx, idx], end))
}

// ---------------------------------------------------------------------------
// Schema index
// ---------------------------------------------------------------------------

/// Pre-built index for fast message/enum lookup by fully-qualified name.
struct SchemaIndex<'a> {
    messages: HashMap<String, &'a DescriptorProto>,
    enums: HashMap<String, &'a EnumDescriptorProto>,
}

impl<'a> SchemaIndex<'a> {
    fn build(schema: &'a FileDescriptorSet) -> Self {
        let mut messages = HashMap::new();
        let mut enums = HashMap::new();

        for file in &schema.file {
            let pkg = file.package.as_deref().unwrap_or("");
            let prefix = if pkg.is_empty() {
                ".".to_string()
            } else {
                format!(".{}.", pkg)
            };
            Self::index_messages(&prefix, &file.message_type, &mut messages, &mut enums);
            Self::index_enums(&prefix, &file.enum_type, &mut enums);
        }

        Self { messages, enums }
    }

    fn index_messages(
        prefix: &str,
        msgs: &'a [DescriptorProto],
        messages: &mut HashMap<String, &'a DescriptorProto>,
        enums: &mut HashMap<String, &'a EnumDescriptorProto>,
    ) {
        for msg in msgs {
            let name = msg.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            messages.insert(fqn.clone(), msg);
            let nested_prefix = format!("{}.", fqn);
            Self::index_messages(&nested_prefix, &msg.nested_type, messages, enums);
            Self::index_enums(&nested_prefix, &msg.enum_type, enums);
        }
    }

    fn index_enums(
        prefix: &str,
        enum_types: &'a [EnumDescriptorProto],
        enums: &mut HashMap<String, &'a EnumDescriptorProto>,
    ) {
        for e in enum_types {
            let name = e.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            enums.insert(fqn, e);
        }
    }

    fn find_message(&self, name: &str) -> Option<&'a DescriptorProto> {
        // Try exact match first, then with leading dot
        self.messages
            .get(name)
            .or_else(|| {
                let with_dot = if name.starts_with('.') {
                    name.to_string()
                } else {
                    format!(".{}", name)
                };
                self.messages.get(&with_dot)
            })
            .copied()
    }

    fn find_enum(&self, name: &str) -> Option<&'a EnumDescriptorProto> {
        self.enums
            .get(name)
            .or_else(|| {
                let with_dot = if name.starts_with('.') {
                    name.to_string()
                } else {
                    format!(".{}", name)
                };
                self.enums.get(&with_dot)
            })
            .copied()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a map from field number to field descriptor for a message.
fn build_field_map(msg: &DescriptorProto) -> HashMap<u32, &FieldDescriptorProto> {
    let mut map = HashMap::new();
    for field in &msg.field {
        if let Some(num) = field.number {
            map.insert(num as u32, field);
        }
    }
    map
}

/// Find a nested message by its fully-qualified type name.
fn find_nested_message_by_type<'a>(
    type_name: &str,
    parent_msg: &'a DescriptorProto,
    index: &'a SchemaIndex<'a>,
) -> Option<&'a DescriptorProto> {
    // First check nested types of parent
    let short = type_name.rsplit('.').next().unwrap_or(type_name);
    for nested in &parent_msg.nested_type {
        if nested.name.as_deref() == Some(short) {
            return Some(nested);
        }
    }
    // Fall back to global index
    index.find_message(type_name)
}

/// Check if a field type can be packed.
fn is_packable_type(field_info: Option<&FieldDescriptorProto>) -> bool {
    let field = match field_info {
        Some(f) => f,
        None => return false,
    };
    // Must be repeated
    if field.label != Some(FieldLabel::Repeated) {
        return false;
    }
    // Must be a packable scalar type
    matches!(
        field.r#type,
        Some(
            FieldType::Int32
                | FieldType::Int64
                | FieldType::Uint32
                | FieldType::Uint64
                | FieldType::Sint32
                | FieldType::Sint64
                | FieldType::Bool
                | FieldType::Enum
                | FieldType::Fixed32
                | FieldType::Fixed64
                | FieldType::Sfixed32
                | FieldType::Sfixed64
                | FieldType::Float
                | FieldType::Double
        )
    )
}

/// Extract short name from fully-qualified name (e.g., ".pkg.Foo" -> "Foo").
fn short_name(fqn: &str) -> String {
    fqn.rsplit('.').next().unwrap_or(fqn).to_string()
}

/// Truncate a string for display.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_simple_schema() -> FileDescriptorSet {
        FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                package: Some("test".to_string()),
                syntax: Some("proto3".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("Person".to_string()),
                    field: vec![
                        FieldDescriptorProto {
                            name: Some("name".to_string()),
                            number: Some(1),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::String),
                            ..Default::default()
                        },
                        FieldDescriptorProto {
                            name: Some("id".to_string()),
                            number: Some(2),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::Int32),
                            ..Default::default()
                        },
                        FieldDescriptorProto {
                            name: Some("email".to_string()),
                            number: Some(3),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::String),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    /// Encode a simple Person { name: "Alice", id: 42, email: "a@b.c" }
    fn encode_test_person() -> Vec<u8> {
        let mut buf = Vec::new();
        // field 1 (name), wire type 2 (LEN): tag = 0x0A
        buf.push(0x0A);
        buf.push(5); // length
        buf.extend_from_slice(b"Alice");
        // field 2 (id), wire type 0 (varint): tag = 0x10
        buf.push(0x10);
        buf.push(42);
        // field 3 (email), wire type 2 (LEN): tag = 0x1A
        buf.push(0x1A);
        buf.push(5);
        buf.extend_from_slice(b"a@b.c");
        buf
    }

    #[test]
    fn walk_simple_message() {
        let schema = make_simple_schema();
        let data = encode_test_person();
        let regions = walk_protobuf(&data, &schema, ".test.Person").unwrap();

        // Root region should be the message
        assert!(!regions.is_empty());
        assert!(matches!(
            &regions[0].kind,
            ProtoRegionKind::Message { type_name } if type_name == ".test.Person"
        ));
        assert_eq!(regions[0].byte_range, 0..data.len());
        // Should have 3 children (one group per field)
        assert_eq!(regions[0].children.len(), 3);
    }

    #[test]
    fn walk_decodes_string_field() {
        let schema = make_simple_schema();
        let data = encode_test_person();
        let regions = walk_protobuf(&data, &schema, ".test.Person").unwrap();

        // Find the string data region for "name"
        let string_region = regions.iter().find(|r| {
            matches!(
                &r.kind,
                ProtoRegionKind::StringData { field_name } if field_name == "name"
            )
        });
        assert!(string_region.is_some());
        let sr = string_region.unwrap();
        assert!(sr.value_display.contains("Alice"));
    }

    #[test]
    fn walk_decodes_varint_field() {
        let schema = make_simple_schema();
        let data = encode_test_person();
        let regions = walk_protobuf(&data, &schema, ".test.Person").unwrap();

        // Find the varint region for "id"
        let varint_region = regions.iter().find(|r| {
            matches!(
                &r.kind,
                ProtoRegionKind::Varint { field_name } if field_name == "id"
            )
        });
        assert!(varint_region.is_some());
        assert!(varint_region.unwrap().value_display.contains("42"));
    }

    #[test]
    fn walk_handles_unknown_fields() {
        let schema = make_simple_schema();
        // Encode field 99 (not in schema) as varint
        let mut data = Vec::new();
        data.push((99 << 3) | 0); // field 99, varint
                                  // 99 << 3 = 792, | 0 = 792. But that's > 127, need multi-byte tag
                                  // Let me just use a small field number not in schema
        data.clear();
        data.push((10 << 3) | 0); // field 10, varint -- tag = 0x50
        data.push(7);

        let regions = walk_protobuf(&data, &schema, ".test.Person").unwrap();
        assert!(!regions.is_empty());
    }

    #[test]
    fn walk_nested_message() {
        let schema = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                package: Some("test".to_string()),
                syntax: Some("proto3".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("Outer".to_string()),
                    field: vec![FieldDescriptorProto {
                        name: Some("inner".to_string()),
                        number: Some(1),
                        label: Some(FieldLabel::Optional),
                        r#type: Some(FieldType::Message),
                        type_name: Some(".test.Outer.Inner".to_string()),
                        ..Default::default()
                    }],
                    nested_type: vec![DescriptorProto {
                        name: Some("Inner".to_string()),
                        field: vec![FieldDescriptorProto {
                            name: Some("value".to_string()),
                            number: Some(1),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::Int32),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        // Outer { inner: Inner { value: 123 } }
        let mut inner_buf = Vec::new();
        inner_buf.push(0x08); // field 1, varint
        inner_buf.push(123);

        let mut data = Vec::new();
        data.push(0x0A); // field 1, LEN
        data.push(inner_buf.len() as u8);
        data.extend_from_slice(&inner_buf);

        let regions = walk_protobuf(&data, &schema, ".test.Outer").unwrap();

        // Should find the nested varint with value 123
        let inner_varint = regions.iter().find(|r| {
            matches!(
                &r.kind,
                ProtoRegionKind::Varint { field_name } if field_name == "value"
            )
        });
        assert!(inner_varint.is_some());
        assert!(inner_varint.unwrap().value_display.contains("123"));
    }

    #[test]
    fn walk_enum_field() {
        let schema = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                package: Some("test".to_string()),
                syntax: Some("proto3".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("Msg".to_string()),
                    field: vec![FieldDescriptorProto {
                        name: Some("status".to_string()),
                        number: Some(1),
                        label: Some(FieldLabel::Optional),
                        r#type: Some(FieldType::Enum),
                        type_name: Some(".test.Status".to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                enum_type: vec![EnumDescriptorProto {
                    name: Some("Status".to_string()),
                    value: vec![
                        EnumValueDescriptorProto {
                            name: Some("UNKNOWN".to_string()),
                            number: Some(0),
                            ..Default::default()
                        },
                        EnumValueDescriptorProto {
                            name: Some("ACTIVE".to_string()),
                            number: Some(1),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        // Msg { status: ACTIVE (1) }
        let data = vec![0x08, 0x01]; // field 1, varint, value 1

        let regions = walk_protobuf(&data, &schema, ".test.Msg").unwrap();

        let enum_region = regions.iter().find(|r| {
            matches!(
                &r.kind,
                ProtoRegionKind::EnumValue { value_name, .. } if value_name == "ACTIVE"
            )
        });
        assert!(enum_region.is_some());
    }

    #[test]
    fn walk_packed_repeated() {
        let schema = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                package: Some("test".to_string()),
                syntax: Some("proto3".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("Msg".to_string()),
                    field: vec![FieldDescriptorProto {
                        name: Some("values".to_string()),
                        number: Some(1),
                        label: Some(FieldLabel::Repeated),
                        r#type: Some(FieldType::Int32),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        // Packed repeated: field 1, LEN, [1, 2, 3]
        let data = vec![0x0A, 0x03, 0x01, 0x02, 0x03];

        let regions = walk_protobuf(&data, &schema, ".test.Msg").unwrap();

        let packed = regions.iter().find(|r| {
            matches!(
                &r.kind,
                ProtoRegionKind::PackedRepeated { field_name, .. } if field_name == "values"
            )
        });
        assert!(packed.is_some());
        assert_eq!(packed.unwrap().children.len(), 3); // 3 elements
    }

    #[test]
    fn walk_empty_message() {
        let schema = make_simple_schema();
        let data = vec![];
        let regions = walk_protobuf(&data, &schema, ".test.Person").unwrap();
        assert_eq!(regions.len(), 1); // just the root message
        assert!(regions[0].children.is_empty());
    }

    #[test]
    fn walk_message_not_found() {
        let schema = make_simple_schema();
        let data = vec![];
        let err = walk_protobuf(&data, &schema, ".test.NotExist").unwrap_err();
        assert!(matches!(err, WalkError::MessageNotFound(_)));
    }
}
