//! Typed extension field accessors.
//!
//! Extensions are fields defined in `extend` blocks that target a message
//! with declared `extension_range`. On the wire, extension field values
//! are stored in the message's `UnknownFields`. This module provides
//! typed accessors to decode extension values from unknown fields.

use crate::error::DecodeError;
use crate::unknown_fields::UnknownFields;
use crate::wire::{self, WireType};

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Function type for decoding a scalar value from wire bytes.
pub type DecodeFn<T> = fn(&[u8]) -> Result<(T, usize), DecodeError>;

/// Descriptor for a scalar extension field.
pub struct ExtensionDescriptor<T> {
    /// The field number of the extension.
    pub field_number: u32,
    /// The wire type used for encoding.
    pub wire_type: WireType,
    /// Function to decode a value from raw wire bytes.
    pub decode_fn: DecodeFn<T>,
    /// The default value when the extension is not set.
    pub default_value: T,
}

impl<T: Clone> ExtensionDescriptor<T> {
    /// Get the extension value from a message's unknown fields.
    ///
    /// Returns the last occurrence if the field appears multiple times
    /// (last-write-wins semantics per protobuf spec).
    pub fn get(&self, unknown_fields: &UnknownFields) -> T {
        let mut result = self.default_value.clone();
        for field in unknown_fields.iter() {
            if field.number == self.field_number {
                match &field.data {
                    crate::unknown_fields::UnknownFieldData::Varint(v)
                        if self.wire_type == WireType::Varint =>
                    {
                        let mut buf = Vec::new();
                        wire::encode_varint(*v, &mut buf);
                        if let Ok((val, _)) = (self.decode_fn)(&buf) {
                            result = val;
                        }
                    }
                    crate::unknown_fields::UnknownFieldData::Fixed32(v)
                        if self.wire_type == WireType::Fixed32 =>
                    {
                        let buf = v.to_le_bytes();
                        if let Ok((val, _)) = (self.decode_fn)(&buf) {
                            result = val;
                        }
                    }
                    crate::unknown_fields::UnknownFieldData::Fixed64(v)
                        if self.wire_type == WireType::Fixed64 =>
                    {
                        let buf = v.to_le_bytes();
                        if let Ok((val, _)) = (self.decode_fn)(&buf) {
                            result = val;
                        }
                    }
                    crate::unknown_fields::UnknownFieldData::LengthDelimited(bytes)
                        if self.wire_type == WireType::LengthDelimited =>
                    {
                        if let Ok((val, _)) = (self.decode_fn)(bytes) {
                            result = val;
                        }
                    }
                    _ => {}
                }
            }
        }
        result
    }

    /// Check if the extension is present in the unknown fields.
    pub fn is_set(&self, unknown_fields: &UnknownFields) -> bool {
        unknown_fields.iter().any(|f| f.number == self.field_number)
    }
}

/// Descriptor for a message-typed extension field.
pub struct MessageExtensionDescriptor<T> {
    /// The field number of the extension.
    pub field_number: u32,
    /// Function to decode a message from raw wire bytes.
    pub decode_fn: fn(&[u8]) -> Result<T, DecodeError>,
}

impl<T: Clone> MessageExtensionDescriptor<T> {
    /// Get the extension value, if present.
    pub fn get(&self, unknown_fields: &UnknownFields) -> Option<T> {
        // Find last length-delimited field with matching number
        let mut result = None;
        for field in unknown_fields.iter() {
            if field.number == self.field_number {
                if let crate::unknown_fields::UnknownFieldData::LengthDelimited(bytes) = &field.data
                {
                    if let Ok(val) = (self.decode_fn)(bytes) {
                        result = Some(val);
                    }
                }
            }
        }
        result
    }

    /// Check if the extension is present.
    pub fn is_set(&self, unknown_fields: &UnknownFields) -> bool {
        unknown_fields.iter().any(|f| f.number == self.field_number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unknown_fields::{UnknownField, UnknownFieldData};

    fn decode_varint_i32(buf: &[u8]) -> Result<(i32, usize), DecodeError> {
        let (v, c) = wire::decode_varint(buf)?;
        Ok((v as i32, c))
    }

    #[test]
    fn scalar_extension_get() {
        let ext = ExtensionDescriptor {
            field_number: 100,
            wire_type: WireType::Varint,
            decode_fn: decode_varint_i32,
            default_value: 0,
        };

        let mut uf = UnknownFields::default();
        assert_eq!(ext.get(&uf), 0); // default
        assert!(!ext.is_set(&uf));

        uf.push(UnknownField {
            number: 100,
            data: UnknownFieldData::Varint(42),
        });
        assert_eq!(ext.get(&uf), 42);
        assert!(ext.is_set(&uf));
    }

    #[test]
    fn scalar_extension_last_wins() {
        let ext = ExtensionDescriptor {
            field_number: 100,
            wire_type: WireType::Varint,
            decode_fn: decode_varint_i32,
            default_value: 0,
        };

        let mut uf = UnknownFields::default();
        uf.push(UnknownField {
            number: 100,
            data: UnknownFieldData::Varint(10),
        });
        uf.push(UnknownField {
            number: 100,
            data: UnknownFieldData::Varint(20),
        });
        assert_eq!(ext.get(&uf), 20); // last wins
    }
}
