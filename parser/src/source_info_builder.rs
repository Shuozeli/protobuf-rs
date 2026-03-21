use protoc_rs_schema::{SourceCodeInfo, SourceLocation, Span};

/// Field numbers from descriptor.proto for path encoding.
/// Only constants actually used by the parser are defined here.
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
    pub const FILE_EDITION: i32 = 14;

    // DescriptorProto
    pub const MSG_NAME: i32 = 1;
    pub const MSG_FIELD: i32 = 2;
    pub const MSG_NESTED_TYPE: i32 = 3;
    pub const MSG_ENUM_TYPE: i32 = 4;
    pub const MSG_EXTENSION_RANGE: i32 = 5;
    pub const MSG_EXTENSION: i32 = 6;
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

    // EnumDescriptorProto
    pub const ENUM_NAME: i32 = 1;
    pub const ENUM_VALUE: i32 = 2;
    pub const ENUM_RESERVED_RANGE: i32 = 4;
    pub const ENUM_RESERVED_NAME: i32 = 5;

    // EnumValueDescriptorProto
    pub const ENUM_VALUE_NAME: i32 = 1;
    pub const ENUM_VALUE_NUMBER: i32 = 2;

    // OneofDescriptorProto
    pub const ONEOF_NAME: i32 = 1;

    // ServiceDescriptorProto
    pub const SERVICE_NAME: i32 = 1;
    pub const SERVICE_METHOD: i32 = 2;

    // MethodDescriptorProto
    pub const METHOD_NAME: i32 = 1;
    pub const METHOD_INPUT_TYPE: i32 = 2;
    pub const METHOD_OUTPUT_TYPE: i32 = 3;
    pub const METHOD_CLIENT_STREAMING: i32 = 5;
    pub const METHOD_SERVER_STREAMING: i32 = 6;
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
