//! Protobuf wire format primitives with byte-range tracking.

use std::ops::Range;
use thiserror::Error;

// Re-export WireType from the schema crate (single source of truth).
pub use protoc_rs_schema::WireType;

#[derive(Debug, Error)]
pub enum WireError {
    #[error("unexpected end of data at offset {0}")]
    UnexpectedEof(usize),
    #[error("varint too long at offset {0}")]
    VarintTooLong(usize),
    #[error("unknown wire type {wire_type} at offset {offset}")]
    UnknownWireType { wire_type: u32, offset: usize },
}

/// Decoded tag: field number + wire type + byte range of the tag varint.
#[derive(Debug, Clone)]
pub struct Tag {
    pub field_number: u32,
    pub wire_type: WireType,
    pub byte_range: Range<usize>,
}

/// Decode a varint at the given offset. Returns (value, byte_range).
pub fn decode_varint(buf: &[u8], offset: usize) -> Result<(u64, Range<usize>), WireError> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    let start = offset;
    let mut pos = offset;

    loop {
        if pos >= buf.len() {
            return Err(WireError::UnexpectedEof(pos));
        }
        let byte = buf[pos];
        value |= ((byte & 0x7F) as u64) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            return Ok((value, start..pos));
        }
        shift += 7;
        if shift >= 64 {
            return Err(WireError::VarintTooLong(start));
        }
    }
}

/// Decode a tag at the given offset.
pub fn decode_tag(buf: &[u8], offset: usize) -> Result<(Tag, usize), WireError> {
    let (raw, range) = decode_varint(buf, offset)?;
    let wire_type_val = (raw & 0x07) as u32;
    let field_number = (raw >> 3) as u32;
    let wire_type = WireType::from_u32(wire_type_val).ok_or(WireError::UnknownWireType {
        wire_type: wire_type_val,
        offset,
    })?;
    Ok((
        Tag {
            field_number,
            wire_type,
            byte_range: range.clone(),
        },
        range.end,
    ))
}

/// Skip a field value based on wire type, returning the byte range consumed.
pub fn skip_field(
    buf: &[u8],
    offset: usize,
    wire_type: WireType,
) -> Result<Range<usize>, WireError> {
    match wire_type {
        WireType::Varint => {
            let (_, range) = decode_varint(buf, offset)?;
            Ok(range)
        }
        WireType::Fixed64 => {
            let end = offset + 8;
            if end > buf.len() {
                return Err(WireError::UnexpectedEof(offset));
            }
            Ok(offset..end)
        }
        WireType::Fixed32 => {
            let end = offset + 4;
            if end > buf.len() {
                return Err(WireError::UnexpectedEof(offset));
            }
            Ok(offset..end)
        }
        WireType::LengthDelimited => {
            let (len, len_range) = decode_varint(buf, offset)?;
            let data_end = len_range.end + len as usize;
            if data_end > buf.len() {
                return Err(WireError::UnexpectedEof(len_range.end));
            }
            Ok(offset..data_end)
        }
        WireType::StartGroup => {
            // Skip until matching EndGroup
            let mut pos = offset;
            loop {
                let (tag, next_pos) = decode_tag(buf, pos)?;
                if tag.wire_type == WireType::EndGroup {
                    return Ok(offset..next_pos);
                }
                let field_range = skip_field(buf, next_pos, tag.wire_type)?;
                pos = field_range.end;
            }
        }
        WireType::EndGroup => Ok(offset..offset),
    }
}

/// Decode a zigzag-encoded signed integer.
pub fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_varint_single_byte() {
        let buf = [0x08]; // varint 8
        let (val, range) = decode_varint(&buf, 0).unwrap();
        assert_eq!(val, 8);
        assert_eq!(range, 0..1);
    }

    #[test]
    fn decode_varint_multi_byte() {
        let buf = [0xAC, 0x02]; // 300
        let (val, range) = decode_varint(&buf, 0).unwrap();
        assert_eq!(val, 300);
        assert_eq!(range, 0..2);
    }

    #[test]
    fn decode_varint_at_offset() {
        let buf = [0x00, 0x00, 0x96, 0x01]; // varint 150 at offset 2
        let (val, range) = decode_varint(&buf, 2).unwrap();
        assert_eq!(val, 150);
        assert_eq!(range, 2..4);
    }

    #[test]
    fn decode_varint_eof() {
        let buf = [0x80]; // continuation bit set but no next byte
        assert!(decode_varint(&buf, 0).is_err());
    }

    #[test]
    fn decode_tag_field1_varint() {
        // field 1, wire type 0 (varint) -> tag byte = (1 << 3) | 0 = 0x08
        let buf = [0x08];
        let (tag, next) = decode_tag(&buf, 0).unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(tag.wire_type, WireType::Varint);
        assert_eq!(next, 1);
    }

    #[test]
    fn decode_tag_field2_len() {
        // field 2, wire type 2 (length-delimited) -> (2 << 3) | 2 = 0x12
        let buf = [0x12];
        let (tag, _) = decode_tag(&buf, 0).unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(tag.wire_type, WireType::LengthDelimited);
    }

    #[test]
    fn skip_varint_field() {
        let buf = [0xAC, 0x02]; // varint 300
        let range = skip_field(&buf, 0, WireType::Varint).unwrap();
        assert_eq!(range, 0..2);
    }

    #[test]
    fn skip_fixed64_field() {
        let buf = [0; 8];
        let range = skip_field(&buf, 0, WireType::Fixed64).unwrap();
        assert_eq!(range, 0..8);
    }

    #[test]
    fn skip_length_delimited_field() {
        // length = 3, then 3 bytes of data
        let buf = [0x03, 0x41, 0x42, 0x43];
        let range = skip_field(&buf, 0, WireType::LengthDelimited).unwrap();
        assert_eq!(range, 0..4);
    }

    #[test]
    fn zigzag_decode_values() {
        assert_eq!(zigzag_decode(0), 0);
        assert_eq!(zigzag_decode(1), -1);
        assert_eq!(zigzag_decode(2), 1);
        assert_eq!(zigzag_decode(3), -2);
        assert_eq!(zigzag_decode(4), 2);
    }
}
