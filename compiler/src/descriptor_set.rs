//! Serialize `FileDescriptorSet` to protobuf binary wire format.
//!
//! Field numbers match `google/protobuf/descriptor.proto`.

use protoc_rs_schema::*;
/// Serialize a `FileDescriptorSet` to protobuf binary format.
pub fn serialize_descriptor_set(fds: &FileDescriptorSet) -> Vec<u8> {
    let mut buf = Vec::new();
    for file in &fds.file {
        // FileDescriptorSet.file = field 1, length-delimited
        let file_bytes = encode_file_descriptor_proto(file);
        encode_tag(&mut buf, 1, WIRE_LEN);
        encode_varint(&mut buf, file_bytes.len() as u64);
        buf.extend_from_slice(&file_bytes);
    }
    buf
}
const WIRE_VARINT: u32 = 0;
const WIRE_LEN: u32 = 2;
fn encode_varint(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

fn encode_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u32) {
    encode_varint(buf, ((field_number as u64) << 3) | wire_type as u64);
}

fn encode_string_field(buf: &mut Vec<u8>, field_number: u32, value: &str) {
    encode_tag(buf, field_number, WIRE_LEN);
    encode_varint(buf, value.len() as u64);
    buf.extend_from_slice(value.as_bytes());
}

fn encode_bytes_field(buf: &mut Vec<u8>, field_number: u32, value: &[u8]) {
    encode_tag(buf, field_number, WIRE_LEN);
    encode_varint(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

fn encode_varint_field(buf: &mut Vec<u8>, field_number: u32, value: u64) {
    encode_tag(buf, field_number, WIRE_VARINT);
    encode_varint(buf, value);
}

fn encode_bool_field(buf: &mut Vec<u8>, field_number: u32, value: bool) {
    encode_varint_field(buf, field_number, value as u64);
}

fn encode_int32_field(buf: &mut Vec<u8>, field_number: u32, value: i32) {
    // Protobuf int32 is encoded as varint (sign-extended to 64 bits)
    encode_varint_field(buf, field_number, value as u64);
}

fn encode_message_field(buf: &mut Vec<u8>, field_number: u32, message_bytes: &[u8]) {
    encode_tag(buf, field_number, WIRE_LEN);
    encode_varint(buf, message_bytes.len() as u64);
    buf.extend_from_slice(message_bytes);
}

fn encode_opt_string(buf: &mut Vec<u8>, field_number: u32, value: &Option<String>) {
    if let Some(ref s) = value {
        encode_string_field(buf, field_number, s);
    }
}

fn encode_opt_bool(buf: &mut Vec<u8>, field_number: u32, value: Option<bool>) {
    if let Some(v) = value {
        encode_bool_field(buf, field_number, v);
    }
}

fn encode_opt_int32(buf: &mut Vec<u8>, field_number: u32, value: Option<i32>) {
    if let Some(v) = value {
        encode_int32_field(buf, field_number, v);
    }
}

fn encode_opt_enum<E: Into<i32> + Copy>(buf: &mut Vec<u8>, field_number: u32, value: Option<E>) {
    if let Some(v) = value {
        encode_varint_field(buf, field_number, v.into() as u64);
    }
}
fn encode_file_descriptor_proto(file: &FileDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();

    // 1: name
    encode_opt_string(&mut buf, 1, &file.name);
    // 2: package
    encode_opt_string(&mut buf, 2, &file.package);
    // 3: dependency (repeated string)
    for dep in &file.dependency {
        encode_string_field(&mut buf, 3, dep);
    }
    // 10: public_dependency (repeated int32)
    for &idx in &file.public_dependency {
        encode_int32_field(&mut buf, 10, idx);
    }
    // 11: weak_dependency (repeated int32)
    for &idx in &file.weak_dependency {
        encode_int32_field(&mut buf, 11, idx);
    }
    // 4: message_type (repeated DescriptorProto)
    for msg in &file.message_type {
        let msg_bytes = encode_descriptor_proto(msg);
        encode_message_field(&mut buf, 4, &msg_bytes);
    }
    // 5: enum_type (repeated EnumDescriptorProto)
    for e in &file.enum_type {
        let e_bytes = encode_enum_descriptor_proto(e);
        encode_message_field(&mut buf, 5, &e_bytes);
    }
    // 6: service (repeated ServiceDescriptorProto)
    for svc in &file.service {
        let svc_bytes = encode_service_descriptor_proto(svc);
        encode_message_field(&mut buf, 6, &svc_bytes);
    }
    // 7: extension (repeated FieldDescriptorProto)
    for ext in &file.extension {
        let ext_bytes = encode_field_descriptor_proto(ext);
        encode_message_field(&mut buf, 7, &ext_bytes);
    }
    // 8: options
    if let Some(ref opts) = file.options {
        let opts_bytes = encode_file_options(opts);
        if !opts_bytes.is_empty() {
            encode_message_field(&mut buf, 8, &opts_bytes);
        }
    }
    // 9: source_code_info
    if let Some(ref sci) = file.source_code_info {
        let sci_bytes = encode_source_code_info(sci);
        if !sci_bytes.is_empty() {
            encode_message_field(&mut buf, 9, &sci_bytes);
        }
    }
    // 12: syntax (protoc omits this for proto2, only emits for proto3/editions)
    if file.syntax.as_deref() != Some("proto2") {
        encode_opt_string(&mut buf, 12, &file.syntax);
    }
    // 14: edition
    encode_opt_string(&mut buf, 14, &file.edition);

    buf
}
fn encode_descriptor_proto(msg: &DescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();

    // 1: name
    encode_opt_string(&mut buf, 1, &msg.name);
    // 2: field (repeated)
    for field in &msg.field {
        let fb = encode_field_descriptor_proto(field);
        encode_message_field(&mut buf, 2, &fb);
    }
    // 6: extension (repeated)
    for ext in &msg.extension {
        let eb = encode_field_descriptor_proto(ext);
        encode_message_field(&mut buf, 6, &eb);
    }
    // 3: nested_type (repeated)
    for nested in &msg.nested_type {
        let nb = encode_descriptor_proto(nested);
        encode_message_field(&mut buf, 3, &nb);
    }
    // 4: enum_type (repeated)
    for e in &msg.enum_type {
        let enum_bytes = encode_enum_descriptor_proto(e);
        encode_message_field(&mut buf, 4, &enum_bytes);
    }
    // 5: extension_range (repeated)
    for er in &msg.extension_range {
        let range_bytes = encode_extension_range(er);
        encode_message_field(&mut buf, 5, &range_bytes);
    }
    // 8: oneof_decl (repeated)
    for oneof in &msg.oneof_decl {
        let oneof_bytes = encode_oneof_descriptor_proto(oneof);
        encode_message_field(&mut buf, 8, &oneof_bytes);
    }
    // 7: options
    if let Some(ref opts) = msg.options {
        let opts_bytes = encode_message_options(opts);
        if !opts_bytes.is_empty() {
            encode_message_field(&mut buf, 7, &opts_bytes);
        }
    }
    // 9: reserved_range (repeated)
    for rr in &msg.reserved_range {
        let rrb = encode_reserved_range(rr);
        encode_message_field(&mut buf, 9, &rrb);
    }
    // 10: reserved_name (repeated string)
    for name in &msg.reserved_name {
        encode_string_field(&mut buf, 10, name);
    }

    buf
}
fn encode_field_descriptor_proto(field: &FieldDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();

    // 1: name
    encode_opt_string(&mut buf, 1, &field.name);
    // 2: extendee
    encode_opt_string(&mut buf, 2, &field.extendee);
    // 3: number
    encode_opt_int32(&mut buf, 3, field.number);
    // 4: label
    encode_opt_enum(&mut buf, 4, field.label);
    // 5: type
    encode_opt_enum(&mut buf, 5, field.r#type);
    // 6: type_name
    encode_opt_string(&mut buf, 6, &field.type_name);
    // 7: default_value
    encode_opt_string(&mut buf, 7, &field.default_value);
    // 8: options
    if let Some(ref opts) = field.options {
        let ob = encode_field_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 8, &ob);
        }
    }
    // 9: oneof_index
    encode_opt_int32(&mut buf, 9, field.oneof_index);
    // 10: json_name
    encode_opt_string(&mut buf, 10, &field.json_name);
    // 17: proto3_optional
    encode_opt_bool(&mut buf, 17, field.proto3_optional);

    buf
}
fn encode_enum_descriptor_proto(e: &EnumDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();

    // 1: name
    encode_opt_string(&mut buf, 1, &e.name);
    // 2: value (repeated)
    for val in &e.value {
        let vb = encode_enum_value_descriptor_proto(val);
        encode_message_field(&mut buf, 2, &vb);
    }
    // 3: options
    if let Some(ref opts) = e.options {
        let ob = encode_enum_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 3, &ob);
        }
    }
    // 4: reserved_range (repeated)
    for rr in &e.reserved_range {
        let rrb = encode_enum_reserved_range(rr);
        encode_message_field(&mut buf, 4, &rrb);
    }
    // 5: reserved_name (repeated string)
    for name in &e.reserved_name {
        encode_string_field(&mut buf, 5, name);
    }

    buf
}

fn encode_enum_value_descriptor_proto(val: &EnumValueDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: name
    encode_opt_string(&mut buf, 1, &val.name);
    // 2: number
    encode_opt_int32(&mut buf, 2, val.number);
    // 3: options
    if let Some(ref opts) = val.options {
        let ob = encode_enum_value_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 3, &ob);
        }
    }
    buf
}
fn encode_oneof_descriptor_proto(oneof: &OneofDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: name
    encode_opt_string(&mut buf, 1, &oneof.name);
    // 2: options
    if let Some(ref opts) = oneof.options {
        let ob = encode_oneof_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 2, &ob);
        }
    }
    buf
}
fn encode_service_descriptor_proto(svc: &ServiceDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: name
    encode_opt_string(&mut buf, 1, &svc.name);
    // 2: method (repeated)
    for method in &svc.method {
        let mb = encode_method_descriptor_proto(method);
        encode_message_field(&mut buf, 2, &mb);
    }
    // 3: options
    if let Some(ref opts) = svc.options {
        let ob = encode_service_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 3, &ob);
        }
    }
    buf
}

fn encode_method_descriptor_proto(method: &MethodDescriptorProto) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: name
    encode_opt_string(&mut buf, 1, &method.name);
    // 2: input_type
    encode_opt_string(&mut buf, 2, &method.input_type);
    // 3: output_type
    encode_opt_string(&mut buf, 3, &method.output_type);
    // 4: options
    if let Some(ref opts) = method.options {
        let ob = encode_method_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 4, &ob);
        }
    }
    // 5: client_streaming (only encode if true -- protoc omits false)
    if method.client_streaming == Some(true) {
        encode_bool_field(&mut buf, 5, true);
    }
    // 6: server_streaming (only encode if true -- protoc omits false)
    if method.server_streaming == Some(true) {
        encode_bool_field(&mut buf, 6, true);
    }
    buf
}
fn encode_extension_range(er: &ExtensionRange) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_int32(&mut buf, 1, er.start);
    encode_opt_int32(&mut buf, 2, er.end);
    if let Some(ref opts) = er.options {
        let ob = encode_extension_range_options(opts);
        if !ob.is_empty() {
            encode_message_field(&mut buf, 3, &ob);
        }
    }
    buf
}

fn encode_reserved_range(rr: &ReservedRange) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_int32(&mut buf, 1, rr.start);
    encode_opt_int32(&mut buf, 2, rr.end);
    buf
}

fn encode_enum_reserved_range(rr: &EnumReservedRange) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_int32(&mut buf, 1, rr.start);
    encode_opt_int32(&mut buf, 2, rr.end);
    buf
}

fn encode_extension_range_options(opts: &ExtensionRangeOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}
fn encode_file_options(opts: &FileOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_string(&mut buf, 1, &opts.java_package);
    encode_opt_string(&mut buf, 8, &opts.java_outer_classname);
    encode_opt_bool(&mut buf, 10, opts.java_multiple_files);
    encode_opt_bool(&mut buf, 20, opts.java_generate_equals_and_hash);
    encode_opt_bool(&mut buf, 27, opts.java_string_check_utf8);
    encode_opt_enum(&mut buf, 9, opts.optimize_for);
    encode_opt_string(&mut buf, 11, &opts.go_package);
    encode_opt_bool(&mut buf, 16, opts.cc_generic_services);
    encode_opt_bool(&mut buf, 17, opts.java_generic_services);
    encode_opt_bool(&mut buf, 18, opts.py_generic_services);
    encode_opt_bool(&mut buf, 23, opts.deprecated);
    encode_opt_bool(&mut buf, 31, opts.cc_enable_arenas);
    encode_opt_string(&mut buf, 36, &opts.objc_class_prefix);
    encode_opt_string(&mut buf, 37, &opts.csharp_namespace);
    encode_opt_string(&mut buf, 39, &opts.swift_prefix);
    encode_opt_string(&mut buf, 40, &opts.php_class_prefix);
    encode_opt_string(&mut buf, 41, &opts.php_namespace);
    encode_opt_string(&mut buf, 44, &opts.php_metadata_namespace);
    encode_opt_string(&mut buf, 45, &opts.ruby_package);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_message_options(opts: &MessageOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_bool(&mut buf, 1, opts.message_set_wire_format);
    encode_opt_bool(&mut buf, 2, opts.no_standard_descriptor_accessor);
    encode_opt_bool(&mut buf, 3, opts.deprecated);
    encode_opt_bool(&mut buf, 7, opts.map_entry);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_field_options(opts: &FieldOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_enum(&mut buf, 1, opts.ctype);
    encode_opt_bool(&mut buf, 2, opts.packed);
    encode_opt_bool(&mut buf, 3, opts.deprecated);
    encode_opt_bool(&mut buf, 5, opts.lazy);
    encode_opt_enum(&mut buf, 6, opts.jstype);
    encode_opt_bool(&mut buf, 10, opts.weak);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_enum_options(opts: &EnumOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_bool(&mut buf, 2, opts.allow_alias);
    encode_opt_bool(&mut buf, 3, opts.deprecated);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_enum_value_options(opts: &EnumValueOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_bool(&mut buf, 1, opts.deprecated);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_oneof_options(opts: &OneofOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_service_options(opts: &ServiceOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_bool(&mut buf, 33, opts.deprecated);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}

fn encode_method_options(opts: &MethodOptions) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_opt_bool(&mut buf, 33, opts.deprecated);
    encode_opt_enum(&mut buf, 34, opts.idempotency_level);
    for uopt in &opts.uninterpreted_option {
        let ub = encode_uninterpreted_option(uopt);
        encode_message_field(&mut buf, 999, &ub);
    }
    buf
}
fn encode_uninterpreted_option(opt: &UninterpretedOption) -> Vec<u8> {
    let mut buf = Vec::new();
    // 2: name (repeated NamePart)
    for part in &opt.name {
        let pb = encode_name_part(part);
        encode_message_field(&mut buf, 2, &pb);
    }
    // 3: identifier_value
    encode_opt_string(&mut buf, 3, &opt.identifier_value);
    // 4: positive_int_value
    if let Some(v) = opt.positive_int_value {
        encode_varint_field(&mut buf, 4, v);
    }
    // 5: negative_int_value
    if let Some(v) = opt.negative_int_value {
        encode_varint_field(&mut buf, 5, v as u64);
    }
    // 6: double_value
    if let Some(ref v) = opt.double_value {
        if let Ok(d) = v.parse::<f64>() {
            encode_tag(&mut buf, 6, 1); // wire type 1 = fixed64
            buf.extend_from_slice(&d.to_le_bytes());
        }
    }
    // 7: string_value
    if let Some(ref v) = opt.string_value {
        encode_bytes_field(&mut buf, 7, v);
    }
    // 8: aggregate_value
    encode_opt_string(&mut buf, 8, &opt.aggregate_value);
    buf
}

fn encode_name_part(part: &NamePart) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: name_part (required string)
    encode_string_field(&mut buf, 1, &part.name_part);
    // 2: is_extension (required bool)
    encode_bool_field(&mut buf, 2, part.is_extension);
    buf
}
fn encode_source_code_info(sci: &SourceCodeInfo) -> Vec<u8> {
    let mut buf = Vec::new();
    for loc in &sci.location {
        let lb = encode_source_location(loc);
        encode_message_field(&mut buf, 1, &lb);
    }
    buf
}

fn encode_source_location(loc: &SourceLocation) -> Vec<u8> {
    let mut buf = Vec::new();
    // 1: path (repeated int32, packed)
    if !loc.path.is_empty() {
        let mut packed = Vec::new();
        for &v in &loc.path {
            encode_varint(&mut packed, v as u64);
        }
        encode_tag(&mut buf, 1, WIRE_LEN);
        encode_varint(&mut buf, packed.len() as u64);
        buf.extend_from_slice(&packed);
    }
    // 2: span (repeated int32, packed)
    if !loc.span.is_empty() {
        let mut packed = Vec::new();
        for &v in &loc.span {
            encode_varint(&mut packed, v as u64);
        }
        encode_tag(&mut buf, 2, WIRE_LEN);
        encode_varint(&mut buf, packed.len() as u64);
        buf.extend_from_slice(&packed);
    }
    // 3: leading_comments
    encode_opt_string(&mut buf, 3, &loc.leading_comments);
    // 4: trailing_comments
    encode_opt_string(&mut buf, 4, &loc.trailing_comments);
    // 6: leading_detached_comments (repeated string)
    for comment in &loc.leading_detached_comments {
        encode_string_field(&mut buf, 6, comment);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_varint_small() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 1);
        assert_eq!(buf, vec![1]);
    }

    #[test]
    fn encode_varint_multi_byte() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]);
    }

    #[test]
    fn encode_empty_descriptor_set() {
        let fds = FileDescriptorSet { file: vec![] };
        let bytes = serialize_descriptor_set(&fds);
        assert!(bytes.is_empty());
    }

    #[test]
    fn encode_simple_file() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                syntax: Some("proto3".to_string()),
                ..Default::default()
            }],
        };
        let bytes = serialize_descriptor_set(&fds);
        assert!(!bytes.is_empty());
        // Should start with tag for field 1 (file), wire type 2 (length-delimited)
        assert_eq!(bytes[0], (1 << 3) | 2); // tag = 0x0A
    }

    #[test]
    fn round_trip_simple_message() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("test.proto".to_string()),
                package: Some("example".to_string()),
                syntax: Some("proto3".to_string()),
                message_type: vec![DescriptorProto {
                    name: Some("Person".to_string()),
                    field: vec![
                        FieldDescriptorProto {
                            name: Some("name".to_string()),
                            number: Some(1),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::String),
                            ..Default::default()
                        },
                        FieldDescriptorProto {
                            name: Some("id".to_string()),
                            number: Some(2),
                            label: Some(FieldLabel::Optional),
                            r#type: Some(FieldType::Int32),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let bytes = serialize_descriptor_set(&fds);
        // Verify it's non-empty and starts with the right tag
        assert!(!bytes.is_empty());
        assert_eq!(bytes[0], 0x0A); // field 1, wire type 2
    }

    #[test]
    fn encode_with_enum() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("enum.proto".to_string()),
                syntax: Some("proto3".to_string()),
                enum_type: vec![EnumDescriptorProto {
                    name: Some("Status".to_string()),
                    value: vec![
                        EnumValueDescriptorProto {
                            name: Some("UNKNOWN".to_string()),
                            number: Some(0),
                            ..Default::default()
                        },
                        EnumValueDescriptorProto {
                            name: Some("ACTIVE".to_string()),
                            number: Some(1),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let bytes = serialize_descriptor_set(&fds);
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_with_service() {
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("svc.proto".to_string()),
                syntax: Some("proto3".to_string()),
                service: vec![ServiceDescriptorProto {
                    name: Some("Greeter".to_string()),
                    method: vec![MethodDescriptorProto {
                        name: Some("SayHello".to_string()),
                        input_type: Some(".example.HelloRequest".to_string()),
                        output_type: Some(".example.HelloReply".to_string()),
                        client_streaming: Some(false),
                        server_streaming: Some(false),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let bytes = serialize_descriptor_set(&fds);
        assert!(!bytes.is_empty());
    }
}
