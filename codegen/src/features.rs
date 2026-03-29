//! Protobuf editions feature resolution.
//!
//! Editions (2023, 2024) make feature behavior explicit. Proto2 and proto3
//! are treated as feature presets:
//!
//! | Feature                    | proto2    | proto3    | edition 2023 default |
//! |---------------------------|-----------|-----------|---------------------|
//! | field_presence            | EXPLICIT  | IMPLICIT  | EXPLICIT            |
//! | enum_type                 | CLOSED    | OPEN      | OPEN                |
//! | repeated_field_encoding   | EXPANDED  | PACKED    | PACKED              |
//! | utf8_validation           | NONE      | VERIFY    | VERIFY              |
//! | message_encoding          | LENGTH_PREFIXED | LENGTH_PREFIXED | LENGTH_PREFIXED |

use protoc_rs_schema::*;

/// Resolved feature set for a proto file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFeatures {
    /// Whether singular fields have explicit presence tracking.
    /// EXPLICIT = proto2 behavior (has_field / optional)
    /// IMPLICIT = proto3 behavior (no has_field for scalars)
    pub field_presence: FieldPresence,

    /// Whether enums are open (accept unknown values) or closed.
    pub enum_type: EnumType,

    /// Whether repeated scalar fields use packed encoding by default.
    pub packed_by_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPresence {
    /// Fields have explicit presence (proto2 behavior).
    Explicit,
    /// Scalar fields have implicit presence (proto3 behavior).
    Implicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumType {
    /// Enums are open (unknown values preserved). Proto3 default.
    Open,
    /// Enums are closed (unknown values rejected/stored as unknown fields). Proto2 default.
    Closed,
}

impl ResolvedFeatures {
    /// Resolve features for a file based on its syntax/edition.
    pub fn from_file(file: &FileDescriptorProto) -> Self {
        let syntax = file.syntax_enum();
        match &syntax {
            Syntax::Proto2 => Self::proto2(),
            Syntax::Proto3 => Self::proto3(),
            Syntax::Unknown(s) => {
                // Editions: parse edition string
                match s.as_str() {
                    "editions" => {
                        // Look at the edition field
                        match file.edition.as_deref() {
                            Some("2023") => Self::edition_2023(),
                            Some("2024") => Self::edition_2024(),
                            _ => Self::edition_2023(), // default to 2023
                        }
                    }
                    _ => Self::proto3(), // fallback
                }
            }
        }
    }

    /// Proto2 feature preset.
    pub fn proto2() -> Self {
        Self {
            field_presence: FieldPresence::Explicit,
            enum_type: EnumType::Closed,
            packed_by_default: false,
        }
    }

    /// Proto3 feature preset.
    pub fn proto3() -> Self {
        Self {
            field_presence: FieldPresence::Implicit,
            enum_type: EnumType::Open,
            packed_by_default: true,
        }
    }

    /// Edition 2023 defaults.
    pub fn edition_2023() -> Self {
        Self {
            field_presence: FieldPresence::Explicit,
            enum_type: EnumType::Open,
            packed_by_default: true,
        }
    }

    /// Edition 2024 defaults.
    pub fn edition_2024() -> Self {
        Self {
            field_presence: FieldPresence::Explicit,
            enum_type: EnumType::Open,
            packed_by_default: true,
        }
    }

    /// Whether a scalar field should be optional (have presence tracking).
    pub fn is_scalar_optional(&self, field: &FieldDescriptorProto) -> bool {
        let label = field.label.unwrap_or(FieldLabel::Optional);
        if label == FieldLabel::Repeated {
            return false;
        }
        if field.proto3_optional == Some(true) {
            return true;
        }
        let field_type = field.r#type.unwrap_or(FieldType::Int32);
        match self.field_presence {
            FieldPresence::Implicit => {
                // Proto3 behavior: only message fields are optional by default
                field_type == FieldType::Message
            }
            FieldPresence::Explicit => {
                // Proto2/editions behavior: optional/required labels
                label != FieldLabel::Required
            }
        }
    }

    /// Whether a repeated scalar field should use packed encoding.
    pub fn is_packed(&self, field: &FieldDescriptorProto) -> bool {
        let field_type = field.r#type.unwrap_or(FieldType::Int32);
        if !field_type.is_packable() {
            return false;
        }
        self.packed_by_default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto2_features() {
        let f = ResolvedFeatures::proto2();
        assert_eq!(f.field_presence, FieldPresence::Explicit);
        assert_eq!(f.enum_type, EnumType::Closed);
        assert!(!f.packed_by_default);
    }

    #[test]
    fn proto3_features() {
        let f = ResolvedFeatures::proto3();
        assert_eq!(f.field_presence, FieldPresence::Implicit);
        assert_eq!(f.enum_type, EnumType::Open);
        assert!(f.packed_by_default);
    }

    #[test]
    fn edition_2023_features() {
        let f = ResolvedFeatures::edition_2023();
        assert_eq!(f.field_presence, FieldPresence::Explicit);
        assert_eq!(f.enum_type, EnumType::Open);
        assert!(f.packed_by_default);
    }

    #[test]
    fn scalar_optional_proto3() {
        let f = ResolvedFeatures::proto3();
        let field = FieldDescriptorProto {
            label: Some(FieldLabel::Optional),
            r#type: Some(FieldType::Int32),
            ..Default::default()
        };
        // Proto3: scalar not optional
        assert!(!f.is_scalar_optional(&field));
    }

    #[test]
    fn scalar_optional_proto2() {
        let f = ResolvedFeatures::proto2();
        let field = FieldDescriptorProto {
            label: Some(FieldLabel::Optional),
            r#type: Some(FieldType::Int32),
            ..Default::default()
        };
        // Proto2: optional scalar IS optional
        assert!(f.is_scalar_optional(&field));
    }

    #[test]
    fn required_not_optional() {
        let f = ResolvedFeatures::proto2();
        let field = FieldDescriptorProto {
            label: Some(FieldLabel::Required),
            r#type: Some(FieldType::Int32),
            ..Default::default()
        };
        assert!(!f.is_scalar_optional(&field));
    }

    #[test]
    fn proto3_optional_explicit() {
        let f = ResolvedFeatures::proto3();
        let field = FieldDescriptorProto {
            label: Some(FieldLabel::Optional),
            r#type: Some(FieldType::Int32),
            proto3_optional: Some(true),
            ..Default::default()
        };
        assert!(f.is_scalar_optional(&field));
    }
}
