use std::ops::Range;

/// Source location information for error reporting.
/// Maps schema elements back to their position in the `.proto` source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceCodeInfo {
    pub location: Vec<SourceLocation>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLocation {
    /// Path to the element, encoded as a sequence of field numbers / indices.
    /// See descriptor.proto for the encoding scheme.
    pub path: Vec<i32>,
    pub span: Vec<i32>,
    pub leading_comments: Option<String>,
    pub trailing_comments: Option<String>,
    pub leading_detached_comments: Vec<String>,
}

/// A span in the source text, used for error reporting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn point(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    pub fn byte_range(&self) -> Range<usize> {
        self.start.byte_offset..self.end.byte_offset
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start.line == self.end.line {
            write!(
                f,
                "{}:{}-{}",
                self.start.line, self.start.column, self.end.column
            )
        } else {
            write!(
                f,
                "{}:{}-{}:{}",
                self.start.line, self.start.column, self.end.line, self.end.column
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
    /// 0-based byte offset into the source text.
    pub byte_offset: usize,
}

impl Position {
    pub fn new(line: usize, column: usize, byte_offset: usize) -> Self {
        Self {
            line,
            column,
            byte_offset,
        }
    }
}
