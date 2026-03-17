use crate::AnalyzeError;
use protoc_rs_schema::*;
use std::collections::{HashMap, HashSet};

const MIN_FIELD_NUMBER: i32 = 1;
const MAX_FIELD_NUMBER: i32 = 536870911; // 2^29 - 1
const RESERVED_RANGE_START: i32 = 19000;
const RESERVED_RANGE_END: i32 = 19999;

fn make_err(message: String, file_name: &str, span: Option<Span>) -> AnalyzeError {
    AnalyzeError {
        message,
        file: Some(file_name.to_string()),
        span,
    }
}

trait RangeFields {
    fn start_val(&self) -> i32;
    fn end_val(&self) -> i32;
}

impl RangeFields for ReservedRange {
    fn start_val(&self) -> i32 { self.start.unwrap_or(0) }
    fn end_val(&self) -> i32 { self.end.unwrap_or(0) }
}

impl RangeFields for ExtensionRange {
    fn start_val(&self) -> i32 { self.start.unwrap_or(0) }
    fn end_val(&self) -> i32 { self.end.unwrap_or(0) }
}

fn check_exclusive_range_overlaps<R: RangeFields>(
    ranges: &[R],
    kind: &str,
    file_name: &str,
    span: Option<Span>,
) -> Result<(), AnalyzeError> {
    for (i, r1) in ranges.iter().enumerate() {
        let (s1, e1) = (r1.start_val(), r1.end_val());
        for r2 in &ranges[i + 1..] {
            let (s2, e2) = (r2.start_val(), r2.end_val());
            if s1 < e2 && s2 < e1 {
                return Err(make_err(
                    format!(
                        "{kind} range {} to {} overlaps with already-defined range {} to {}.",
                        s2, e2 - 1, s1, e1 - 1,
                    ),
                    file_name,
                    span,
                ));
            }
        }
    }
    Ok(())
}

fn check_cross_range_overlaps<A: RangeFields, B: RangeFields>(
    a_ranges: &[A],
    a_kind: &str,
    b_ranges: &[B],
    b_kind: &str,
    file_name: &str,
    span: Option<Span>,
) -> Result<(), AnalyzeError> {
    for a in a_ranges {
        let (as_, ae) = (a.start_val(), a.end_val());
        for b in b_ranges {
            let (bs, be) = (b.start_val(), b.end_val());
            if as_ < be && bs < ae {
                return Err(make_err(
                    format!(
                        "{a_kind} range {} to {} overlaps with {b_kind} range {} to {}.",
                        as_, ae - 1, bs, be - 1,
                    ),
                    file_name,
                    span,
                ));
            }
        }
    }
    Ok(())
}

/// Validate a single FileDescriptorProto.
pub fn validate_file(file: &FileDescriptorProto) -> Result<(), AnalyzeError> {
    let is_proto3 = file.syntax.as_deref() == Some("proto3");
    let file_name = file.name.as_deref().unwrap_or("<unknown>");

    // Check for duplicate imports (across both dependency and option_dependency)
    {
        let mut seen_imports: HashSet<&str> = HashSet::new();
        for dep in &file.dependency {
            if !seen_imports.insert(dep.as_str()) {
                return Err(AnalyzeError {
                    message: format!("Import \"{}\" was listed twice.", dep),
                    file: Some(file_name.to_string()),
                    span: None,
                });
            }
        }
        for dep in &file.option_dependency {
            if !seen_imports.insert(dep.as_str()) {
                return Err(AnalyzeError {
                    message: format!("Import \"{}\" was listed twice.", dep),
                    file: Some(file_name.to_string()),
                    span: None,
                });
            }
        }
    }

    // Check for duplicate top-level names (messages, enums, services)
    {
        let mut top_names: HashSet<&str> = HashSet::new();
        for msg in &file.message_type {
            if let Some(ref name) = msg.name {
                if !top_names.insert(name.as_str()) {
                    return Err(AnalyzeError {
                        message: format!("\"{}\" is already defined.", name),
                        file: Some(file_name.to_string()),
                        span: msg.source_span,
                    });
                }
            }
        }
        for e in &file.enum_type {
            if let Some(ref name) = e.name {
                if !top_names.insert(name.as_str()) {
                    return Err(AnalyzeError {
                        message: format!("\"{}\" is already defined.", name),
                        file: Some(file_name.to_string()),
                        span: e.source_span,
                    });
                }
            }
        }
        for svc in &file.service {
            if let Some(ref name) = svc.name {
                if !top_names.insert(name.as_str()) {
                    return Err(AnalyzeError {
                        message: format!("\"{}\" is already defined.", name),
                        file: Some(file_name.to_string()),
                        span: None,
                    });
                }
            }
        }
    }

    // Validate top-level messages
    for msg in &file.message_type {
        validate_message(msg, is_proto3, file_name)?;
    }

    // Validate top-level enums
    for e in &file.enum_type {
        validate_enum(e, is_proto3, file_name)?;
    }

    // Validate services
    for svc in &file.service {
        validate_service(svc, file_name)?;
    }

    // Validate top-level extensions
    for ext in &file.extension {
        validate_extension_field(ext, is_proto3, file_name)?;
    }

    // Proto3: no extensions
    if is_proto3 && !file.extension.is_empty() {
        return Err(AnalyzeError {
            message: "Extensions in proto3 are only allowed for defining options.".to_string(),
            file: Some(file_name.to_string()),
            span: None,
        });
    }

    // Validate file-level options
    if let Some(ref opts) = file.options {
        validate_file_options(opts, file_name)?;
    }

    Ok(())
}

fn validate_message(
    msg: &DescriptorProto,
    is_proto3: bool,
    file_name: &str,
) -> Result<(), AnalyzeError> {
    let msg_name = msg.name.as_deref().unwrap_or("<unknown>");

    // Check for duplicate field names
    {
        let mut seen_names: HashSet<&str> = HashSet::new();
        for field in &msg.field {
            if let Some(ref name) = field.name {
                if !seen_names.insert(name.as_str()) {
                    return Err(AnalyzeError {
                        message: format!("\"{}\" is already defined in \"{}\".", name, msg_name),
                        file: Some(file_name.to_string()),
                        span: field.source_span,
                    });
                }
            }
        }
    }

    // Check for duplicate field numbers
    let mut seen_numbers: HashSet<i32> = HashSet::new();
    for field in &msg.field {
        if let Some(num) = field.number {
            // Validate field number range
            if !(MIN_FIELD_NUMBER..=MAX_FIELD_NUMBER).contains(&num) {
                return Err(AnalyzeError {
                    message: format!(
                        "field number {} in {}.{} is out of valid range ({}-{})",
                        num,
                        msg_name,
                        field.name.as_deref().unwrap_or("?"),
                        MIN_FIELD_NUMBER,
                        MAX_FIELD_NUMBER
                    ),
                    file: Some(file_name.to_string()),
                    span: field.source_span,
                });
            }

            // Check reserved implementation range
            if (RESERVED_RANGE_START..=RESERVED_RANGE_END).contains(&num) {
                return Err(AnalyzeError {
                    message: format!(
                        "field number {} in {}.{} is in the reserved range {}-{}",
                        num,
                        msg_name,
                        field.name.as_deref().unwrap_or("?"),
                        RESERVED_RANGE_START,
                        RESERVED_RANGE_END
                    ),
                    file: Some(file_name.to_string()),
                    span: field.source_span,
                });
            }

            if !seen_numbers.insert(num) {
                return Err(AnalyzeError {
                    message: format!("duplicate field number {} in message {}", num, msg_name),
                    file: Some(file_name.to_string()),
                    span: field.source_span,
                });
            }

            // Check against reserved ranges
            for range in &msg.reserved_range {
                let start = range.start.unwrap_or(0);
                let end = range.end.unwrap_or(0); // exclusive
                if num >= start && num < end {
                    return Err(AnalyzeError {
                        message: format!(
                            "field number {} in {}.{} conflicts with reserved range {}-{}",
                            num,
                            msg_name,
                            field.name.as_deref().unwrap_or("?"),
                            start,
                            end - 1
                        ),
                        file: Some(file_name.to_string()),
                        span: field.source_span,
                    });
                }
            }

            // Check against reserved names
            if let Some(ref field_name) = field.name {
                if msg.reserved_name.contains(field_name) {
                    return Err(AnalyzeError {
                        message: format!(
                            "field name '{}' in message {} conflicts with reserved name",
                            field_name, msg_name
                        ),
                        file: Some(file_name.to_string()),
                        span: field.source_span,
                    });
                }
            }
        }

        // Proto3: no required fields
        if is_proto3 && field.label == Some(FieldLabel::Required) {
            return Err(AnalyzeError {
                message: format!(
                    "required fields are not allowed in proto3 (field {}.{})",
                    msg_name,
                    field.name.as_deref().unwrap_or("?")
                ),
                file: Some(file_name.to_string()),
                span: field.source_span,
            });
        }

        // Proto3: no explicit default values
        if is_proto3 && field.default_value.is_some() {
            return Err(AnalyzeError {
                message: format!(
                    "explicit default values are not allowed in proto3 (field {}.{})",
                    msg_name,
                    field.name.as_deref().unwrap_or("?")
                ),
                file: Some(file_name.to_string()),
                span: field.source_span,
            });
        }

        // Validate field-level options
        validate_field_options(field, file_name)?;
    }

    // Validate extension ranges
    for range in &msg.extension_range {
        let start = range.start.unwrap_or(0);
        let end = range.end.unwrap_or(0);
        if start < MIN_FIELD_NUMBER {
            return Err(AnalyzeError {
                message: "Extension numbers must be positive integers.".to_string(),
                file: Some(file_name.to_string()),
                span: msg.source_span,
            });
        }
        if end <= start {
            return Err(AnalyzeError {
                message: "Extension range end number must be greater than start number."
                    .to_string(),
                file: Some(file_name.to_string()),
                span: msg.source_span,
            });
        }
    }

    // Proto3: no extension ranges
    if is_proto3 && !msg.extension_range.is_empty() {
        return Err(AnalyzeError {
            message: "Extension ranges are not allowed in proto3.".to_string(),
            file: Some(file_name.to_string()),
            span: msg.source_span,
        });
    }

    // Validate reserved ranges (end must be > start, no overlaps)
    for range in &msg.reserved_range {
        let start = range.start.unwrap_or(0);
        let end = range.end.unwrap_or(0);
        if end <= start {
            return Err(AnalyzeError {
                message: "Reserved range end number must be greater than start number.".to_string(),
                file: Some(file_name.to_string()),
                span: msg.source_span,
            });
        }
    }
    check_exclusive_range_overlaps(&msg.reserved_range, "Reserved", file_name, msg.source_span)?;
    check_exclusive_range_overlaps(&msg.extension_range, "Extension", file_name, msg.source_span)?;
    check_cross_range_overlaps(
        &msg.extension_range, "Extension",
        &msg.reserved_range, "reserved",
        file_name, msg.source_span,
    )?;

    // Check for fields whose numbers fall in an extension range
    for field in &msg.field {
        if let Some(num) = field.number {
            for range in &msg.extension_range {
                let start = range.start.unwrap_or(0);
                let end = range.end.unwrap_or(0);
                if num >= start && num < end {
                    return Err(AnalyzeError {
                        message: format!(
                            "Extension range {} to {} includes field \"{}\" ({}).",
                            start,
                            end - 1,
                            field.name.as_deref().unwrap_or("?"),
                            num
                        ),
                        file: Some(file_name.to_string()),
                        span: field.source_span,
                    });
                }
            }
        }
    }

    // Check for deprecated_legacy_json_field_conflicts option (suppresses JSON conflict checks)
    let has_legacy_json_flag = msg.options.as_ref().is_some_and(|opts| {
        opts.uninterpreted_option.iter().any(|o| {
            o.name.len() == 1
                && o.name[0].name_part == "deprecated_legacy_json_field_conflicts"
                && !o.name[0].is_extension
                && o.identifier_value.as_deref() == Some("true")
        })
    });

    if !has_legacy_json_flag {
        // Proto3: check JSON name conflicts
        if is_proto3 {
            validate_json_name_conflicts(&msg.field, msg_name, file_name, true)?;
        } else {
            // Proto2: only check custom json_name conflicts (not default name conflicts)
            validate_json_name_conflicts(&msg.field, msg_name, file_name, false)?;
        }
    }

    // Check for explicit map_entry option set via uninterpreted_option
    for nested in &msg.nested_type {
        if let Some(ref opts) = nested.options {
            if opts
                .uninterpreted_option
                .iter()
                .any(|o| o.name.iter().any(|n| n.name_part == "map_entry"))
            {
                return Err(AnalyzeError {
                    message: "map_entry should not be set explicitly. Use map<KeyType, ValueType> instead.".to_string(),
                    file: Some(file_name.to_string()),
                    span: nested.source_span,
                });
            }
        }
    }

    // Validate oneofs: must have at least one field
    for oneof in &msg.oneof_decl {
        let oneof_name = oneof.name.as_deref().unwrap_or("<unknown>");
        let has_fields = msg.field.iter().any(|f| {
            f.oneof_index.is_some()
                && msg.oneof_decl.iter().position(|o| o.name == oneof.name)
                    == f.oneof_index.map(|i| i as usize)
        });
        if !has_fields {
            return Err(AnalyzeError {
                message: format!(
                    "Oneof must have at least one field (oneof \"{}\" in \"{}\").",
                    oneof_name, msg_name
                ),
                file: Some(file_name.to_string()),
                span: msg.source_span,
            });
        }
    }

    // Proto3: no message_set_wire_format
    if is_proto3 {
        if let Some(ref opts) = msg.options {
            if opts.message_set_wire_format == Some(true) {
                return Err(AnalyzeError {
                    message: "MessageSet is not supported in proto3.".to_string(),
                    file: Some(file_name.to_string()),
                    span: msg.source_span,
                });
            }
        }
    }

    // Validate map entry key types
    for nested in &msg.nested_type {
        let is_map_entry = nested
            .options
            .as_ref()
            .and_then(|o| o.map_entry)
            .unwrap_or(false);
        if is_map_entry {
            if let Some(key_field) = nested.field.first() {
                let bad_type = match key_field.r#type {
                    Some(FieldType::Float | FieldType::Double) => Some("float/double"),
                    Some(FieldType::Bytes) => Some("bytes"),
                    Some(FieldType::Message | FieldType::Group) => Some("message"),
                    Some(FieldType::Enum) => Some("enum"),
                    None if key_field.type_name.is_some() => Some("message or enum"),
                    _ => None,
                };
                if let Some(type_name) = bad_type {
                    return Err(make_err(
                        format!("Key in map fields cannot be {type_name} types."),
                        file_name,
                        key_field.source_span,
                    ));
                }
            }
        }
    }

    // Validate nested messages
    for nested in &msg.nested_type {
        // Skip map entry synthetic messages
        if nested
            .options
            .as_ref()
            .and_then(|o| o.map_entry)
            .unwrap_or(false)
        {
            continue;
        }
        validate_message(nested, is_proto3, file_name)?;
    }

    // Validate nested enums
    for e in &msg.enum_type {
        validate_enum(e, is_proto3, file_name)?;
    }

    Ok(())
}

fn validate_enum(
    e: &EnumDescriptorProto,
    is_proto3: bool,
    file_name: &str,
) -> Result<(), AnalyzeError> {
    let enum_name = e.name.as_deref().unwrap_or("<unknown>");

    // Proto3: first enum value must be 0
    if is_proto3 {
        if let Some(first) = e.value.first() {
            if first.number != Some(0) {
                return Err(AnalyzeError {
                    message: format!(
                        "first enum value of {} must be 0 in proto3 (got {:?})",
                        enum_name, first.number
                    ),
                    file: Some(file_name.to_string()),
                    span: e.source_span,
                });
            }
        }
    }

    // Check for duplicate enum value names
    {
        let mut seen_names: HashSet<&str> = HashSet::new();
        for val in &e.value {
            if let Some(ref name) = val.name {
                if !seen_names.insert(name.as_str()) {
                    return Err(AnalyzeError {
                        message: format!("\"{}\" is already defined.", name),
                        file: Some(file_name.to_string()),
                        span: val.source_span,
                    });
                }
            }
        }
    }

    // Check for duplicate enum values (unless allow_alias)
    let allow_alias = e
        .options
        .as_ref()
        .and_then(|o| o.allow_alias)
        .unwrap_or(false);

    if !allow_alias {
        let mut seen_values: HashMap<i32, &str> = HashMap::new();
        for val in &e.value {
            if let Some(num) = val.number {
                let val_name = val.name.as_deref().unwrap_or("?");
                if let Some(existing) = seen_values.get(&num) {
                    return Err(AnalyzeError {
                        message: format!(
                            "\"{}\" uses the same enum value as \"{}\". If this is intended, \
                             set 'option allow_alias = true;' to the enum definition.",
                            val_name, existing
                        ),
                        file: Some(file_name.to_string()),
                        span: val.source_span,
                    });
                }
                seen_values.insert(num, val_name);
            }
        }
    }

    // Validate enum reserved ranges (end must be >= start; enum ranges use inclusive end)
    for range in &e.reserved_range {
        let start = range.start.unwrap_or(0);
        let end = range.end.unwrap_or(0);
        if end < start {
            return Err(AnalyzeError {
                message: "Reserved range end number must be greater than start number.".to_string(),
                file: Some(file_name.to_string()),
                span: e.source_span,
            });
        }
    }

    // Check for overlapping enum reserved ranges (inclusive end)
    for (i, r1) in e.reserved_range.iter().enumerate() {
        let s1 = r1.start.unwrap_or(0);
        let e1 = r1.end.unwrap_or(0);
        for r2 in &e.reserved_range[i + 1..] {
            let s2 = r2.start.unwrap_or(0);
            let e2 = r2.end.unwrap_or(0);
            // Inclusive ranges overlap if s1 <= e2 && s2 <= e1
            if s1 <= e2 && s2 <= e1 {
                return Err(AnalyzeError {
                    message: format!(
                        "Reserved range {} to {} overlaps with already-defined range {} to {}.",
                        s2, e2, s1, e1
                    ),
                    file: Some(file_name.to_string()),
                    span: e.source_span,
                });
            }
        }
    }

    // Check enum values against reserved numbers (inclusive end)
    for val in &e.value {
        if let Some(num) = val.number {
            for range in &e.reserved_range {
                let start = range.start.unwrap_or(0);
                let end = range.end.unwrap_or(0);
                if num >= start && num <= end {
                    return Err(AnalyzeError {
                        message: format!(
                            "Enum value \"{}\" uses reserved number {}.",
                            val.name.as_deref().unwrap_or("?"),
                            num
                        ),
                        file: Some(file_name.to_string()),
                        span: val.source_span,
                    });
                }
            }
        }
    }

    // Check enum values against reserved names
    for val in &e.value {
        if let Some(ref val_name) = val.name {
            if e.reserved_name.contains(val_name) {
                return Err(AnalyzeError {
                    message: format!(
                        "Enum value \"{}\" is using reserved name \"{}\".",
                        val_name, val_name
                    ),
                    file: Some(file_name.to_string()),
                    span: val.source_span,
                });
            }
        }
    }

    // Check for redundant (duplicate) reserved names
    {
        let mut seen_reserved: HashSet<&str> = HashSet::new();
        for name in &e.reserved_name {
            if !seen_reserved.insert(name.as_str()) {
                return Err(AnalyzeError {
                    message: format!("Enum value \"{}\" is reserved multiple times.", name),
                    file: Some(file_name.to_string()),
                    span: e.source_span,
                });
            }
        }
    }

    Ok(())
}

fn validate_service(svc: &ServiceDescriptorProto, file_name: &str) -> Result<(), AnalyzeError> {
    let svc_name = svc.name.as_deref().unwrap_or("<unknown>");

    // Check for duplicate method names
    let mut seen_names: HashSet<&str> = HashSet::new();
    for method in &svc.method {
        if let Some(ref name) = method.name {
            if !seen_names.insert(name.as_str()) {
                return Err(AnalyzeError {
                    message: format!("\"{}\" is already defined in \"{}\".", name, svc_name),
                    file: Some(file_name.to_string()),
                    span: None,
                });
            }
        }
    }

    Ok(())
}

fn validate_extension_field(
    ext: &FieldDescriptorProto,
    _is_proto3: bool,
    file_name: &str,
) -> Result<(), AnalyzeError> {
    // json_name is not allowed on extension fields.
    // The parser auto-generates json_name from field name (to_camel_case).
    // An explicit json_name is one that differs from the auto-generated default.
    if let (Some(ref json_name), Some(ref field_name)) = (&ext.json_name, &ext.name) {
        let auto = default_json_name(field_name);
        if *json_name != auto {
            return Err(AnalyzeError {
                message: "option json_name is not allowed on extension fields.".to_string(),
                file: Some(file_name.to_string()),
                span: ext.source_span,
            });
        }
    }

    Ok(())
}

/// Compute the default JSON name for a protobuf field name.
/// Converts snake_case to lowerCamelCase.
fn default_json_name(proto_name: &str) -> String {
    let mut result = String::with_capacity(proto_name.len());
    let mut capitalize_next = false;
    for ch in proto_name.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Validate JSON name conflicts within a message's fields.
/// In proto3: both default and custom JSON name conflicts are errors.
/// In proto2: only custom JSON name conflicts are errors (not default name conflicts).
fn validate_json_name_conflicts(
    fields: &[FieldDescriptorProto],
    _msg_name: &str,
    file_name: &str,
    check_default_conflicts: bool,
) -> Result<(), AnalyzeError> {
    // Check if deprecated_legacy_json_field_conflicts is set
    // (suppresses JSON conflict checks entirely)
    // We skip this check for now since it's stored as uninterpreted option

    // Map from JSON name -> (field name, is_custom)
    let mut json_names: HashMap<String, (&str, bool)> = HashMap::new();

    for field in fields {
        let field_name = field.name.as_deref().unwrap_or("?");
        let auto_json = default_json_name(field_name);
        // A custom json_name is one explicitly set by the user (differs from auto-generated)
        let has_custom_json = field.json_name.as_ref().is_some_and(|jn| *jn != auto_json);
        let json_name = field.json_name.as_ref().cloned().unwrap_or(auto_json);

        if let Some((existing_field, existing_is_custom)) = json_names.get(&json_name) {
            let both_custom = has_custom_json && *existing_is_custom;

            // In proto2, only custom-vs-custom conflicts are errors.
            // Default-vs-default and custom-vs-default are warnings in C++ protoc
            // (we don't emit warnings, so we skip them).
            if !check_default_conflicts && !both_custom {
                continue;
            }

            let description = match (existing_is_custom, has_custom_json) {
                (true, true) => format!(
                    "The custom JSON name of field \"{}\" (\"{}\") conflicts with \
                     the custom JSON name of field \"{}\".",
                    field_name, json_name, existing_field
                ),
                (true, false) => format!(
                    "The default JSON name of field \"{}\" (\"{}\") conflicts with \
                     the custom JSON name of field \"{}\".",
                    field_name, json_name, existing_field
                ),
                (false, true) => format!(
                    "The default JSON name of field \"{}\" (\"{}\") conflicts with \
                     the custom JSON name of field \"{}\".",
                    existing_field, json_name, field_name
                ),
                (false, false) => format!(
                    "The default JSON name of field \"{}\" (\"{}\") conflicts with \
                     the default JSON name of field \"{}\".",
                    field_name, json_name, existing_field
                ),
            };

            return Err(AnalyzeError {
                message: description,
                file: Some(file_name.to_string()),
                span: field.source_span,
            });
        }

        json_names.insert(json_name, (field_name, has_custom_json));
    }

    Ok(())
}
/// Known file option names and their expected types.
const KNOWN_FILE_OPTIONS: &[(&str, OptionType)] = &[
    ("java_package", OptionType::String),
    ("java_outer_classname", OptionType::String),
    ("java_multiple_files", OptionType::Bool),
    ("java_generate_equals_and_hash", OptionType::Bool),
    ("java_string_check_utf8", OptionType::Bool),
    ("optimize_for", OptionType::Enum),
    ("go_package", OptionType::String),
    ("cc_generic_services", OptionType::Bool),
    ("java_generic_services", OptionType::Bool),
    ("py_generic_services", OptionType::Bool),
    ("deprecated", OptionType::Bool),
    ("cc_enable_arenas", OptionType::Bool),
    ("objc_class_prefix", OptionType::String),
    ("csharp_namespace", OptionType::String),
    ("swift_prefix", OptionType::String),
    ("php_class_prefix", OptionType::String),
    ("php_namespace", OptionType::String),
    ("php_metadata_namespace", OptionType::String),
    ("ruby_package", OptionType::String),
];

/// Known field option names and their expected types.
const KNOWN_FIELD_OPTIONS: &[(&str, OptionType)] = &[
    ("ctype", OptionType::Enum),
    ("packed", OptionType::Bool),
    ("jstype", OptionType::Enum),
    ("lazy", OptionType::Bool),
    ("deprecated", OptionType::Bool),
    ("weak", OptionType::Bool),
];

#[derive(Clone, Copy, PartialEq)]
enum OptionType {
    String,
    Bool,
    Enum,
}

fn validate_file_options(opts: &FileOptions, file_name: &str) -> Result<(), AnalyzeError> {
    for uopt in &opts.uninterpreted_option {
        if uopt.name.len() == 1 && !uopt.name[0].is_extension {
            let name = &uopt.name[0].name_part;
            validate_known_option(
                name,
                uopt,
                KNOWN_FILE_OPTIONS,
                "google.protobuf.FileOptions",
                file_name,
            )?;
        }
    }
    Ok(())
}

pub fn validate_field_options(
    field: &FieldDescriptorProto,
    file_name: &str,
) -> Result<(), AnalyzeError> {
    if let Some(ref opts) = field.options {
        for uopt in &opts.uninterpreted_option {
            if uopt.name.len() == 1 && !uopt.name[0].is_extension {
                let name = &uopt.name[0].name_part;
                validate_known_option(
                    name,
                    uopt,
                    KNOWN_FIELD_OPTIONS,
                    "google.protobuf.FieldOptions",
                    file_name,
                )?;
            }
        }
    }
    Ok(())
}

fn validate_known_option(
    name: &str,
    uopt: &UninterpretedOption,
    known: &[(&str, OptionType)],
    parent_type: &str,
    file_name: &str,
) -> Result<(), AnalyzeError> {
    if let Some((_, expected_type)) = known.iter().find(|(n, _)| *n == name) {
        // Known option -- validate value type
        match expected_type {
            OptionType::String => {
                if uopt.string_value.is_none() && uopt.identifier_value.is_none() {
                    return Err(AnalyzeError {
                        message: format!(
                            "Value must be quoted string for string option \"{}.{}\".",
                            parent_type, name
                        ),
                        file: Some(file_name.to_string()),
                        span: None,
                    });
                }
            }
            OptionType::Enum => {
                if uopt.identifier_value.is_none() {
                    return Err(AnalyzeError {
                        message: format!(
                            "Value must be identifier for enum-valued option \"{}.{}\".",
                            parent_type, name
                        ),
                        file: Some(file_name.to_string()),
                        span: None,
                    });
                }
            }
            OptionType::Bool => {
                // Bool options accept identifier "true"/"false"
                // Already validated by parser typically
            }
        }
    } else {
        // Unknown option
        return Err(AnalyzeError {
            message: format!(
                "Option \"{}\" unknown. Ensure that your proto definition file \
                 imports the proto which defines the option.",
                name
            ),
            file: Some(file_name.to_string()),
            span: None,
        });
    }
    Ok(())
}
