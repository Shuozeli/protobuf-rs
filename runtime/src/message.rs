//! The core `Message` trait for protobuf message types.
//!
//! All generated message types implement this trait, providing a uniform
//! interface for two-pass serialization, deserialization, and size computation.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::cached_size::CachedSize;
use crate::error::DecodeError;
use crate::unknown_fields::UnknownFields;
use crate::wire::{self, WireType};

/// Default recursion limit for nested message decoding.
pub const DEFAULT_RECURSION_LIMIT: u32 = 100;

/// Options for decoding protobuf messages.
#[derive(Debug, Clone)]
pub struct DecodeOptions {
    /// Maximum recursion depth for nested messages.
    pub recursion_limit: u32,
    /// Maximum message size in bytes (0 = unlimited).
    pub max_message_size: usize,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            recursion_limit: DEFAULT_RECURSION_LIMIT,
            max_message_size: 0,
        }
    }
}

/// The core trait for all protobuf message types.
///
/// Generated message types implement this trait to support encoding, decoding,
/// and size computation using a two-pass strategy:
///
/// 1. `compute_size()` — walk the message tree, compute and cache sizes.
/// 2. `write_to()` — walk again, using cached sizes for length prefixes.
///
/// Both passes are O(n) in total message size.
pub trait Message: Default + Clone + PartialEq {
    /// Compute the serialized size and cache it. Returns the size in bytes.
    ///
    /// This is pass 1 of two-pass serialization. Implementors should store
    /// the result in their `CachedSize` field.
    fn compute_size(&self) -> u32;

    /// Write the message to the buffer.
    ///
    /// This is pass 2 of two-pass serialization. Must be called after
    /// `compute_size()`. Uses cached sizes for length-delimited field prefixes.
    fn write_to(&self, buf: &mut Vec<u8>);

    /// Merge a single field from the wire into this message.
    ///
    /// Called during decoding for each tag encountered. Returns `Ok(consumed)`
    /// with the number of bytes consumed from `buf` (after the tag).
    fn merge_field(
        &mut self,
        field_number: u32,
        wire_type: WireType,
        buf: &[u8],
        options: &DecodeOptions,
    ) -> Result<usize, DecodeError>;

    /// Reset all fields to their default values.
    fn clear(&mut self);

    /// Get a reference to the message's cached size.
    fn cached_size(&self) -> &CachedSize;

    /// Get a reference to the message's unknown fields.
    fn unknown_fields(&self) -> &UnknownFields;

    /// Get a mutable reference to the message's unknown fields.
    fn unknown_fields_mut(&mut self) -> &mut UnknownFields;

    /// The fully-qualified protobuf type name (e.g., "my.package.MyMessage").
    fn full_name() -> &'static str;
}

/// Extension methods for `Message` types.
///
/// These are provided as free functions rather than default methods to keep
/// the `Message` trait minimal for implementors.
pub fn encode<M: Message>(msg: &M) -> Vec<u8> {
    let size = msg.compute_size() as usize;
    let mut buf = Vec::with_capacity(size);
    msg.write_to(&mut buf);
    debug_assert_eq!(
        buf.len(),
        size,
        "compute_size() returned {} but write_to() wrote {} bytes for {}",
        size,
        buf.len(),
        M::full_name(),
    );
    buf
}

/// Decode a message from a byte slice.
pub fn decode<M: Message>(buf: &[u8]) -> Result<M, DecodeError> {
    decode_with_options(buf, &DecodeOptions::default())
}

/// Decode a message from a byte slice with custom options.
pub fn decode_with_options<M: Message>(
    buf: &[u8],
    options: &DecodeOptions,
) -> Result<M, DecodeError> {
    if options.max_message_size > 0 && buf.len() > options.max_message_size {
        return Err(DecodeError::MessageTooLarge {
            size: buf.len(),
            limit: options.max_message_size,
        });
    }

    let mut msg = M::default();
    let mut pos = 0;

    while pos < buf.len() {
        let (tag, tag_consumed) = wire::decode_tag(&buf[pos..])?;
        pos += tag_consumed;

        let field_consumed = msg.merge_field(tag.field_number, tag.wire_type, &buf[pos..], options)?;
        pos += field_consumed;
    }

    Ok(msg)
}

/// Write a length-delimited (nested message) field.
///
/// Uses the message's cached size to write the length prefix without
/// recomputing. `compute_size()` must have been called first.
pub fn write_message_field<M: Message>(
    field_number: u32,
    msg: &M,
    buf: &mut Vec<u8>,
) {
    wire::encode_tag(field_number, WireType::LengthDelimited, buf);
    wire::encode_varint(msg.cached_size().get() as u64, buf);
    msg.write_to(buf);
}

/// Compute the encoded size of a length-delimited (nested message) field.
pub fn message_field_size<M: Message>(field_number: u32, msg: &M) -> u32 {
    let tag_size = wire::varint_len(((field_number as u64) << 3) | WireType::LengthDelimited as u64);
    let msg_size = msg.compute_size();
    let len_prefix_size = wire::varint_len(msg_size as u64);
    (tag_size + len_prefix_size + msg_size as usize) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cached_size::CachedSize;
    use crate::unknown_fields::{self, UnknownFields};

    /// Minimal test message: a single varint field (field 1, int32).
    #[derive(Clone, Default, PartialEq, Debug)]
    struct SimpleMsg {
        pub value: i32,
        cached_size: CachedSize,
        unknown_fields: UnknownFields,
    }

    impl Message for SimpleMsg {
        fn compute_size(&self) -> u32 {
            let mut size = 0u32;
            if self.value != 0 {
                // tag(1, varint) = 1 byte + varint(value)
                size += 1 + wire::varint_len(self.value as u64) as u32;
            }
            size += self.unknown_fields.encoded_len() as u32;
            self.cached_size.set(size);
            size
        }

        fn write_to(&self, buf: &mut Vec<u8>) {
            if self.value != 0 {
                wire::encode_tag(1, WireType::Varint, buf);
                wire::encode_varint(self.value as u64, buf);
            }
            self.unknown_fields.write_to(buf);
        }

        fn merge_field(
            &mut self,
            field_number: u32,
            wire_type: WireType,
            buf: &[u8],
            options: &DecodeOptions,
        ) -> Result<usize, DecodeError> {
            match field_number {
                1 => {
                    if wire_type != WireType::Varint {
                        return Err(DecodeError::WireTypeMismatch {
                            expected: WireType::Varint,
                            actual: wire_type,
                        });
                    }
                    let (v, consumed) = wire::decode_varint(buf)?;
                    self.value = v as i32;
                    Ok(consumed)
                }
                _ => {
                    let (field, consumed) = unknown_fields::decode_unknown_field(
                        buf,
                        field_number,
                        wire_type,
                        options.recursion_limit,
                    )?;
                    self.unknown_fields.push(field);
                    Ok(consumed)
                }
            }
        }

        fn clear(&mut self) {
            self.value = 0;
            self.cached_size.set(0);
            self.unknown_fields.clear();
        }

        fn cached_size(&self) -> &CachedSize {
            &self.cached_size
        }

        fn unknown_fields(&self) -> &UnknownFields {
            &self.unknown_fields
        }

        fn unknown_fields_mut(&mut self) -> &mut UnknownFields {
            &mut self.unknown_fields
        }

        fn full_name() -> &'static str {
            "test.SimpleMsg"
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let msg = SimpleMsg { value: 42, ..Default::default() };
        let bytes = encode(&msg);
        let decoded: SimpleMsg = decode(&bytes).unwrap();
        assert_eq!(decoded.value, 42);
    }

    #[test]
    fn encode_default_is_empty() {
        let msg = SimpleMsg::default();
        let bytes = encode(&msg);
        assert!(bytes.is_empty());
    }

    #[test]
    fn unknown_fields_preserved() {
        // Encode a message with an extra field (field 99, varint 7).
        let mut buf = Vec::new();
        wire::encode_tag(1, WireType::Varint, &mut buf);
        wire::encode_varint(42, &mut buf);
        wire::encode_tag(99, WireType::Varint, &mut buf);
        wire::encode_varint(7, &mut buf);

        let msg: SimpleMsg = decode(&buf).unwrap();
        assert_eq!(msg.value, 42);
        assert_eq!(msg.unknown_fields().len(), 1);

        // Re-encode and verify the unknown field is preserved.
        let re_encoded = encode(&msg);
        assert_eq!(re_encoded, buf);
    }

    #[test]
    fn max_message_size_enforced() {
        let buf = vec![0u8; 100];
        let opts = DecodeOptions {
            max_message_size: 50,
            ..Default::default()
        };
        let result = decode_with_options::<SimpleMsg>(&buf, &opts);
        assert!(matches!(result, Err(DecodeError::MessageTooLarge { .. })));
    }

    #[test]
    fn clear_resets_all_fields() {
        let mut msg = SimpleMsg { value: 42, ..Default::default() };
        msg.unknown_fields_mut().push(crate::unknown_fields::UnknownField {
            number: 99,
            data: crate::unknown_fields::UnknownFieldData::Varint(1),
        });
        msg.clear();
        assert_eq!(msg.value, 0);
        assert!(msg.unknown_fields().is_empty());
    }
}
