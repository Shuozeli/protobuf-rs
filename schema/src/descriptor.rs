use crate::source_info::{SourceCodeInfo, Span};

// ---------------------------------------------------------------------------
// Top-level containers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileDescriptorSet {
    pub file: Vec<FileDescriptorProto>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileDescriptorProto {
    pub name: Option<String>,
    pub package: Option<String>,
    pub dependency: Vec<String>,
    pub public_dependency: Vec<i32>,
    pub weak_dependency: Vec<i32>,
    /// Editions: option imports (don't require the file to be loaded).
    pub option_dependency: Vec<String>,
    pub message_type: Vec<DescriptorProto>,
    pub enum_type: Vec<EnumDescriptorProto>,
    pub service: Vec<ServiceDescriptorProto>,
    pub extension: Vec<FieldDescriptorProto>,
    pub options: Option<FileOptions>,
    pub source_code_info: Option<SourceCodeInfo>,
    pub syntax: Option<String>,
    pub edition: Option<String>,
}

impl FileDescriptorProto {
    pub fn syntax_enum(&self) -> Syntax {
        match self.syntax.as_deref() {
            Some("proto3") => Syntax::Proto3,
            Some("proto2") | None => Syntax::Proto2,
            Some(other) => Syntax::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Syntax {
    Proto2,
    Proto3,
    Unknown(String),
}

// ---------------------------------------------------------------------------
// Visibility (editions 2024+)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Visibility {
    Export = 0,
    Local = 1,
}

// ---------------------------------------------------------------------------
// Message descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DescriptorProto {
    pub name: Option<String>,
    pub field: Vec<FieldDescriptorProto>,
    pub extension: Vec<FieldDescriptorProto>,
    pub nested_type: Vec<DescriptorProto>,
    pub enum_type: Vec<EnumDescriptorProto>,
    pub extension_range: Vec<ExtensionRange>,
    pub oneof_decl: Vec<OneofDescriptorProto>,
    pub options: Option<MessageOptions>,
    pub reserved_range: Vec<ReservedRange>,
    pub reserved_name: Vec<String>,
    /// Visibility modifier (editions 2024+).
    pub visibility: Option<Visibility>,
    /// Source location of the `message` keyword. Populated by parser, not serialized.
    #[doc(hidden)]
    pub source_span: Option<Span>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionRange {
    pub start: Option<i32>,
    pub end: Option<i32>,
    pub options: Option<ExtensionRangeOptions>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionRangeOptions {
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReservedRange {
    pub start: Option<i32>,
    pub end: Option<i32>,
}

// ---------------------------------------------------------------------------
// Field descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldDescriptorProto {
    pub name: Option<String>,
    pub number: Option<i32>,
    pub label: Option<FieldLabel>,
    pub r#type: Option<FieldType>,
    /// For message/enum fields: the fully-qualified type name (e.g. ".pkg.MyMessage").
    /// Unresolved during parsing; resolved by analyzer.
    pub type_name: Option<String>,
    /// For extension fields: the message being extended.
    pub extendee: Option<String>,
    pub default_value: Option<String>,
    /// Index into the containing message's `oneof_decl` list.
    pub oneof_index: Option<i32>,
    pub json_name: Option<String>,
    pub options: Option<FieldOptions>,
    /// True if this is a proto3 optional field (has synthetic oneof).
    pub proto3_optional: Option<bool>,
    /// Source location of the field definition. Populated by parser, not serialized.
    #[doc(hidden)]
    pub source_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FieldType {
    Double = 1,
    Float = 2,
    Int64 = 3,
    Uint64 = 4,
    Int32 = 5,
    Fixed64 = 6,
    Fixed32 = 7,
    Bool = 8,
    String = 9,
    Group = 10,
    Message = 11,
    Bytes = 12,
    Uint32 = 13,
    Enum = 14,
    Sfixed32 = 15,
    Sfixed64 = 16,
    Sint32 = 17,
    Sint64 = 18,
}

impl FieldType {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Double),
            2 => Some(Self::Float),
            3 => Some(Self::Int64),
            4 => Some(Self::Uint64),
            5 => Some(Self::Int32),
            6 => Some(Self::Fixed64),
            7 => Some(Self::Fixed32),
            8 => Some(Self::Bool),
            9 => Some(Self::String),
            10 => Some(Self::Group),
            11 => Some(Self::Message),
            12 => Some(Self::Bytes),
            13 => Some(Self::Uint32),
            14 => Some(Self::Enum),
            15 => Some(Self::Sfixed32),
            16 => Some(Self::Sfixed64),
            17 => Some(Self::Sint32),
            18 => Some(Self::Sint64),
            _ => None,
        }
    }

    /// Returns the protobuf wire type for this field type.
    pub fn wire_type(&self) -> WireType {
        match self {
            Self::Double | Self::Fixed64 | Self::Sfixed64 => WireType::Fixed64,
            Self::Float | Self::Fixed32 | Self::Sfixed32 => WireType::Fixed32,
            Self::Int64
            | Self::Uint64
            | Self::Int32
            | Self::Uint32
            | Self::Sint32
            | Self::Sint64
            | Self::Bool
            | Self::Enum => WireType::Varint,
            Self::String | Self::Bytes | Self::Message => WireType::LengthDelimited,
            Self::Group => WireType::StartGroup,
        }
    }

    /// Name as it appears in .proto files.
    pub fn proto_name(&self) -> &'static str {
        match self {
            Self::Double => "double",
            Self::Float => "float",
            Self::Int64 => "int64",
            Self::Uint64 => "uint64",
            Self::Int32 => "int32",
            Self::Fixed64 => "fixed64",
            Self::Fixed32 => "fixed32",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Group => "group",
            Self::Message => "message",
            Self::Bytes => "bytes",
            Self::Uint32 => "uint32",
            Self::Enum => "enum",
            Self::Sfixed32 => "sfixed32",
            Self::Sfixed64 => "sfixed64",
            Self::Sint32 => "sint32",
            Self::Sint64 => "sint64",
        }
    }

    /// Parse a scalar type name from .proto source. Returns None for message/enum/group.
    pub fn from_proto_name(name: &str) -> Option<Self> {
        match name {
            "double" => Some(Self::Double),
            "float" => Some(Self::Float),
            "int64" => Some(Self::Int64),
            "uint64" => Some(Self::Uint64),
            "int32" => Some(Self::Int32),
            "fixed64" => Some(Self::Fixed64),
            "fixed32" => Some(Self::Fixed32),
            "bool" => Some(Self::Bool),
            "string" => Some(Self::String),
            "bytes" => Some(Self::Bytes),
            "uint32" => Some(Self::Uint32),
            "sfixed32" => Some(Self::Sfixed32),
            "sfixed64" => Some(Self::Sfixed64),
            "sint32" => Some(Self::Sint32),
            "sint64" => Some(Self::Sint64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FieldLabel {
    Optional = 1,
    Required = 2,
    Repeated = 3,
}

impl FieldLabel {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Optional),
            2 => Some(Self::Required),
            3 => Some(Self::Repeated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum WireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    StartGroup = 3,
    EndGroup = 4,
    Fixed32 = 5,
}

impl WireType {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Varint),
            1 => Some(Self::Fixed64),
            2 => Some(Self::LengthDelimited),
            3 => Some(Self::StartGroup),
            4 => Some(Self::EndGroup),
            5 => Some(Self::Fixed32),
            _ => None,
        }
    }

    /// Size in bytes for fixed-size wire types. None for variable-size.
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            Self::Fixed64 => Some(8),
            Self::Fixed32 => Some(4),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Enum descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumDescriptorProto {
    pub name: Option<String>,
    pub value: Vec<EnumValueDescriptorProto>,
    pub options: Option<EnumOptions>,
    pub reserved_range: Vec<EnumReservedRange>,
    pub reserved_name: Vec<String>,
    /// Visibility modifier (editions 2024+).
    pub visibility: Option<Visibility>,
    /// Source location of the `enum` keyword. Populated by parser, not serialized.
    #[doc(hidden)]
    pub source_span: Option<Span>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumValueDescriptorProto {
    pub name: Option<String>,
    pub number: Option<i32>,
    pub options: Option<EnumValueOptions>,
    /// Source location of the enum value. Populated by parser, not serialized.
    #[doc(hidden)]
    pub source_span: Option<Span>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumReservedRange {
    pub start: Option<i32>,
    pub end: Option<i32>,
}

// ---------------------------------------------------------------------------
// Oneof descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OneofDescriptorProto {
    pub name: Option<String>,
    pub options: Option<OneofOptions>,
}

// ---------------------------------------------------------------------------
// Service descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceDescriptorProto {
    pub name: Option<String>,
    pub method: Vec<MethodDescriptorProto>,
    pub options: Option<ServiceOptions>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodDescriptorProto {
    pub name: Option<String>,
    pub input_type: Option<String>,
    pub output_type: Option<String>,
    pub options: Option<MethodOptions>,
    pub client_streaming: Option<bool>,
    pub server_streaming: Option<bool>,
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileOptions {
    pub java_package: Option<String>,
    pub java_outer_classname: Option<String>,
    pub java_multiple_files: Option<bool>,
    pub java_generate_equals_and_hash: Option<bool>,
    pub java_string_check_utf8: Option<bool>,
    pub optimize_for: Option<OptimizeMode>,
    pub go_package: Option<String>,
    pub cc_generic_services: Option<bool>,
    pub java_generic_services: Option<bool>,
    pub py_generic_services: Option<bool>,
    pub deprecated: Option<bool>,
    pub cc_enable_arenas: Option<bool>,
    pub objc_class_prefix: Option<String>,
    pub csharp_namespace: Option<String>,
    pub swift_prefix: Option<String>,
    pub php_class_prefix: Option<String>,
    pub php_namespace: Option<String>,
    pub php_metadata_namespace: Option<String>,
    pub ruby_package: Option<String>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum OptimizeMode {
    Speed = 1,
    CodeSize = 2,
    LiteRuntime = 3,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageOptions {
    pub message_set_wire_format: Option<bool>,
    pub no_standard_descriptor_accessor: Option<bool>,
    pub deprecated: Option<bool>,
    pub map_entry: Option<bool>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldOptions {
    pub ctype: Option<FieldCType>,
    pub packed: Option<bool>,
    pub jstype: Option<FieldJsType>,
    pub lazy: Option<bool>,
    pub deprecated: Option<bool>,
    pub weak: Option<bool>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FieldCType {
    String = 0,
    Cord = 1,
    StringPiece = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FieldJsType {
    JsNormal = 0,
    JsString = 1,
    JsNumber = 2,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OneofOptions {
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumOptions {
    pub allow_alias: Option<bool>,
    pub deprecated: Option<bool>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumValueOptions {
    pub deprecated: Option<bool>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceOptions {
    pub deprecated: Option<bool>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodOptions {
    pub deprecated: Option<bool>,
    pub idempotency_level: Option<IdempotencyLevel>,
    pub uninterpreted_option: Vec<UninterpretedOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum IdempotencyLevel {
    IdempotencyUnknown = 0,
    NoSideEffects = 1,
    Idempotent = 2,
}

// ---------------------------------------------------------------------------
// Uninterpreted option (catch-all for custom options)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninterpretedOption {
    pub name: Vec<NamePart>,
    pub identifier_value: Option<String>,
    pub positive_int_value: Option<u64>,
    pub negative_int_value: Option<i64>,
    pub double_value: Option<String>, // stored as string to preserve precision
    pub string_value: Option<Vec<u8>>,
    pub aggregate_value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamePart {
    pub name_part: String,
    pub is_extension: bool,
}
