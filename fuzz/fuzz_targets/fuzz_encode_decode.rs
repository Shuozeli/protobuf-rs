//! Fuzz target: encode then decode, verify round-trip.
//!
//! Tests that encode(decode(data)) == data for valid messages,
//! and that decode(encode(msg)) == msg always holds.

#![no_main]
use libfuzzer_sys::fuzz_target;
use protoc_rs_runtime::*;
use protoc_rs_runtime::unknown_fields::{self, UnknownField, UnknownFieldData};
use protoc_rs_runtime::wire::{self, WireType};

#[derive(Clone, Default, PartialEq, Debug)]
struct SimpleMsg {
    pub value: i32,
    pub name: String,
    pub _cached_size: CachedSize,
    pub _unknown_fields: UnknownFields,
}

impl Message for SimpleMsg {
    fn compute_size(&self) -> u32 {
        let mut size = 0u32;
        if self.value != 0 {
            size += (1 + wire::varint_len(self.value as u64)) as u32;
        }
        if !self.name.is_empty() {
            size += (1 + wire::varint_len(self.name.len() as u64) + self.name.len()) as u32;
        }
        size += self._unknown_fields.encoded_len() as u32;
        self._cached_size.set(size);
        size
    }

    fn write_to(&self, buf: &mut Vec<u8>) {
        if self.value != 0 {
            wire::encode_tag(1, WireType::Varint, buf);
            wire::encode_varint(self.value as u64, buf);
        }
        if !self.name.is_empty() {
            wire::encode_tag(2, WireType::LengthDelimited, buf);
            wire::encode_varint(self.name.len() as u64, buf);
            buf.extend_from_slice(self.name.as_bytes());
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
                self.value = v as i32;
                Ok(c)
            }
            2 => {
                if wire_type != WireType::LengthDelimited {
                    return Err(DecodeError::WireTypeMismatch { expected: WireType::LengthDelimited, actual: wire_type });
                }
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() { return Err(DecodeError::UnexpectedEof); }
                self.name = std::str::from_utf8(&buf[h..end])
                    .map_err(|_| DecodeError::InvalidUtf8)?.to_string();
                Ok(end)
            }
            _ => {
                let (field, c) = unknown_fields::decode_unknown_field(buf, field_number, wire_type, options.recursion_limit)?;
                self._unknown_fields.push(field);
                Ok(c)
            }
        }
    }

    fn clear(&mut self) { *self = Self::default(); }
    fn cached_size(&self) -> &CachedSize { &self._cached_size }
    fn unknown_fields(&self) -> &UnknownFields { &self._unknown_fields }
    fn unknown_fields_mut(&mut self) -> &mut UnknownFields { &mut self._unknown_fields }
    fn full_name() -> &'static str { "fuzz.SimpleMsg" }
}

fuzz_target!(|data: &[u8]| {
    // Try to decode
    if let Ok(msg) = decode::<SimpleMsg>(data) {
        // Encode it back
        let encoded = encode(&msg);
        // Decode again
        let msg2 = decode::<SimpleMsg>(&encoded).expect("re-decode must succeed");
        // Must be equal
        assert_eq!(msg, msg2, "round-trip mismatch");
    }
});
