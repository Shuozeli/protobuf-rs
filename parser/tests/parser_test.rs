/// Comprehensive parser tests modeled after the C++ parser_unittest.cc.
///
/// Categories:
///   1. Basic parsing (syntax, BOM, empty files)
///   2. Message parsing (fields, labels, types, nesting)
///   3. Field options and defaults
///   4. Oneof, map, group
///   5. Enum parsing
///   6. Service/RPC parsing
///   7. Import and package
///   8. Extensions and extend
///   9. Reserved fields and ranges
///  10. Custom options (uninterpreted)
///  11. Parse error tests
///
/// Test tiers:
///   - parse_ok:   Parses without error
///   - parses_to:  Parses and validates structure
///   - has_errors: Expects specific parse errors
use protoc_rs_parser::{parse, parse_collecting};
use protoc_rs_schema::*;
use protoc_rs_test_utils::*;

// =========================================================================
// 1. BASIC PARSING
// =========================================================================

#[test]
fn empty_file() {
    let file = parse("").unwrap();
    assert!(file.message_type.is_empty());
    assert!(file.enum_type.is_empty());
    assert!(file.service.is_empty());
}

#[test]
fn syntax_proto2() {
    let file = parse("syntax = \"proto2\";").unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
}

#[test]
fn syntax_proto3() {
    let file = parse("syntax = \"proto3\";").unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto3);
}

#[test]
fn implicit_syntax_defaults_to_proto2() {
    let file = parse("message Foo {}").unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
}

#[test]
fn comments_only_file() {
    let file = parse("// just a comment\n/* block comment */\n").unwrap();
    assert!(file.message_type.is_empty());
}

// =========================================================================
// 2. MESSAGE PARSING
// =========================================================================

#[test]
fn simple_message() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            required int32 foo = 1;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.field.len(), 1);
    let f = find_field(msg, "foo");
    assert_eq!(f.r#type, Some(FieldType::Int32));
    assert_eq!(f.number, Some(1));
    assert_eq!(f.label, Some(FieldLabel::Required));
}

#[test]
fn simple_fields_all_labels() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            required int32 foo = 15;
            optional int32 bar = 34;
            repeated int32 baz = 3;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.field.len(), 3);
    assert_eq!(find_field(msg, "foo").label, Some(FieldLabel::Required));
    assert_eq!(find_field(msg, "bar").label, Some(FieldLabel::Optional));
    assert_eq!(find_field(msg, "baz").label, Some(FieldLabel::Repeated));
    assert_eq!(find_field(msg, "foo").number, Some(15));
    assert_eq!(find_field(msg, "bar").number, Some(34));
    assert_eq!(find_field(msg, "baz").number, Some(3));
}

#[test]
fn primitive_field_types() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            required int32    a = 1;
            required int64    b = 2;
            required uint32   c = 3;
            required uint64   d = 4;
            required sint32   e = 5;
            required sint64   f = 6;
            required fixed32  g = 7;
            required fixed64  h = 8;
            required sfixed32 i = 9;
            required sfixed64 j = 10;
            required float    k = 11;
            required double   l = 12;
            required string   m = 13;
            required bytes    n = 14;
            required bool     o = 15;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.field.len(), 15);

    let expected: Vec<(&str, FieldType)> = vec![
        ("a", FieldType::Int32),
        ("b", FieldType::Int64),
        ("c", FieldType::Uint32),
        ("d", FieldType::Uint64),
        ("e", FieldType::Sint32),
        ("f", FieldType::Sint64),
        ("g", FieldType::Fixed32),
        ("h", FieldType::Fixed64),
        ("i", FieldType::Sfixed32),
        ("j", FieldType::Sfixed64),
        ("k", FieldType::Float),
        ("l", FieldType::Double),
        ("m", FieldType::String),
        ("n", FieldType::Bytes),
        ("o", FieldType::Bool),
    ];
    for (name, ft) in expected {
        assert_eq!(
            find_field(msg, name).r#type,
            Some(ft),
            "type mismatch for {}",
            name
        );
    }
}

#[test]
fn message_type_field() {
    let file = parse(
        r#"
        syntax = "proto2";
        message Outer {
            optional Inner inner = 1;
        }
        message Inner {
            optional int32 x = 1;
        }
    "#,
    )
    .unwrap();
    let outer = find_msg(&file, "Outer");
    let f = find_field(outer, "inner");
    assert_eq!(f.type_name, Some("Inner".to_string()));
    // Type will be Message once resolved, but at parse time may be unset
    // for user-defined types. Our parser sets it to Message for type_name fields.
    assert!(
        f.r#type == Some(FieldType::Message) || f.r#type.is_none(),
        "type for message field should be Message or unset, got {:?}",
        f.r#type
    );
}

#[test]
fn nested_message() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Outer {
            message Inner {
                int32 x = 1;
            }
            Inner inner = 1;
        }
    "#,
    )
    .unwrap();
    let outer = find_msg(&file, "Outer");
    assert_eq!(outer.nested_type.len(), 1);
    let inner = find_nested_msg(outer, "Inner");
    assert_eq!(inner.field.len(), 1);
    assert_eq!(find_field(inner, "x").r#type, Some(FieldType::Int32));
}

#[test]
fn nested_enum_in_message() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Outer {
            enum Color {
                RED = 0;
                GREEN = 1;
                BLUE = 2;
            }
            Color color = 1;
        }
    "#,
    )
    .unwrap();
    let outer = find_msg(&file, "Outer");
    assert_eq!(outer.enum_type.len(), 1);
    let color = find_nested_enum(outer, "Color");
    assert_eq!(color.value.len(), 3);
    assert_eq!(find_enum_value(color, "RED").number, Some(0));
    assert_eq!(find_enum_value(color, "GREEN").number, Some(1));
    assert_eq!(find_enum_value(color, "BLUE").number, Some(2));
}

#[test]
fn deeply_nested_messages() {
    let file = parse(
        r#"
        syntax = "proto3";
        message A {
            message B {
                message C {
                    int32 val = 1;
                }
            }
        }
    "#,
    )
    .unwrap();
    let a = find_msg(&file, "A");
    let b = find_nested_msg(a, "B");
    let c = find_nested_msg(b, "C");
    assert_eq!(c.field.len(), 1);
    assert_eq!(find_field(c, "val").number, Some(1));
}

#[test]
fn multiple_messages() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Foo { int32 a = 1; }
        message Bar { string b = 1; }
        message Baz { bool c = 1; }
    "#,
    )
    .unwrap();
    assert_eq!(file.message_type.len(), 3);
    find_msg(&file, "Foo");
    find_msg(&file, "Bar");
    find_msg(&file, "Baz");
}

#[test]
fn empty_message() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Empty {}
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "Empty");
    assert!(msg.field.is_empty());
    assert!(msg.nested_type.is_empty());
    assert!(msg.enum_type.is_empty());
}

#[test]
fn proto3_fields_no_label() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            int32 foo = 1;
            string bar = 2;
            repeated double baz = 3;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.field.len(), 3);
    // Proto3 singular fields have no explicit label (defaults to optional in descriptor)
    let foo = find_field(msg, "foo");
    assert_eq!(foo.r#type, Some(FieldType::Int32));
    let baz = find_field(msg, "baz");
    assert_eq!(baz.label, Some(FieldLabel::Repeated));
}

#[test]
fn proto3_optional_field() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            optional int32 foo = 1;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "foo");
    assert_eq!(f.label, Some(FieldLabel::Optional));
    assert_eq!(f.proto3_optional, Some(true));
}

// =========================================================================
// 3. FIELD OPTIONS AND DEFAULTS
// =========================================================================

#[test]
fn field_default_values() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional int32 foo = 1 [default = 42];
            optional string bar = 2 [default = "hello"];
            optional bool baz = 3 [default = true];
            optional float qux = 4 [default = 1.5];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(find_field(msg, "foo").default_value, Some("42".to_string()));
    assert_eq!(
        find_field(msg, "bar").default_value,
        Some("hello".to_string())
    );
    assert_eq!(
        find_field(msg, "baz").default_value,
        Some("true".to_string())
    );
    assert_eq!(
        find_field(msg, "qux").default_value,
        Some("1.5".to_string())
    );
}

#[test]
fn field_default_negative_int() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional int32 foo = 1 [default = -42];
            optional sint64 bar = 2 [default = -9223372036854775808];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(
        find_field(msg, "foo").default_value,
        Some("-42".to_string())
    );
}

#[test]
fn field_default_special_floats() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional float a = 1 [default = inf];
            optional float b = 2 [default = -inf];
            optional float c = 3 [default = nan];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(find_field(msg, "a").default_value, Some("inf".to_string()));
    assert_eq!(find_field(msg, "b").default_value, Some("-inf".to_string()));
    assert_eq!(find_field(msg, "c").default_value, Some("nan".to_string()));
}

#[test]
fn field_default_hex() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional int32 foo = 1 [default = 0x1F];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    // Hex values may be stored as decimal or hex string
    let val = find_field(msg, "foo").default_value.as_ref().unwrap();
    assert!(
        val == "31" || val == "0x1F" || val == "0x1f",
        "unexpected default: {}",
        val
    );
}

#[test]
fn field_default_enum_value() {
    let file = parse(
        r#"
        syntax = "proto2";
        enum MyEnum { A = 0; B = 1; }
        message TestMessage {
            optional MyEnum e = 1 [default = B];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(find_field(msg, "e").default_value, Some("B".to_string()));
}

#[test]
fn field_packed_option() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            repeated int32 values = 1 [packed = true];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "values");
    assert_eq!(f.options.as_ref().unwrap().packed, Some(true));
}

#[test]
fn field_deprecated_option() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            int32 old_field = 1 [deprecated = true];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "old_field");
    assert_eq!(f.options.as_ref().unwrap().deprecated, Some(true));
}

#[test]
fn field_json_name() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            string foo_bar = 1 [json_name = "@type"];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "foo_bar");
    assert_eq!(f.json_name, Some("@type".to_string()));
}

#[test]
fn field_ctype_option() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional string a = 1 [ctype = STRING_PIECE];
            optional string b = 2 [ctype = CORD];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(
        find_field(msg, "a").options.as_ref().unwrap().ctype,
        Some(FieldCType::StringPiece)
    );
    assert_eq!(
        find_field(msg, "b").options.as_ref().unwrap().ctype,
        Some(FieldCType::Cord)
    );
}

#[test]
fn multiple_field_options() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            repeated int32 values = 1 [packed = true, deprecated = true];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let opts = find_field(msg, "values").options.as_ref().unwrap();
    assert_eq!(opts.packed, Some(true));
    assert_eq!(opts.deprecated, Some(true));
}

// =========================================================================
// 4. ONEOF, MAP, GROUP
// =========================================================================

#[test]
fn basic_oneof() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            oneof test_oneof {
                int32 foo = 1;
                string bar = 2;
            }
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.oneof_decl.len(), 1);
    assert_eq!(msg.oneof_decl[0].name, Some("test_oneof".to_string()));

    let foo = find_field(msg, "foo");
    assert_eq!(foo.oneof_index, Some(0));
    let bar = find_field(msg, "bar");
    assert_eq!(bar.oneof_index, Some(0));
}

#[test]
fn multiple_oneofs() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            oneof first {
                int32 a = 1;
            }
            oneof second {
                string b = 2;
            }
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.oneof_decl.len(), 2);
    assert_eq!(find_field(msg, "a").oneof_index, Some(0));
    assert_eq!(find_field(msg, "b").oneof_index, Some(1));
}

#[test]
fn map_field() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            map<string, int32> my_map = 1;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "my_map");
    // Map fields are desugared to repeated message
    assert_eq!(f.label, Some(FieldLabel::Repeated));
    assert_eq!(f.r#type, Some(FieldType::Message));

    // Should have a synthetic nested type for the map entry
    assert_eq!(msg.nested_type.len(), 1);
    let entry = &msg.nested_type[0];
    assert_eq!(entry.options.as_ref().unwrap().map_entry, Some(true));

    // Entry has key and value fields
    let key = find_field(entry, "key");
    assert_eq!(key.r#type, Some(FieldType::String));
    assert_eq!(key.number, Some(1));
    let value = find_field(entry, "value");
    assert_eq!(value.r#type, Some(FieldType::Int32));
    assert_eq!(value.number, Some(2));
}

#[test]
fn map_with_message_value() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            map<int32, Inner> msg_map = 1;
            message Inner { int32 x = 1; }
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    // 1 explicit nested (Inner) + 1 synthetic map entry
    assert_eq!(msg.nested_type.len(), 2);
}

#[test]
fn map_various_key_types() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            map<int32, string> a = 1;
            map<int64, string> b = 2;
            map<uint32, string> c = 3;
            map<uint64, string> d = 4;
            map<sint32, string> e = 5;
            map<sint64, string> f = 6;
            map<fixed32, string> g = 7;
            map<fixed64, string> h = 8;
            map<sfixed32, string> i = 9;
            map<sfixed64, string> j = 10;
            map<bool, string> k = 11;
            map<string, string> l = 12;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.field.len(), 12);
    // All map entries should be repeated message type
    for f in &msg.field {
        assert_eq!(f.label, Some(FieldLabel::Repeated));
        assert_eq!(f.r#type, Some(FieldType::Message));
    }
}

#[test]
fn group_field_proto2() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            optional group MyGroup = 1 {
                optional int32 a = 2;
            }
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    // Group creates a field + nested message
    let f = find_field(msg, "mygroup"); // group field name is lowercased
    assert_eq!(f.r#type, Some(FieldType::Group));
    assert_eq!(f.number, Some(1));
    assert_eq!(f.type_name, Some("MyGroup".to_string()));

    let nested = find_nested_msg(msg, "MyGroup");
    assert_eq!(nested.field.len(), 1);
    assert_eq!(find_field(nested, "a").number, Some(2));
}

// =========================================================================
// 5. ENUM PARSING
// =========================================================================

#[test]
fn simple_enum() {
    let file = parse(
        r#"
        syntax = "proto3";
        enum TestEnum {
            UNKNOWN = 0;
            FOO = 1;
            BAR = 2;
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    assert_eq!(e.value.len(), 3);
    assert_eq!(find_enum_value(e, "UNKNOWN").number, Some(0));
    assert_eq!(find_enum_value(e, "FOO").number, Some(1));
    assert_eq!(find_enum_value(e, "BAR").number, Some(2));
}

#[test]
fn enum_with_negative_values() {
    let file = parse(
        r#"
        syntax = "proto2";
        enum TestEnum {
            ZERO = 0;
            NEG = -1;
            BIG = 0x7FFFFFFF;
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    assert_eq!(find_enum_value(e, "NEG").number, Some(-1));
}

#[test]
fn enum_with_allow_alias() {
    let file = parse(
        r#"
        syntax = "proto2";
        enum TestEnum {
            option allow_alias = true;
            FOO = 0;
            BAR = 0;
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    assert_eq!(e.options.as_ref().unwrap().allow_alias, Some(true));
    assert_eq!(e.value.len(), 2);
    assert_eq!(find_enum_value(e, "FOO").number, Some(0));
    assert_eq!(find_enum_value(e, "BAR").number, Some(0));
}

#[test]
fn enum_value_options() {
    let file = parse(
        r#"
        syntax = "proto3";
        enum TestEnum {
            UNKNOWN = 0;
            FOO = 1 [deprecated = true];
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    let foo = find_enum_value(e, "FOO");
    assert_eq!(foo.options.as_ref().unwrap().deprecated, Some(true));
}

#[test]
fn enum_reserved_ranges() {
    let file = parse(
        r#"
        syntax = "proto3";
        enum TestEnum {
            UNKNOWN = 0;
            reserved 1 to 5, 10;
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    assert_eq!(e.reserved_range.len(), 2);
    assert_eq!(e.reserved_range[0].start, Some(1));
    assert_eq!(e.reserved_range[0].end, Some(5)); // inclusive for enum ranges
    assert_eq!(e.reserved_range[1].start, Some(10));
    assert_eq!(e.reserved_range[1].end, Some(10)); // inclusive for enum ranges
}

#[test]
fn enum_reserved_names() {
    let file = parse(
        r#"
        syntax = "proto2";
        enum TestEnum {
            UNKNOWN = 0;
            reserved "FOO", "BAR";
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "TestEnum");
    assert_eq!(e.reserved_name.len(), 2);
    assert!(e.reserved_name.contains(&"FOO".to_string()));
    assert!(e.reserved_name.contains(&"BAR".to_string()));
}

// =========================================================================
// 6. SERVICE / RPC
// =========================================================================

#[test]
fn simple_service() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Request {}
        message Response {}
        service TestService {
            rpc DoSomething(Request) returns (Response);
        }
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "TestService");
    assert_eq!(svc.method.len(), 1);
    let m = find_method(svc, "DoSomething");
    assert_eq!(m.input_type, Some("Request".to_string()));
    assert_eq!(m.output_type, Some("Response".to_string()));
    assert_ne!(m.client_streaming, Some(true));
    assert_ne!(m.server_streaming, Some(true));
}

#[test]
fn service_with_streaming() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Req {}
        message Resp {}
        service StreamService {
            rpc ServerStream(Req) returns (stream Resp);
            rpc ClientStream(stream Req) returns (Resp);
            rpc BidiStream(stream Req) returns (stream Resp);
        }
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "StreamService");
    assert_eq!(svc.method.len(), 3);

    let ss = find_method(svc, "ServerStream");
    assert_ne!(ss.client_streaming, Some(true));
    assert_eq!(ss.server_streaming, Some(true));

    let cs = find_method(svc, "ClientStream");
    assert_eq!(cs.client_streaming, Some(true));
    assert_ne!(cs.server_streaming, Some(true));

    let bidi = find_method(svc, "BidiStream");
    assert_eq!(bidi.client_streaming, Some(true));
    assert_eq!(bidi.server_streaming, Some(true));
}

#[test]
fn service_with_options() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Req {}
        message Resp {}
        service TestService {
            rpc Foo(Req) returns (Resp) {
                option deprecated = true;
            }
        }
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "TestService");
    let m = find_method(svc, "Foo");
    assert_eq!(m.options.as_ref().unwrap().deprecated, Some(true));
}

#[test]
fn multiple_services() {
    let file = parse(
        r#"
        syntax = "proto3";
        message M {}
        service A { rpc Foo(M) returns (M); }
        service B { rpc Bar(M) returns (M); }
    "#,
    )
    .unwrap();
    assert_eq!(file.service.len(), 2);
    find_service(&file, "A");
    find_service(&file, "B");
}

// =========================================================================
// 7. IMPORT AND PACKAGE
// =========================================================================

#[test]
fn single_import() {
    let file = parse(
        r#"
        syntax = "proto3";
        import "foo/bar/baz.proto";
    "#,
    )
    .unwrap();
    assert_eq!(file.dependency.len(), 1);
    assert_eq!(file.dependency[0], "foo/bar/baz.proto");
}

#[test]
fn multiple_imports() {
    let file = parse(
        r#"
        syntax = "proto3";
        import "foo.proto";
        import "bar.proto";
        import "baz.proto";
    "#,
    )
    .unwrap();
    assert_eq!(file.dependency.len(), 3);
}

#[test]
fn public_import() {
    let file = parse(
        r#"
        syntax = "proto3";
        import "foo.proto";
        import public "bar.proto";
    "#,
    )
    .unwrap();
    assert_eq!(file.dependency.len(), 2);
    assert_eq!(file.public_dependency.len(), 1);
    // public_dependency contains index into dependency list
    assert_eq!(file.public_dependency[0], 1);
}

#[test]
fn weak_import() {
    let file = parse(
        r#"
        syntax = "proto2";
        import weak "weak_dep.proto";
    "#,
    )
    .unwrap();
    assert_eq!(file.dependency.len(), 1);
    assert_eq!(file.weak_dependency.len(), 1);
    assert_eq!(file.weak_dependency[0], 0);
}

#[test]
fn package_declaration() {
    let file = parse(
        r#"
        syntax = "proto3";
        package foo.bar.baz;
    "#,
    )
    .unwrap();
    assert_eq!(file.package, Some("foo.bar.baz".to_string()));
}

#[test]
fn no_package() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Foo {}
    "#,
    )
    .unwrap();
    assert!(file.package.is_none());
}

// =========================================================================
// 8. EXTENSIONS AND EXTEND
// =========================================================================

#[test]
fn extension_range() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            extensions 100 to 199;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.extension_range.len(), 1);
    assert_eq!(msg.extension_range[0].start, Some(100));
    assert_eq!(msg.extension_range[0].end, Some(200)); // exclusive
}

#[test]
fn compound_extension_range() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            extensions 1 to 5, 10 to 20, 100 to max;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.extension_range.len(), 3);
    assert_eq!(msg.extension_range[0].start, Some(1));
    assert_eq!(msg.extension_range[0].end, Some(6));
    assert_eq!(msg.extension_range[1].start, Some(10));
    assert_eq!(msg.extension_range[1].end, Some(21));
}

#[test]
fn extend_block() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            extensions 100 to 199;
        }
        extend TestMessage {
            optional int32 bar = 100;
        }
    "#,
    )
    .unwrap();
    assert_eq!(file.extension.len(), 1);
    let ext = &file.extension[0];
    assert_eq!(ext.name, Some("bar".to_string()));
    assert_eq!(ext.number, Some(100));
    assert_eq!(ext.extendee, Some("TestMessage".to_string()));
}

#[test]
fn multiple_extensions_one_extendee() {
    let file = parse(
        r#"
        syntax = "proto2";
        message Foo { extensions 1 to 100; }
        extend Foo {
            optional int32 bar = 1;
            optional string baz = 2;
        }
    "#,
    )
    .unwrap();
    assert_eq!(file.extension.len(), 2);
    assert_eq!(file.extension[0].extendee, Some("Foo".to_string()));
    assert_eq!(file.extension[1].extendee, Some("Foo".to_string()));
}

#[test]
fn extensions_in_message_scope() {
    let file = parse(
        r#"
        syntax = "proto2";
        message Outer {
            extensions 100 to 199;
            extend Outer {
                optional int32 inner_ext = 100;
            }
        }
    "#,
    )
    .unwrap();
    let outer = find_msg(&file, "Outer");
    assert_eq!(outer.extension.len(), 1);
    assert_eq!(outer.extension[0].name, Some("inner_ext".to_string()));
}

// =========================================================================
// 9. RESERVED FIELDS AND RANGES
// =========================================================================

#[test]
fn reserved_range() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            reserved 1 to 5, 10;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.reserved_range.len(), 2);
    assert_eq!(msg.reserved_range[0].start, Some(1));
    assert_eq!(msg.reserved_range[0].end, Some(6)); // exclusive
    assert_eq!(msg.reserved_range[1].start, Some(10));
    assert_eq!(msg.reserved_range[1].end, Some(11)); // exclusive
}

#[test]
fn reserved_names() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            reserved "foo", "bar";
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.reserved_name.len(), 2);
    assert!(msg.reserved_name.contains(&"foo".to_string()));
    assert!(msg.reserved_name.contains(&"bar".to_string()));
}

#[test]
fn reserved_max() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            reserved 1 to max;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.reserved_range.len(), 1);
    assert_eq!(msg.reserved_range[0].start, Some(1));
    // max should be a very large number (536870911 = 0x1FFFFFFF)
    assert!(msg.reserved_range[0].end.unwrap() > 1000);
}

// =========================================================================
// 10. FILE OPTIONS AND CUSTOM OPTIONS
// =========================================================================

#[test]
fn file_options_java_package() {
    let file = parse(
        r#"
        syntax = "proto3";
        option java_package = "com.example.foo";
        option java_multiple_files = true;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.java_package, Some("com.example.foo".to_string()));
    assert_eq!(opts.java_multiple_files, Some(true));
}

#[test]
fn file_option_optimize_for() {
    let file = parse(
        r#"
        syntax = "proto3";
        option optimize_for = SPEED;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.optimize_for, Some(OptimizeMode::Speed));
}

#[test]
fn file_option_optimize_for_code_size() {
    let file = parse(
        r#"
        syntax = "proto3";
        option optimize_for = CODE_SIZE;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.optimize_for, Some(OptimizeMode::CodeSize));
}

#[test]
fn file_option_optimize_for_lite() {
    let file = parse(
        r#"
        syntax = "proto3";
        option optimize_for = LITE_RUNTIME;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.optimize_for, Some(OptimizeMode::LiteRuntime));
}

#[test]
fn file_option_go_package() {
    let file = parse(
        r#"
        syntax = "proto3";
        option go_package = "example.com/proto";
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.go_package, Some("example.com/proto".to_string()));
}

#[test]
fn message_option_deprecated() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            option deprecated = true;
            int32 x = 1;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.options.as_ref().unwrap().deprecated, Some(true));
}

#[test]
fn custom_option_stored_as_uninterpreted() {
    let file = parse(
        r#"
        syntax = "proto3";
        import "google/protobuf/descriptor.proto";
        option (my_custom_option) = 42;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert!(!opts.uninterpreted_option.is_empty());
    let uo = &opts.uninterpreted_option[0];
    assert_eq!(uo.positive_int_value, Some(42));
}

#[test]
fn custom_option_with_dotted_name() {
    let file = parse(
        r#"
        syntax = "proto3";
        option (com.example.my_option) = "hello";
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert!(!opts.uninterpreted_option.is_empty());
    let uo = &opts.uninterpreted_option[0];
    assert_eq!(uo.string_value, Some(b"hello".to_vec()));
}

#[test]
fn custom_option_aggregate_value() {
    let file = parse(
        r#"
        syntax = "proto3";
        option (my_option) = { key: "value" num: 42 };
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert!(!opts.uninterpreted_option.is_empty());
    let uo = &opts.uninterpreted_option[0];
    assert!(uo.aggregate_value.is_some());
}

#[test]
fn complex_option_name_with_chained_extensions() {
    // Tests the fix for unittest_custom_options.proto line 337
    let file = parse(
        r#"
        syntax = "proto2";
        message Foo {
            option (ext1).(.ext2) = 42;
            option (ext1).(ext3).sub = 100;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "Foo");
    let opts = msg.options.as_ref().unwrap();
    assert_eq!(opts.uninterpreted_option.len(), 2);
}

// =========================================================================
// 11. PARSE ERROR TESTS
// =========================================================================

macro_rules! expect_error {
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            assert!(
                parse($source).is_err(),
                "expected parse error for {:?}",
                stringify!($name)
            );
        }
    };
    ($name:ident, $source:expr, $substr:expr) => {
        #[test]
        fn $name() {
            let err = parse($source).unwrap_err();
            assert!(
                err.to_string().contains($substr),
                "error '{}' should contain '{}'",
                err,
                $substr
            );
        }
    };
}

// -- Syntax errors --

expect_error!(
    err_missing_syntax_semicolon,
    "syntax = \"proto3\"\nmessage Foo {}"
);

expect_error!(err_invalid_syntax_value, "syntax = \"foobar\";");

// -- Top-level errors --

expect_error!(err_unexpected_top_level, "syntax = \"proto3\";\n} }");

expect_error!(err_garbage_top_level, "syntax = \"proto3\";\nblah;");

// -- Message errors --

expect_error!(err_message_missing_name, "syntax = \"proto3\";\nmessage {}");

expect_error!(
    err_message_missing_body,
    "syntax = \"proto3\";\nmessage Foo;"
);

expect_error!(err_eof_in_message, "syntax = \"proto3\";\nmessage Foo {");

// -- Field errors --

expect_error!(
    err_missing_field_number,
    "syntax = \"proto2\";\nmessage Foo { optional int32 x; }"
);

expect_error!(
    err_missing_field_number_value,
    "syntax = \"proto2\";\nmessage Foo { optional int32 x = ; }"
);

expect_error!(
    err_invalid_field_number,
    "syntax = \"proto3\";\nmessage Foo { int32 x = abc; }"
);

// -- Enum errors --

expect_error!(err_enum_missing_name, "syntax = \"proto3\";\nenum {}");

expect_error!(err_enum_missing_body, "syntax = \"proto3\";\nenum Foo;");

expect_error!(
    err_enum_value_missing_number,
    "syntax = \"proto3\";\nenum Foo { BAR; }"
);

expect_error!(err_eof_in_enum, "syntax = \"proto3\";\nenum Foo { BAR = 0;");

// -- Service errors --

expect_error!(err_service_missing_name, "syntax = \"proto3\";\nservice {}");

expect_error!(err_eof_in_service, "syntax = \"proto3\";\nservice Foo {");

// -- Import errors --

expect_error!(err_import_not_quoted, "syntax = \"proto3\";\nimport foo;");

// -- String literal errors --

expect_error!(err_unterminated_string, r#"syntax = "proto3"#);

// -- Option errors --

expect_error!(
    err_empty_field_options,
    "syntax = \"proto2\";\nmessage Foo { optional uint32 x = 1 []; }"
);

// -- Oneof errors --

expect_error!(
    err_oneof_missing_name,
    "syntax = \"proto3\";\nmessage Foo { oneof { int32 x = 1; } }"
);

// -- Map errors --

expect_error!(
    err_map_missing_key_type,
    "syntax = \"proto3\";\nmessage Foo { map<, string> m = 1; }"
);

expect_error!(
    err_map_missing_close,
    "syntax = \"proto3\";\nmessage Foo { map<string, string m = 1; }"
);

// -- Multiple packages --

expect_error!(
    err_multiple_packages,
    "syntax = \"proto3\";\npackage foo;\npackage bar;"
);

// -- Multiple syntax --

expect_error!(
    err_multiple_syntax,
    "syntax = \"proto3\";\nsyntax = \"proto2\";"
);

// -- Extension errors --

expect_error!(
    err_extend_missing_name,
    "syntax = \"proto2\";\nextend { optional int32 x = 1; }"
);

// -- Reserved errors --

expect_error!(
    err_reserved_missing_semicolon,
    "syntax = \"proto3\";\nmessage Foo { reserved 1 }"
);

// =========================================================================
// Additional structural tests covering edge cases
// =========================================================================

#[test]
fn field_with_fully_qualified_type() {
    let file = parse(
        r#"
        syntax = "proto2";
        package foo;
        message Bar {}
        message Baz {
            optional .foo.Bar ref = 1;
        }
    "#,
    )
    .unwrap();
    let baz = find_msg(&file, "Baz");
    let f = find_field(baz, "ref");
    assert_eq!(f.type_name, Some(".foo.Bar".to_string()));
}

#[test]
fn oneof_with_message_field() {
    let file = parse(
        r#"
        syntax = "proto3";
        message Inner { int32 x = 1; }
        message Outer {
            oneof choice {
                int32 a = 1;
                Inner b = 2;
                string c = 3;
            }
        }
    "#,
    )
    .unwrap();
    let outer = find_msg(&file, "Outer");
    assert_eq!(outer.oneof_decl.len(), 1);
    assert_eq!(outer.field.len(), 3);
    for f in &outer.field {
        assert_eq!(f.oneof_index, Some(0));
    }
}

#[test]
fn service_with_fully_qualified_types() {
    let file = parse(
        r#"
        syntax = "proto3";
        package myservice;
        message Request {}
        message Response {}
        service MyService {
            rpc Get(.myservice.Request) returns (.myservice.Response);
        }
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "MyService");
    let m = find_method(svc, "Get");
    assert_eq!(m.input_type, Some(".myservice.Request".to_string()));
    assert_eq!(m.output_type, Some(".myservice.Response".to_string()));
}

#[test]
fn file_with_all_top_level_constructs() {
    let file = parse(
        r#"
        syntax = "proto3";
        package test;
        import "google/protobuf/any.proto";
        option java_package = "com.test";

        message Foo {
            int32 id = 1;
            Bar bar = 2;
            map<string, int32> tags = 3;
            oneof payload {
                string text = 4;
                bytes data = 5;
            }
            reserved 10 to 20;
            reserved "old_field";
        }
        message Bar {
            string name = 1;
        }
        enum Status {
            UNKNOWN = 0;
            ACTIVE = 1;
            INACTIVE = 2;
        }
        service MyService {
            rpc Get(Foo) returns (Bar);
            rpc Stream(Foo) returns (stream Bar);
        }
    "#,
    )
    .unwrap();

    assert_eq!(file.package, Some("test".to_string()));
    assert_eq!(file.dependency.len(), 1);
    assert!(file.options.is_some());
    assert_eq!(file.message_type.len(), 2);
    assert_eq!(file.enum_type.len(), 1);
    assert_eq!(file.service.len(), 1);

    let foo = find_msg(&file, "Foo");
    assert!(!foo.field.is_empty());
    assert!(!foo.oneof_decl.is_empty());
    assert!(!foo.reserved_range.is_empty());
    assert!(!foo.reserved_name.is_empty());
}

#[test]
fn proto2_extensions_with_options() {
    let file = parse(
        r#"
        syntax = "proto2";
        message Foo {
            extensions 100 to 199 [(my_ext_option) = "hello"];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "Foo");
    assert_eq!(msg.extension_range.len(), 1);
    // Extension range options should have been parsed
    assert!(msg.extension_range[0].options.is_some());
}

#[test]
fn all_file_option_types() {
    let file = parse(
        r#"
        syntax = "proto3";
        option java_package = "com.example";
        option java_outer_classname = "MyProto";
        option java_multiple_files = true;
        option optimize_for = SPEED;
        option go_package = "example.com/proto";
        option cc_enable_arenas = true;
        option objc_class_prefix = "MY";
        option csharp_namespace = "Example";
        option deprecated = true;
    "#,
    )
    .unwrap();
    let opts = file.options.as_ref().unwrap();
    assert_eq!(opts.java_package, Some("com.example".to_string()));
    assert_eq!(opts.java_outer_classname, Some("MyProto".to_string()));
    assert_eq!(opts.java_multiple_files, Some(true));
    assert_eq!(opts.optimize_for, Some(OptimizeMode::Speed));
    assert_eq!(opts.go_package, Some("example.com/proto".to_string()));
    assert_eq!(opts.cc_enable_arenas, Some(true));
    assert_eq!(opts.objc_class_prefix, Some("MY".to_string()));
    assert_eq!(opts.csharp_namespace, Some("Example".to_string()));
    assert_eq!(opts.deprecated, Some(true));
}

#[test]
fn string_escape_sequences() {
    // Test that the parser handles C-style escape sequences in strings
    let file = parse(
        r#"
        syntax = "proto2";
        message Foo {
            optional string s = 1 [default = "hello\nworld\t\"escape\\"];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "Foo");
    let f = find_field(msg, "s");
    let val = f.default_value.as_ref().unwrap();
    assert!(val.contains('\n'));
    assert!(val.contains('\t'));
}

#[test]
fn hex_and_octal_int_literals() {
    let file = parse(
        r#"
        syntax = "proto2";
        message Foo {
            optional int32 hex = 1 [default = 0xFF];
            optional int32 oct = 2 [default = 077];
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "Foo");
    let hex_val = find_field(msg, "hex").default_value.as_ref().unwrap();
    assert!(
        hex_val == "255" || hex_val == "0xFF" || hex_val == "0xff",
        "hex default: {}",
        hex_val
    );
    let oct_val = find_field(msg, "oct").default_value.as_ref().unwrap();
    assert!(
        oct_val == "63" || oct_val == "077",
        "octal default: {}",
        oct_val
    );
}

#[test]
fn interleaved_messages_and_enums() {
    let file = parse(
        r#"
        syntax = "proto3";
        message A { int32 x = 1; }
        enum E1 { E1_UNKNOWN = 0; }
        message B { string y = 1; }
        enum E2 { E2_UNKNOWN = 0; }
    "#,
    )
    .unwrap();
    assert_eq!(file.message_type.len(), 2);
    assert_eq!(file.enum_type.len(), 2);
    find_msg(&file, "A");
    find_msg(&file, "B");
    find_enum(&file, "E1");
    find_enum(&file, "E2");
}

#[test]
fn group_inside_oneof() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            oneof choice {
                int32 a = 1;
                group MyGroup = 2 {
                    optional int32 val = 3;
                }
            }
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(msg.oneof_decl.len(), 1);
    // group creates a nested type
    find_nested_msg(msg, "MyGroup");
    let group_field = find_field(msg, "mygroup");
    assert_eq!(group_field.r#type, Some(FieldType::Group));
    assert_eq!(group_field.oneof_index, Some(0));
}

#[test]
fn proto2_required_and_optional_mix() {
    let file = parse(
        r#"
        syntax = "proto2";
        message TestMessage {
            required int32 id = 1;
            optional string name = 2;
            required bool active = 3;
            repeated int32 values = 4;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    assert_eq!(find_field(msg, "id").label, Some(FieldLabel::Required));
    assert_eq!(find_field(msg, "name").label, Some(FieldLabel::Optional));
    assert_eq!(find_field(msg, "active").label, Some(FieldLabel::Required));
    assert_eq!(find_field(msg, "values").label, Some(FieldLabel::Repeated));
}

#[test]
fn enum_with_hex_values() {
    let file = parse(
        r#"
        syntax = "proto2";
        enum Flags {
            NONE = 0;
            FLAG_A = 0x01;
            FLAG_B = 0x02;
            FLAG_ALL = 0x7FFFFFFF;
        }
    "#,
    )
    .unwrap();
    let e = find_enum(&file, "Flags");
    assert_eq!(e.value.len(), 4);
    assert_eq!(find_enum_value(e, "NONE").number, Some(0));
    assert_eq!(find_enum_value(e, "FLAG_A").number, Some(1));
    assert_eq!(find_enum_value(e, "FLAG_B").number, Some(2));
    assert_eq!(find_enum_value(e, "FLAG_ALL").number, Some(0x7FFFFFFF));
}

#[test]
fn service_option_deprecated() {
    let file = parse(
        r#"
        syntax = "proto3";
        message M {}
        service TestService {
            option deprecated = true;
            rpc Foo(M) returns (M);
        }
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "TestService");
    assert_eq!(svc.options.as_ref().unwrap().deprecated, Some(true));
}

#[test]
fn consecutive_reserved_statements() {
    let file = parse(
        r#"
        syntax = "proto3";
        message TestMessage {
            reserved 1, 2, 3;
            reserved 10 to 20;
            reserved "a", "b";
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    // 3 individual + 1 range = 4 reserved ranges
    assert_eq!(msg.reserved_range.len(), 4);
    assert_eq!(msg.reserved_name.len(), 2);
}

#[test]
fn empty_service() {
    let file = parse(
        r#"
        syntax = "proto3";
        service EmptyService {}
    "#,
    )
    .unwrap();
    let svc = find_service(&file, "EmptyService");
    assert!(svc.method.is_empty());
}

#[test]
fn map_of_enums() {
    let file = parse(
        r#"
        syntax = "proto3";
        enum Status { UNKNOWN = 0; OK = 1; }
        message TestMessage {
            map<string, Status> status_map = 1;
        }
    "#,
    )
    .unwrap();
    let msg = find_msg(&file, "TestMessage");
    let f = find_field(msg, "status_map");
    assert_eq!(f.label, Some(FieldLabel::Repeated));
    assert_eq!(f.r#type, Some(FieldType::Message));
}

// =========================================================================
// C++ PARITY: Error tests ported from parser_unittest.cc
// Reference: original-protobuf/src/google/protobuf/compiler/parser_unittest.cc
// =========================================================================

// -- Field number validation --

expect_error!(
    cpp_field_number_out_of_range,
    "syntax = \"proto2\";\nmessage M {\n  optional int32 foo = 0x100000000;\n}",
    "out of range"
);

expect_error!(
    cpp_field_number_out_of_range_decimal,
    "syntax = \"proto3\";\nmessage M {\n  int32 foo = 4294967296;\n}",
    "out of range"
);

// -- Enum value range --

expect_error!(
    cpp_enum_value_too_big_hex,
    "syntax = \"proto2\";\nenum E {\n  TOO_BIG = 0x80000000;\n}",
    "out of range"
);

expect_error!(
    cpp_enum_value_too_big_decimal,
    "syntax = \"proto2\";\nenum E {\n  TOO_BIG = 2147483648;\n}",
    "out of range"
);

expect_error!(
    cpp_enum_value_too_small,
    "syntax = \"proto2\";\nenum E {\n  TOO_SMALL = -2147483649;\n}",
    "out of range"
);

// -- Group validation --

expect_error!(
    cpp_group_not_capitalized,
    "syntax = \"proto2\";\nmessage M {\n  optional group foo = 1 {}\n}",
    "capital letter"
);

expect_error!(
    cpp_group_missing_body,
    "syntax = \"proto2\";\nmessage M {\n  optional group Foo = 1;\n}",
    "Missing group body"
);

// -- Extend validation --

expect_error!(
    cpp_extending_primitive,
    "syntax = \"proto2\";\nextend int32 { optional string foo = 4; }",
    "message type"
);

// -- Service/RPC validation --

expect_error!(
    cpp_service_method_primitive_input,
    "syntax = \"proto3\";\nservice S {\n  rpc Foo(int32) returns (Bar);\n}",
    "message type"
);

expect_error!(
    cpp_service_method_primitive_output,
    "syntax = \"proto3\";\nservice S {\n  rpc Foo(Bar) returns (string);\n}",
    "message type"
);

expect_error!(
    cpp_eof_in_method_options,
    "syntax = \"proto3\";\nservice S {\n  rpc Foo(Bar) returns(Bar) {",
    "end of input in method options"
);

// -- Reserved validation --

expect_error!(
    cpp_reserved_negative_number,
    "syntax = \"proto3\";\nmessage M {\n  reserved -10;\n}",
    "positive"
);

expect_error!(
    cpp_reserved_number_out_of_range,
    "syntax = \"proto3\";\nmessage M {\n  reserved 2147483648;\n}",
    "out of range"
);

expect_error!(
    cpp_reserved_mix_name_and_number,
    "syntax = \"proto3\";\nmessage M {\n  reserved 10, \"foo\";\n}"
);

// -- Enum reserved validation --

expect_error!(
    cpp_enum_reserved_mix_name_and_number,
    "syntax = \"proto2\";\nenum E {\n  A = 0;\n  reserved 10, \"foo\";\n}"
);

// -- Map extension --

expect_error!(
    cpp_map_in_extend,
    "syntax = \"proto2\";\nmessage M { extensions 100 to 200; }\nextend M {\n  map<int32, int32> m = 100;\n}",
    "Map fields are not allowed to be extensions"
);

// -- Oneof validation --

expect_error!(
    cpp_label_in_oneof,
    "syntax = \"proto3\";\nmessage M {\n  oneof foo {\n    optional int32 bar = 1;\n  }\n}"
);

expect_error!(
    cpp_map_in_oneof,
    "syntax = \"proto3\";\nmessage M {\n  oneof foo {\n    map<int32, int32> m = 1;\n  }\n}"
);

// -- Label for map --

expect_error!(
    cpp_label_for_map_optional,
    "syntax = \"proto3\";\nmessage M {\n  optional map<int32, int32> m = 1;\n}"
);

expect_error!(
    cpp_label_for_map_repeated,
    "syntax = \"proto3\";\nmessage M {\n  repeated map<int32, int32> m = 1;\n}"
);

// -- Already covered but verifying exact behavior --

expect_error!(
    cpp_unknown_syntax,
    "syntax = \"no_such_syntax\";",
    "Unrecognized syntax identifier"
);

expect_error!(cpp_unmatched_close_brace, "}");

expect_error!(
    cpp_message_missing_name,
    "syntax = \"proto3\";\nmessage {}",
    "identifier"
);

expect_error!(
    cpp_message_missing_body,
    "syntax = \"proto3\";\nmessage Foo;",
    "LBrace"
);

expect_error!(cpp_eof_in_message, "syntax = \"proto3\";\nmessage Foo {");

expect_error!(
    cpp_missing_field_number,
    "syntax = \"proto2\";\nmessage M {\n  optional int32 foo;\n}"
);

expect_error!(
    cpp_expected_field_number,
    "syntax = \"proto2\";\nmessage M {\n  optional int32 foo = ;\n}"
);

expect_error!(
    cpp_expected_option_name,
    "syntax = \"proto2\";\nmessage M {\n  optional uint32 foo = 1 [];\n}"
);

expect_error!(cpp_expected_top_level, "blah;");

expect_error!(
    cpp_enum_value_missing_number,
    "syntax = \"proto2\";\nenum E {\n  FOO;\n}"
);

expect_error!(cpp_eof_in_enum, "syntax = \"proto3\";\nenum E { BAR = 0;");

expect_error!(cpp_eof_in_service, "syntax = \"proto3\";\nservice S {");

expect_error!(cpp_import_not_quoted, "syntax = \"proto3\";\nimport foo;");

expect_error!(cpp_multiple_packages, "package foo;\npackage bar;");

expect_error!(
    cpp_missing_oneof_name,
    "syntax = \"proto3\";\nmessage M {\n  oneof {\n    int32 bar = 1;\n  }\n}",
    "identifier"
);

expect_error!(
    cpp_error_in_extension,
    "syntax = \"proto2\";\nmessage Foo { extensions 100 to 199; }\nextend Foo { optional string foo; }"
);

// -- Malformed maps --

expect_error!(
    cpp_map_missing_comma,
    "syntax = \"proto3\";\nmessage M {\n  map<string> m = 1;\n}"
);

expect_error!(
    cpp_map_empty_types,
    "syntax = \"proto3\";\nmessage M {\n  map<> m = 1;\n}"
);

expect_error!(
    cpp_map_missing_close_angle,
    "syntax = \"proto3\";\nmessage M {\n  map<string,string m = 1;\n}"
);

// -- Proto2 missing label --

expect_error!(
    cpp_missing_label_proto2,
    "message TestMessage {\n  int32 foo = 1;\n}",
    "required"
);

// -- Default value type checking --

expect_error!(
    cpp_default_value_type_mismatch,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [default=true];\n}",
    "Expected integer"
);

expect_error!(
    cpp_default_value_not_boolean,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional bool foo = 1 [default=blah];\n}",
    "Expected \"true\" or \"false\""
);

expect_error!(
    cpp_default_value_not_string,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional string foo = 1 [default=1];\n}",
    "Expected string"
);

expect_error!(
    cpp_default_value_unsigned_negative,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [default=-1];\n}",
    "Unsigned field can't have negative default value"
);

expect_error!(
    cpp_default_value_missing,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [default=];\n}",
    "expected option value"
);

expect_error!(
    cpp_default_value_for_group,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional group Foo = 1 [default=blah] {}\n}",
    "Messages can't have default values"
);

// -- Duplicate option detection --

expect_error!(
    cpp_duplicate_default_value,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [default=1,default=2];\n}",
    "Already set option \"default\""
);

expect_error!(
    cpp_duplicate_json_name,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [json_name=\"a\",json_name=\"b\"];\n}",
    "Already set option \"json_name\""
);

expect_error!(
    cpp_json_name_not_string,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional string foo = 1 [json_name=1];\n}",
    "Expected string for JSON name"
);

// -- Enum allow_alias semantics --

expect_error!(
    cpp_enum_allow_alias_false,
    "enum Foo {\n  option allow_alias = false;\n  BAR = 1;\n  BAZ = 2;\n}",
    "allow_alias = false"
);

expect_error!(
    cpp_unnecessary_enum_allow_alias,
    "enum Foo {\n  option allow_alias = true;\n  BAR = 1;\n  BAZ = 2;\n}",
    "no enum values share field numbers"
);

// -- Label in oneof (required in proto2) --

expect_error!(
    cpp_required_in_oneof,
    "syntax = \"proto2\";\nmessage TestMessage {\n  oneof foo {\n    required int32 bar = 1;\n  }\n}",
    "Fields in oneofs must not have labels"
);

// -- Verify allow_alias = true with actual aliases is accepted --

#[test]
fn enum_allow_alias_with_actual_aliases() {
    let input = "enum Foo {\n  option allow_alias = true;\n  BAR = 1;\n  BAZ = 1;\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(
        result.is_ok(),
        "allow_alias=true with actual aliases should be accepted: {:?}",
        result.err()
    );
}

// -- Proto3 fields don't need labels --

#[test]
fn proto3_field_no_label_ok() {
    let input = "syntax = \"proto3\";\nmessage Foo {\n  int32 bar = 1;\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(
        result.is_ok(),
        "proto3 fields don't need labels: {:?}",
        result.err()
    );
}

// -- Default value type checking: valid cases --

#[test]
fn default_value_int_field_ok() {
    let input = "syntax = \"proto2\";\nmessage Foo {\n  optional int32 bar = 1 [default=42];\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn default_value_bool_field_ok() {
    let input = "syntax = \"proto2\";\nmessage Foo {\n  optional bool bar = 1 [default=true];\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn default_value_string_field_ok() {
    let input =
        "syntax = \"proto2\";\nmessage Foo {\n  optional string bar = 1 [default=\"hello\"];\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn default_value_float_field_ok() {
    let input = "syntax = \"proto2\";\nmessage Foo {\n  optional float bar = 1 [default=1.5];\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn default_value_negative_int_ok() {
    let input = "syntax = \"proto2\";\nmessage Foo {\n  optional int32 bar = 1 [default=-42];\n}";
    let result = protoc_rs_parser::parse(input);
    assert!(result.is_ok(), "{:?}", result.err());
}

// ==========================================================================
// Reserved identifier/mixing tests (C++ parity)
// ==========================================================================

// -- Message reserved: bare identifiers must be string literals --

expect_error!(
    cpp_reserved_missing_quotes,
    "message Foo {\n  reserved foo;\n}",
    "must be string literals"
);

expect_error!(
    cpp_reserved_standalone_max,
    "message Foo {\n  reserved max;\n}",
    "must be string literals"
);

expect_error!(
    cpp_reserved_mix_number_and_name,
    "message Foo {\n  reserved 10, \"foo\";\n}",
    "Expected field number range"
);

// -- Enum reserved: bare identifiers must be string literals --

expect_error!(
    cpp_enum_reserved_missing_quotes,
    "enum TestEnum {\n  FOO = 0;\n  reserved foo;\n}",
    "must be string literals"
);

expect_error!(
    cpp_enum_reserved_standalone_max,
    "enum TestEnum {\n  FOO = 0;\n  reserved max;\n}",
    "must be string literals"
);

expect_error!(
    cpp_enum_reserved_mix_number_and_name,
    "enum TestEnum {\n  FOO = 0;\n  reserved 10, \"foo\";\n}",
    "Expected enum number range"
);

// -- Enum reserved: range validation --

expect_error!(
    cpp_enum_reserved_positive_out_of_range,
    "enum TestEnum {\n  FOO = 0;\n  reserved 2147483648;\n}",
    "out of range"
);

expect_error!(
    cpp_enum_reserved_negative_out_of_range,
    "enum TestEnum {\n  FOO = 0;\n  reserved -2147483649;\n}",
    "out of range"
);

// -- EOF in aggregate value --

expect_error!(
    cpp_eof_in_aggregate_value,
    "option (fileopt) = { i:100\n",
    "unterminated aggregate"
);

// -- Nesting limit --

#[test]
fn cpp_nesting_is_limited_without_crashing() {
    // 33 levels of nesting should exceed the limit
    let mut input = String::from("syntax = \"proto2\";\n");
    for _ in 0..33 {
        input.push_str("message M {");
    }
    for _ in 0..33 {
        input.push('}');
    }
    let result = protoc_rs_parser::parse(&input);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message.contains("maximum recursion limit"),
        "error '{}' should mention recursion limit",
        err.message
    );
}

#[test]
fn nesting_within_limit_ok() {
    // 32 levels should be fine
    let mut input = String::from("syntax = \"proto2\";\n");
    for _ in 0..32 {
        input.push_str("message M {");
    }
    for _ in 0..32 {
        input.push('}');
    }
    let result = protoc_rs_parser::parse(&input);
    assert!(
        result.is_ok(),
        "32 levels should be within limit: {:?}",
        result.err()
    );
}

// -- Non-extension option name beginning with dot --

expect_error!(
    cpp_non_extension_option_dot,
    "syntax = \"proto2\";\nmessage TestMessage {\n  optional uint32 foo = 1 [.foo=1];\n}",
    "expected identifier"
);

// -- SimpleSyntaxError --

expect_error!(cpp_simple_syntax_error, "message TestMessage @#$ { blah }");

// -- DefaultValueTooLarge --

expect_error!(
    cpp_default_value_too_large_int32_positive,
    "syntax = \"proto2\";\nmessage M {\n  optional int32 foo = 1 [default=0x80000000];\n}",
    "out of range"
);

expect_error!(
    cpp_default_value_too_large_int32_negative,
    "syntax = \"proto2\";\nmessage M {\n  optional int32 foo = 1 [default=-0x80000001];\n}",
    "out of range"
);

expect_error!(
    cpp_default_value_too_large_uint32,
    "syntax = \"proto2\";\nmessage M {\n  optional uint32 foo = 1 [default=0x100000000];\n}",
    "out of range"
);

expect_error!(
    cpp_default_value_too_large_int64,
    "syntax = \"proto2\";\nmessage M {\n  optional int64 foo = 1 [default=0x80000000000000000];\n}",
    "out of range"
);

expect_error!(
    cpp_default_value_too_large_uint64,
    "syntax = \"proto2\";\nmessage M {\n  optional uint64 foo = 1 [default=0x100000000000000000];\n}",
    "out of range"
);

// -- MalformedMaps: additional sub-cases --

expect_error!(
    cpp_malformed_map_missing_value_type,
    "syntax = \"proto3\";\nmessage M {\n  map<string,> m = 1;\n}"
);

expect_error!(
    cpp_malformed_map_missing_key_type,
    "syntax = \"proto3\";\nmessage M {\n  map<,string> m = 1;\n}"
);

// -- MethodOptionTypeError --

expect_error!(
    cpp_method_option_type_error,
    "message Baz {}\nservice Foo {\n  rpc Bar(Baz) returns(Baz) { option invalid syntax; }\n}"
);

// ---------------------------------------------------------------------------
// C++ ParseErrorTest: final 9 cases (error recovery, warnings, editions)
// ---------------------------------------------------------------------------

// MultipleParseErrors: error recovery collects multiple errors from one message body.
// C++ test uses `!` which our lexer rejects, so we use valid-token invalid-syntax instead.
// The key invariant: multiple errors are collected from a single message body.
#[test]
fn cpp_multiple_parse_errors() {
    // Two separate errors: missing field number, then missing semicolon after field with block
    let input = "syntax = \"proto2\";\n\
                 message TestMessage {\n\
                   optional int32 foo;\n\
                   optional int32 bar = 3 {}\n\
                 }\n";
    let result = parse_collecting(input).expect("should not have fatal parse error");
    assert!(
        result.errors.len() >= 2,
        "Expected at least 2 errors from recovery, got {}: {:?}",
        result.errors.len(),
        result.errors
    );
    // First error: missing field number on `foo` (parser expects `=` after field name)
    assert!(
        result.errors[0].message.contains("Missing field number")
            || result.errors[0].message.contains("field number")
            || result.errors[0].message.contains("expected Equals"),
        "First error should be about missing field number, got: {}",
        result.errors[0].message
    );
}

// ReservedInvalidIdentifier: warning for reserved string that isn't a valid identifier
#[test]
fn cpp_reserved_invalid_identifier() {
    let input = "syntax = \"proto2\";\n\
                 message Foo {\n\
                   reserved \"foo bar\";\n\
                 }\n";
    let result = parse_collecting(input).expect("should parse successfully");
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got {:?}",
        result.errors
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("not a valid identifier")),
        "Expected warning about invalid identifier, got: {:?}",
        result.warnings
    );
}

// EnumReservedInvalidIdentifier: same for enum reserved
#[test]
fn cpp_enum_reserved_invalid_identifier() {
    let input = "syntax = \"proto2\";\n\
                 enum TestEnum {\n\
                   FOO = 1;\n\
                   reserved \"foo bar\";\n\
                 }\n";
    let result = parse_collecting(input).expect("should parse successfully");
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got {:?}",
        result.errors
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("not a valid identifier")),
        "Expected warning about invalid identifier, got: {:?}",
        result.warnings
    );
}

// MsgReservedNameStringNotInEditions: editions mode rejects string literal reserved names
#[test]
fn cpp_msg_reserved_name_string_not_in_editions() {
    let input = "edition = \"2023\";\n\
                 message TestMessage {\n\
                   reserved \"foo\", \"bar\";\n\
                 }\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message.contains("must be identifiers in editions"),
        "Expected editions identifier error, got: {}",
        err.message
    );
}

// EnumReservedNameStringNotInEditions: same for enum
#[test]
fn cpp_enum_reserved_name_string_not_in_editions() {
    let input = "edition = \"2023\";\n\
                 enum TestEnum {\n\
                   FOO = 0;\n\
                   reserved \"foo\", \"bar\";\n\
                 }\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message.contains("must be identifiers in editions"),
        "Expected editions identifier error, got: {}",
        err.message
    );
}

// ReservedMixNameAndNumberEditions: in editions, mixing numbers and identifiers is rejected
#[test]
fn cpp_reserved_mix_name_and_number_editions() {
    let input = "edition = \"2023\";\n\
                 message Foo {\n\
                   reserved 10, foo;\n\
                 }\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message.contains("Expected field number range"),
        "Expected field number range error, got: {}",
        err.message
    );
}

// EnumReservedMixNameAndNumberEditions: same for enum
#[test]
fn cpp_enum_reserved_mix_name_and_number_editions() {
    let input = "edition = \"2023\";\n\
                 enum TestEnum {\n\
                   FOO = 1;\n\
                   reserved 10, foo;\n\
                 }\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message.contains("Expected enum number range"),
        "Expected enum number range error, got: {}",
        err.message
    );
}

// OptionImportBefore2024: `import option` outside editions is rejected
#[test]
fn cpp_option_import_before_2024() {
    let input = "edition = \"2023\";\n\
                 import option \"foo.proto\";\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message
            .contains("option import is not supported before edition 2024"),
        "Expected option import error, got: {}",
        err.message
    );
}

// WeakImportAfter2024: `import weak` in editions 2024+ is rejected
#[test]
fn cpp_weak_import_after_2024() {
    let input = "edition = \"2024\";\n\
                 import weak \"foo.proto\";\n";
    let err = parse(input).unwrap_err();
    assert!(
        err.message
            .contains("weak import is not supported in edition 2024"),
        "Expected weak import error, got: {}",
        err.message
    );
}

// ---------------------------------------------------------------------------
// ParseEditionsTest -- C++ parser_unittest.cc editions tests
// ---------------------------------------------------------------------------

// Editions: basic edition parsing, field gets LABEL_OPTIONAL by default
#[test]
fn cpp_editions_basic() {
    let input = "edition = \"2023\";\nmessage A {\n  int32 b = 1;\n}\n";
    let file = parse(input).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("editions"));
    assert_eq!(file.edition.as_deref(), Some("2023"));
    assert_eq!(file.message_type.len(), 1);
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("A"));
    assert_eq!(msg.field.len(), 1);
    let f = &msg.field[0];
    assert_eq!(f.name.as_deref(), Some("b"));
    assert_eq!(f.number, Some(1));
    assert_eq!(f.label, Some(FieldLabel::Optional));
    assert_eq!(f.r#type, Some(FieldType::Int32));
}

// TestEdition: accept test-only editions
#[test]
fn cpp_editions_test_edition() {
    let input = "edition = \"99998_TEST_ONLY\";\n";
    let file = parse(input).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("editions"));
    assert_eq!(file.edition.as_deref(), Some("99998_TEST_ONLY"));
}

// ExtensionsParse: extensions work in editions
#[test]
fn cpp_editions_extensions_parse() {
    let input = "edition = \"2023\";\n\
                 message Foo {\n  extensions 100 to 199;\n}\n\
                 extend Foo {\n  string foo = 101;\n}\n";
    let file = parse(input).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("editions"));
    assert_eq!(file.edition.as_deref(), Some("2023"));
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("Foo"));
    assert_eq!(msg.extension_range.len(), 1);
    assert_eq!(msg.extension_range[0].start, Some(100));
    assert_eq!(msg.extension_range[0].end, Some(200));
    assert_eq!(file.extension.len(), 1);
    assert_eq!(file.extension[0].name.as_deref(), Some("foo"));
    assert_eq!(file.extension[0].number, Some(101));
}

// MapFeatures: map field with features option in editions
#[test]
fn cpp_editions_map_features() {
    let input = "edition = \"2023\";\n\
                 message Foo {\n\
                   map<string, int32> map_field = 1 [features.my_feature = SOMETHING];\n\
                 }\n";
    let file = parse(input).unwrap();
    assert_eq!(file.syntax.as_deref(), Some("editions"));
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("Foo"));
    // Map field produces a field + nested MapFieldEntry type
    assert_eq!(msg.field.len(), 1);
    assert_eq!(msg.field[0].name.as_deref(), Some("map_field"));
    // The features option goes to uninterpreted_option
    let opts = msg.field[0].options.as_ref().unwrap();
    assert!(
        !opts.uninterpreted_option.is_empty(),
        "features option should be in uninterpreted_option"
    );
}

// EmptyEdition: reject empty edition string
expect_error!(
    cpp_editions_empty_edition,
    "edition = \"\";\nmessage A { int32 b = 1; }",
    "Unknown edition"
);

// InvalidEdition: reject unknown edition string
expect_error!(
    cpp_editions_invalid_edition,
    "edition = \"2023_INVALID\";\nmessage A { int32 b = 1; }",
    "Unknown edition"
);

// UnknownEdition: reject "UNKNOWN"
expect_error!(
    cpp_editions_unknown_edition,
    "edition = \"UNKNOWN\";\nmessage A { int32 b = 1; }",
    "Unknown edition"
);

// LegacyProto2Edition: reject "PROTO2" as edition
expect_error!(
    cpp_editions_legacy_proto2,
    "edition = \"PROTO2\";\nmessage A { int32 b = 1; }",
    "Unknown edition"
);

// LegacyProto3Edition: reject "PROTO3" as edition
expect_error!(
    cpp_editions_legacy_proto3,
    "edition = \"PROTO3\";\nmessage A { int32 b = 1; }",
    "Unknown edition"
);

// SyntaxEditions: `syntax = "editions"` is rejected
expect_error!(
    cpp_editions_syntax_editions,
    "syntax = \"editions\";\nmessage A { int32 b = 1; }",
    "Unrecognized syntax identifier"
);

// MixedSyntaxAndEdition: syntax then edition is an error
expect_error!(
    cpp_editions_mixed_syntax_and_edition,
    "syntax = \"proto2\";\nedition = \"2023\";\nmessage A { int32 b = 1; }",
    "Expected top-level statement"
);

// MixedEditionAndSyntax: edition then syntax is an error
expect_error!(
    cpp_editions_mixed_edition_and_syntax,
    "edition = \"2023\";\nsyntax = \"proto2\";\nmessage A { int32 b = 1; }",
    "Expected top-level statement"
);

// OptionalKeywordBanned: `optional` label is banned in editions
expect_error!(
    cpp_editions_optional_keyword_banned,
    "edition = \"2023\";\nmessage A {\n  optional int32 b = 1;\n}",
    "Label \"optional\" is not supported in editions"
);

// RequiredKeywordBanned: `required` label is banned in editions
expect_error!(
    cpp_editions_required_keyword_banned,
    "edition = \"2023\";\nmessage A {\n  required int32 b = 1;\n}",
    "Label \"required\" is not supported in editions"
);

// GroupsBanned: group syntax is banned in editions
expect_error!(
    cpp_editions_groups_banned,
    "edition = \"2023\";\nmessage TestMessage {\n  group TestGroup = 1 {};\n}",
    "Group syntax is no longer supported in editions"
);

// ---------------------------------------------------------------------------
// ParseEditionsTest -- scalar types in edition 2024
// ---------------------------------------------------------------------------

#[test]
fn cpp_editions_int32_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { int32 value = 1; }\n";
    let file = parse(input).unwrap();
    let f = &file.message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Int32));
    assert_eq!(f.label, Some(FieldLabel::Optional));
}

#[test]
fn cpp_editions_uint32_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { uint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Uint32));
}

#[test]
fn cpp_editions_int64_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { int64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Int64));
}

#[test]
fn cpp_editions_uint64_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { uint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Uint64));
}

#[test]
fn cpp_editions_fixed32_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { fixed32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Fixed32));
}

#[test]
fn cpp_editions_fixed64_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { fixed64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Fixed64));
}

#[test]
fn cpp_editions_sfixed32_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { sfixed32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sfixed32));
}

#[test]
fn cpp_editions_sfixed64_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { sfixed64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sfixed64));
}

#[test]
fn cpp_editions_sint32_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { sint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sint32));
}

#[test]
fn cpp_editions_sint64_edition2024() {
    let input = "edition = \"2024\";\nmessage Test { sint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sint64));
}

// ---------------------------------------------------------------------------
// ParseMessageTest -- UNSTABLE edition new types
// ---------------------------------------------------------------------------

#[test]
fn cpp_varint32_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { varint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Int32));
    assert_eq!(f.label, Some(FieldLabel::Optional));
}

#[test]
fn cpp_varint64_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { varint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Int64));
}

#[test]
fn cpp_uvarint32_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { uvarint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Uint32));
}

#[test]
fn cpp_uvarint64_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { uvarint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Uint64));
}

#[test]
fn cpp_zigzag32_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { zigzag32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sint32));
}

#[test]
fn cpp_zigzag64_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { zigzag64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sint64));
}

// ---------------------------------------------------------------------------
// ParseMessageTest -- UNSTABLE edition type remapping
// ---------------------------------------------------------------------------

#[test]
fn cpp_int32_unstable() {
    // In UNSTABLE, int32 -> TYPE_SFIXED32
    let input = "edition = \"UNSTABLE\";\nmessage Test { int32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sfixed32));
}

#[test]
fn cpp_int64_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { int64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Sfixed64));
}

#[test]
fn cpp_uint32_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { uint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Fixed32));
}

#[test]
fn cpp_uint64_unstable() {
    let input = "edition = \"UNSTABLE\";\nmessage Test { uint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, Some(FieldType::Fixed64));
}

// ---------------------------------------------------------------------------
// ParseMessageTest -- UNSTABLE edition obsolete types
// ---------------------------------------------------------------------------

expect_error!(
    cpp_fixed32_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { fixed32 value = 1; }",
    "Type 'fixed32' is obsolete in unstable editions, use 'uint32' instead."
);

expect_error!(
    cpp_fixed64_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { fixed64 value = 1; }",
    "Type 'fixed64' is obsolete in unstable editions, use 'uint64' instead."
);

expect_error!(
    cpp_sfixed32_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { sfixed32 value = 1; }",
    "Type 'sfixed32' is obsolete in unstable editions, use 'int32' instead."
);

expect_error!(
    cpp_sfixed64_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { sfixed64 value = 1; }",
    "Type 'sfixed64' is obsolete in unstable editions, use 'int64' instead."
);

expect_error!(
    cpp_sint32_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { sint32 value = 1; }",
    "Type 'sint32' is obsolete in unstable editions, use 'zigzag32' instead."
);

expect_error!(
    cpp_sint64_unstable_fails,
    "edition = \"UNSTABLE\";\nmessage Test { sint64 value = 1; }",
    "Type 'sint64' is obsolete in unstable editions, use 'zigzag64' instead."
);

// ---------------------------------------------------------------------------
// ParseMessageTest -- edition 2024 rejects UNSTABLE-only types
// ---------------------------------------------------------------------------

#[test]
fn cpp_varint32_edition2024_fails() {
    // varint32 is not a keyword in 2024, so it parses as a type name reference
    // which would fail at the analyzer level ("not defined").
    // At parser level, it should parse successfully as a type reference.
    let input = "edition = \"2024\";\nmessage Test { varint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    // Treated as unresolved type reference, not a scalar
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("varint32"));
}

#[test]
fn cpp_varint64_edition2024_fails() {
    let input = "edition = \"2024\";\nmessage Test { varint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("varint64"));
}

#[test]
fn cpp_uvarint32_edition2024_fails() {
    let input = "edition = \"2024\";\nmessage Test { uvarint32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("uvarint32"));
}

#[test]
fn cpp_uvarint64_edition2024_fails() {
    let input = "edition = \"2024\";\nmessage Test { uvarint64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("uvarint64"));
}

#[test]
fn cpp_zigzag32_edition2024_fails() {
    let input = "edition = \"2024\";\nmessage Test { zigzag32 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("zigzag32"));
}

#[test]
fn cpp_zigzag64_edition2024_fails() {
    let input = "edition = \"2024\";\nmessage Test { zigzag64 value = 1; }\n";
    let f = &parse(input).unwrap().message_type[0].field[0];
    assert_eq!(f.r#type, None);
    assert_eq!(f.type_name.as_deref(), Some("zigzag64"));
}

// ---------------------------------------------------------------------------
// ParseEditionsTest -- feature validation
// ---------------------------------------------------------------------------

#[test]
fn cpp_editions_features_without_editions() {
    // C++ ParseEditionsTest::FeaturesWithoutEditions
    // Features options are only valid under editions, not proto2/proto3.
    let input = "syntax = \"proto3\";\n\
                 option features.field_presence = IMPLICIT;\n\
                 message TestMessage {\n\
                   string foo = 1 [\n\
                     default = \"hello\",\n\
                     features.field_presence = EXPLICIT\n\
                   ];\n\
                 }";
    let result = parse_collecting(input).expect("should not have fatal parse error");
    assert!(
        result.errors.len() >= 2,
        "Expected at least 2 errors, got {}: {:?}",
        result.errors.len(),
        result.errors
    );
    assert!(
        result.errors[0]
            .message
            .contains("Features are only valid under editions"),
        "First error should be about features: {}",
        result.errors[0].message
    );
    assert!(
        result.errors[1]
            .message
            .contains("Features are only valid under editions"),
        "Second error should be about features: {}",
        result.errors[1].message
    );
}

// ---------------------------------------------------------------------------
// ParseVisibilityTest -- C++ parser_unittest.cc visibility tests
// ---------------------------------------------------------------------------

// Edition2023: `export` before message is rejected (not a keyword in 2023)
expect_error!(
    cpp_visibility_edition2023,
    "edition = \"2023\";\nexport message A { int32 b = 1; }",
    "Expected top-level statement"
);

// Edition2024NonStrictExportMessage: export message with nested local/export
#[test]
fn cpp_visibility_export_message_2024() {
    let input = "edition = \"2024\";\n\
                 export message A {\n\
                   local enum NestedEnum { A_VAL = 0; }\n\
                   export message NestedMessage { }\n\
                 }\n";
    let file = parse(input).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("A"));
    assert_eq!(msg.visibility, Some(Visibility::Export));
    let nested_enum = &msg.enum_type[0];
    assert_eq!(nested_enum.name.as_deref(), Some("NestedEnum"));
    assert_eq!(nested_enum.visibility, Some(Visibility::Local));
    let nested_msg = &msg.nested_type[0];
    assert_eq!(nested_msg.name.as_deref(), Some("NestedMessage"));
    assert_eq!(nested_msg.visibility, Some(Visibility::Export));
}

// Edition2024NonStrictLocalMessage: local message with nested export/local
#[test]
fn cpp_visibility_local_message_2024() {
    let input = "edition = \"2024\";\n\
                 local message B {\n\
                   export enum NestedEnum { B_VAL = 0; }\n\
                   local message NestedMessage { }\n\
                 }\n";
    let file = parse(input).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("B"));
    assert_eq!(msg.visibility, Some(Visibility::Local));
    let nested_enum = &msg.enum_type[0];
    assert_eq!(nested_enum.name.as_deref(), Some("NestedEnum"));
    assert_eq!(nested_enum.visibility, Some(Visibility::Export));
    let nested_msg = &msg.nested_type[0];
    assert_eq!(nested_msg.name.as_deref(), Some("NestedMessage"));
    assert_eq!(nested_msg.visibility, Some(Visibility::Local));
}

// Edition2024NonStrictExportEnum: export on top-level enum
#[test]
fn cpp_visibility_export_enum_2024() {
    let input = "edition = \"2024\";\nexport enum C { C_VAL = 0; }\n";
    let file = parse(input).unwrap();
    let e = &file.enum_type[0];
    assert_eq!(e.name.as_deref(), Some("C"));
    assert_eq!(e.visibility, Some(Visibility::Export));
    assert_eq!(e.value[0].name.as_deref(), Some("C_VAL"));
}

// Edition2024NonStrictLocalEnum: local on top-level enum
#[test]
fn cpp_visibility_local_enum_2024() {
    let input = "edition = \"2024\";\nlocal enum D { D_VAL = 0; }\n";
    let file = parse(input).unwrap();
    let e = &file.enum_type[0];
    assert_eq!(e.name.as_deref(), Some("D"));
    assert_eq!(e.visibility, Some(Visibility::Local));
}

// Edition2024NoServiceVisibility: visibility on service is an error
expect_error!(
    cpp_visibility_no_service_2024,
    "edition = \"2024\";\nlocal service Foo { }",
    "visibility modifiers are valid only on 'message' and 'enum'"
);

// Edition2024NoExtendVisibility: visibility on extend is an error
expect_error!(
    cpp_visibility_no_extend_2024,
    "edition = \"2024\";\nexport extend Foo { string yes = 12; }",
    "visibility modifiers are valid only on 'message' and 'enum'"
);

// Edition2023NestedKeywordName: local/export as type name components in 2023
#[test]
fn cpp_visibility_nested_keyword_name_2023() {
    let input = "edition = \"2023\";\n\
                 message MySimpleMessage {\n\
                   local.pkg_name.SomeMessage msg = 1;\n\
                   export.other_pkg_name.SomeMessage other_msg = 2;\n\
                 }\n";
    let file = parse(input).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("MySimpleMessage"));
    assert_eq!(msg.field.len(), 2);
    assert_eq!(
        msg.field[0].type_name.as_deref(),
        Some("local.pkg_name.SomeMessage")
    );
    assert_eq!(msg.field[0].name.as_deref(), Some("msg"));
    assert_eq!(
        msg.field[1].type_name.as_deref(),
        Some("export.other_pkg_name.SomeMessage")
    );
    assert_eq!(msg.field[1].name.as_deref(), Some("other_msg"));
}

// Edition2024NestedKeywordName: local/export as type names with leading dot in 2024
#[test]
fn cpp_visibility_nested_keyword_name_2024() {
    let input = "edition = \"2024\";\n\
                 message MySimpleMessage {\n\
                   .local.pkg_name.SomeMessage msg = 1;\n\
                   .export.other_pkg_name.SomeMessage other_msg = 2;\n\
                 }\n";
    let file = parse(input).unwrap();
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("MySimpleMessage"));
    assert_eq!(msg.field.len(), 2);
    assert_eq!(
        msg.field[0].type_name.as_deref(),
        Some(".local.pkg_name.SomeMessage")
    );
    assert_eq!(msg.field[0].name.as_deref(), Some("msg"));
    assert_eq!(
        msg.field[1].type_name.as_deref(),
        Some(".export.other_pkg_name.SomeMessage")
    );
    assert_eq!(msg.field[1].name.as_deref(), Some("other_msg"));
}

// ---------------------------------------------------------------------------
// ParseMiscTest -- InterpretedOptions (parser-level coverage)
// ---------------------------------------------------------------------------

#[test]
fn cpp_interpreted_options_inf_nan() {
    // C++ ParseMiscTest::InterpretedOptions -- tests that inf/nan float values
    // in custom message options are correctly stored. The C++ test uses runtime
    // descriptor pool interpretation; we verify the parser stores these correctly
    // as uninterpreted options.
    let input = r#"syntax = "proto2";
package test;
import "google/protobuf/descriptor.proto";
extend google.protobuf.MessageOptions {
  optional float float_opt = 7101;
  optional double double_opt = 7102;
}
message SettingRealsFromInf {
  option (float_opt) = inf;
  option (double_opt) = inf;
}
message SettingRealsFromNegativeInf {
  option (float_opt) = -inf;
  option (double_opt) = -inf;
}
message SettingRealsFromNan {
  option (float_opt) = nan;
  option (double_opt) = nan;
}
"#;
    let file = parse(input).unwrap();

    // SettingRealsFromInf
    let msg = &file.message_type[0];
    assert_eq!(msg.name.as_deref(), Some("SettingRealsFromInf"));
    let opts = msg.options.as_ref().unwrap();
    assert_eq!(opts.uninterpreted_option.len(), 2);
    assert_eq!(
        opts.uninterpreted_option[0].identifier_value.as_deref(),
        Some("inf")
    );
    assert_eq!(
        opts.uninterpreted_option[1].identifier_value.as_deref(),
        Some("inf")
    );

    // SettingRealsFromNegativeInf -- negative values stored differently
    let msg = &file.message_type[1];
    assert_eq!(msg.name.as_deref(), Some("SettingRealsFromNegativeInf"));
    let opts = msg.options.as_ref().unwrap();
    assert_eq!(opts.uninterpreted_option.len(), 2);

    // SettingRealsFromNan
    let msg = &file.message_type[2];
    assert_eq!(msg.name.as_deref(), Some("SettingRealsFromNan"));
    let opts = msg.options.as_ref().unwrap();
    assert_eq!(opts.uninterpreted_option.len(), 2);
    assert_eq!(
        opts.uninterpreted_option[0].identifier_value.as_deref(),
        Some("nan")
    );
    assert_eq!(
        opts.uninterpreted_option[1].identifier_value.as_deref(),
        Some("nan")
    );
}
