//! Integration tests for the protobuf annotator.
//!
//! Uses proto-gen to generate random schemas + binaries, then verifies
//! the annotator can walk them without errors.

use protoc_rs_analyzer::analyze;
use protoc_rs_annotator::walk_protobuf;
use protoc_rs_proto_gen::{generate, GenConfig};

/// Walk random proto-gen output with schema and verify no errors.
#[test]
fn annotate_random_binaries_no_errors() {
    let config = GenConfig {
        max_messages: 3,
        max_fields_per_message: 5,
        max_enums: 2,
        max_enum_values: 4,
        max_nesting_depth: 2,
        prob_message_field: 0.3,
        prob_enum_field: 0.2,
        prob_repeated: 0.2,
        prob_nested_message: 0.2,
    };

    let mut successes = 0;
    let mut failures = Vec::new();

    for seed in 0..50 {
        let gen = generate(seed, config.clone());

        // Parse and analyze the schema
        let fds = match analyze(&gen.schema_text) {
            Ok(fds) => fds,
            Err(e) => {
                // Some random schemas may not analyze (rare edge cases)
                failures.push(format!("seed {}: analyze error: {}", seed, e));
                continue;
            }
        };

        // Proto-gen doesn't set a package, so FQN is just ".MessageName"
        let root_fqn = format!(".{}", gen.root_message);
        match walk_protobuf(&gen.binary_data, &fds, &root_fqn) {
            Ok(regions) => {
                // Basic sanity: root region should exist
                assert!(!regions.is_empty(), "seed {}: no regions produced", seed);
                // Root region should cover the entire binary
                assert_eq!(
                    regions[0].byte_range,
                    0..gen.binary_data.len(),
                    "seed {}: root region doesn't cover full binary",
                    seed
                );
                successes += 1;
            }
            Err(e) => {
                failures.push(format!("seed {}: walk error: {}", seed, e));
            }
        }
    }

    eprintln!(
        "\n=== Annotator round-trip: {} passed, {} failed ===",
        successes,
        failures.len()
    );
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("  {}", f);
        }
    }
    // Some seeds produce schemas with duplicate enum values (proto-gen bug),
    // causing analyze errors. These are not annotator failures.
    // Count only walk errors (not analyze errors) as real failures.
    let walk_failures: Vec<_> = failures
        .iter()
        .filter(|f| f.contains("walk error"))
        .collect();
    assert!(
        walk_failures.is_empty(),
        "Walk failures (annotator bugs): {:?}",
        walk_failures
    );
}

/// Walk a known simple binary and verify byte coverage.
#[test]
fn annotate_known_binary_full_coverage() {
    let proto = r#"
        syntax = "proto3";
        package test;
        message Person {
            string name = 1;
            int32 id = 2;
        }
    "#;
    let fds = analyze(proto).unwrap();

    // Person { name: "Bob", id: 7 }
    let mut data = Vec::new();
    data.push(0x0A); // field 1, LEN
    data.push(3); // length
    data.extend_from_slice(b"Bob");
    data.push(0x10); // field 2, varint
    data.push(7);

    let regions = walk_protobuf(&data, &fds, ".test.Person").unwrap();

    // Collect all leaf byte ranges
    let mut covered = vec![false; data.len()];
    for region in &regions {
        for i in region.byte_range.clone() {
            if i < covered.len() {
                covered[i] = true;
            }
        }
    }

    // Every byte should be covered by at least one region
    for (i, &c) in covered.iter().enumerate() {
        assert!(c, "byte {} not covered by any region", i);
    }
}

/// Verify enum values are decoded by name.
#[test]
fn annotate_enum_shows_value_name() {
    let proto = r#"
        syntax = "proto3";
        package test;
        enum Color { RED = 0; GREEN = 1; BLUE = 2; }
        message Paint { Color color = 1; }
    "#;
    let fds = analyze(proto).unwrap();

    // Paint { color: GREEN (1) }
    let data = vec![0x08, 0x01];
    let regions = walk_protobuf(&data, &fds, ".test.Paint").unwrap();

    let enum_region = regions.iter().find(|r| {
        matches!(
            &r.kind,
            protoc_rs_annotator::ProtoRegionKind::EnumValue { value_name, .. }
            if value_name == "GREEN"
        )
    });
    assert!(enum_region.is_some(), "should find GREEN enum value");
}

/// Verify nested messages are recursively annotated.
#[test]
fn annotate_nested_message_recursive() {
    let proto = r#"
        syntax = "proto3";
        package test;
        message Outer {
            message Inner { int32 x = 1; }
            Inner child = 1;
        }
    "#;
    let fds = analyze(proto).unwrap();

    // Outer { child: Inner { x: 99 } }
    let mut inner = Vec::new();
    inner.push(0x08); // field 1, varint
    inner.push(99);

    let mut data = Vec::new();
    data.push(0x0A); // field 1, LEN
    data.push(inner.len() as u8);
    data.extend_from_slice(&inner);

    let regions = walk_protobuf(&data, &fds, ".test.Outer").unwrap();

    // Should find the inner varint with value 99
    let inner_val = regions.iter().find(|r| r.value_display.contains("99"));
    assert!(inner_val.is_some(), "should find nested value 99");

    // Should have depth > 1 for the inner field
    let deep_regions: Vec<_> = regions.iter().filter(|r| r.depth >= 2).collect();
    assert!(
        !deep_regions.is_empty(),
        "should have deeply nested regions"
    );
}
