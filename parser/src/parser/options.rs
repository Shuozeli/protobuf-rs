use super::parse_int;
use super::ParseError;
use protoc_rs_schema::*;
#[derive(Debug, Clone)]
pub(super) enum OptionValue {
    String(String),
    Int(String),
    Float(String),
    Bool(bool),
    Ident(String),
    Aggregate(String),
}

pub(super) fn option_value_to_string(v: &OptionValue) -> String {
    match v {
        OptionValue::String(s) => s.clone(),
        OptionValue::Int(s) | OptionValue::Float(s) => s.clone(),
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Ident(s) => s.clone(),
        OptionValue::Aggregate(s) => s.clone(),
    }
}

pub(super) fn expect_string_option(
    name: &str,
    value: &OptionValue,
    parent: &str,
) -> Result<String, ParseError> {
    if let OptionValue::String(s) = value {
        Ok(s.clone())
    } else {
        Err(ParseError {
            message: format!(
                "Value must be quoted string for string option \"{}.{}\".",
                parent, name
            ),
            span: Span::default(),
        })
    }
}

pub(super) fn expect_enum_option(
    name: &str,
    value: &OptionValue,
    parent: &str,
) -> Result<String, ParseError> {
    if let OptionValue::Ident(s) = value {
        Ok(s.clone())
    } else {
        Err(ParseError {
            message: format!(
                "Value must be identifier for enum-valued option \"{}.{}\".",
                parent, name
            ),
            span: Span::default(),
        })
    }
}

pub(super) fn apply_file_option(
    opts: &mut FileOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    const P: &str = "google.protobuf.FileOptions";
    match name.as_str() {
        "java_package" => opts.java_package = Some(expect_string_option(&name, &value, P)?),
        "java_outer_classname" => {
            opts.java_outer_classname = Some(expect_string_option(&name, &value, P)?)
        }
        "go_package" => opts.go_package = Some(expect_string_option(&name, &value, P)?),
        "objc_class_prefix" => {
            opts.objc_class_prefix = Some(expect_string_option(&name, &value, P)?)
        }
        "csharp_namespace" => opts.csharp_namespace = Some(expect_string_option(&name, &value, P)?),
        "swift_prefix" => opts.swift_prefix = Some(expect_string_option(&name, &value, P)?),
        "php_class_prefix" => opts.php_class_prefix = Some(expect_string_option(&name, &value, P)?),
        "php_namespace" => opts.php_namespace = Some(expect_string_option(&name, &value, P)?),
        "php_metadata_namespace" => {
            opts.php_metadata_namespace = Some(expect_string_option(&name, &value, P)?)
        }
        "ruby_package" => opts.ruby_package = Some(expect_string_option(&name, &value, P)?),
        "java_multiple_files" => {
            if let OptionValue::Bool(b) = value {
                opts.java_multiple_files = Some(b);
            }
        }
        "java_string_check_utf8" => {
            if let OptionValue::Bool(b) = value {
                opts.java_string_check_utf8 = Some(b);
            }
        }
        "java_generate_equals_and_hash" => {
            if let OptionValue::Bool(b) = value {
                opts.java_generate_equals_and_hash = Some(b);
            }
        }
        "optimize_for" => {
            let s = expect_enum_option(&name, &value, P)?;
            opts.optimize_for = match s.as_str() {
                "SPEED" => Some(OptimizeMode::Speed),
                "CODE_SIZE" => Some(OptimizeMode::CodeSize),
                "LITE_RUNTIME" => Some(OptimizeMode::LiteRuntime),
                _ => None,
            };
        }
        "cc_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.cc_generic_services = Some(b);
            }
        }
        "java_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.java_generic_services = Some(b);
            }
        }
        "py_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.py_generic_services = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "cc_enable_arenas" => {
            if let OptionValue::Bool(b) = value {
                opts.cc_enable_arenas = Some(b);
            }
        }
        _ => {
            // Unknown/custom option -- store as uninterpreted
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

pub(super) fn apply_message_option(
    opts: &mut MessageOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "message_set_wire_format" => {
            if let OptionValue::Bool(b) = value {
                opts.message_set_wire_format = Some(b);
            }
        }
        "no_standard_descriptor_accessor" => {
            if let OptionValue::Bool(b) = value {
                opts.no_standard_descriptor_accessor = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "map_entry" => {
            if let OptionValue::Bool(b) = value {
                opts.map_entry = Some(b);
                if b {
                    return Err(ParseError {
                        message: "map_entry should not be set explicitly. Use map<KeyType, ValueType> instead.".to_string(),
                        span: Span::default(),
                    });
                }
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

pub(super) fn apply_field_option(
    opts: &mut FieldOptions,
    name: &str,
    value: &OptionValue,
) -> Result<(), ParseError> {
    match name {
        "packed" => {
            if let OptionValue::Bool(b) = value {
                opts.packed = Some(*b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(*b);
            }
        }
        "lazy" => {
            if let OptionValue::Bool(b) = value {
                opts.lazy = Some(*b);
            }
        }
        "weak" => {
            if let OptionValue::Bool(b) = value {
                opts.weak = Some(*b);
            }
        }
        "jstype" => {
            let s = expect_enum_option(name, value, "google.protobuf.FieldOptions")?;
            opts.jstype = match s.as_str() {
                "JS_NORMAL" => Some(FieldJsType::JsNormal),
                "JS_STRING" => Some(FieldJsType::JsString),
                "JS_NUMBER" => Some(FieldJsType::JsNumber),
                _ => None,
            };
        }
        "ctype" => {
            let s = expect_enum_option(name, value, "google.protobuf.FieldOptions")?;
            opts.ctype = match s.as_str() {
                "STRING" => Some(FieldCType::String),
                "CORD" => Some(FieldCType::Cord),
                "STRING_PIECE" => Some(FieldCType::StringPiece),
                _ => None,
            };
        }
        "default" | "json_name" => {
            // Handled by caller, not stored in FieldOptions
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(name, value));
        }
    }
    Ok(())
}

/// Process default= and json_name= options on a field, with type validation
/// and duplicate detection. `is_group` indicates this is a group field.
pub(super) fn apply_field_special_options(
    field: &mut FieldDescriptorProto,
    opts: Vec<(String, OptionValue)>,
    is_group: bool,
) -> Result<(), ParseError> {
    let span = field.source_span.unwrap_or_default();
    let mut has_default = false;
    let mut has_json_name = false;

    for (name, value) in opts {
        if name == "default" {
            if has_default {
                return Err(ParseError {
                    message: "Already set option \"default\".".to_string(),
                    span,
                });
            }
            has_default = true;

            if is_group || field.r#type == Some(FieldType::Group) {
                return Err(ParseError {
                    message: "Messages can't have default values.".to_string(),
                    span,
                });
            }

            // Type-check the default value against the field type
            if let Some(ref ft) = field.r#type {
                validate_default_value(ft, &value, span)?;
            }

            field.default_value = Some(option_value_to_string(&value));
        } else if name == "json_name" {
            if has_json_name {
                return Err(ParseError {
                    message: "Already set option \"json_name\".".to_string(),
                    span,
                });
            }
            has_json_name = true;

            match value {
                OptionValue::String(s) => {
                    field.json_name = Some(s);
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected string for JSON name.".to_string(),
                        span,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Validate that a default value matches the expected field type.
pub(super) fn validate_default_value(
    field_type: &FieldType,
    value: &OptionValue,
    span: Span,
) -> Result<(), ParseError> {
    match field_type {
        // Signed integer types need int values with range check
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => match value {
            OptionValue::Int(s) => validate_int_range(s, i32::MIN as i64, i32::MAX as i64, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => match value {
            OptionValue::Int(s) => validate_int_range(s, i64::MIN, i64::MAX, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        // Unsigned integer types need non-negative int values with range check
        FieldType::Uint32 | FieldType::Fixed32 => match value {
            OptionValue::Int(s) if s.starts_with('-') => Err(ParseError {
                message: "Unsigned field can't have negative default value.".to_string(),
                span,
            }),
            OptionValue::Int(s) => validate_uint_range(s, u32::MAX as u64, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        FieldType::Uint64 | FieldType::Fixed64 => match value {
            OptionValue::Int(s) if s.starts_with('-') => Err(ParseError {
                message: "Unsigned field can't have negative default value.".to_string(),
                span,
            }),
            OptionValue::Int(s) => validate_uint_range(s, u64::MAX, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        // Bool needs true/false
        FieldType::Bool => match value {
            OptionValue::Bool(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected \"true\" or \"false\".".to_string(),
                span,
            }),
        },
        // String/bytes need string literal
        FieldType::String | FieldType::Bytes => match value {
            OptionValue::String(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected string for field default value.".to_string(),
                span,
            }),
        },
        // Float/double accept int, float, or special idents (inf, nan)
        FieldType::Float | FieldType::Double => match value {
            OptionValue::Float(_) | OptionValue::Int(_) => Ok(()),
            OptionValue::Ident(s) if s == "inf" || s == "nan" => Ok(()),
            _ => Err(ParseError {
                message: "Expected number for field default value.".to_string(),
                span,
            }),
        },
        // Enum default is an ident
        FieldType::Enum => match value {
            OptionValue::Ident(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected enum value for field default value.".to_string(),
                span,
            }),
        },
        // Group/message can't have defaults (handled by caller)
        _ => Ok(()),
    }
}

/// Check that a signed integer string fits in the range [min, max].
pub(super) fn validate_int_range(
    s: &str,
    min: i64,
    max: i64,
    span: Span,
) -> Result<(), ParseError> {
    let is_negative = s.starts_with('-');
    if is_negative {
        let abs_text = &s[1..];
        let abs_val = parse_int(abs_text).map_err(|_| ParseError {
            message: "Integer out of range.".to_string(),
            span,
        })?;
        // Check if -(abs_val) can be represented
        // abs_val as i64 might overflow if abs_val > i64::MAX
        if abs_val > (i64::MAX as u64) + 1 {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
        let signed = -(abs_val as i128) as i64;
        if signed < min || signed > max {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
    } else {
        let val = parse_int(s).map_err(|_| ParseError {
            message: "Integer out of range.".to_string(),
            span,
        })?;
        if val > max as u64 {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
    }
    Ok(())
}

/// Check that an unsigned integer string fits in [0, max].
pub(super) fn validate_uint_range(s: &str, max: u64, span: Span) -> Result<(), ParseError> {
    let val = parse_int(s).map_err(|_| ParseError {
        message: "Integer out of range.".to_string(),
        span,
    })?;
    if val > max {
        return Err(ParseError {
            message: "Integer out of range.".to_string(),
            span,
        });
    }
    Ok(())
}

pub(super) fn apply_enum_option(
    opts: &mut EnumOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "allow_alias" => {
            if let OptionValue::Bool(b) = value {
                opts.allow_alias = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

pub(super) fn apply_service_option(
    opts: &mut ServiceOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

pub(super) fn apply_method_option(
    opts: &mut MethodOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "idempotency_level" => {
            if let OptionValue::Ident(s) = &value {
                match s.as_str() {
                    "IDEMPOTENCY_UNKNOWN" => {
                        opts.idempotency_level = Some(IdempotencyLevel::IdempotencyUnknown)
                    }
                    "NO_SIDE_EFFECTS" => {
                        opts.idempotency_level = Some(IdempotencyLevel::NoSideEffects)
                    }
                    "IDEMPOTENT" => opts.idempotency_level = Some(IdempotencyLevel::Idempotent),
                    _ => {
                        opts.uninterpreted_option
                            .push(make_uninterpreted(&name, &value));
                    }
                }
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

/// Parse an option name string like "(baz.bar).foo" into NamePart list.
/// Parenthesized groups are extension references; bare identifiers are field names.
pub(super) fn parse_option_name_parts(name: &str) -> Vec<NamePart> {
    let mut parts = Vec::new();
    let mut rest = name;
    while !rest.is_empty() {
        // Skip leading dot separator
        if rest.starts_with('.') {
            rest = &rest[1..];
        }
        if rest.starts_with('(') {
            // Extension part: find matching ')'
            if let Some(close) = rest.find(')') {
                let inner = &rest[1..close];
                parts.push(NamePart {
                    name_part: inner.to_string(),
                    is_extension: true,
                });
                rest = &rest[close + 1..];
            } else {
                // Malformed -- treat rest as plain name
                parts.push(NamePart {
                    name_part: rest.to_string(),
                    is_extension: false,
                });
                break;
            }
        } else {
            // Plain identifier: take until next '.' or '('
            let end = rest.find(['.', '(']).unwrap_or(rest.len());
            if end > 0 {
                parts.push(NamePart {
                    name_part: rest[..end].to_string(),
                    is_extension: false,
                });
            }
            rest = &rest[end..];
        }
    }
    parts
}

pub(super) fn make_uninterpreted(name: &str, value: &OptionValue) -> UninterpretedOption {
    let name_parts = parse_option_name_parts(name);

    let mut opt = UninterpretedOption {
        name: name_parts,
        ..Default::default()
    };

    match value {
        OptionValue::String(s) => opt.string_value = Some(s.as_bytes().to_vec()),
        OptionValue::Int(s) => {
            if s.starts_with('-') {
                opt.negative_int_value = s.parse().ok();
            } else {
                opt.positive_int_value = s.parse().ok();
            }
        }
        OptionValue::Float(s) => opt.double_value = Some(s.clone()),
        OptionValue::Bool(b) => opt.identifier_value = Some(b.to_string()),
        OptionValue::Ident(s) => opt.identifier_value = Some(s.clone()),
        OptionValue::Aggregate(s) => opt.aggregate_value = Some(s.clone()),
    }

    opt
}
