//! Protobuf wire format encoding and decoding.
//!
//! This module provides the low-level primitives for reading and writing
//! protobuf's binary wire format: varints, fixed-width integers, tags,
//! zigzag encoding, and field skipping.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::error::DecodeError;

/// Protobuf wire types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum WireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    StartGroup = 3,
    EndGroup = 4,
    Fixed32 = 5,
}

impl WireType {
    /// Convert from a raw u32 wire type value.
    pub fn from_u32(val: u32) -> Option<WireType> {
        match val {
            0 => Some(WireType::Varint),
            1 => Some(WireType::Fixed64),
            2 => Some(WireType::LengthDelimited),
            3 => Some(WireType::StartGroup),
            4 => Some(WireType::EndGroup),
            5 => Some(WireType::Fixed32),
            _ => None,
        }
    }
}

/// A decoded protobuf tag (field number + wire type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub field_number: u32,
    pub wire_type: WireType,
}

impl Tag {
    /// Encode this tag as a u32 value.
    pub fn encode_value(&self) -> u32 {
        (self.field_number << 3) | (self.wire_type as u32)
    }
}

// ---------------------------------------------------------------------------
// Varint encoding
// ---------------------------------------------------------------------------

/// Encode a varint into the buffer. Returns the number of bytes written.
///
/// Uses an unbounded loop (no iteration counter) so LLVM can optimize
/// without loop-counter overhead (~40% faster than bounded `for _ in 0..10`).
pub fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        if value < 0x80 {
            buf.push(value as u8);
            return;
        }
        buf.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
}

/// Return the number of bytes needed to encode a varint.
pub fn varint_len(value: u64) -> usize {
    // Each byte encodes 7 bits. We need ceil((bits_needed) / 7).
    // Special case: 0 still needs 1 byte.
    if value == 0 {
        return 1;
    }
    let bits = 64 - value.leading_zeros() as usize;
    (bits + 6) / 7
}

/// Decode a varint from a byte slice. Returns (value, bytes_consumed).
///
/// Uses a fast path for single-byte varints (field numbers 1-15 and small
/// values), which covers the vast majority of real-world protobuf data.
pub fn decode_varint(buf: &[u8]) -> Result<(u64, usize), DecodeError> {
    if buf.is_empty() {
        return Err(DecodeError::UnexpectedEof);
    }

    // Fast path: single-byte varint (values 0-127).
    // Covers field numbers 1-15 and most small integer values.
    let first = buf[0];
    if first < 0x80 {
        return Ok((first as u64, 1));
    }

    decode_varint_slow(buf)
}

/// Slow path for multi-byte varints.
#[cold]
fn decode_varint_slow(buf: &[u8]) -> Result<(u64, usize), DecodeError> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;

    for (i, &byte) in buf.iter().enumerate() {
        if shift >= 64 {
            return Err(DecodeError::VarintTooLong);
        }
        value |= ((byte & 0x7F) as u64) << shift;
        if byte < 0x80 {
            return Ok((value, i + 1));
        }
        shift += 7;
    }

    Err(DecodeError::UnexpectedEof)
}

// ---------------------------------------------------------------------------
// Tag encoding/decoding
// ---------------------------------------------------------------------------

/// Encode a tag (field number + wire type) into the buffer.
pub fn encode_tag(field_number: u32, wire_type: WireType, buf: &mut Vec<u8>) {
    let tag_value = ((field_number as u64) << 3) | (wire_type as u64);
    encode_varint(tag_value, buf);
}

/// Decode a tag from the buffer. Returns (Tag, bytes_consumed).
pub fn decode_tag(buf: &[u8]) -> Result<(Tag, usize), DecodeError> {
    let (raw, consumed) = decode_varint(buf)?;
    let wire_type_val = (raw & 0x07) as u32;
    let field_number = (raw >> 3) as u32;

    if field_number == 0 {
        return Err(DecodeError::InvalidFieldNumber);
    }

    let wire_type =
        WireType::from_u32(wire_type_val).ok_or(DecodeError::UnknownWireType(wire_type_val))?;

    Ok((Tag { field_number, wire_type }, consumed))
}

// ---------------------------------------------------------------------------
// Fixed-width encoding/decoding
// ---------------------------------------------------------------------------

pub fn encode_fixed32(value: u32, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub fn decode_fixed32(buf: &[u8]) -> Result<(u32, usize), DecodeError> {
    if buf.len() < 4 {
        return Err(DecodeError::UnexpectedEof);
    }
    let value = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    Ok((value, 4))
}

pub fn encode_fixed64(value: u64, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub fn decode_fixed64(buf: &[u8]) -> Result<(u64, usize), DecodeError> {
    if buf.len() < 8 {
        return Err(DecodeError::UnexpectedEof);
    }
    let value = u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]);
    Ok((value, 8))
}

// ---------------------------------------------------------------------------
// Zigzag encoding (for sint32/sint64)
// ---------------------------------------------------------------------------

/// Zigzag-encode a signed 32-bit integer.
pub fn zigzag_encode_32(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

/// Zigzag-decode a 32-bit value to a signed integer.
pub fn zigzag_decode_32(v: u32) -> i32 {
    ((v >> 1) as i32) ^ (-((v & 1) as i32))
}

/// Zigzag-encode a signed 64-bit integer.
pub fn zigzag_encode_64(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Zigzag-decode a 64-bit value to a signed integer.
pub fn zigzag_decode_64(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

// ---------------------------------------------------------------------------
// Field skipping
// ---------------------------------------------------------------------------

/// Skip a field value based on wire type. Returns bytes consumed.
///
/// `recursion_limit` prevents stack overflow on malicious deeply-nested groups.
pub fn skip_field(
    buf: &[u8],
    wire_type: WireType,
    recursion_limit: u32,
) -> Result<usize, DecodeError> {
    match wire_type {
        WireType::Varint => {
            let (_, consumed) = decode_varint(buf)?;
            Ok(consumed)
        }
        WireType::Fixed64 => {
            if buf.len() < 8 {
                return Err(DecodeError::UnexpectedEof);
            }
            Ok(8)
        }
        WireType::Fixed32 => {
            if buf.len() < 4 {
                return Err(DecodeError::UnexpectedEof);
            }
            Ok(4)
        }
        WireType::LengthDelimited => {
            let (len, header_consumed) = decode_varint(buf)?;
            let total = header_consumed + len as usize;
            if total > buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            Ok(total)
        }
        WireType::StartGroup => {
            if recursion_limit == 0 {
                return Err(DecodeError::RecursionLimitExceeded);
            }
            let mut pos = 0;
            loop {
                let (tag, tag_consumed) = decode_tag(&buf[pos..])?;
                pos += tag_consumed;
                if tag.wire_type == WireType::EndGroup {
                    return Ok(pos);
                }
                let field_consumed =
                    skip_field(&buf[pos..], tag.wire_type, recursion_limit - 1)?;
                pos += field_consumed;
            }
        }
        WireType::EndGroup => Ok(0),
    }
}

// ---------------------------------------------------------------------------
// Packed repeated field helpers
// ---------------------------------------------------------------------------

/// Encode a packed repeated varint field (tag + length prefix + values).
pub fn encode_packed_varints(field_number: u32, values: &[u64], buf: &mut Vec<u8>) {
    if values.is_empty() {
        return;
    }
    encode_tag(field_number, WireType::LengthDelimited, buf);
    let content_len: usize = values.iter().map(|v| varint_len(*v)).sum();
    encode_varint(content_len as u64, buf);
    for v in values {
        encode_varint(*v, buf);
    }
}

/// Encode a packed repeated fixed32 field.
pub fn encode_packed_fixed32(field_number: u32, values: &[u32], buf: &mut Vec<u8>) {
    if values.is_empty() {
        return;
    }
    encode_tag(field_number, WireType::LengthDelimited, buf);
    encode_varint((values.len() * 4) as u64, buf);
    for v in values {
        encode_fixed32(*v, buf);
    }
}

/// Encode a packed repeated fixed64 field.
pub fn encode_packed_fixed64(field_number: u32, values: &[u64], buf: &mut Vec<u8>) {
    if values.is_empty() {
        return;
    }
    encode_tag(field_number, WireType::LengthDelimited, buf);
    encode_varint((values.len() * 8) as u64, buf);
    for v in values {
        encode_fixed64(*v, buf);
    }
}

/// Compute the encoded size of a packed varint field (tag + length prefix + values).
pub fn packed_varints_size(field_number: u32, values: &[u64]) -> usize {
    if values.is_empty() {
        return 0;
    }
    let tag_size = varint_len(((field_number as u64) << 3) | WireType::LengthDelimited as u64);
    let content_len: usize = values.iter().map(|v| varint_len(*v)).sum();
    let len_prefix_size = varint_len(content_len as u64);
    tag_size + len_prefix_size + content_len
}

/// Compute the encoded size of a packed fixed32 field.
pub fn packed_fixed32_size(field_number: u32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let tag_size = varint_len(((field_number as u64) << 3) | WireType::LengthDelimited as u64);
    let content_len = count * 4;
    let len_prefix_size = varint_len(content_len as u64);
    tag_size + len_prefix_size + content_len
}

/// Compute the encoded size of a packed fixed64 field.
pub fn packed_fixed64_size(field_number: u32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let tag_size = varint_len(((field_number as u64) << 3) | WireType::LengthDelimited as u64);
    let content_len = count * 8;
    let len_prefix_size = varint_len(content_len as u64);
    tag_size + len_prefix_size + content_len
}

/// Decode packed varints from a length-delimited buffer.
/// `buf` starts after the tag. Returns (values, total_bytes_consumed).
pub fn decode_packed_varints(buf: &[u8]) -> Result<(Vec<u64>, usize), DecodeError> {
    let (len, header) = decode_varint(buf)?;
    let end = header + len as usize;
    if end > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let mut values = Vec::new();
    let mut pos = header;
    while pos < end {
        let (v, consumed) = decode_varint(&buf[pos..])?;
        values.push(v);
        pos += consumed;
    }
    Ok((values, end))
}

/// Decode packed fixed32 values from a length-delimited buffer.
pub fn decode_packed_fixed32(buf: &[u8]) -> Result<(Vec<u32>, usize), DecodeError> {
    let (len, header) = decode_varint(buf)?;
    let end = header + len as usize;
    if end > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let content = &buf[header..end];
    if content.len() % 4 != 0 {
        return Err(DecodeError::Custom("packed fixed32 length not multiple of 4".into()));
    }
    let mut values = Vec::with_capacity(content.len() / 4);
    let mut pos = 0;
    while pos < content.len() {
        let v = u32::from_le_bytes([content[pos], content[pos + 1], content[pos + 2], content[pos + 3]]);
        values.push(v);
        pos += 4;
    }
    Ok((values, end))
}

/// Decode packed fixed64 values from a length-delimited buffer.
pub fn decode_packed_fixed64(buf: &[u8]) -> Result<(Vec<u64>, usize), DecodeError> {
    let (len, header) = decode_varint(buf)?;
    let end = header + len as usize;
    if end > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let content = &buf[header..end];
    if content.len() % 8 != 0 {
        return Err(DecodeError::Custom("packed fixed64 length not multiple of 8".into()));
    }
    let mut values = Vec::with_capacity(content.len() / 8);
    let mut pos = 0;
    while pos < content.len() {
        let v = u64::from_le_bytes([
            content[pos], content[pos + 1], content[pos + 2], content[pos + 3],
            content[pos + 4], content[pos + 5], content[pos + 6], content[pos + 7],
        ]);
        values.push(v);
        pos += 8;
    }
    Ok((values, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Varint round-trip ---------------------------------------------------

    #[test]
    fn varint_round_trip() {
        let cases: &[u64] = &[0, 1, 127, 128, 300, 16384, u32::MAX as u64, u64::MAX];
        for &val in cases {
            let mut buf = Vec::new();
            encode_varint(val, &mut buf);
            assert_eq!(buf.len(), varint_len(val), "varint_len mismatch for {val}");
            let (decoded, consumed) = decode_varint(&buf).unwrap();
            assert_eq!(decoded, val);
            assert_eq!(consumed, buf.len());
        }
    }

    #[test]
    fn varint_single_byte_fast_path() {
        for val in 0..128u8 {
            let buf = [val];
            let (decoded, consumed) = decode_varint(&buf).unwrap();
            assert_eq!(decoded, val as u64);
            assert_eq!(consumed, 1);
        }
    }

    #[test]
    fn varint_too_long() {
        // 11 bytes with continuation bits set
        let buf = [0x80; 11];
        assert!(matches!(decode_varint(&buf), Err(DecodeError::VarintTooLong)));
    }

    #[test]
    fn varint_empty_buffer() {
        assert!(matches!(decode_varint(&[]), Err(DecodeError::UnexpectedEof)));
    }

    // -- Tag round-trip ------------------------------------------------------

    #[test]
    fn tag_round_trip() {
        let cases = [
            (1, WireType::Varint),
            (2, WireType::LengthDelimited),
            (15, WireType::Varint),
            (16, WireType::Fixed64),
            (100, WireType::Fixed32),
            (536_870_911, WireType::Varint), // max field number
        ];
        for (field, wt) in cases {
            let mut buf = Vec::new();
            encode_tag(field, wt, &mut buf);
            let (tag, consumed) = decode_tag(&buf).unwrap();
            assert_eq!(tag.field_number, field);
            assert_eq!(tag.wire_type, wt);
            assert_eq!(consumed, buf.len());
        }
    }

    #[test]
    fn tag_field_number_zero_rejected() {
        // field 0, wire type 0
        let buf = [0x00];
        assert!(matches!(decode_tag(&buf), Err(DecodeError::InvalidFieldNumber)));
    }

    // -- Fixed width ---------------------------------------------------------

    #[test]
    fn fixed32_round_trip() {
        let mut buf = Vec::new();
        encode_fixed32(0xDEADBEEF, &mut buf);
        let (val, consumed) = decode_fixed32(&buf).unwrap();
        assert_eq!(val, 0xDEADBEEF);
        assert_eq!(consumed, 4);
    }

    #[test]
    fn fixed64_round_trip() {
        let mut buf = Vec::new();
        encode_fixed64(0xDEAD_BEEF_CAFE_BABE, &mut buf);
        let (val, consumed) = decode_fixed64(&buf).unwrap();
        assert_eq!(val, 0xDEAD_BEEF_CAFE_BABE);
        assert_eq!(consumed, 8);
    }

    // -- Zigzag --------------------------------------------------------------

    #[test]
    fn zigzag_32_round_trip() {
        let cases: &[i32] = &[0, -1, 1, -2, 2, i32::MIN, i32::MAX];
        for &val in cases {
            assert_eq!(zigzag_decode_32(zigzag_encode_32(val)), val);
        }
    }

    #[test]
    fn zigzag_64_round_trip() {
        let cases: &[i64] = &[0, -1, 1, -2, 2, i64::MIN, i64::MAX];
        for &val in cases {
            assert_eq!(zigzag_decode_64(zigzag_encode_64(val)), val);
        }
    }

    // -- Skip field ----------------------------------------------------------

    #[test]
    fn skip_varint_field() {
        let mut buf = Vec::new();
        encode_varint(300, &mut buf);
        assert_eq!(skip_field(&buf, WireType::Varint, 100).unwrap(), buf.len());
    }

    #[test]
    fn skip_fixed32_field() {
        let buf = [0u8; 4];
        assert_eq!(skip_field(&buf, WireType::Fixed32, 100).unwrap(), 4);
    }

    #[test]
    fn skip_fixed64_field() {
        let buf = [0u8; 8];
        assert_eq!(skip_field(&buf, WireType::Fixed64, 100).unwrap(), 8);
    }

    #[test]
    fn skip_length_delimited_field() {
        let mut buf = Vec::new();
        encode_varint(5, &mut buf); // length = 5
        buf.extend_from_slice(&[0x41, 0x42, 0x43, 0x44, 0x45]);
        assert_eq!(
            skip_field(&buf, WireType::LengthDelimited, 100).unwrap(),
            buf.len()
        );
    }

    #[test]
    fn skip_group_field() {
        let mut buf = Vec::new();
        // Inner field: field 1, varint, value 42
        encode_tag(1, WireType::Varint, &mut buf);
        encode_varint(42, &mut buf);
        // End group: field 1, EndGroup
        encode_tag(1, WireType::EndGroup, &mut buf);
        assert_eq!(
            skip_field(&buf, WireType::StartGroup, 100).unwrap(),
            buf.len()
        );
    }

    // -- Packed repeated fields -----------------------------------------------

    #[test]
    fn packed_varint_round_trip() {
        let values = vec![1u64, 127, 128, 300, 0, u64::MAX];
        let mut buf = Vec::new();
        encode_packed_varints(1, &values, &mut buf);

        // Skip the tag
        let (tag, tag_consumed) = decode_tag(&buf).unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(tag.wire_type, WireType::LengthDelimited);

        let (decoded, consumed) = decode_packed_varints(&buf[tag_consumed..]).unwrap();
        assert_eq!(decoded, values);
        assert_eq!(tag_consumed + consumed, buf.len());

        // Size must match
        assert_eq!(buf.len(), packed_varints_size(1, &values));
    }

    #[test]
    fn packed_fixed32_round_trip() {
        let values = vec![0u32, 1, 0xDEADBEEF, u32::MAX];
        let mut buf = Vec::new();
        encode_packed_fixed32(1, &values, &mut buf);

        let (_, tag_consumed) = decode_tag(&buf).unwrap();
        let (decoded, consumed) = decode_packed_fixed32(&buf[tag_consumed..]).unwrap();
        assert_eq!(decoded, values);
        assert_eq!(tag_consumed + consumed, buf.len());
        assert_eq!(buf.len(), packed_fixed32_size(1, values.len()));
    }

    #[test]
    fn packed_fixed64_round_trip() {
        let values = vec![0u64, 1, 0xDEAD_BEEF_CAFE_BABE, u64::MAX];
        let mut buf = Vec::new();
        encode_packed_fixed64(1, &values, &mut buf);

        let (_, tag_consumed) = decode_tag(&buf).unwrap();
        let (decoded, consumed) = decode_packed_fixed64(&buf[tag_consumed..]).unwrap();
        assert_eq!(decoded, values);
        assert_eq!(tag_consumed + consumed, buf.len());
        assert_eq!(buf.len(), packed_fixed64_size(1, values.len()));
    }

    #[test]
    fn packed_empty_produces_nothing() {
        let mut buf = Vec::new();
        encode_packed_varints(1, &[], &mut buf);
        assert!(buf.is_empty());
        assert_eq!(packed_varints_size(1, &[]), 0);
    }
}
