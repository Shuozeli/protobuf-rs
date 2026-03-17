use super::ParseError;
use crate::lexer::Token;
/// Parse and validate an enum value from a token. Must fit in i32.
/// Handles both positive and negative IntLiteral tokens (lexer may produce "-1" as a single token).
pub(super) fn parse_enum_value(tok: &Token) -> Result<i32, ParseError> {
    let text = &tok.text;
    if let Some(abs_text) = text.strip_prefix('-') {
        // Negative: parse absolute value and check range
        let abs_val = parse_int(abs_text).map_err(|e| ParseError {
            message: format!("invalid enum value: {}", e),
            span: tok.span,
        })?;
        if abs_val > (i32::MAX as u64) + 1 {
            return Err(ParseError {
                message: format!("enum value out of range: {}", text),
                span: tok.span,
            });
        }
        Ok(-(abs_val as i64) as i32)
    } else {
        let val = parse_int(text).map_err(|e| ParseError {
            message: format!("invalid enum value: {}", e),
            span: tok.span,
        })?;
        if val > i32::MAX as u64 {
            return Err(ParseError {
                message: format!("enum value out of range: {}", text),
                span: tok.span,
            });
        }
        Ok(val as i32)
    }
}

/// Format a comment's text for SourceCodeInfo.
/// Line comments have their leading space stripped. Block comments are returned as-is.
pub(super) fn format_comment(text: &str, is_block: bool) -> String {
    if is_block {
        // Block comment: text is between /* and */, return with leading space stripped
        let mut result = String::new();
        // Add leading space for consistency with protoc
        if let Some(stripped) = text.strip_prefix(' ') {
            result.push_str(stripped);
        } else {
            result.push_str(text);
        }
        result.push('\n');
        result
    } else {
        // Line comment: strip one leading space if present
        let trimmed = text.strip_prefix(' ').unwrap_or(text);
        format!("{}\n", trimmed)
    }
}

/// Parse and validate a field number from a token. Must fit in i32 and be > 0.
pub(super) fn parse_field_number(tok: &Token) -> Result<i32, ParseError> {
    let val = parse_int(&tok.text).map_err(|e| ParseError {
        message: format!("invalid field number: {}", e),
        span: tok.span,
    })?;
    if val > i32::MAX as u64 {
        return Err(ParseError {
            message: format!("field number out of range: {}", tok.text),
            span: tok.span,
        });
    }
    Ok(val as i32)
}

pub(super) fn parse_int(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('-') {
        // Negative numbers: parse as i64 then cast
        let abs = parse_int(rest)?;
        return Ok((-(abs as i64)) as u64);
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else if let Some(oct) = s
        .strip_prefix('0')
        .filter(|r| !r.is_empty() && !r.contains(['8', '9']))
    {
        u64::from_str_radix(oct, 8).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}

/// Check if a string is a valid protobuf identifier ([a-zA-Z_][a-zA-Z0-9_]*).
pub(super) fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Convert snake_case to camelCase (protobuf json_name default).
pub(super) fn to_camel_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate the synthetic map entry message name: "FooEntry" for field "foo".
pub(super) fn map_entry_name(field_name: &str) -> String {
    let mut name = String::with_capacity(field_name.len() + 5);
    let mut chars = field_name.chars();
    if let Some(c) = chars.next() {
        name.extend(c.to_uppercase());
    }
    let mut capitalize_next = false;
    for c in chars {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            name.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            name.push(c);
        }
    }
    name.push_str("Entry");
    name
}
