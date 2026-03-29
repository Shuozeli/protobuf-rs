//! Fuzz target: decode arbitrary bytes as a protobuf message.
//!
//! Tests that the decoder never panics on arbitrary input.

#![no_main]
use libfuzzer_sys::fuzz_target;
use protoc_rs_runtime::*;
use protoc_rs_runtime::unknown_fields::{self, UnknownField, UnknownFieldData};
use protoc_rs_runtime::wire::{self, WireType};

/// A simple message with fields of various types for fuzzing.
#[derive(Clone, Default, PartialEq, Debug)]
struct FuzzMsg {
    pub int_field: i32,
    pub str_field: String,
    pub bytes_field: Vec<u8>,
    pub repeated_int: Vec<i32>,
    pub _cached_size: CachedSize,
    pub _unknown_fields: UnknownFields,
}

impl Message for FuzzMsg {
    fn compute_size(&self) -> u32 {
        let mut size = 0u32;
        if self.int_field != 0 {
            size += (1 + wire::varint_len(self.int_field as u64)) as u32;
        }
        if !self.str_field.is_empty() {
            size += (1 + wire::varint_len(self.str_field.len() as u64) + self.str_field.len()) as u32;
        }
        if !self.bytes_field.is_empty() {
            size += (1 + wire::varint_len(self.bytes_field.len() as u64) + self.bytes_field.len()) as u32;
        }
        for v in &self.repeated_int {
            size += (1 + wire::varint_len(*v as u64)) as u32;
        }
        size += self._unknown_fields.encoded_len() as u32;
        self._cached_size.set(size);
        size
    }

    fn write_to(&self, buf: &mut Vec<u8>) {
        if self.int_field != 0 {
            wire::encode_tag(1, WireType::Varint, buf);
            wire::encode_varint(self.int_field as u64, buf);
        }
        if !self.str_field.is_empty() {
            wire::encode_tag(2, WireType::LengthDelimited, buf);
            wire::encode_varint(self.str_field.len() as u64, buf);
            buf.extend_from_slice(self.str_field.as_bytes());
        }
        if !self.bytes_field.is_empty() {
            wire::encode_tag(3, WireType::LengthDelimited, buf);
            wire::encode_varint(self.bytes_field.len() as u64, buf);
            buf.extend_from_slice(&self.bytes_field);
        }
        for v in &self.repeated_int {
            wire::encode_tag(4, WireType::Varint, buf);
            wire::encode_varint(*v as u64, buf);
        }
        self._unknown_fields.write_to(buf);
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
                    return Err(DecodeError::WireTypeMismatch { expected: WireType::Varint, actual: wire_type });
                }
                let (v, c) = wire::decode_varint(buf)?;
                self.int_field = v as i32;
                Ok(c)
            }
            2 => {
                if wire_type != WireType::LengthDelimited {
                    return Err(DecodeError::WireTypeMismatch { expected: WireType::LengthDelimited, actual: wire_type });
                }
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() { return Err(DecodeError::UnexpectedEof); }
                self.str_field = std::str::from_utf8(&buf[h..end])
                    .map_err(|_| DecodeError::InvalidUtf8)?.to_string();
                Ok(end)
            }
            3 => {
                if wire_type != WireType::LengthDelimited {
                    return Err(DecodeError::WireTypeMismatch { expected: WireType::LengthDelimited, actual: wire_type });
                }
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() { return Err(DecodeError::UnexpectedEof); }
                self.bytes_field = buf[h..end].to_vec();
                Ok(end)
            }
            4 => {
                if wire_type != WireType::Varint {
                    return Err(DecodeError::WireTypeMismatch { expected: WireType::Varint, actual: wire_type });
                }
                let (v, c) = wire::decode_varint(buf)?;
                self.repeated_int.push(v as i32);
                Ok(c)
            }
            _ => {
                let (field, c) = unknown_fields::decode_unknown_field(buf, field_number, wire_type, options.recursion_limit)?;
                self._unknown_fields.push(field);
                Ok(c)
            }
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn cached_size(&self) -> &CachedSize { &self._cached_size }
    fn unknown_fields(&self) -> &UnknownFields { &self._unknown_fields }
    fn unknown_fields_mut(&mut self) -> &mut UnknownFields { &mut self._unknown_fields }
    fn full_name() -> &'static str { "fuzz.FuzzMsg" }
}

fuzz_target!(|data: &[u8]| {
    // Must not panic on any input
    let _ = decode::<FuzzMsg>(data);
});
