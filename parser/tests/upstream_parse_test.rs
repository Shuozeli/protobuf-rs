use protoc_rs_parser::parse;
use protoc_rs_schema::*;

fn upstream(name: &str) -> String {
    let path = format!(
        "{}/testdata/upstream/{}",
        env!("CARGO_MANIFEST_DIR").replace("/parser", ""),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e))
}

// ---------------------------------------------------------------------------
// Tier 1: Parse without errors (all upstream files)
// ---------------------------------------------------------------------------

macro_rules! parse_ok {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let source = upstream($file);
            let result = parse(&source);
            assert!(
                result.is_ok(),
                "failed to parse {}: {}",
                $file,
                result.unwrap_err()
            );
        }
    };
}

parse_ok!(parse_bad_names, "bad_names.proto");
parse_ok!(parse_dots_in_package, "dots_in_package.proto");
parse_ok!(parse_enums, "enums.proto");
parse_ok!(parse_imported_types, "imported_types.proto");
parse_ok!(
    parse_import_public_grandparent,
    "import_public_grandparent.proto"
);
parse_ok!(
    parse_import_public_non_primary_src1,
    "import_public_non_primary_src1.proto"
);
parse_ok!(
    parse_import_public_non_primary_src2,
    "import_public_non_primary_src2.proto"
);
parse_ok!(parse_nested, "nested.proto");
parse_ok!(parse_no_package_import, "no_package_import.proto");
parse_ok!(parse_no_package_other, "no_package_other.proto");
parse_ok!(
    parse_package_disambiguation1,
    "package_disabiguation1.proto"
);
parse_ok!(parse_package_import, "package_import.proto");
parse_ok!(
    parse_package_other_different,
    "package_other_different.proto"
);
parse_ok!(parse_package_other, "package_other.proto");
parse_ok!(parse_parent, "parent.proto");
parse_ok!(parse_unittest_import, "unittest_import.proto");
parse_ok!(parse_test_messages_proto2, "test_messages_proto2.proto");
parse_ok!(parse_test_messages_proto3, "test_messages_proto3.proto");

// ---------------------------------------------------------------------------
// Tier 2: ExpectParsesTo -- validate specific FileDescriptorProto structure
// ---------------------------------------------------------------------------

// Helper: find message by name in a FileDescriptorProto
fn find_msg<'a>(file: &'a FileDescriptorProto, name: &str) -> &'a DescriptorProto {
    file.message_type
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("message '{}' not found", name))
}

fn find_field<'a>(msg: &'a DescriptorProto, name: &str) -> &'a FieldDescriptorProto {
    msg.field
        .iter()
        .find(|f| f.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("field '{}' not found in {:?}", name, msg.name))
}

fn find_enum<'a>(file: &'a FileDescriptorProto, name: &str) -> &'a EnumDescriptorProto {
    file.enum_type
        .iter()
        .find(|e| e.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("enum '{}' not found", name))
}

fn find_nested_msg<'a>(msg: &'a DescriptorProto, name: &str) -> &'a DescriptorProto {
    msg.nested_type
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nested message '{}' not found in {:?}", name, msg.name))
}

fn find_nested_enum<'a>(msg: &'a DescriptorProto, name: &str) -> &'a EnumDescriptorProto {
    msg.enum_type
        .iter()
        .find(|e| e.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("nested enum '{}' not found in {:?}", name, msg.name))
}

// -- bad_names.proto --

#[test]
fn bad_names_structure() {
    let file = parse(&upstream("bad_names.proto")).unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
    assert_eq!(file.package, Some("type.type".to_string()));

    // message Self { optional int32 for = 1; optional Self self = 2; ... }
    let self_msg = find_msg(&file, "Self");
    assert_eq!(self_msg.field.len(), 7);
    let for_field = find_field(self_msg, "for");
    assert_eq!(for_field.r#type, Some(FieldType::Int32));
    assert_eq!(for_field.number, Some(1));
    assert_eq!(for_field.label, Some(FieldLabel::Optional));

    let self_field = find_field(self_msg, "self");
    assert_eq!(self_field.type_name, Some("Self".to_string()));

    let true_field = find_field(self_msg, "true");
    assert_eq!(true_field.r#type, Some(FieldType::Bool));

    let match_field = find_field(self_msg, "match");
    assert_eq!(match_field.label, Some(FieldLabel::Repeated));

    // message Pub { enum Self { enum = 0; } }
    let pub_msg = find_msg(&file, "Pub");
    assert_eq!(pub_msg.enum_type.len(), 1);
    let pub_enum = &pub_msg.enum_type[0];
    assert_eq!(pub_enum.name, Some("Self".to_string()));
    assert_eq!(pub_enum.value[0].name, Some("enum".to_string()));
    assert_eq!(pub_enum.value[0].number, Some(0));

    // message enum {}
    let enum_msg = find_msg(&file, "enum");
    assert!(enum_msg.field.is_empty());

    // message Ref { oneof self { .type.type.Pub.Self const = 3; } }
    let ref_msg = find_msg(&file, "Ref");
    assert_eq!(ref_msg.oneof_decl.len(), 1);
    assert_eq!(ref_msg.oneof_decl[0].name, Some("self".to_string()));
    assert_eq!(ref_msg.field.len(), 1);
    let const_field = find_field(ref_msg, "const");
    assert_eq!(const_field.oneof_index, Some(0));
    assert_eq!(
        const_field.type_name,
        Some(".type.type.Pub.Self".to_string())
    );

    // message AccessorsCollide
    let ac = find_msg(&file, "AccessorsCollide");
    assert_eq!(ac.nested_type.len(), 2); // X, SetX
    assert_eq!(ac.oneof_decl.len(), 1);

    // Top-level: SomeMsg, SomeMsgView
    find_msg(&file, "SomeMsg");
    let view_enum = find_enum(&file, "SomeMsgView");
    assert_eq!(view_enum.value.len(), 1);
}

// -- nested.proto --

#[test]
fn nested_structure() {
    let file = parse(&upstream("nested.proto")).unwrap();
    assert_eq!(file.package, Some("nest".to_string()));
    assert_eq!(file.message_type.len(), 3); // Outer, NotInside, Recursive

    let outer = find_msg(&file, "Outer");
    assert_eq!(outer.nested_type.len(), 1); // Inner
    let inner = find_nested_msg(outer, "Inner");

    // All 15 scalar types + message/enum/map fields
    assert_eq!(inner.field.len(), 20);
    assert_eq!(find_field(inner, "double").r#type, Some(FieldType::Double));
    assert_eq!(find_field(inner, "float").r#type, Some(FieldType::Float));
    assert_eq!(find_field(inner, "int32").r#type, Some(FieldType::Int32));
    assert_eq!(find_field(inner, "int64").r#type, Some(FieldType::Int64));
    assert_eq!(find_field(inner, "uint32").r#type, Some(FieldType::Uint32));
    assert_eq!(find_field(inner, "uint64").r#type, Some(FieldType::Uint64));
    assert_eq!(find_field(inner, "sint32").r#type, Some(FieldType::Sint32));
    assert_eq!(find_field(inner, "sint64").r#type, Some(FieldType::Sint64));
    assert_eq!(
        find_field(inner, "fixed32").r#type,
        Some(FieldType::Fixed32)
    );
    assert_eq!(
        find_field(inner, "fixed64").r#type,
        Some(FieldType::Fixed64)
    );
    assert_eq!(
        find_field(inner, "sfixed32").r#type,
        Some(FieldType::Sfixed32)
    );
    assert_eq!(
        find_field(inner, "sfixed64").r#type,
        Some(FieldType::Sfixed64)
    );
    assert_eq!(find_field(inner, "bool").r#type, Some(FieldType::Bool));
    assert_eq!(find_field(inner, "string").r#type, Some(FieldType::String));
    assert_eq!(find_field(inner, "bytes").r#type, Some(FieldType::Bytes));

    // packed repeated
    let packed = find_field(inner, "repeated_int32");
    assert_eq!(packed.label, Some(FieldLabel::Repeated));
    assert_eq!(packed.options.as_ref().unwrap().packed, Some(true));

    // map<string, string>
    let string_map = find_field(inner, "string_map");
    assert_eq!(string_map.label, Some(FieldLabel::Repeated));
    assert_eq!(string_map.r#type, Some(FieldType::Message));

    // Nested enums
    assert_eq!(inner.enum_type.len(), 1);
    let inner_enum = find_nested_enum(inner, "InnerEnum");
    assert_eq!(inner_enum.value.len(), 2);

    // Deep nesting: Inner > SuperInner > DuperInner > EvenMoreInner > CantBelieveItsSoInner
    let super_inner = find_nested_msg(inner, "SuperInner");
    let duper = find_nested_msg(super_inner, "DuperInner");
    let even_more = find_nested_msg(duper, "EvenMoreInner");
    let cant_believe = find_nested_msg(even_more, "CantBelieveItsSoInner");
    assert_eq!(cant_believe.field.len(), 1);
    assert_eq!(find_field(cant_believe, "num").number, Some(99));

    // Deep nested enum
    let too_inner = find_nested_enum(even_more, "JustWayTooInner");
    assert_eq!(too_inner.value.len(), 1);

    // Outer field referencing deep type
    let deep = find_field(outer, "deep");
    assert!(deep
        .type_name
        .as_ref()
        .unwrap()
        .contains("CantBelieveItsSoInner"));

    // Recursive message
    let recursive = find_msg(&file, "Recursive");
    let rec_field = find_field(recursive, "rec");
    assert_eq!(rec_field.type_name, Some("Recursive".to_string()));
}

// -- enums.proto --

#[test]
fn enums_structure() {
    let file = parse(&upstream("enums.proto")).unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto3);
    assert_eq!(file.package, Some("enums".to_string()));

    // TestEnumWithDuplicateStrippedPrefixNames -- allow_alias with many aliases
    let dup_enum = find_enum(&file, "TestEnumWithDuplicateStrippedPrefixNames");
    assert_eq!(dup_enum.options.as_ref().unwrap().allow_alias, Some(true));
    assert_eq!(dup_enum.value.len(), 9); // UNKNOWN, UNRECOGNIZED, 3x FOO, 3x BAR, DIFFERENT_NAME_ALIAS

    // TestEnumWithNumericNames
    let num_enum = find_enum(&file, "TestEnumWithNumericNames");
    assert_eq!(num_enum.value.len(), 4);

    // TestEnumValueNameSameAsEnum
    let same_enum = find_enum(&file, "TestEnumValueNameSameAsEnum");
    assert_eq!(same_enum.value.len(), 2);

    // TestMapWithNestedEnum -- map<string, InnerNested.NestedEnum>
    let map_msg = find_msg(&file, "TestMapWithNestedEnum");
    assert_eq!(map_msg.field.len(), 1);
    let string_map = find_field(map_msg, "string_map");
    assert_eq!(string_map.label, Some(FieldLabel::Repeated));
    assert_eq!(string_map.r#type, Some(FieldType::Message));

    // InnerNested with NestedEnum
    let inner = find_nested_msg(map_msg, "InnerNested");
    let nested_enum = find_nested_enum(inner, "NestedEnum");
    assert_eq!(nested_enum.value.len(), 2);
}

// -- test_messages_proto2.proto (exhaustive) --

#[test]
fn test_messages_proto2_structure() {
    let file = parse(&upstream("test_messages_proto2.proto")).unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
    assert_eq!(
        file.package,
        Some("protobuf_test_messages.proto2".to_string())
    );

    // File options
    let opts = file.options.as_ref().unwrap();
    assert_eq!(
        opts.java_package,
        Some("com.google.protobuf_test_messages.proto2".to_string())
    );
    assert_eq!(opts.optimize_for, Some(OptimizeMode::Speed));
    assert_eq!(opts.cc_enable_arenas, Some(true));

    // TestAllTypesProto2 -- the main exhaustive message
    let msg = find_msg(&file, "TestAllTypesProto2");

    // Check all singular scalar types are present
    for (name, expected_type) in [
        ("optional_int32", FieldType::Int32),
        ("optional_int64", FieldType::Int64),
        ("optional_uint32", FieldType::Uint32),
        ("optional_uint64", FieldType::Uint64),
        ("optional_sint32", FieldType::Sint32),
        ("optional_sint64", FieldType::Sint64),
        ("optional_fixed32", FieldType::Fixed32),
        ("optional_fixed64", FieldType::Fixed64),
        ("optional_sfixed32", FieldType::Sfixed32),
        ("optional_sfixed64", FieldType::Sfixed64),
        ("optional_float", FieldType::Float),
        ("optional_double", FieldType::Double),
        ("optional_bool", FieldType::Bool),
        ("optional_string", FieldType::String),
        ("optional_bytes", FieldType::Bytes),
    ] {
        let f = find_field(msg, name);
        assert_eq!(f.r#type, Some(expected_type), "type mismatch for {}", name);
        assert_eq!(
            f.label,
            Some(FieldLabel::Optional),
            "label mismatch for {}",
            name
        );
    }

    // Repeated scalar types
    for name in [
        "repeated_int32",
        "repeated_int64",
        "repeated_uint32",
        "repeated_uint64",
        "repeated_sint32",
        "repeated_sint64",
        "repeated_fixed32",
        "repeated_fixed64",
        "repeated_sfixed32",
        "repeated_sfixed64",
        "repeated_float",
        "repeated_double",
        "repeated_bool",
        "repeated_string",
        "repeated_bytes",
    ] {
        let f = find_field(msg, name);
        assert_eq!(
            f.label,
            Some(FieldLabel::Repeated),
            "expected repeated for {}",
            name
        );
    }

    // NestedMessage
    let nested = find_nested_msg(msg, "NestedMessage");
    assert_eq!(nested.field.len(), 2);
    find_field(nested, "a");
    find_field(nested, "corecursive"); // recursive reference

    // NestedEnum with negative value
    let nested_enum = find_nested_enum(msg, "NestedEnum");
    assert_eq!(nested_enum.value.len(), 4);
    let neg = nested_enum
        .value
        .iter()
        .find(|v| v.name.as_deref() == Some("NEG"))
        .unwrap();
    assert_eq!(neg.number, Some(-1));

    // ctype options
    let string_piece = find_field(msg, "optional_string_piece");
    assert_eq!(
        string_piece.options.as_ref().unwrap().ctype,
        Some(FieldCType::StringPiece)
    );
    let cord = find_field(msg, "optional_cord");
    assert_eq!(cord.options.as_ref().unwrap().ctype, Some(FieldCType::Cord));

    // Foreign message reference
    find_field(msg, "optional_foreign_message");
    find_field(msg, "optional_foreign_enum");

    // Should have at least one oneof
    assert!(!msg.oneof_decl.is_empty(), "expected oneof declarations");

    // ForeignMessageProto2 should exist at top level
    find_msg(&file, "ForeignMessageProto2");
}

// -- test_messages_proto3.proto (exhaustive, has imports but still parseable) --

#[test]
fn test_messages_proto3_structure() {
    let file = parse(&upstream("test_messages_proto3.proto")).unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto3);
    assert_eq!(
        file.package,
        Some("protobuf_test_messages.proto3".to_string())
    );

    // Has 7 imports (well-known types)
    assert_eq!(file.dependency.len(), 7);
    assert!(file
        .dependency
        .contains(&"google/protobuf/any.proto".to_string()));
    assert!(file
        .dependency
        .contains(&"google/protobuf/timestamp.proto".to_string()));

    // TestAllTypesProto3
    let msg = find_msg(&file, "TestAllTypesProto3");

    // All singular scalar types
    for (name, expected_type) in [
        ("optional_int32", FieldType::Int32),
        ("optional_int64", FieldType::Int64),
        ("optional_uint32", FieldType::Uint32),
        ("optional_uint64", FieldType::Uint64),
        ("optional_sint32", FieldType::Sint32),
        ("optional_sint64", FieldType::Sint64),
        ("optional_fixed32", FieldType::Fixed32),
        ("optional_fixed64", FieldType::Fixed64),
        ("optional_sfixed32", FieldType::Sfixed32),
        ("optional_sfixed64", FieldType::Sfixed64),
        ("optional_float", FieldType::Float),
        ("optional_double", FieldType::Double),
        ("optional_bool", FieldType::Bool),
        ("optional_string", FieldType::String),
        ("optional_bytes", FieldType::Bytes),
    ] {
        let f = find_field(msg, name);
        assert_eq!(f.r#type, Some(expected_type), "type mismatch for {}", name);
    }

    // Nested types
    find_nested_msg(msg, "NestedMessage");
    find_nested_enum(msg, "NestedEnum");

    // Should have map fields
    let has_map = msg
        .field
        .iter()
        .any(|f| f.name.as_deref().is_some_and(|n| n.starts_with("map_")));
    assert!(has_map, "expected map fields in TestAllTypesProto3");

    // Should have oneofs
    assert!(
        !msg.oneof_decl.is_empty(),
        "expected oneof in TestAllTypesProto3"
    );
}

// -- unittest_import.proto --

#[test]
fn unittest_import_structure() {
    let file = parse(&upstream("unittest_import.proto")).unwrap();
    assert_eq!(file.syntax_enum(), Syntax::Proto2);
    assert_eq!(file.package, Some("rust_unittest_import".to_string()));

    // Should have ImportMessage, ImportEnum, ImportEnumForMap
    find_msg(&file, "ImportMessage");
    find_enum(&file, "ImportEnum");
    find_enum(&file, "ImportEnumForMap");
}

// -- parent.proto + dots_in_package.proto --

#[test]
fn parent_structure() {
    let file = parse(&upstream("parent.proto")).unwrap();
    assert_eq!(file.package, Some("parent_package".to_string()));
    let parent = find_msg(&file, "Parent");
    assert!(parent.field.is_empty()); // empty message
}

#[test]
fn dots_in_package_structure() {
    let file = parse(&upstream("dots_in_package.proto")).unwrap();
    // Package with dots
    assert!(file.package.as_ref().unwrap().contains('.'));
    assert!(!file.message_type.is_empty());
}

// -- package files --

#[test]
fn package_handling() {
    let pkg = parse(&upstream("package_other.proto")).unwrap();
    assert!(pkg.package.is_some());
    assert!(!pkg.message_type.is_empty());

    let pkg_diff = parse(&upstream("package_other_different.proto")).unwrap();
    assert!(pkg_diff.package.is_some());
    assert_ne!(pkg.package, pkg_diff.package);

    let pkg_import = parse(&upstream("package_import.proto")).unwrap();
    assert!(pkg_import.package.is_some());
}

// -- no_package files --

#[test]
fn no_package_handling() {
    let np_import = parse(&upstream("no_package_import.proto")).unwrap();
    // no_package_import should not have a package
    assert!(
        np_import.package.is_none(),
        "expected no package in no_package_import.proto"
    );

    let np_other = parse(&upstream("no_package_other.proto")).unwrap();
    assert!(np_other.package.is_none());
}

// ---------------------------------------------------------------------------
// Tier 3: Error tests (ExpectHasErrors pattern)
// ---------------------------------------------------------------------------

#[test]
fn error_missing_semicolon() {
    let result = parse("syntax = \"proto3\"\nmessage Foo {}");
    assert!(result.is_err());
}

#[test]
fn error_missing_field_number() {
    let result = parse(
        r#"
        syntax = "proto3";
        message Foo { int32 x; }
    "#,
    );
    assert!(result.is_err());
}

#[test]
fn error_unterminated_string() {
    let result = parse(r#"syntax = "proto3"#);
    assert!(result.is_err());
}

#[test]
fn error_unterminated_message() {
    let result = parse(
        r#"
        syntax = "proto3";
        message Foo {
    "#,
    );
    assert!(result.is_err());
}

#[test]
fn error_unexpected_token() {
    let result = parse(
        r#"
        syntax = "proto3";
        } }
    "#,
    );
    assert!(result.is_err());
}

#[test]
fn error_invalid_field_number() {
    let result = parse(
        r#"
        syntax = "proto3";
        message Foo { int32 x = abc; }
    "#,
    );
    assert!(result.is_err());
}
