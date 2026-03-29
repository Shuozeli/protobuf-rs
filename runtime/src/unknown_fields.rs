//! Unknown field preservation for round-trip fidelity.
//!
//! When a message is decoded with a schema older than the data, fields with
//! unrecognized field numbers are stored in `UnknownFields`. This allows
//! re-encoding the message without losing data.

#[cfg(not(feature = "std"))]
use alloc::{format, string::ToString, vec::Vec};

use crate::error::DecodeError;
use crate::wire::{self, WireType};

/// A collection of unknown fields on a message.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnknownFields {
    fields: Vec<UnknownField>,
}

impl UnknownFields {
    /// Create an empty collection.
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Returns `true` if there are no unknown fields.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Number of unknown fields.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Add an unknown field.
    pub fn push(&mut self, field: UnknownField) {
        self.fields.push(field);
    }

    /// Iterate over the unknown fields.
    pub fn iter(&self) -> impl Iterator<Item = &UnknownField> {
        self.fields.iter()
    }

    /// Remove all fields with the given field number.
    pub fn retain(&mut self, mut predicate: impl FnMut(&UnknownField) -> bool) {
        self.fields.retain(|f| predicate(f));
    }

    /// Clear all unknown fields.
    pub fn clear(&mut self) {
        self.fields.clear();
    }

    /// Compute the encoded size of all unknown fields.
    pub fn encoded_len(&self) -> usize {
        self.fields.iter().map(|f| f.encoded_len()).sum()
    }

    /// Write all unknown fields to the buffer.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        for field in &self.fields {
            field.write_to(buf);
        }
    }
}

// Serde: UnknownFields are skipped in JSON serialization.
#[cfg(feature = "json")]
impl serde::Serialize for UnknownFields {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

#[cfg(feature = "json")]
impl<'de> serde::Deserialize<'de> for UnknownFields {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(UnknownFields::new())
    }
}

impl<'a> IntoIterator for &'a UnknownFields {
    type Item = &'a UnknownField;
    type IntoIter = core::slice::Iter<'a, UnknownField>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.iter()
    }
}

impl IntoIterator for UnknownFields {
    type Item = UnknownField;
    type IntoIter = <Vec<UnknownField> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

/// A single unknown field (field number + wire data).
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownField {
    pub number: u32,
    pub data: UnknownFieldData,
}

/// The wire data of an unknown field.
#[derive(Debug, Clone, PartialEq)]
pub enum UnknownFieldData {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    LengthDelimited(Vec<u8>),
    /// A group (wire type 3/4). Contains the group's inner fields.
    Group(UnknownFields),
}

impl UnknownField {
    /// Compute the encoded length of this field (tag + value).
    pub fn encoded_len(&self) -> usize {
        let tag_len = wire::varint_len(((self.number as u64) << 3) | self.data.wire_type_value());
        let value_len = match &self.data {
            UnknownFieldData::Varint(v) => wire::varint_len(*v),
            UnknownFieldData::Fixed64(_) => 8,
            UnknownFieldData::Fixed32(_) => 4,
            UnknownFieldData::LengthDelimited(bytes) => {
                wire::varint_len(bytes.len() as u64) + bytes.len()
            }
            UnknownFieldData::Group(inner) => {
                // Inner fields + end-group tag.
                let end_tag_len =
                    wire::varint_len(((self.number as u64) << 3) | WireType::EndGroup as u64);
                inner.encoded_len() + end_tag_len
            }
        };
        tag_len + value_len
    }

    /// Write this field to the buffer.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        match &self.data {
            UnknownFieldData::Varint(v) => {
                wire::encode_tag(self.number, WireType::Varint, buf);
                wire::encode_varint(*v, buf);
            }
            UnknownFieldData::Fixed64(v) => {
                wire::encode_tag(self.number, WireType::Fixed64, buf);
                wire::encode_fixed64(*v, buf);
            }
            UnknownFieldData::Fixed32(v) => {
                wire::encode_tag(self.number, WireType::Fixed32, buf);
                wire::encode_fixed32(*v, buf);
            }
            UnknownFieldData::LengthDelimited(bytes) => {
                wire::encode_tag(self.number, WireType::LengthDelimited, buf);
                wire::encode_varint(bytes.len() as u64, buf);
                buf.extend_from_slice(bytes);
            }
            UnknownFieldData::Group(inner) => {
                wire::encode_tag(self.number, WireType::StartGroup, buf);
                inner.write_to(buf);
                wire::encode_tag(self.number, WireType::EndGroup, buf);
            }
        }
    }
}

impl UnknownFieldData {
    fn wire_type_value(&self) -> u64 {
        match self {
            UnknownFieldData::Varint(_) => WireType::Varint as u64,
            UnknownFieldData::Fixed64(_) => WireType::Fixed64 as u64,
            UnknownFieldData::Fixed32(_) => WireType::Fixed32 as u64,
            UnknownFieldData::LengthDelimited(_) => WireType::LengthDelimited as u64,
            UnknownFieldData::Group(_) => WireType::StartGroup as u64,
        }
    }
}

/// Decode an unknown field from the buffer. Returns (UnknownField, bytes_consumed).
pub fn decode_unknown_field(
    buf: &[u8],
    field_number: u32,
    wire_type: WireType,
    recursion_limit: u32,
) -> Result<(UnknownField, usize), DecodeError> {
    let (data, consumed) = match wire_type {
        WireType::Varint => {
            let (v, consumed) = wire::decode_varint(buf)?;
            (UnknownFieldData::Varint(v), consumed)
        }
        WireType::Fixed64 => {
            let (v, consumed) = wire::decode_fixed64(buf)?;
            (UnknownFieldData::Fixed64(v), consumed)
        }
        WireType::Fixed32 => {
            let (v, consumed) = wire::decode_fixed32(buf)?;
            (UnknownFieldData::Fixed32(v), consumed)
        }
        WireType::LengthDelimited => {
            let (len, header_consumed) = wire::decode_varint(buf)?;
            let len = len as usize;
            let end = header_consumed + len;
            if end > buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let bytes = buf[header_consumed..end].to_vec();
            (UnknownFieldData::LengthDelimited(bytes), end)
        }
        WireType::StartGroup => {
            if recursion_limit == 0 {
                return Err(DecodeError::RecursionLimitExceeded);
            }
            let mut pos = 0;
            let mut inner = UnknownFields::new();
            loop {
                let (tag, tag_consumed) = wire::decode_tag(&buf[pos..])?;
                pos += tag_consumed;
                if tag.wire_type == WireType::EndGroup {
                    if tag.field_number != field_number {
                        return Err(DecodeError::Custom(format!(
                            "mismatched end-group field number: expected {}, got {}",
                            field_number, tag.field_number
                        )));
                    }
                    break;
                }
                let (field, field_consumed) = decode_unknown_field(
                    &buf[pos..],
                    tag.field_number,
                    tag.wire_type,
                    recursion_limit - 1,
                )?;
                pos += field_consumed;
                inner.push(field);
            }
            (UnknownFieldData::Group(inner), pos)
        }
        WireType::EndGroup => {
            // EndGroup should be handled by the caller (StartGroup decoder).
            return Err(DecodeError::Custom(
                "unexpected EndGroup wire type".to_string(),
            ));
        }
    };

    Ok((
        UnknownField {
            number: field_number,
            data,
        },
        consumed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(field: &UnknownField) -> UnknownField {
        let mut buf = Vec::new();
        field.write_to(&mut buf);
        assert_eq!(
            buf.len(),
            field.encoded_len(),
            "encoded_len mismatch for {:?}",
            field
        );
        // Decode: skip the tag first.
        let (tag, tag_consumed) = wire::decode_tag(&buf).unwrap();
        assert_eq!(tag.field_number, field.number);
        let (decoded, _) =
            decode_unknown_field(&buf[tag_consumed..], tag.field_number, tag.wire_type, 100)
                .unwrap();
        decoded
    }

    #[test]
    fn round_trip_varint() {
        let field = UnknownField {
            number: 1,
            data: UnknownFieldData::Varint(300),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn round_trip_fixed64() {
        let field = UnknownField {
            number: 2,
            data: UnknownFieldData::Fixed64(0xDEADBEEF),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn round_trip_fixed32() {
        let field = UnknownField {
            number: 3,
            data: UnknownFieldData::Fixed32(0xCAFE),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn round_trip_length_delimited() {
        let field = UnknownField {
            number: 4,
            data: UnknownFieldData::LengthDelimited(b"hello".to_vec()),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn round_trip_group() {
        let mut inner = UnknownFields::new();
        inner.push(UnknownField {
            number: 1,
            data: UnknownFieldData::Varint(42),
        });
        let field = UnknownField {
            number: 5,
            data: UnknownFieldData::Group(inner),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn round_trip_nested_group() {
        let mut innermost = UnknownFields::new();
        innermost.push(UnknownField {
            number: 1,
            data: UnknownFieldData::Fixed32(7),
        });

        let mut inner = UnknownFields::new();
        inner.push(UnknownField {
            number: 2,
            data: UnknownFieldData::Group(innermost),
        });

        let field = UnknownField {
            number: 10,
            data: UnknownFieldData::Group(inner),
        };
        assert_eq!(round_trip(&field), field);
    }

    #[test]
    fn encoded_len_matches_write_to() {
        let cases = vec![
            UnknownField {
                number: 1,
                data: UnknownFieldData::Varint(0),
            },
            UnknownField {
                number: 1,
                data: UnknownFieldData::Varint(u64::MAX),
            },
            UnknownField {
                number: 15,
                data: UnknownFieldData::Fixed64(0),
            },
            UnknownField {
                number: 16,
                data: UnknownFieldData::Fixed32(0),
            },
            UnknownField {
                number: 100,
                data: UnknownFieldData::LengthDelimited(vec![0; 128]),
            },
        ];
        for field in &cases {
            let mut buf = Vec::new();
            field.write_to(&mut buf);
            assert_eq!(buf.len(), field.encoded_len(), "mismatch for {:?}", field);
        }
    }

    #[test]
    fn empty_unknown_fields() {
        let uf = UnknownFields::new();
        assert!(uf.is_empty());
        assert_eq!(uf.len(), 0);
        assert_eq!(uf.encoded_len(), 0);
    }

    #[test]
    fn retain_filters_fields() {
        let mut uf = UnknownFields::new();
        uf.push(UnknownField {
            number: 1,
            data: UnknownFieldData::Varint(1),
        });
        uf.push(UnknownField {
            number: 2,
            data: UnknownFieldData::Varint(2),
        });
        uf.push(UnknownField {
            number: 1,
            data: UnknownFieldData::Varint(3),
        });
        uf.retain(|f| f.number != 1);
        assert_eq!(uf.len(), 1);
        assert_eq!(uf.fields[0].number, 2);
    }
}
