//! Random protobuf binary data generator.
//!
//! Given a [`SchemaIR`], produces valid protobuf wire-format bytes for the
//! root message.

use crate::schema_gen::{FieldDef, FieldTypeDef, GenConfig, MessageDef, SchemaIR};
use rand::rngs::StdRng;
use rand::Rng;

pub fn generate_data(rng: &mut StdRng, schema: &SchemaIR, config: &GenConfig) -> Vec<u8> {
    let root = schema
        .messages
        .iter()
        .find(|m| m.name == schema.root_message);
    let root = match root {
        Some(m) => m,
        None => return Vec::new(),
    };
    let max_depth = config.max_nesting_depth.max(1);
    encode_message(rng, root, schema, 0, max_depth)
}

fn encode_message(
    rng: &mut StdRng,
    msg: &MessageDef,
    schema: &SchemaIR,
    depth: usize,
    max_depth: usize,
) -> Vec<u8> {
    let mut buf = Vec::new();

    for (i, field) in msg.fields.iter().enumerate() {
        let is_root_first = depth == 0 && i == 0;
        if field.repeated {
            let count = rng.gen_range(1..=4);
            for _ in 0..count {
                encode_field(rng, field, msg, schema, depth, max_depth, &mut buf);
            }
        } else {
            // In proto3, fields with default values are omitted ~10% of the time,
            // but always include at least the first field for the root message.
            if !is_root_first && rng.gen_bool(0.1) {
                continue;
            }
            encode_field(rng, field, msg, schema, depth, max_depth, &mut buf);
        }
    }

    buf
}

fn encode_field(
    rng: &mut StdRng,
    field: &FieldDef,
    parent_msg: &MessageDef,
    schema: &SchemaIR,
    depth: usize,
    max_depth: usize,
    buf: &mut Vec<u8>,
) {
    let field_number = field.number as u32;

    match &field.field_type {
        // Varint-encoded scalars (wire type 0)
        FieldTypeDef::Int32
        | FieldTypeDef::Int64
        | FieldTypeDef::Uint32
        | FieldTypeDef::Uint64
        | FieldTypeDef::Sint32
        | FieldTypeDef::Sint64
        | FieldTypeDef::Bool => {
            write_tag(buf, field_number, 0);
            write_varint(buf, random_varint(rng, &field.field_type));
        }
        // 32-bit fixed (wire type 5)
        FieldTypeDef::Float | FieldTypeDef::Fixed32 | FieldTypeDef::Sfixed32 => {
            write_tag(buf, field_number, 5);
            buf.extend_from_slice(&random_fixed32(rng, &field.field_type));
        }
        // 64-bit fixed (wire type 1)
        FieldTypeDef::Double | FieldTypeDef::Fixed64 | FieldTypeDef::Sfixed64 => {
            write_tag(buf, field_number, 1);
            buf.extend_from_slice(&random_fixed64(rng, &field.field_type));
        }
        // Length-delimited (wire type 2)
        FieldTypeDef::String => {
            write_tag(buf, field_number, 2);
            let s = random_string(rng);
            write_varint(buf, s.len() as u64);
            buf.extend_from_slice(s.as_bytes());
        }
        FieldTypeDef::Bytes => {
            write_tag(buf, field_number, 2);
            let len = rng.gen_range(1..=8);
            write_varint(buf, len as u64);
            for _ in 0..len {
                buf.push(rng.gen());
            }
        }
        FieldTypeDef::Enum(enum_name) => {
            write_tag(buf, field_number, 0);
            let enum_def = schema.enums.iter().find(|e| &e.name == enum_name);
            let value = match enum_def {
                Some(e) if !e.values.is_empty() => {
                    let idx = rng.gen_range(0..e.values.len());
                    e.values[idx].1
                }
                _ => 0,
            };
            write_varint(buf, value as u64);
        }
        FieldTypeDef::Message(msg_name) => {
            if depth >= max_depth {
                return;
            }
            let msg_def = parent_msg
                .nested_messages
                .iter()
                .find(|m| &m.name == msg_name)
                .or_else(|| schema.messages.iter().find(|m| &m.name == msg_name));
            if let Some(msg_def) = msg_def {
                let inner = encode_message(rng, msg_def, schema, depth + 1, max_depth);
                write_tag(buf, field_number, 2);
                write_varint(buf, inner.len() as u64);
                buf.extend_from_slice(&inner);
            }
        }
    }
}

/// Generate a random varint value appropriate for the given scalar type.
fn random_varint(rng: &mut StdRng, ft: &FieldTypeDef) -> u64 {
    match ft {
        FieldTypeDef::Int32 => rng.gen_range(-100..=100i32) as i64 as u64,
        FieldTypeDef::Int64 => rng.gen_range(-1000..=1000i64) as u64,
        FieldTypeDef::Uint32 => rng.gen_range(0..=200u32) as u64,
        FieldTypeDef::Uint64 => rng.gen_range(0..=2000u64),
        FieldTypeDef::Sint32 => zigzag_encode_32(rng.gen_range(-100..=100)),
        FieldTypeDef::Sint64 => zigzag_encode_64(rng.gen_range(-1000..=1000)),
        FieldTypeDef::Bool => u64::from(rng.gen_bool(0.5)),
        _ => 0,
    }
}

/// Generate random 4-byte fixed-width value.
fn random_fixed32(rng: &mut StdRng, ft: &FieldTypeDef) -> [u8; 4] {
    match ft {
        FieldTypeDef::Float => rng.gen_range(-1000.0..1000.0f32).to_le_bytes(),
        FieldTypeDef::Fixed32 => rng.gen_range(0..=1000u32).to_le_bytes(),
        FieldTypeDef::Sfixed32 => rng.gen_range(-500..=500i32).to_le_bytes(),
        _ => [0; 4],
    }
}

/// Generate random 8-byte fixed-width value.
fn random_fixed64(rng: &mut StdRng, ft: &FieldTypeDef) -> [u8; 8] {
    match ft {
        FieldTypeDef::Double => rng.gen_range(-1000.0..1000.0f64).to_le_bytes(),
        FieldTypeDef::Fixed64 => rng.gen_range(0..=10000u64).to_le_bytes(),
        FieldTypeDef::Sfixed64 => rng.gen_range(-5000..=5000i64).to_le_bytes(),
        _ => [0; 8],
    }
}

// ---------------------------------------------------------------------------
// Wire format helpers
// ---------------------------------------------------------------------------

fn write_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u32) {
    write_varint(buf, ((field_number << 3) | wire_type) as u64);
}

fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn zigzag_encode_32(v: i32) -> u64 {
    ((v << 1) ^ (v >> 31)) as u32 as u64
}

fn zigzag_encode_64(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

// ---------------------------------------------------------------------------
// Random string generation
// ---------------------------------------------------------------------------

const WORDS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "lambda", "mu", "nu", "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi",
    "psi", "omega", "hello", "world", "test", "example", "foo", "bar", "baz", "qux",
];

fn random_string(rng: &mut StdRng) -> String {
    let word_count = rng.gen_range(1..=3);
    let words: Vec<&str> = (0..word_count)
        .map(|_| WORDS[rng.gen_range(0..WORDS.len())])
        .collect();
    words.join("_")
}
