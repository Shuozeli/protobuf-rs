//! Fuzz target: JSON serialization/deserialization of EnumValue.

#![no_main]
use libfuzzer_sys::fuzz_target;
use protoc_rs_runtime::{EnumValue, Enumeration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
enum TestEnum {
    Unknown = 0,
    Foo = 1,
    Bar = 2,
}

impl Enumeration for TestEnum {
    fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(TestEnum::Unknown),
            1 => Some(TestEnum::Foo),
            2 => Some(TestEnum::Bar),
            _ => None,
        }
    }
    fn to_i32(&self) -> i32 { *self as i32 }
    fn proto_name(&self) -> &'static str {
        match self {
            TestEnum::Unknown => "UNKNOWN",
            TestEnum::Foo => "FOO",
            TestEnum::Bar => "BAR",
        }
    }
    fn from_proto_name(name: &str) -> Option<Self> {
        match name {
            "UNKNOWN" => Some(TestEnum::Unknown),
            "FOO" => Some(TestEnum::Foo),
            "BAR" => Some(TestEnum::Bar),
            _ => None,
        }
    }
    fn default_value() -> Self { TestEnum::Unknown }
}

fuzz_target!(|data: &[u8]| {
    // Try to deserialize arbitrary JSON as an EnumValue
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(v) = serde_json::from_str::<EnumValue<TestEnum>>(s) {
            // Re-serialize and verify it doesn't panic
            let json = serde_json::to_string(&v).expect("serialize must succeed");

            // Re-deserialize
            let v2: EnumValue<TestEnum> = serde_json::from_str(&json)
                .expect("re-deserialize must succeed");
            assert_eq!(v, v2, "JSON round-trip mismatch");
        }
    }
});
