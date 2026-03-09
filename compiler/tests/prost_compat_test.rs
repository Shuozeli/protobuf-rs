//! Discrepancy tests: compare protoc-rs codegen output against prost-build.
//!
//! These tests compile .proto files with both our codegen and prost-build,
//! then compare the generated Rust code structurally.
//!
//! Requires `protoc` to be installed (prost-build shells out to it).

use std::collections::HashMap;

/// Generate Rust code using our codegen pipeline (analyzer -> codegen).
fn our_codegen(proto_source: &str) -> HashMap<String, String> {
    let fds =
        protoc_rs_analyzer::analyze(proto_source).expect("our analyzer failed to parse proto");
    protoc_rs_codegen::generate_rust(&fds).expect("our codegen failed")
}

/// Generate Rust code using prost-build (which calls protoc under the hood).
fn prost_codegen(proto_source: &str, filename: &str) -> HashMap<String, String> {
    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let proto_path = tmp.path().join(filename);
    let out_dir = tmp.path().join("out");
    std::fs::create_dir(&out_dir).expect("failed to create out dir");

    std::fs::write(&proto_path, proto_source).expect("failed to write proto file");

    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&[&proto_path], &[tmp.path()])
        .expect("prost-build failed");

    // Read all generated .rs files
    let mut result = HashMap::new();
    for entry in std::fs::read_dir(&out_dir).expect("failed to read out dir") {
        let entry = entry.expect("failed to read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let filename = path.file_name().unwrap().to_str().unwrap().to_string();
            let content = std::fs::read_to_string(&path).expect("failed to read generated file");
            result.insert(filename, content);
        }
    }
    result
}

/// Normalize generated source for comparison:
/// - Strip comment lines (// ...)
/// - Strip blank lines
/// - Trim leading/trailing whitespace per line
/// - Join continuation lines (lines not starting with a keyword/annotation/closing brace)
fn normalize(source: &str) -> Vec<String> {
    let raw: Vec<&str> = source
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("//"))
        .collect();

    // Join continuation lines: if a line doesn't start with a recognized
    // pattern, append it to the previous line. This handles prost's
    // multi-line formatting for long type declarations.
    let mut result = Vec::new();
    for line in raw {
        let is_new_item = line.starts_with("#[")
            || line.starts_with("pub ")
            || line.starts_with("}")
            || line.starts_with("{")
            || line.starts_with("impl ")
            || line.starts_with("match ")
            || line.starts_with("Self::")
            || line.starts_with("\"")
            || line.starts_with("_ =>")
            || line.ends_with(" {")
            || result.is_empty();

        if !is_new_item && !result.is_empty() {
            // Continuation line -- append to previous
            let last: &mut String = result.last_mut().unwrap();
            last.push(' ');
            last.push_str(line);
        } else {
            result.push(line.to_string());
        }
    }

    // Normalize whitespace and trailing commas in generics
    result
        .into_iter()
        .map(|l| {
            let normalized = l.split_whitespace().collect::<Vec<_>>().join(" ");
            // Normalize formatting differences in generics
            normalized
                .replace(", >", ">")
                .replace(",>", ">")
                .replace("< ", "<")
                .replace(" >", ">")
        })
        .collect()
}

/// Extract specific patterns from generated code for structural comparison.
/// Returns a sorted list of extracted items for stable comparison.
struct CodeFacts {
    structs: Vec<String>,
    enums: Vec<String>,
    /// field_name -> (prost_annotation, rust_type)
    fields: Vec<(String, String)>,
    #[allow(dead_code)]
    derives: Vec<String>,
}

fn extract_facts(source: &str) -> CodeFacts {
    let lines = normalize(source);
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut fields = Vec::new();
    let mut derives = Vec::new();

    let mut pending_annotation = String::new();

    for line in &lines {
        if line.starts_with("pub struct ") {
            let name = line
                .trim_start_matches("pub struct ")
                .trim_end_matches(" {")
                .to_string();
            structs.push(name);
        } else if line.starts_with("pub enum ") {
            let name = line
                .trim_start_matches("pub enum ")
                .trim_end_matches(" {")
                .to_string();
            enums.push(name);
        } else if line.starts_with("#[prost(") {
            pending_annotation = line.clone();
        } else if line.starts_with("pub ") && line.contains(':') && !pending_annotation.is_empty() {
            fields.push((pending_annotation.clone(), line.clone()));
            pending_annotation.clear();
        } else if line.starts_with("#[derive(") {
            derives.push(line.clone());
        }
    }

    structs.sort();
    enums.sort();

    CodeFacts {
        structs,
        enums,
        fields,
        derives,
    }
}

/// Assert that two sets of code facts match, with detailed error messages.
fn assert_facts_match(ours: &CodeFacts, prost: &CodeFacts, context: &str) {
    // Compare structs
    assert_eq!(
        ours.structs, prost.structs,
        "{context}: struct names differ"
    );

    // Compare enums
    assert_eq!(ours.enums, prost.enums, "{context}: enum names differ");

    // Compare fields (by annotation + type)
    if ours.fields.len() != prost.fields.len() {
        eprintln!("=== OUR FIELDS ({}) ===", ours.fields.len());
        for (ann, field) in &ours.fields {
            eprintln!("  {ann}");
            eprintln!("  {field}");
        }
        eprintln!("=== PROST FIELDS ({}) ===", prost.fields.len());
        for (ann, field) in &prost.fields {
            eprintln!("  {ann}");
            eprintln!("  {field}");
        }
        panic!(
            "{context}: field count differs: ours={}, prost={}",
            ours.fields.len(),
            prost.fields.len()
        );
    }

    for (i, ((our_ann, our_field), (prost_ann, prost_field))) in
        ours.fields.iter().zip(prost.fields.iter()).enumerate()
    {
        if our_ann != prost_ann {
            eprintln!("Field {i} annotation mismatch:");
            eprintln!("  ours:  {our_ann}");
            eprintln!("  prost: {prost_ann}");
        }
        if our_field != prost_field {
            eprintln!("Field {i} declaration mismatch:");
            eprintln!("  ours:  {our_field}");
            eprintln!("  prost: {prost_field}");
        }
    }

    // Collect mismatches for a single assertion
    let annotation_mismatches: Vec<_> = ours
        .fields
        .iter()
        .zip(prost.fields.iter())
        .enumerate()
        .filter(|(_, ((oa, _), (pa, _)))| oa != pa)
        .map(|(i, ((oa, _), (pa, _)))| format!("field {i}: ours={oa}, prost={pa}"))
        .collect();

    let field_mismatches: Vec<_> = ours
        .fields
        .iter()
        .zip(prost.fields.iter())
        .enumerate()
        .filter(|(_, ((_, of), (_, pf)))| of != pf)
        .map(|(i, ((_, of), (_, pf)))| format!("field {i}: ours={of}, prost={pf}"))
        .collect();

    if !annotation_mismatches.is_empty() {
        panic!(
            "{context}: annotation mismatches:\n{}",
            annotation_mismatches.join("\n")
        );
    }

    if !field_mismatches.is_empty() {
        panic!(
            "{context}: field type mismatches:\n{}",
            field_mismatches.join("\n")
        );
    }
}

/// Run a full discrepancy test: compile with both, compare facts.
fn run_discrepancy_test(proto_source: &str, filename: &str, expected_module: &str) {
    let ours = our_codegen(proto_source);
    let prost = prost_codegen(proto_source, filename);

    let our_key = format!("{expected_module}.rs");
    let our_source = ours.get(&our_key).unwrap_or_else(|| {
        panic!(
            "our codegen missing module {our_key}, keys: {:?}",
            ours.keys().collect::<Vec<_>>()
        )
    });

    let prost_key = format!("{expected_module}.rs");
    let prost_source = prost.get(&prost_key).unwrap_or_else(|| {
        panic!(
            "prost missing module {prost_key}, keys: {:?}",
            prost.keys().collect::<Vec<_>>()
        )
    });

    eprintln!("=== OUR OUTPUT ===");
    eprintln!("{our_source}");
    eprintln!("=== PROST OUTPUT ===");
    eprintln!("{prost_source}");

    let our_facts = extract_facts(our_source);
    let prost_facts = extract_facts(prost_source);

    assert_facts_match(&our_facts, &prost_facts, filename);
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

#[test]
fn compat_simple_scalars() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message Scalars {
                double f_double = 1;
                float f_float = 2;
                int32 f_int32 = 3;
                int64 f_int64 = 4;
                uint32 f_uint32 = 5;
                uint64 f_uint64 = 6;
                sint32 f_sint32 = 7;
                sint64 f_sint64 = 8;
                fixed32 f_fixed32 = 9;
                fixed64 f_fixed64 = 10;
                sfixed32 f_sfixed32 = 11;
                sfixed64 f_sfixed64 = 12;
                bool f_bool = 13;
                string f_string = 14;
                bytes f_bytes = 15;
            }
        "#,
        "scalars.proto",
        "compat",
    );
}

#[test]
fn compat_proto3_optional() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message OptionalFields {
                optional int32 opt_int = 1;
                optional string opt_str = 2;
                optional double opt_dbl = 3;
                optional bool opt_bool = 4;
            }
        "#,
        "optional.proto",
        "compat",
    );
}

#[test]
fn compat_repeated_fields() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message Repeated {
                repeated int32 ids = 1;
                repeated string names = 2;
                repeated bytes blobs = 3;
            }
        "#,
        "repeated.proto",
        "compat",
    );
}

#[test]
fn compat_enum() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            enum Color {
                COLOR_UNKNOWN = 0;
                COLOR_RED = 1;
                COLOR_GREEN = 2;
                COLOR_BLUE = 3;
            }

            message WithEnum {
                Color color = 1;
                repeated Color favorites = 2;
            }
        "#,
        "enum.proto",
        "compat",
    );
}

#[test]
fn compat_nested_message() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message Outer {
                message Inner {
                    int32 value = 1;
                    string label = 2;
                }
                Inner inner = 1;
                repeated Inner items = 2;
            }
        "#,
        "nested.proto",
        "compat",
    );
}

#[test]
fn compat_oneof() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message WithOneof {
                string common = 1;
                oneof payload {
                    string text = 2;
                    int32 number = 3;
                    bool flag = 4;
                }
            }
        "#,
        "oneof.proto",
        "compat",
    );
}

#[test]
fn compat_map_fields() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message WithMaps {
                map<string, string> str_map = 1;
                map<int32, string> int_map = 2;
                map<string, int64> val_map = 3;
            }
        "#,
        "maps.proto",
        "compat",
    );
}

#[test]
fn compat_message_field() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message Address {
                string street = 1;
                string city = 2;
                int32 zip = 3;
            }

            message Person {
                string name = 1;
                Address address = 2;
                repeated Address other_addresses = 3;
            }
        "#,
        "message_field.proto",
        "compat",
    );
}

#[test]
fn compat_proto2_basic() {
    run_discrepancy_test(
        r#"
            syntax = "proto2";
            package compat;

            message Proto2Msg {
                optional int32 opt_field = 1;
                required string req_field = 2;
                repeated int32 rep_field = 3;
            }
        "#,
        "proto2.proto",
        "compat",
    );
}

#[test]
fn compat_keyword_fields() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            message Keywords {
                string type = 1;
                int32 ref = 2;
                bool match = 3;
                string self = 4;
            }
        "#,
        "keywords.proto",
        "compat",
    );
}

#[test]
fn compat_complex_combined() {
    run_discrepancy_test(
        r#"
            syntax = "proto3";
            package compat;

            enum Status {
                STATUS_UNKNOWN = 0;
                STATUS_ACTIVE = 1;
                STATUS_INACTIVE = 2;
            }

            message Person {
                string name = 1;
                int32 id = 2;
                string email = 3;
                Status status = 4;
                repeated string tags = 5;
                optional double score = 6;

                message Address {
                    string street = 1;
                    string city = 2;
                }

                Address home_address = 7;
                map<string, string> metadata = 8;

                oneof contact {
                    string phone = 9;
                    string fax = 10;
                }
            }
        "#,
        "complex.proto",
        "compat",
    );
}
