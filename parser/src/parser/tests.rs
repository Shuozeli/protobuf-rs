use super::*;

#[test]
fn test_parse_minimal_proto3() {
    let source = r#"syntax = "proto3";"#;
    let file = parse(source).unwrap();
    assert_eq!(file.syntax, Some("proto3".to_string()));
    assert_eq!(file.syntax_enum(), Syntax::Proto3);
}

#[test]
fn test_parse_minimal_proto2() {
    let source = r#"syntax = "proto2";"#;
    let file = parse(source).unwrap();
    assert_eq!(file.syntax, Some("proto2".to_string()));
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
}

#[test]
fn test_parse_package() {
    let source = r#"
        syntax = "proto3";
        package example.foo;
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.package, Some("example.foo".to_string()));
}

#[test]
fn test_parse_import() {
    let source = r#"
        syntax = "proto3";
        import "other.proto";
        import public "public_dep.proto";
        import weak "weak_dep.proto";
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.dependency.len(), 3);
    assert_eq!(file.dependency[0], "other.proto");
    assert_eq!(file.dependency[1], "public_dep.proto");
    assert_eq!(file.dependency[2], "weak_dep.proto");
    assert_eq!(file.public_dependency, vec![1]);
    assert_eq!(file.weak_dependency, vec![2]);
}

#[test]
fn test_parse_import_option_editions() {
    let source = r#"
        edition = "2024";
        import option "unloaded.proto";
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.dependency.len(), 0);
    assert_eq!(file.option_dependency.len(), 1);
    assert_eq!(file.option_dependency[0], "unloaded.proto");
}

#[test]
fn test_parse_import_option_not_editions_error() {
    let source = r#"
        syntax = "proto3";
        import option "unloaded.proto";
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.contains("not supported before edition 2024"),
        "got: {}",
        err.message
    );
}

#[test]
fn test_parse_simple_message() {
    let source = r#"
        syntax = "proto3";

        message Person {
            string name = 1;
            int32 age = 2;
            repeated string emails = 3;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.message_type.len(), 1);
    let msg = &file.message_type[0];
    assert_eq!(msg.name, Some("Person".to_string()));
    assert_eq!(msg.field.len(), 3);

    let name_field = &msg.field[0];
    assert_eq!(name_field.name, Some("name".to_string()));
    assert_eq!(name_field.r#type, Some(FieldType::String));
    assert_eq!(name_field.number, Some(1));

    let age_field = &msg.field[1];
    assert_eq!(age_field.name, Some("age".to_string()));
    assert_eq!(age_field.r#type, Some(FieldType::Int32));
    assert_eq!(age_field.number, Some(2));

    let emails_field = &msg.field[2];
    assert_eq!(emails_field.name, Some("emails".to_string()));
    assert_eq!(emails_field.r#type, Some(FieldType::String));
    assert_eq!(emails_field.label, Some(FieldLabel::Repeated));
    assert_eq!(emails_field.number, Some(3));
}

#[test]
fn test_parse_nested_message() {
    let source = r#"
        syntax = "proto3";

        message Outer {
            message Inner {
                int32 x = 1;
            }
            Inner inner = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let outer = &file.message_type[0];
    assert_eq!(outer.nested_type.len(), 1);
    assert_eq!(outer.nested_type[0].name, Some("Inner".to_string()));
    assert_eq!(outer.field.len(), 1);
    assert_eq!(outer.field[0].type_name, Some("Inner".to_string()));
}

#[test]
fn test_parse_enum() {
    let source = r#"
        syntax = "proto3";

        enum Status {
            UNKNOWN = 0;
            ACTIVE = 1;
            INACTIVE = 2;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.enum_type.len(), 1);
    let e = &file.enum_type[0];
    assert_eq!(e.name, Some("Status".to_string()));
    assert_eq!(e.value.len(), 3);
    assert_eq!(e.value[0].name, Some("UNKNOWN".to_string()));
    assert_eq!(e.value[0].number, Some(0));
    assert_eq!(e.value[1].name, Some("ACTIVE".to_string()));
    assert_eq!(e.value[1].number, Some(1));
}

#[test]
fn test_parse_oneof() {
    let source = r#"
        syntax = "proto3";

        message Msg {
            oneof test_oneof {
                string name = 1;
                int32 id = 2;
            }
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.oneof_decl.len(), 1);
    assert_eq!(msg.oneof_decl[0].name, Some("test_oneof".to_string()));
    assert_eq!(msg.field.len(), 2);
    assert_eq!(msg.field[0].oneof_index, Some(0));
    assert_eq!(msg.field[1].oneof_index, Some(0));
}

#[test]
fn test_parse_map() {
    let source = r#"
        syntax = "proto3";

        message Msg {
            map<string, int32> tags = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.field.len(), 1);
    assert_eq!(msg.field[0].name, Some("tags".to_string()));
    assert_eq!(msg.field[0].label, Some(FieldLabel::Repeated));
    assert_eq!(msg.field[0].r#type, Some(FieldType::Message));

    // Synthetic nested message
    assert_eq!(msg.nested_type.len(), 1);
    let entry = &msg.nested_type[0];
    assert_eq!(entry.name, Some("TagsEntry".to_string()));
    assert_eq!(entry.options.as_ref().unwrap().map_entry, Some(true));
    assert_eq!(entry.field.len(), 2);
    assert_eq!(entry.field[0].name, Some("key".to_string()));
    assert_eq!(entry.field[1].name, Some("value".to_string()));
}

#[test]
fn test_parse_service() {
    let source = r#"
        syntax = "proto3";

        service Greeter {
            rpc SayHello (HelloRequest) returns (HelloReply);
            rpc StreamHello (stream HelloRequest) returns (stream HelloReply) {}
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.service.len(), 1);
    let svc = &file.service[0];
    assert_eq!(svc.name, Some("Greeter".to_string()));
    assert_eq!(svc.method.len(), 2);

    let m1 = &svc.method[0];
    assert_eq!(m1.name, Some("SayHello".to_string()));
    assert_eq!(m1.input_type, Some("HelloRequest".to_string()));
    assert_eq!(m1.output_type, Some("HelloReply".to_string()));
    assert_eq!(m1.client_streaming, Some(false));
    assert_eq!(m1.server_streaming, Some(false));

    let m2 = &svc.method[1];
    assert_eq!(m2.client_streaming, Some(true));
    assert_eq!(m2.server_streaming, Some(true));
}

#[test]
fn test_parse_reserved() {
    let source = r#"
        syntax = "proto3";

        message Msg {
            reserved 2, 15, 9 to 11;
            reserved "foo", "bar";
            int32 x = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.reserved_range.len(), 3);
    assert_eq!(msg.reserved_range[0].start, Some(2));
    assert_eq!(msg.reserved_range[0].end, Some(3)); // exclusive
    assert_eq!(msg.reserved_range[1].start, Some(15));
    assert_eq!(msg.reserved_range[2].start, Some(9));
    assert_eq!(msg.reserved_range[2].end, Some(12)); // exclusive
    assert_eq!(msg.reserved_name, vec!["foo", "bar"]);
}

#[test]
fn test_parse_field_options() {
    let source = r#"
        syntax = "proto2";

        message Msg {
            optional int32 old_field = 1 [deprecated = true];
            repeated int32 values = 2 [packed = true];
            optional string name = 3 [default = "hello"];
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];

    let f1 = &msg.field[0];
    assert_eq!(f1.options.as_ref().unwrap().deprecated, Some(true));

    let f2 = &msg.field[1];
    assert_eq!(f2.options.as_ref().unwrap().packed, Some(true));

    let f3 = &msg.field[2];
    assert_eq!(f3.default_value, Some("hello".to_string()));
}

#[test]
fn test_parse_file_options() {
    let source = r#"
        syntax = "proto3";
        option java_package = "com.example";
        option java_multiple_files = true;
        option optimize_for = SPEED;
        option go_package = "example.com/pkg";
    "#;
    let file = parse(source).unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.java_package, Some("com.example".to_string()));
    assert_eq!(opts.java_multiple_files, Some(true));
    assert_eq!(opts.optimize_for, Some(OptimizeMode::Speed));
    assert_eq!(opts.go_package, Some("example.com/pkg".to_string()));
}

#[test]
fn test_parse_extensions() {
    let source = r#"
        syntax = "proto2";

        message Msg {
            extensions 100 to 199;
            extensions 1000 to max;
            optional int32 x = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.extension_range.len(), 2);
    assert_eq!(msg.extension_range[0].start, Some(100));
    assert_eq!(msg.extension_range[0].end, Some(200));
    assert_eq!(msg.extension_range[1].start, Some(1000));
}

#[test]
fn test_parse_extend() {
    let source = r#"
        syntax = "proto2";

        message Foo {
            extensions 100 to 200;
        }

        extend Foo {
            optional int32 bar = 100;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.extension.len(), 1);
    assert_eq!(file.extension[0].name, Some("bar".to_string()));
    assert_eq!(file.extension[0].extendee, Some("Foo".to_string()));
    assert_eq!(file.extension[0].number, Some(100));
}

#[test]
fn test_json_name_default() {
    let source = r#"
        syntax = "proto3";
        message Msg {
            int32 my_field_name = 1;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(
        file.message_type[0].field[0].json_name,
        Some("myFieldName".to_string())
    );
}

#[test]
fn test_to_camel_case() {
    assert_eq!(to_camel_case("my_field"), "myField");
    assert_eq!(to_camel_case("some_long_name"), "someLongName");
    assert_eq!(to_camel_case("already"), "already");
    assert_eq!(to_camel_case("a_b_c"), "aBC");
}

#[test]
fn test_parse_enum_with_options() {
    let source = r#"
        syntax = "proto3";

        enum Corpus {
            option allow_alias = true;
            CORPUS_UNSPECIFIED = 0;
            CORPUS_UNIVERSAL = 1;
            CORPUS_WEB = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let e = &file.enum_type[0];
    assert_eq!(e.options.as_ref().unwrap().allow_alias, Some(true));
    assert_eq!(e.value.len(), 3);
}

#[test]
fn test_parse_enum_reserved() {
    let source = r#"
        syntax = "proto3";

        enum Foo {
            reserved 2, 15, 9 to 11;
            reserved "FOO", "BAR";
            UNSPECIFIED = 0;
        }
    "#;
    let file = parse(source).unwrap();
    let e = &file.enum_type[0];
    assert_eq!(e.reserved_range.len(), 3);
    assert_eq!(e.reserved_name, vec!["FOO", "BAR"]);
}

#[test]
fn test_parse_comprehensive() {
    let source = r#"
        syntax = "proto3";

        package example.v1;

        import "google/protobuf/timestamp.proto";

        option java_package = "com.example.v1";

        enum Status {
            STATUS_UNSPECIFIED = 0;
            STATUS_ACTIVE = 1;
            STATUS_INACTIVE = 2;
        }

        message User {
            string id = 1;
            string name = 2;
            string email = 3;
            Status status = 4;
            google.protobuf.Timestamp created_at = 5;
            repeated string tags = 6;
            map<string, string> metadata = 7;

            message Address {
                string street = 1;
                string city = 2;
                string country = 3;
            }

            repeated Address addresses = 8;

            oneof contact {
                string phone = 9;
                string fax = 10;
            }
        }

        service UserService {
            rpc GetUser (GetUserRequest) returns (User);
            rpc ListUsers (ListUsersRequest) returns (stream User);
        }

        message GetUserRequest {
            string id = 1;
        }

        message ListUsersRequest {
            int32 page_size = 1;
            string page_token = 2;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.syntax, Some("proto3".to_string()));
    assert_eq!(file.package, Some("example.v1".to_string()));
    assert_eq!(file.dependency.len(), 1);
    assert_eq!(file.enum_type.len(), 1);
    assert_eq!(file.message_type.len(), 3); // User, GetUserRequest, ListUsersRequest
    assert_eq!(file.service.len(), 1);

    let user = &file.message_type[0];
    assert_eq!(user.field.len(), 10); // 8 regular + 2 oneof
    assert_eq!(user.nested_type.len(), 2); // Address + MetadataEntry (map)
    assert_eq!(user.oneof_decl.len(), 1);
}

// =======================================================================
// ParseMessageTest parity (C++ parser_unittest.cc)
// =======================================================================

#[test]
fn cpp_ignore_bom() {
    // UTF-8 BOM (\xEF\xBB\xBF) at start should be silently skipped
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(b"syntax = \"proto2\";\nmessage Foo {\n  required int32 bar = 1;\n}\n");
    let source = String::from_utf8(input).unwrap();
    let file = parse(&source).unwrap();
    assert_eq!(file.message_type.len(), 1);
    assert_eq!(file.message_type[0].name.as_deref(), Some("Foo"));
}

#[test]
fn cpp_bom_error() {
    // 0xEF without full BOM should error
    let mut input = vec![0xEF];
    input.extend_from_slice(b" message Foo {}\n");
    let source = String::from_utf8_lossy(&input).to_string();
    let err = parse(&source).unwrap_err();
    assert!(
        err.message.contains("0xEF") && err.message.contains("BOM"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_parse_group() {
    // Group field creates nested type + field with TYPE_GROUP
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            optional group TestGroup = 1 {}
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.nested_type.len(), 1);
    assert_eq!(msg.nested_type[0].name.as_deref(), Some("TestGroup"));
    assert_eq!(msg.field.len(), 1);
    let f = &msg.field[0];
    assert_eq!(f.name.as_deref(), Some("testgroup"));
    assert_eq!(f.r#type, Some(FieldType::Group));
    assert_eq!(f.type_name.as_deref(), Some("TestGroup"));
    assert_eq!(f.number, Some(1));
    assert_eq!(f.label, Some(FieldLabel::Optional));
}

#[test]
fn cpp_reserved_range_on_message_set() {
    // reserved with max in message with message_set_wire_format option
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            option message_set_wire_format = true;
            reserved 20 to max;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.reserved_range.len(), 1);
    assert_eq!(msg.reserved_range[0].start, Some(20));
    // "max" maps to MAX_FIELD_NUMBER (536870911), end is exclusive (+1 = 536870912)
    assert_eq!(msg.reserved_range[0].end, Some(536870912));
    assert!(msg.options.is_some());
}

#[test]
fn cpp_max_int_extension_does_not_overflow() {
    // extensions 2147483647 should error (out of bounds)
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            extensions 2147483647;
        }
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("out of") || err.message.contains("bound"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_max_int_extension_range_overflow() {
    // extensions 1 to 2147483647 should error
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            extensions 1 to 2147483647;
        }
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("out of") || err.message.contains("bound"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_larger_max_for_message_set() {
    // message_set_wire_format allows extensions 4 to max
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            extensions 4 to max;
            option message_set_wire_format = true;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.extension_range.len(), 1);
    assert_eq!(msg.extension_range[0].start, Some(4));
    // "max" maps to 0x7fffffff = 2147483647
    assert!(msg.extension_range[0].end.unwrap() >= 536870912);
}

#[test]
fn cpp_field_options_large_decimal() {
    // Large decimal literal > uint64 max should parse as double default
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            optional double a = 1 [default = 18446744073709551616];
            optional double b = 2 [default = -18446744073709551616];
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert!(msg.field[0].default_value.is_some());
    assert!(msg.field[1].default_value.is_some());
}

#[test]
fn cpp_field_options_inf_nan() {
    // inf and nan as option values should parse as identifiers
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            optional double a = 1 [default = inf];
            optional double b = 2 [default = -inf];
            optional double c = 3 [default = nan];
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.field[0].default_value.as_deref(), Some("inf"));
    assert_eq!(msg.field[1].default_value.as_deref(), Some("-inf"));
    assert_eq!(msg.field[2].default_value.as_deref(), Some("nan"));
}

#[test]
fn cpp_multiple_extensions_one_extendee() {
    // Multiple extension fields in one extend block
    let source = r#"
        syntax = "proto2";
        extend Extendee1 {
            optional int32 foo = 12;
            repeated string bar = 22;
        }
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.extension.len(), 2);
    assert_eq!(file.extension[0].extendee.as_deref(), Some("Extendee1"));
    assert_eq!(file.extension[1].extendee.as_deref(), Some("Extendee1"));
    assert_eq!(file.extension[0].name.as_deref(), Some("foo"));
    assert_eq!(file.extension[1].name.as_deref(), Some("bar"));
}

#[test]
fn cpp_extensions_in_message_scope() {
    // extend inside a message body
    let source = r#"
        syntax = "proto2";
        message TestMessage {
            extend Extendee1 { optional int32 foo = 12; }
            extend Extendee2 { repeated int32 bar = 22; }
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.extension.len(), 2);
    assert_eq!(msg.extension[0].extendee.as_deref(), Some("Extendee1"));
    assert_eq!(msg.extension[1].extendee.as_deref(), Some("Extendee2"));
}

#[test]
fn cpp_explicit_optional_label_proto3() {
    // proto3 explicit optional creates synthetic oneof
    let source = r#"
        syntax = "proto3";
        message TestMessage {
            optional int32 foo = 1;
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.field[0].proto3_optional, Some(true));
    assert_eq!(msg.field[0].oneof_index, Some(0));
    assert_eq!(msg.oneof_decl.len(), 1);
    assert_eq!(msg.oneof_decl[0].name.as_deref(), Some("_foo"));
}

#[test]
fn cpp_explicit_optional_proto3_collision() {
    // Synthetic oneof name collision with existing oneof
    let source = r#"
        syntax = "proto3";
        message TestMessage {
            optional int32 foo = 1;
            oneof _foo {
                int32 bar = 2;
            }
        }
    "#;
    let file = parse(source).unwrap();
    let msg = &file.message_type[0];
    // _foo is taken, so synthetic oneof should be X_foo
    assert_eq!(msg.oneof_decl.len(), 2);
    // The real oneof comes first
    let oneof_names: Vec<_> = msg.oneof_decl.iter().map(|o| o.name.as_deref()).collect();
    assert!(
        oneof_names.contains(&Some("_foo")),
        "missing _foo: {:?}",
        oneof_names
    );
}

#[test]
fn cpp_can_handle_error_on_first_token() {
    // Invalid first token should produce a clear error
    let source = "/";
    let err = parse(source).unwrap_err();
    assert!(
        err.message.contains("Expected")
            || err.message.contains("expected")
            || err.message.contains("unexpected"),
        "got: {}",
        err.message
    );
}

// =======================================================================
// ParseImportTest parity
// =======================================================================

#[test]
fn cpp_option_import_before_import_bad_order() {
    // option import before regular import should error
    let source = r#"
        edition = "2024";
        import option "baz.proto";
        import "foo.proto";
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.contains("imports should precede"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_option_import_before_public_import_bad_order() {
    // option import before public import should error
    let source = r#"
        edition = "2024";
        import option "baz.proto";
        import public "bar.proto";
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.contains("imports should precede"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_option_import_before_multiple_imports_bad_order() {
    // Multiple regular imports after option import: each should error
    // (we stop at first error, so just check the first one)
    let source = r#"
        edition = "2024";
        import option "baz.proto";
        import "foo.proto";
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.message.contains("imports should precede"),
        "got: {}",
        err.message
    );
}

#[test]
fn cpp_option_and_public_imports() {
    // Mixing public and option imports in proper order
    let source = r#"
        edition = "2024";
        import "foo.proto";
        import public "bar.proto";
        import option "baz.proto";
        import option "qux.proto";
    "#;
    let file = parse(source).unwrap();
    assert_eq!(file.dependency, vec!["foo.proto", "bar.proto"]);
    assert_eq!(file.public_dependency, vec![1]);
    assert_eq!(file.option_dependency, vec!["baz.proto", "qux.proto"]);
}

// =======================================================================
// ParseMiscTest parity
// =======================================================================

#[test]
fn cpp_parse_package_with_spaces() {
    // Package name with whitespace between dot-separated identifiers
    let source = "syntax = \"proto2\";\npackage foo   .   bar.\n  baz;\n";
    let file = parse(source).unwrap();
    assert_eq!(file.package.as_deref(), Some("foo.bar.baz"));
}

// =======================================================================
// SourceInfoTest parity (C++ parser_unittest.cc)
// =======================================================================

/// Helper: find a SourceLocation by path in SourceCodeInfo.
fn find_location(file: &FileDescriptorProto, path: &[i32]) -> Option<Vec<i32>> {
    file.source_code_info
        .as_ref()?
        .location
        .iter()
        .find(|loc| loc.path == path)
        .map(|loc| loc.span.clone())
}

fn find_location_full<'a>(
    file: &'a FileDescriptorProto,
    path: &[i32],
) -> Option<&'a protoc_rs_schema::SourceLocation> {
    file.source_code_info
        .as_ref()?
        .location
        .iter()
        .find(|loc| loc.path == path)
}

#[test]
fn source_info_basic_file_decls() {
    let source = concat!(
        "syntax = \"proto2\";\n",  // line 0 (0-based)
        "package foo.bar;\n",      // line 1
        "import \"baz.proto\";\n", // line 2
        "import \"qux.proto\";\n", // line 3
    );
    let file = parse(source).unwrap();
    let sci = file
        .source_code_info
        .as_ref()
        .expect("source_code_info should be populated");
    assert!(!sci.location.is_empty(), "should have locations");

    // syntax statement: path=[12], spans line 0
    let span = find_location(&file, &[12]).expect("syntax location");
    assert_eq!(span[0], 0, "syntax start line"); // line 0

    // package statement: path=[2], spans line 1
    let span = find_location(&file, &[2]).expect("package location");
    assert_eq!(span[0], 1, "package start line"); // line 1

    // dependency[0]: path=[3, 0], spans line 2
    let span = find_location(&file, &[3, 0]).expect("dependency[0] location");
    assert_eq!(span[0], 2, "dep[0] start line");

    // dependency[1]: path=[3, 1], spans line 3
    let span = find_location(&file, &[3, 1]).expect("dependency[1] location");
    assert_eq!(span[0], 3, "dep[1] start line");
}

#[test]
fn source_info_messages() {
    let source = concat!(
        "syntax = \"proto3\";\n", // line 0
        "message Foo {}\n",       // line 1
        "message Bar {}\n",       // line 2
    );
    let file = parse(source).unwrap();

    // message_type[0]: path=[4, 0]
    let span = find_location(&file, &[4, 0]).expect("message[0] location");
    assert_eq!(span[0], 1, "message[0] start line");

    // message_type[1]: path=[4, 1]
    let span = find_location(&file, &[4, 1]).expect("message[1] location");
    assert_eq!(span[0], 2, "message[1] start line");
}

#[test]
fn source_info_enums() {
    let source = concat!(
        "syntax = \"proto3\";\n", // line 0
        "enum Color {\n",         // line 1
        "  RED = 0;\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // enum_type[0]: path=[5, 0]
    let span = find_location(&file, &[5, 0]).expect("enum[0] location");
    assert_eq!(span[0], 1, "enum[0] start line");
}

#[test]
fn source_info_services() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Req {}\n",
        "message Resp {}\n",
        "service MyService {\n", // line 3
        "  rpc DoThing(Req) returns (Resp);\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // service[0]: path=[6, 0]
    let span = find_location(&file, &[6, 0]).expect("service[0] location");
    assert_eq!(span[0], 3, "service[0] start line");
}

#[test]
fn source_info_imports_with_public() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "import \"foo.proto\";\n",        // line 1: dependency[0]
        "import public \"bar.proto\";\n", // line 2: dependency[1], public_dependency[0]
    );
    let file = parse(source).unwrap();

    // dependency[0]: path=[3, 0]
    assert!(find_location(&file, &[3, 0]).is_some(), "dependency[0]");

    // dependency[1]: path=[3, 1]
    assert!(find_location(&file, &[3, 1]).is_some(), "dependency[1]");

    // public_dependency[0]: path=[10, 0]
    assert!(
        find_location(&file, &[10, 0]).is_some(),
        "public_dependency[0]"
    );
}

#[test]
fn source_info_populated_after_parse() {
    // Verify source_code_info is always populated (even for empty files)
    let source = "syntax = \"proto3\";\n";
    let file = parse(source).unwrap();
    assert!(file.source_code_info.is_some());
    let sci = file.source_code_info.unwrap();
    // At minimum, we should have the syntax location
    assert!(!sci.location.is_empty());
}

#[test]
fn source_info_message_name() {
    // path [4, 0, 1] = FILE_MESSAGE_TYPE[0].MSG_NAME
    let source = "syntax = \"proto3\";\nmessage Foo {}\n";
    let file = parse(source).unwrap();
    let span = find_location(&file, &[4, 0, 1]).expect("message name location");
    assert_eq!(span[0], 1, "name on line 1");
    // "Foo" starts at col 8
    assert_eq!(span[1], 8, "name start col");
}

#[test]
fn source_info_message_fields() {
    // Tests field container span and field sub-elements
    let source = concat!(
        "syntax = \"proto3\";\n", // line 0
        "message Foo {\n",        // line 1
        "  int32 bar = 1;\n",     // line 2
        "  string baz = 2;\n",    // line 3
        "}\n",
    );
    let file = parse(source).unwrap();

    // field[0] container: path=[4, 0, 2, 0]
    let span = find_location(&file, &[4, 0, 2, 0]).expect("field[0] location");
    assert_eq!(span[0], 2, "field[0] on line 2");

    // field[0] type: path=[4, 0, 2, 0, 5] (FIELD_TYPE=5 for scalar)
    let span = find_location(&file, &[4, 0, 2, 0, 5]).expect("field[0] type");
    assert_eq!(span[0], 2, "type on line 2");
    assert_eq!(span[1], 2, "type start col"); // "  int32" -> col 2

    // field[0] name: path=[4, 0, 2, 0, 1] (FIELD_NAME=1)
    let span = find_location(&file, &[4, 0, 2, 0, 1]).expect("field[0] name");
    assert_eq!(span[0], 2, "name on line 2");
    assert_eq!(span[1], 8, "name start col"); // "  int32 bar" -> col 8

    // field[0] number: path=[4, 0, 2, 0, 3] (FIELD_NUMBER=3)
    let span = find_location(&file, &[4, 0, 2, 0, 3]).expect("field[0] number");
    assert_eq!(span[0], 2, "number on line 2");

    // field[1] container: path=[4, 0, 2, 1]
    let span = find_location(&file, &[4, 0, 2, 1]).expect("field[1] location");
    assert_eq!(span[0], 3, "field[1] on line 3");
}

#[test]
fn source_info_field_with_label() {
    let source = concat!(
        "syntax = \"proto2\";\n",     // line 0
        "message Foo {\n",            // line 1
        "  optional int32 x = 1;\n",  // line 2
        "  repeated string y = 2;\n", // line 3
        "}\n",
    );
    let file = parse(source).unwrap();

    // field[0] label: path=[4, 0, 2, 0, 4] (FIELD_LABEL=4)
    let span = find_location(&file, &[4, 0, 2, 0, 4]).expect("field[0] label");
    assert_eq!(span[0], 2, "label on line 2");
    assert_eq!(span[1], 2, "label start col"); // "  optional" -> col 2

    // field[1] label
    let span = find_location(&file, &[4, 0, 2, 1, 4]).expect("field[1] label");
    assert_eq!(span[0], 3, "field[1] label on line 3");
}

#[test]
fn source_info_field_message_type() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Bar {}\n",
        "message Foo {\n", // line 2
        "  Bar b = 1;\n",  // line 3
        "}\n",
    );
    let file = parse(source).unwrap();

    // field type_name: path=[4, 1, 2, 0, 6] (FIELD_TYPE_NAME=6 for message ref)
    let span = find_location(&file, &[4, 1, 2, 0, 6]).expect("field type_name");
    assert_eq!(span[0], 3, "type_name on line 3");
    assert_eq!(span[1], 2, "type_name start col"); // "  Bar" -> col 2
}

#[test]
fn source_info_group_sub_elements() {
    let source = concat!(
        "syntax = \"proto2\";\n",
        "message Foo {\n",               // line 1
        "  optional group Bar = 1 {}\n", // line 2
        "}\n",
    );
    let file = parse(source).unwrap();

    // field[0] label: path=[4, 0, 2, 0, 4]
    let span = find_location(&file, &[4, 0, 2, 0, 4]).expect("group label");
    assert_eq!(span[0], 2, "group label on line 2");

    // field[0] type (group keyword): path=[4, 0, 2, 0, 5]
    let span = find_location(&file, &[4, 0, 2, 0, 5]).expect("group type");
    assert_eq!(span[0], 2, "group type on line 2");

    // field[0] name: path=[4, 0, 2, 0, 1]
    let span = find_location(&file, &[4, 0, 2, 0, 1]).expect("group name");
    assert_eq!(span[0], 2, "group name on line 2");

    // field[0] number: path=[4, 0, 2, 0, 3]
    let span = find_location(&file, &[4, 0, 2, 0, 3]).expect("group number");
    assert_eq!(span[0], 2, "group number on line 2");
}

#[test]
fn source_info_nested_message() {
    // path [4, 0, 3, 0] = FILE_MESSAGE_TYPE[0].MSG_NESTED_TYPE[0]
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Outer {\n",    // line 1
        "  message Inner {}\n", // line 2
        "}\n",
    );
    let file = parse(source).unwrap();

    // nested_type[0]: path=[4, 0, 3, 0]
    let span = find_location(&file, &[4, 0, 3, 0]).expect("nested message location");
    assert_eq!(span[0], 2, "nested message on line 2");

    // nested message name: path=[4, 0, 3, 0, 1]
    let span = find_location(&file, &[4, 0, 3, 0, 1]).expect("nested message name");
    assert_eq!(span[0], 2, "nested name on line 2");
}

#[test]
fn source_info_enum_name_and_values() {
    let source = concat!(
        "syntax = \"proto3\";\n", // line 0
        "enum Color {\n",         // line 1
        "  RED = 0;\n",           // line 2
        "  GREEN = 1;\n",         // line 3
        "}\n",
    );
    let file = parse(source).unwrap();

    // enum name: path=[5, 0, 1]
    let span = find_location(&file, &[5, 0, 1]).expect("enum name location");
    assert_eq!(span[0], 1, "enum name on line 1");

    // enum value[0]: path=[5, 0, 2, 0]
    let span = find_location(&file, &[5, 0, 2, 0]).expect("enum value[0] location");
    assert_eq!(span[0], 2, "value[0] on line 2");

    // enum value[0] name: path=[5, 0, 2, 0, 1]
    let span = find_location(&file, &[5, 0, 2, 0, 1]).expect("enum value[0] name");
    assert_eq!(span[0], 2, "value name on line 2");

    // enum value[0] number: path=[5, 0, 2, 0, 2]
    let span = find_location(&file, &[5, 0, 2, 0, 2]).expect("enum value[0] number");
    assert_eq!(span[0], 2, "value number on line 2");

    // enum value[1]: path=[5, 0, 2, 1]
    let span = find_location(&file, &[5, 0, 2, 1]).expect("enum value[1] location");
    assert_eq!(span[0], 3, "value[1] on line 3");
}

#[test]
fn source_info_service_name_and_methods() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Req {}\n",
        "message Resp {}\n",
        "service Svc {\n",                 // line 3
        "  rpc Do(Req) returns (Resp);\n", // line 4
        "}\n",
    );
    let file = parse(source).unwrap();

    // service name: path=[6, 0, 1]
    let span = find_location(&file, &[6, 0, 1]).expect("service name location");
    assert_eq!(span[0], 3, "service name on line 3");

    // method[0]: path=[6, 0, 2, 0]
    let span = find_location(&file, &[6, 0, 2, 0]).expect("method[0] location");
    assert_eq!(span[0], 4, "method on line 4");

    // method name: path=[6, 0, 2, 0, 1]
    let span = find_location(&file, &[6, 0, 2, 0, 1]).expect("method name location");
    assert_eq!(span[0], 4, "method name on line 4");

    // method input_type: path=[6, 0, 2, 0, 2]
    let span = find_location(&file, &[6, 0, 2, 0, 2]).expect("method input_type location");
    assert_eq!(span[0], 4, "input_type on line 4");

    // method output_type: path=[6, 0, 2, 0, 3]
    let span = find_location(&file, &[6, 0, 2, 0, 3]).expect("method output_type location");
    assert_eq!(span[0], 4, "output_type on line 4");
}

#[test]
fn source_info_oneof() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Foo {\n",     // line 1
        "  oneof bar {\n",     // line 2
        "    int32 x = 1;\n",  // line 3
        "    string y = 2;\n", // line 4
        "  }\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // oneof[0]: path=[4, 0, 8, 0]
    let span = find_location(&file, &[4, 0, 8, 0]).expect("oneof[0] location");
    assert_eq!(span[0], 2, "oneof on line 2");

    // oneof name: path=[4, 0, 8, 0, 1]
    let span = find_location(&file, &[4, 0, 8, 0, 1]).expect("oneof name location");
    assert_eq!(span[0], 2, "oneof name on line 2");
}

#[test]
fn source_info_nested_enum_in_message() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Foo {\n", // line 1
        "  enum Bar {\n",  // line 2
        "    X = 0;\n",    // line 3
        "  }\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // nested enum: path=[4, 0, 4, 0]
    let span = find_location(&file, &[4, 0, 4, 0]).expect("nested enum location");
    assert_eq!(span[0], 2, "nested enum on line 2");

    // nested enum name: path=[4, 0, 4, 0, 1]
    let span = find_location(&file, &[4, 0, 4, 0, 1]).expect("nested enum name");
    assert_eq!(span[0], 2, "nested enum name on line 2");

    // nested enum value[0]: path=[4, 0, 4, 0, 2, 0]
    let span = find_location(&file, &[4, 0, 4, 0, 2, 0]).expect("nested enum value");
    assert_eq!(span[0], 3, "nested enum value on line 3");
}

#[test]
fn source_info_reserved_and_extensions() {
    let source = concat!(
        "syntax = \"proto2\";\n",
        "message Foo {\n",            // line 1
        "  reserved 1, 2 to 5;\n",    // line 2
        "  extensions 100 to 200;\n", // line 3
        "  optional int32 x = 10;\n", // line 4
        "}\n",
    );
    let file = parse(source).unwrap();

    // reserved_range[0]: path=[4, 0, 9, 0]
    let span = find_location(&file, &[4, 0, 9, 0]).expect("reserved_range[0]");
    assert_eq!(span[0], 2, "reserved on line 2");

    // reserved_range[1]: path=[4, 0, 9, 1]
    let span = find_location(&file, &[4, 0, 9, 1]).expect("reserved_range[1]");
    assert_eq!(span[0], 2, "reserved range 2 on line 2");

    // extension_range[0]: path=[4, 0, 5, 0]
    let span = find_location(&file, &[4, 0, 5, 0]).expect("extension_range[0]");
    assert_eq!(span[0], 3, "extension range on line 3");
}

#[test]
fn source_info_map_field() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Foo {\n",
        "  map<string, int32> tags = 1;\n", // line 2
        "}\n",
    );
    let file = parse(source).unwrap();

    // map field: path=[4, 0, 2, 0]
    let span = find_location(&file, &[4, 0, 2, 0]).expect("map field location");
    assert_eq!(span[0], 2, "map field on line 2");
}

#[test]
fn source_info_file_options() {
    let source = concat!(
        "syntax = \"proto2\";\n",
        "option java_package = \"com.example\";\n", // line 1
    );
    let file = parse(source).unwrap();

    // file options: path=[8]
    let span = find_location(&file, &[8]).expect("file options location");
    assert_eq!(span[0], 1, "file option on line 1");
}

#[test]
fn source_info_file_extensions() {
    let source = concat!(
        "syntax = \"proto2\";\n",
        "message Foo {\n",
        "  extensions 100 to 200;\n",
        "  optional int32 x = 1;\n",
        "}\n",
        "extend Foo {\n",                // line 5
        "  optional int32 bar = 100;\n", // line 6 - the actual extension field
        "}\n",
    );
    let file = parse(source).unwrap();

    // file extension[0]: path=[7, 0] - span covers the field, not the extend keyword
    let span = find_location(&file, &[7, 0]).expect("file extension[0]");
    assert_eq!(span[0], 6, "extension field on line 6");

    // extension field extendee: path=[7, 0, 2] (FIELD_EXTENDEE)
    let span = find_location(&file, &[7, 0, 2]).expect("extension extendee");
    assert_eq!(span[0], 5, "extendee on line 5"); // "Foo" after "extend"

    // extension field label: path=[7, 0, 4]
    let span = find_location(&file, &[7, 0, 4]).expect("extension label");
    assert_eq!(span[0], 6, "extension label on line 6");

    // extension field name: path=[7, 0, 1]
    let span = find_location(&file, &[7, 0, 1]).expect("extension field name");
    assert_eq!(span[0], 6, "extension name on line 6");
}

#[test]
fn source_info_enum_reserved() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "enum Foo {\n",          // line 1
        "  X = 0;\n",            // line 2
        "  reserved 1 to 5;\n",  // line 3
        "  reserved \"BAR\";\n", // line 4
        "}\n",
    );
    let file = parse(source).unwrap();

    // enum reserved_range[0]: path=[5, 0, 4, 0]
    let span = find_location(&file, &[5, 0, 4, 0]).expect("enum reserved_range[0]");
    assert_eq!(span[0], 3, "enum reserved range on line 3");

    // enum reserved_name[0]: path=[5, 0, 5, 0]
    let span = find_location(&file, &[5, 0, 5, 0]).expect("enum reserved_name[0]");
    assert_eq!(span[0], 4, "enum reserved name on line 4");
}

#[test]
fn source_info_streaming_methods() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Req {}\n",
        "message Resp {}\n",
        "service Svc {\n",                                 // line 3
        "  rpc Bidi(stream Req) returns (stream Resp);\n", // line 4
        "}\n",
    );
    let file = parse(source).unwrap();

    // method[0] client_streaming: path=[6, 0, 2, 0, 5]
    let span = find_location(&file, &[6, 0, 2, 0, 5]).expect("client_streaming");
    assert_eq!(span[0], 4, "client streaming on line 4");

    // method[0] server_streaming: path=[6, 0, 2, 0, 6]
    let span = find_location(&file, &[6, 0, 2, 0, 6]).expect("server_streaming");
    assert_eq!(span[0], 4, "server streaming on line 4");
}

#[test]
fn source_info_message_options() {
    let source = concat!(
        "syntax = \"proto2\";\n",
        "message Foo {\n",                            // line 1
        "  option message_set_wire_format = true;\n", // line 2
        "  optional int32 x = 1;\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // Verify the message has the option set
    assert!(file.message_type[0]
        .options
        .as_ref()
        .unwrap()
        .message_set_wire_format
        .is_some());
}

#[test]
fn source_info_leading_comments() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "// Leading comment for Foo\n",
        "message Foo {\n",
        "  // Leading comment for bar\n",
        "  int32 bar = 1;\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // message[0]: path=[4, 0]
    let loc = find_location_full(&file, &[4, 0]).expect("message location");
    assert_eq!(
        loc.leading_comments.as_deref(),
        Some("Leading comment for Foo\n"),
        "message leading comment"
    );

    // field[0]: path=[4, 0, 2, 0]
    let loc = find_location_full(&file, &[4, 0, 2, 0]).expect("field location");
    assert_eq!(
        loc.leading_comments.as_deref(),
        Some("Leading comment for bar\n"),
        "field leading comment"
    );
}

#[test]
fn source_info_trailing_comments() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "enum Color {\n",
        "  RED = 0; // The color red\n",
        "  GREEN = 1; // The color green\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // enum value[0]: path=[5, 0, 2, 0]
    let loc = find_location_full(&file, &[5, 0, 2, 0]).expect("value[0]");
    assert_eq!(
        loc.trailing_comments.as_deref(),
        Some("The color red\n"),
        "trailing comment on RED"
    );

    // enum value[1]: path=[5, 0, 2, 1]
    let loc = find_location_full(&file, &[5, 0, 2, 1]).expect("value[1]");
    assert_eq!(
        loc.trailing_comments.as_deref(),
        Some("The color green\n"),
        "trailing comment on GREEN"
    );
}

#[test]
fn source_info_detached_comments() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "\n",
        "// Detached comment 1\n",
        "\n",
        "// Detached comment 2\n",
        "\n",
        "// Leading comment\n",
        "message Foo {}\n",
    );
    let file = parse(source).unwrap();

    // message[0]: path=[4, 0]
    let loc = find_location_full(&file, &[4, 0]).expect("message location");
    assert_eq!(
        loc.leading_comments.as_deref(),
        Some("Leading comment\n"),
        "leading comment"
    );
    assert_eq!(
        loc.leading_detached_comments.len(),
        2,
        "should have 2 detached comments"
    );
    assert_eq!(loc.leading_detached_comments[0], "Detached comment 1\n");
    assert_eq!(loc.leading_detached_comments[1], "Detached comment 2\n");
}

#[test]
fn source_info_block_comment() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "/* Block leading */\n",
        "message Foo {}\n",
    );
    let file = parse(source).unwrap();

    let loc = find_location_full(&file, &[4, 0]).expect("message location");
    assert_eq!(
        loc.leading_comments.as_deref(),
        Some("Block leading \n"),
        "block comment as leading"
    );
}

#[test]
fn source_info_method_comments() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "message Req {}\n",
        "message Resp {}\n",
        "service Svc {\n",
        "  // The main RPC\n",
        "  rpc Do(Req) returns (Resp);\n",
        "}\n",
    );
    let file = parse(source).unwrap();

    // method[0]: path=[6, 0, 2, 0]
    let loc = find_location_full(&file, &[6, 0, 2, 0]).expect("method location");
    assert_eq!(
        loc.leading_comments.as_deref(),
        Some("The main RPC\n"),
        "method leading comment"
    );
}
