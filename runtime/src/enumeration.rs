//! Type-safe protobuf enum handling with unknown value preservation.
//!
//! Protobuf "open" enums can carry values not defined in the schema (for
//! forward compatibility). `EnumValue<E>` wraps a generated enum `E` to
//! preserve unknown values through round-trips, replacing prost's raw `i32`.

use core::fmt;
use core::hash::{Hash, Hasher};

/// Trait implemented by all generated protobuf enum types.
pub trait Enumeration: Copy + Eq + Hash + fmt::Debug + Sized {
    /// Try to convert from a wire value. Returns `None` for unknown values.
    fn from_i32(value: i32) -> Option<Self>;

    /// Convert to the wire value.
    fn to_i32(&self) -> i32;

    /// The proto field name of this variant (for JSON serialization).
    fn proto_name(&self) -> &'static str;

    /// Convert from a proto field name (for JSON deserialization).
    fn from_proto_name(name: &str) -> Option<Self>;

    /// The default value (wire value 0).
    fn default_value() -> Self;
}

/// A protobuf enum value that preserves unknown variants.
///
/// For open enums (proto3 default, editions with `OPEN`), unknown wire values
/// are stored as `Unknown(i32)` and round-trip correctly. For closed enums
/// (proto2 default, editions with `CLOSED`), unknown values are routed to
/// unknown fields at the message level instead.
#[derive(Copy, Clone)]
pub enum EnumValue<E: Enumeration> {
    /// A recognized enum variant.
    Known(E),
    /// An unrecognized wire value (preserved for round-trip fidelity).
    Unknown(i32),
}

impl<E: Enumeration> EnumValue<E> {
    /// Get the wire value.
    pub fn to_i32(&self) -> i32 {
        match self {
            EnumValue::Known(e) => e.to_i32(),
            EnumValue::Unknown(v) => *v,
        }
    }

    /// Get the known variant, if any.
    pub fn known(&self) -> Option<E> {
        match self {
            EnumValue::Known(e) => Some(*e),
            EnumValue::Unknown(_) => None,
        }
    }

    /// Returns `true` if this is a known variant.
    pub fn is_known(&self) -> bool {
        matches!(self, EnumValue::Known(_))
    }
}

impl<E: Enumeration> From<i32> for EnumValue<E> {
    fn from(value: i32) -> Self {
        match E::from_i32(value) {
            Some(e) => EnumValue::Known(e),
            None => EnumValue::Unknown(value),
        }
    }
}

impl<E: Enumeration> From<E> for EnumValue<E> {
    fn from(e: E) -> Self {
        EnumValue::Known(e)
    }
}

impl<E: Enumeration> Default for EnumValue<E> {
    fn default() -> Self {
        EnumValue::Known(E::default_value())
    }
}

impl<E: Enumeration> PartialEq for EnumValue<E> {
    fn eq(&self, other: &Self) -> bool {
        self.to_i32() == other.to_i32()
    }
}

impl<E: Enumeration> Eq for EnumValue<E> {}

impl<E: Enumeration> PartialEq<E> for EnumValue<E> {
    fn eq(&self, other: &E) -> bool {
        self.to_i32() == other.to_i32()
    }
}

impl<E: Enumeration> Hash for EnumValue<E> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_i32().hash(state);
    }
}

impl<E: Enumeration> fmt::Debug for EnumValue<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnumValue::Known(e) => write!(f, "{:?}", e),
            EnumValue::Unknown(v) => write!(f, "Unknown({})", v),
        }
    }
}

impl<E: Enumeration + fmt::Display> fmt::Display for EnumValue<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnumValue::Known(e) => write!(f, "{}", e),
            EnumValue::Unknown(v) => write!(f, "{}", v),
        }
    }
}

// ---------------------------------------------------------------------------
// Serde support (behind "json" feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "json")]
impl<E: Enumeration> serde::Serialize for EnumValue<E> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            EnumValue::Known(e) => serializer.serialize_str(e.proto_name()),
            EnumValue::Unknown(v) => serializer.serialize_i32(*v),
        }
    }
}

#[cfg(feature = "json")]
impl<'de, E: Enumeration> serde::Deserialize<'de> for EnumValue<E> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EnumValueVisitor<E>(core::marker::PhantomData<E>);

        impl<'de, E: Enumeration> serde::de::Visitor<'de> for EnumValueVisitor<E> {
            type Value = EnumValue<E>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string enum name or integer value")
            }

            fn visit_str<Err: serde::de::Error>(self, v: &str) -> Result<Self::Value, Err> {
                match E::from_proto_name(v) {
                    Some(e) => Ok(EnumValue::Known(e)),
                    None => Err(serde::de::Error::unknown_variant(v, &[])),
                }
            }

            fn visit_i64<Err: serde::de::Error>(self, v: i64) -> Result<Self::Value, Err> {
                Ok(EnumValue::from(v as i32))
            }

            fn visit_u64<Err: serde::de::Error>(self, v: u64) -> Result<Self::Value, Err> {
                Ok(EnumValue::from(v as i32))
            }

        }

        deserializer.deserialize_any(EnumValueVisitor(core::marker::PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        fn to_i32(&self) -> i32 {
            *self as i32
        }

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

        fn default_value() -> Self {
            TestEnum::Unknown
        }
    }

    #[test]
    fn known_value_from_i32() {
        let v: EnumValue<TestEnum> = EnumValue::from(1);
        assert_eq!(v, EnumValue::Known(TestEnum::Foo));
        assert!(v.is_known());
        assert_eq!(v.known(), Some(TestEnum::Foo));
        assert_eq!(v.to_i32(), 1);
    }

    #[test]
    fn unknown_value_from_i32() {
        let v: EnumValue<TestEnum> = EnumValue::from(99);
        assert_eq!(v, EnumValue::Unknown(99));
        assert!(!v.is_known());
        assert_eq!(v.known(), None);
        assert_eq!(v.to_i32(), 99);
    }

    #[test]
    fn default_is_variant_zero() {
        let v: EnumValue<TestEnum> = EnumValue::default();
        assert_eq!(v, EnumValue::Known(TestEnum::Unknown));
    }

    #[test]
    fn equality_compares_wire_value() {
        let a: EnumValue<TestEnum> = EnumValue::Known(TestEnum::Foo);
        let b: EnumValue<TestEnum> = EnumValue::from(1);
        assert_eq!(a, b);
    }

    #[test]
    fn partial_eq_with_bare_enum() {
        let v: EnumValue<TestEnum> = EnumValue::from(2);
        assert_eq!(v, TestEnum::Bar);
    }

    #[test]
    fn unknown_values_not_equal_to_known() {
        let v: EnumValue<TestEnum> = EnumValue::Unknown(99);
        assert_ne!(v, TestEnum::Foo);
        assert_ne!(v, TestEnum::Bar);
    }

    #[test]
    fn hash_consistency() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(EnumValue::Known(TestEnum::Foo));
        assert!(set.contains(&EnumValue::<TestEnum>::from(1)));
    }

    #[cfg(feature = "json")]
    mod serde_tests {
        use super::*;

        #[test]
        fn known_enum_to_string() {
            let v: EnumValue<TestEnum> = EnumValue::Known(TestEnum::Foo);
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, "\"FOO\"");
        }

        #[test]
        fn unknown_enum_to_int() {
            let v: EnumValue<TestEnum> = EnumValue::Unknown(99);
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, "99");
        }

        #[test]
        fn deserialize_from_string() {
            let v: EnumValue<TestEnum> = serde_json::from_str("\"BAR\"").unwrap();
            assert_eq!(v, EnumValue::Known(TestEnum::Bar));
        }

        #[test]
        fn deserialize_from_int() {
            let v: EnumValue<TestEnum> = serde_json::from_str("1").unwrap();
            assert_eq!(v, EnumValue::Known(TestEnum::Foo));
        }

        #[test]
        fn deserialize_unknown_int() {
            let v: EnumValue<TestEnum> = serde_json::from_str("99").unwrap();
            assert_eq!(v, EnumValue::Unknown(99));
        }

        #[test]
        fn round_trip_known() {
            let original: EnumValue<TestEnum> = EnumValue::Known(TestEnum::Bar);
            let json = serde_json::to_string(&original).unwrap();
            let decoded: EnumValue<TestEnum> = serde_json::from_str(&json).unwrap();
            assert_eq!(original, decoded);
        }
    }
}
