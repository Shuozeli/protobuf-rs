//! Zero-copy view types for protobuf messages.
//!
//! View types borrow directly from the wire buffer, avoiding heap allocation
//! for `string` and `bytes` fields. Scalar fields are still decoded (varints
//! must be parsed), but no data is copied for length-delimited fields.
//!
//! # When to use views
//!
//! - **Read-heavy workloads**: parsing a message just to inspect a few fields
//! - **High-throughput pipelines**: avoiding allocator pressure
//! - **Short-lived access**: the view cannot outlive the input buffer
//!
//! # Limitations
//!
//! - Views are immutable (no mutation, no re-serialization)
//! - Views borrow from the input buffer (`'a` lifetime)
//! - Repeated/map fields collect into `Vec` (each element borrows from buffer)

use crate::error::DecodeError;
use crate::wire::{self, WireType};

/// Default recursion limit for view decoding.
pub const DEFAULT_VIEW_RECURSION_LIMIT: u32 = 100;

/// Trait for zero-copy message views decoded from a wire buffer.
///
/// Unlike [`Message`](crate::Message), view types borrow `&'a str` and
/// `&'a [u8]` directly from the input buffer instead of allocating.
pub trait MessageView<'a>: Default {
    /// Merge a single field from the wire buffer into this view.
    ///
    /// Returns the number of bytes consumed from `buf` (after the tag).
    fn merge_field_view(
        &mut self,
        field_number: u32,
        wire_type: WireType,
        buf: &'a [u8],
        recursion_limit: u32,
    ) -> Result<usize, DecodeError>;
}

/// Decode a message view from a byte slice.
pub fn decode_view<'a, V: MessageView<'a>>(buf: &'a [u8]) -> Result<V, DecodeError> {
    decode_view_with_limit(buf, DEFAULT_VIEW_RECURSION_LIMIT)
}

/// Decode a message view from a byte slice with a custom recursion limit.
pub fn decode_view_with_limit<'a, V: MessageView<'a>>(
    buf: &'a [u8],
    recursion_limit: u32,
) -> Result<V, DecodeError> {
    let mut view = V::default();
    let mut pos = 0;

    while pos < buf.len() {
        let (tag, tag_consumed) = wire::decode_tag(&buf[pos..])?;
        pos += tag_consumed;

        let field_consumed = view.merge_field_view(
            tag.field_number,
            tag.wire_type,
            &buf[pos..],
            recursion_limit,
        )?;
        pos += field_consumed;
    }

    Ok(view)
}

/// Decode a length-delimited sub-message view from a buffer.
///
/// Returns (view, total_bytes_consumed) where total includes the length prefix.
pub fn decode_sub_view<'a, V: MessageView<'a>>(
    buf: &'a [u8],
    recursion_limit: u32,
) -> Result<(V, usize), DecodeError> {
    let (len, header) = wire::decode_varint(buf)?;
    let end = header + len as usize;
    if end > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let view = decode_view_with_limit(&buf[header..end], recursion_limit)?;
    Ok((view, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal test view: a single string field (field 1) and an int32 (field 2).
    #[derive(Default, Debug, PartialEq)]
    struct SimpleView<'a> {
        pub name: &'a str,
        pub id: i32,
    }

    impl<'a> MessageView<'a> for SimpleView<'a> {
        fn merge_field_view(
            &mut self,
            field_number: u32,
            wire_type: WireType,
            buf: &'a [u8],
            _recursion_limit: u32,
        ) -> Result<usize, DecodeError> {
            match field_number {
                1 => {
                    if wire_type != WireType::LengthDelimited {
                        return Err(DecodeError::WireTypeMismatch {
                            expected: WireType::LengthDelimited,
                            actual: wire_type,
                        });
                    }
                    let (len, header) = wire::decode_varint(buf)?;
                    let end = header + len as usize;
                    if end > buf.len() {
                        return Err(DecodeError::UnexpectedEof);
                    }
                    self.name = std::str::from_utf8(&buf[header..end])
                        .map_err(|_| DecodeError::InvalidUtf8)?;
                    Ok(end)
                }
                2 => {
                    if wire_type != WireType::Varint {
                        return Err(DecodeError::WireTypeMismatch {
                            expected: WireType::Varint,
                            actual: wire_type,
                        });
                    }
                    let (v, consumed) = wire::decode_varint(buf)?;
                    self.id = v as i32;
                    Ok(consumed)
                }
                _ => wire::skip_field(buf, wire_type, 100),
            }
        }
    }

    #[test]
    fn decode_view_zero_copy_string() {
        // Build wire data: field 1 = "hello", field 2 = 42
        let mut buf = Vec::new();
        wire::encode_tag(1, WireType::LengthDelimited, &mut buf);
        wire::encode_varint(5, &mut buf); // len = 5
        buf.extend_from_slice(b"hello");
        wire::encode_tag(2, WireType::Varint, &mut buf);
        wire::encode_varint(42, &mut buf);

        let view: SimpleView = decode_view(&buf).unwrap();
        assert_eq!(view.name, "hello");
        assert_eq!(view.id, 42);

        // Verify zero-copy: the view's name pointer points into the original buffer.
        let name_ptr = view.name.as_ptr();
        let buf_range = buf.as_ptr_range();
        assert!(
            buf_range.contains(&name_ptr),
            "view.name should point into the original buffer"
        );
    }

    #[test]
    fn decode_view_empty_buffer() {
        let view: SimpleView = decode_view(&[]).unwrap();
        assert_eq!(view.name, "");
        assert_eq!(view.id, 0);
    }

    #[test]
    fn decode_view_skips_unknown_fields() {
        let mut buf = Vec::new();
        // Unknown field 99, varint
        wire::encode_tag(99, WireType::Varint, &mut buf);
        wire::encode_varint(999, &mut buf);
        // Known field 2
        wire::encode_tag(2, WireType::Varint, &mut buf);
        wire::encode_varint(7, &mut buf);

        let view: SimpleView = decode_view(&buf).unwrap();
        assert_eq!(view.id, 7);
        assert_eq!(view.name, ""); // not set
    }
}
