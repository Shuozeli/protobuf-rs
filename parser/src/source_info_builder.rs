use protoc_rs_schema::{SourceCodeInfo, SourceLocation, Span};

/// Field numbers from descriptor.proto for path encoding.
pub mod field_num {
    // FileDescriptorProto
    pub const FILE_SYNTAX: i32 = 12;
    pub const FILE_PACKAGE: i32 = 2;
    pub const FILE_DEPENDENCY: i32 = 3;
    pub const FILE_PUBLIC_DEPENDENCY: i32 = 10;
    pub const FILE_WEAK_DEPENDENCY: i32 = 11;
    pub const FILE_MESSAGE_TYPE: i32 = 4;
    pub const FILE_ENUM_TYPE: i32 = 5;
    pub const FILE_SERVICE: i32 = 6;
    pub const FILE_EXTENSION: i32 = 7;
    pub const FILE_OPTIONS: i32 = 8;
    pub const FILE_SOURCE_CODE_INFO: i32 = 9;
    pub const FILE_EDITION: i32 = 14;

    // DescriptorProto
    pub const MSG_NAME: i32 = 1;
    pub const MSG_FIELD: i32 = 2;
    pub const MSG_NESTED_TYPE: i32 = 3;
    pub const MSG_ENUM_TYPE: i32 = 4;
    pub const MSG_EXTENSION_RANGE: i32 = 5;
    pub const MSG_EXTENSION: i32 = 6;
    pub const MSG_OPTIONS: i32 = 7;
    pub const MSG_ONEOF_DECL: i32 = 8;
    pub const MSG_RESERVED_RANGE: i32 = 9;
    pub const MSG_RESERVED_NAME: i32 = 10;

    // FieldDescriptorProto
    pub const FIELD_NAME: i32 = 1;
    pub const FIELD_EXTENDEE: i32 = 2;
    pub const FIELD_NUMBER: i32 = 3;
    pub const FIELD_LABEL: i32 = 4;
    pub const FIELD_TYPE: i32 = 5;
    pub const FIELD_TYPE_NAME: i32 = 6;
    pub const FIELD_DEFAULT_VALUE: i32 = 7;
    pub const FIELD_OPTIONS: i32 = 8;
    pub const FIELD_ONEOF_INDEX: i32 = 9;
    pub const FIELD_JSON_NAME: i32 = 10;

    // EnumDescriptorProto
    pub const ENUM_NAME: i32 = 1;
    pub const ENUM_VALUE: i32 = 2;
    pub const ENUM_OPTIONS: i32 = 3;
    pub const ENUM_RESERVED_RANGE: i32 = 4;
    pub const ENUM_RESERVED_NAME: i32 = 5;

    // EnumValueDescriptorProto
    pub const ENUM_VALUE_NAME: i32 = 1;
    pub const ENUM_VALUE_NUMBER: i32 = 2;
    pub const ENUM_VALUE_OPTIONS: i32 = 3;

    // OneofDescriptorProto
    pub const ONEOF_NAME: i32 = 1;
    pub const ONEOF_OPTIONS: i32 = 2;

    // ServiceDescriptorProto
    pub const SERVICE_NAME: i32 = 1;
    pub const SERVICE_METHOD: i32 = 2;
    pub const SERVICE_OPTIONS: i32 = 3;

    // MethodDescriptorProto
    pub const METHOD_NAME: i32 = 1;
    pub const METHOD_INPUT_TYPE: i32 = 2;
    pub const METHOD_OUTPUT_TYPE: i32 = 3;
    pub const METHOD_OPTIONS: i32 = 4;
    pub const METHOD_CLIENT_STREAMING: i32 = 5;
    pub const METHOD_SERVER_STREAMING: i32 = 6;

    // ExtensionRange
    pub const EXT_RANGE_START: i32 = 1;
    pub const EXT_RANGE_END: i32 = 2;
    pub const EXT_RANGE_OPTIONS: i32 = 3;

    // ReservedRange
    pub const RESERVED_RANGE_START: i32 = 1;
    pub const RESERVED_RANGE_END: i32 = 2;

    // EnumReservedRange
    pub const ENUM_RESERVED_RANGE_START: i32 = 1;
    pub const ENUM_RESERVED_RANGE_END: i32 = 2;

    // UninterpretedOption
    pub const UOPT_NAME: i32 = 2;
    pub const UOPT_IDENTIFIER_VALUE: i32 = 3;
    pub const UOPT_POSITIVE_INT_VALUE: i32 = 4;
    pub const UOPT_NEGATIVE_INT_VALUE: i32 = 5;
    pub const UOPT_DOUBLE_VALUE: i32 = 6;
    pub const UOPT_STRING_VALUE: i32 = 7;
    pub const UOPT_AGGREGATE_VALUE: i32 = 8;

    // Options containers -- uninterpreted_option field number
    pub const FILE_OPT_UNINTERPRETED: i32 = 999;
    pub const MSG_OPT_UNINTERPRETED: i32 = 999;
    pub const FIELD_OPT_UNINTERPRETED: i32 = 999;
    pub const ENUM_OPT_UNINTERPRETED: i32 = 999;
    pub const ENUM_VALUE_OPT_UNINTERPRETED: i32 = 999;
    pub const SERVICE_OPT_UNINTERPRETED: i32 = 999;
    pub const METHOD_OPT_UNINTERPRETED: i32 = 999;
}

/// Builds SourceCodeInfo by collecting locations during parsing.
pub struct SourceInfoBuilder {
    locations: Vec<SourceLocation>,
    /// Current path stack. Elements are field_num or index pairs.
    path: Vec<i32>,
}

impl Default for SourceInfoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceInfoBuilder {
    pub fn new() -> Self {
        Self {
            locations: Vec::new(),
            path: Vec::new(),
        }
    }

    /// Push a field number onto the current path.
    pub fn push(&mut self, field_num: i32) {
        self.path.push(field_num);
    }

    /// Push a field number and index (for repeated fields).
    pub fn push_index(&mut self, field_num: i32, index: i32) {
        self.path.push(field_num);
        self.path.push(index);
    }

    /// Pop the last element from the path.
    pub fn pop(&mut self) {
        self.path.pop();
    }

    /// Pop the last two elements (field_num + index) from the path.
    pub fn pop_index(&mut self) {
        self.path.pop();
        self.path.pop();
    }

    /// Record a location at the current path with the given span.
    /// Span is encoded as [start_line, start_col, end_line, end_col] (0-based).
    /// If start_line == end_line, span is [line, start_col, end_col].
    pub fn add_location(&mut self, span: Span) {
        self.add_location_with_comments(span, None, None, Vec::new());
    }

    /// Record a location with comments.
    pub fn add_location_with_comments(
        &mut self,
        span: Span,
        leading: Option<String>,
        trailing: Option<String>,
        detached: Vec<String>,
    ) {
        let encoded_span = encode_span(span);
        self.locations.push(SourceLocation {
            path: self.path.clone(),
            span: encoded_span,
            leading_comments: leading,
            trailing_comments: trailing,
            leading_detached_comments: detached,
        });
    }

    /// Consume and build the final SourceCodeInfo.
    pub fn build(self) -> SourceCodeInfo {
        SourceCodeInfo {
            location: self.locations,
        }
    }
}

/// Encode a Span into the protobuf format: [start_line, start_col, end_line, end_col]
/// or [start_line, start_col, end_col] if on the same line.
/// Lines and columns are 0-based in the protobuf format.
fn encode_span(span: Span) -> Vec<i32> {
    let start_line = (span.start.line - 1) as i32; // convert 1-based to 0-based
    let start_col = (span.start.column - 1) as i32;
    let end_line = (span.end.line - 1) as i32;
    let end_col = (span.end.column - 1) as i32;

    if start_line == end_line {
        vec![start_line, start_col, end_col]
    } else {
        vec![start_line, start_col, end_line, end_col]
    }
}
