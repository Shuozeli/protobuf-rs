use protoc_rs_parser::parse;
use protoc_rs_schema::*;

fn read_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{}",
        env!("CARGO_MANIFEST_DIR").replace("/parser", ""),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e))
}

fn read_system_proto(name: &str) -> Option<String> {
    let path = format!("/usr/include/{}", name);
    std::fs::read_to_string(&path).ok()
}

#[test]
fn test_parse_simple_proto() {
    let source = read_testdata("simple.proto");
    let file = parse(&source).unwrap();

    assert_eq!(file.syntax, Some("proto3".to_string()));
    assert_eq!(file.package, Some("test.simple".to_string()));
    assert_eq!(file.message_type.len(), 2);
    assert_eq!(file.enum_type.len(), 1);

    let search_req = &file.message_type[0];
    assert_eq!(search_req.name, Some("SearchRequest".to_string()));
    assert_eq!(search_req.field.len(), 4);

    let corpus = &file.enum_type[0];
    assert_eq!(corpus.name, Some("Corpus".to_string()));
    assert_eq!(corpus.value.len(), 8);

    let search_resp = &file.message_type[1];
    assert_eq!(search_resp.name, Some("SearchResponse".to_string()));
    assert_eq!(search_resp.nested_type.len(), 1);
    assert_eq!(search_resp.nested_type[0].name, Some("Result".to_string()));
}

#[test]
fn test_parse_complex_proto() {
    let source = read_testdata("complex.proto");
    let file = parse(&source).unwrap();

    assert_eq!(file.syntax, Some("proto3".to_string()));
    assert_eq!(file.package, Some("test.complex".to_string()));
    assert_eq!(file.dependency.len(), 2);
    assert_eq!(file.service.len(), 1);

    // User message
    let user = &file.message_type[0];
    assert_eq!(user.name, Some("User".to_string()));
    assert_eq!(user.oneof_decl.len(), 1);
    assert_eq!(user.oneof_decl[0].name, Some("contact_method".to_string()));
    // Nested: Address + MetadataEntry (from map)
    assert_eq!(user.nested_type.len(), 2);
    assert_eq!(user.reserved_range.len(), 2);
    assert_eq!(user.reserved_name.len(), 2);

    // Map field produces a synthetic entry
    let has_metadata = user
        .field
        .iter()
        .any(|f| f.name.as_deref() == Some("metadata"));
    assert!(has_metadata);

    // Service
    let svc = &file.service[0];
    assert_eq!(svc.name, Some("UserService".to_string()));
    assert_eq!(svc.method.len(), 6);

    let watch = svc
        .method
        .iter()
        .find(|m| m.name.as_deref() == Some("WatchUsers"))
        .unwrap();
    assert_eq!(watch.server_streaming, Some(true));
    assert_eq!(watch.client_streaming, Some(false));
}

#[test]
fn test_parse_proto2_features() {
    let source = read_testdata("proto2_features.proto");
    let file = parse(&source).unwrap();

    assert_eq!(file.syntax, Some("proto2".to_string()));

    let msg = &file.message_type[0];
    assert_eq!(msg.name, Some("LegacyMessage".to_string()));

    // Required field
    let name_field = &msg.field[0];
    assert_eq!(name_field.label, Some(FieldLabel::Required));

    // Default values
    let id_field = &msg.field[1];
    assert_eq!(id_field.default_value, Some("-1".to_string()));

    let active_field = &msg.field[4];
    assert_eq!(active_field.default_value, Some("true".to_string()));

    let score_field = &msg.field[5];
    assert_eq!(score_field.default_value, Some("3.14".to_string()));

    // Extensions
    assert_eq!(msg.extension_range.len(), 2);
    assert_eq!(msg.extension_range[0].start, Some(100));
    assert_eq!(msg.extension_range[0].end, Some(200));

    // Reserved
    assert_eq!(msg.reserved_range.len(), 2);
    assert_eq!(msg.reserved_name, vec!["removed_field"]);

    // Nested enum with default
    let priority_field = msg
        .field
        .iter()
        .find(|f| f.name.as_deref() == Some("priority"))
        .unwrap();
    assert_eq!(priority_field.default_value, Some("MEDIUM".to_string()));

    // Top-level extensions
    assert_eq!(file.extension.len(), 2);
    assert_eq!(
        file.extension[0].extendee,
        Some("LegacyMessage".to_string())
    );
}

// Parse well-known types from the system (if available)

#[test]
fn test_parse_timestamp_proto() {
    let Some(source) = read_system_proto("google/protobuf/timestamp.proto") else {
        eprintln!("skipping: timestamp.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    assert_eq!(file.package, Some("google.protobuf".to_string()));
    let ts = &file.message_type[0];
    assert_eq!(ts.name, Some("Timestamp".to_string()));
    assert_eq!(ts.field.len(), 2);
}

#[test]
fn test_parse_any_proto() {
    let Some(source) = read_system_proto("google/protobuf/any.proto") else {
        eprintln!("skipping: any.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    let any = &file.message_type[0];
    assert_eq!(any.name, Some("Any".to_string()));
}

#[test]
fn test_parse_duration_proto() {
    let Some(source) = read_system_proto("google/protobuf/duration.proto") else {
        eprintln!("skipping: duration.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    let dur = &file.message_type[0];
    assert_eq!(dur.name, Some("Duration".to_string()));
}

#[test]
fn test_parse_struct_proto() {
    let Some(source) = read_system_proto("google/protobuf/struct.proto") else {
        eprintln!("skipping: struct.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    assert!(file
        .message_type
        .iter()
        .any(|m| m.name.as_deref() == Some("Struct")));
    assert!(file
        .message_type
        .iter()
        .any(|m| m.name.as_deref() == Some("Value")));
    assert!(file
        .enum_type
        .iter()
        .any(|e| e.name.as_deref() == Some("NullValue")));
}

#[test]
fn test_parse_wrappers_proto() {
    let Some(source) = read_system_proto("google/protobuf/wrappers.proto") else {
        eprintln!("skipping: wrappers.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    // wrappers.proto has DoubleValue, FloatValue, Int64Value, etc.
    assert!(file.message_type.len() >= 8);
}

#[test]
fn test_parse_empty_proto() {
    let Some(source) = read_system_proto("google/protobuf/empty.proto") else {
        eprintln!("skipping: empty.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    let empty = &file.message_type[0];
    assert_eq!(empty.name, Some("Empty".to_string()));
    assert!(empty.field.is_empty());
}

#[test]
fn test_parse_descriptor_proto() {
    let Some(source) = read_system_proto("google/protobuf/descriptor.proto") else {
        eprintln!("skipping: descriptor.proto not found on system");
        return;
    };
    let file = parse(&source).unwrap();
    assert_eq!(file.package, Some("google.protobuf".to_string()));
    // descriptor.proto is large -- just verify it parses and has key types
    let msg_names: Vec<_> = file
        .message_type
        .iter()
        .filter_map(|m| m.name.as_deref())
        .collect();
    assert!(msg_names.contains(&"FileDescriptorProto"));
    assert!(msg_names.contains(&"DescriptorProto"));
    assert!(msg_names.contains(&"FieldDescriptorProto"));
    assert!(msg_names.contains(&"EnumDescriptorProto"));
    assert!(msg_names.contains(&"ServiceDescriptorProto"));

    let enum_names: Vec<_> = file
        .enum_type
        .iter()
        .filter_map(|e| e.name.as_deref())
        .collect();
    assert!(
        enum_names.contains(&"Edition")
            || file.message_type.iter().any(|m| {
                m.enum_type
                    .iter()
                    .any(|e| e.name.as_deref() == Some("Type"))
            })
    );
}
