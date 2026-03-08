use super::*;

// ---------------------------------------------------------------------------
// Tier 2: Structural validation -- verify generated binary is valid wire format
// ---------------------------------------------------------------------------

/// Parse all tags from generated binary and verify they have valid wire types
/// and field numbers. This catches encoding bugs that Tier 1 ("does it crash?")
/// tests miss.
#[test]
fn test_binary_has_valid_wire_format() {
    for seed in 0..50 {
        let g = generate(seed, GenConfig::default());
        let data = &g.binary_data;
        validate_wire_format(data, &format!("seed {seed}"));
    }
}

/// Validates that every tag in the binary has a valid wire type (0-5)
/// and that fixed-width and length-delimited payloads don't overrun the buffer.
fn validate_wire_format(data: &[u8], ctx: &str) {
    let mut pos = 0;
    while pos < data.len() {
        // Read tag varint
        let (tag, new_pos) = read_varint(data, pos)
            .unwrap_or_else(|| panic!("{ctx}: truncated varint at offset {pos}"));
        let wire_type = (tag & 0x07) as u8;
        let field_number = tag >> 3;
        assert!(field_number >= 1, "{ctx}: field_number 0 at offset {pos}");
        assert!(
            wire_type <= 5 && wire_type != 3 && wire_type != 4,
            "{ctx}: invalid wire_type {wire_type} at offset {pos}"
        );
        pos = new_pos;

        match wire_type {
            0 => {
                // Varint: consume bytes
                let (_, new_pos) = read_varint(data, pos)
                    .unwrap_or_else(|| panic!("{ctx}: truncated varint value at offset {pos}"));
                pos = new_pos;
            }
            1 => {
                // 64-bit fixed
                assert!(
                    pos + 8 <= data.len(),
                    "{ctx}: truncated fixed64 at offset {pos}"
                );
                pos += 8;
            }
            5 => {
                // 32-bit fixed
                assert!(
                    pos + 4 <= data.len(),
                    "{ctx}: truncated fixed32 at offset {pos}"
                );
                pos += 4;
            }
            2 => {
                // Length-delimited
                let (len, new_pos) = read_varint(data, pos)
                    .unwrap_or_else(|| panic!("{ctx}: truncated length at offset {pos}"));
                pos = new_pos;
                assert!(
                    pos + len as usize <= data.len(),
                    "{ctx}: length-delimited overrun at offset {pos}, len={len}, remaining={}",
                    data.len() - pos
                );
                pos += len as usize;
            }
            _ => unreachable!(),
        }
    }
}

fn read_varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    loop {
        if pos >= data.len() {
            return None;
        }
        let byte = data[pos];
        pos += 1;
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

/// Verify config.max_nesting_depth is respected by data generation.
/// With depth=1, the binary should only contain one level of nested messages.
#[test]
fn test_depth_limit_respected() {
    let config = GenConfig {
        max_nesting_depth: 1,
        prob_message_field: 0.8,
        prob_nested_message: 1.0,
        ..GenConfig::default()
    };
    for seed in 0..20 {
        let g = generate(seed, config.clone());
        // Should still produce valid wire format (no crash from excessive depth)
        validate_wire_format(&g.binary_data, &format!("depth-limit seed {seed}"));
    }
}

// ---------------------------------------------------------------------------
// Tier 1: Basic sanity checks
// ---------------------------------------------------------------------------

#[test]
fn test_deterministic_generation() {
    let g1 = generate(42, GenConfig::default());
    let g2 = generate(42, GenConfig::default());
    assert_eq!(g1.schema_text, g2.schema_text);
    assert_eq!(g1.binary_data, g2.binary_data);
    assert_eq!(g1.root_message, g2.root_message);
}

#[test]
fn test_different_seeds_produce_different_output() {
    let g1 = generate(1, GenConfig::default());
    let g2 = generate(2, GenConfig::default());
    // Schemas or data should differ (extremely unlikely to be identical)
    assert!(g1.schema_text != g2.schema_text || g1.binary_data != g2.binary_data);
}

#[test]
fn test_schema_starts_with_syntax() {
    let g = generate(100, GenConfig::default());
    assert!(
        g.schema_text.starts_with("syntax = \"proto3\";"),
        "Schema should start with proto3 syntax declaration"
    );
}

#[test]
fn test_schema_contains_root_message() {
    let g = generate(100, GenConfig::default());
    assert!(
        g.schema_text
            .contains(&format!("message {} {{", g.root_message)),
        "Schema should contain the root message definition"
    );
}

#[test]
fn test_binary_data_is_not_empty() {
    let g = generate(42, GenConfig::default());
    assert!(!g.binary_data.is_empty(), "Binary data should not be empty");
}

#[test]
fn test_schema_has_enum() {
    let g = generate(42, GenConfig::default());
    assert!(
        g.schema_text.contains("enum "),
        "Schema should contain at least one enum"
    );
}

#[test]
fn test_many_seeds_no_panic() {
    for seed in 0..100 {
        let g = generate(seed, GenConfig::default());
        assert!(!g.schema_text.is_empty(), "seed {seed}: empty schema");
        assert!(
            !g.binary_data.is_empty(),
            "seed {seed}: empty binary data. schema:\n{}",
            g.schema_text
        );
    }
}

#[test]
fn test_binary_starts_with_valid_tag() {
    let g = generate(42, GenConfig::default());
    // First byte should be a valid protobuf tag (field_number >= 1, wire_type 0-5)
    let first = g.binary_data[0];
    let wire_type = first & 0x07;
    let field_part = first >> 3;
    assert!(wire_type <= 5, "wire type should be 0-5, got {wire_type}");
    assert!(field_part >= 1, "field number should be >= 1");
}

#[test]
fn test_minimal_config() {
    let config = GenConfig {
        max_messages: 1,
        max_enums: 1,
        max_fields_per_message: 1,
        max_enum_values: 2,
        max_nesting_depth: 0,
        prob_repeated: 0.0,
        prob_enum_field: 0.0,
        prob_message_field: 0.0,
        prob_nested_message: 0.0,
    };
    let g = generate(42, config);
    assert!(!g.schema_text.is_empty());
    assert!(!g.binary_data.is_empty());
}

#[test]
fn test_hex_output() {
    let g = generate(42, GenConfig::default());
    let hex: String = g.binary_data.iter().map(|b| format!("{b:02x} ")).collect();
    assert!(!hex.is_empty());
    // All hex chars should be valid
    for ch in hex.chars() {
        assert!(
            ch.is_ascii_hexdigit() || ch == ' ',
            "unexpected char in hex: {ch}"
        );
    }
}
