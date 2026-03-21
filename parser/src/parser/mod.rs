mod options;
mod parse_helpers;

pub use parse_helpers::to_camel_case;
use protoc_rs_schema::*;

use crate::lexer::{LexError, Lexer, Token, TokenKind};
use crate::source_info_builder::{field_num as fnum, SourceInfoBuilder};

use options::*;
use parse_helpers::*;
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.message, self.span)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            message: e.message,
            span: e.span,
        }
    }
}
/// Maximum nesting depth for messages (matches C++ protoc).
const MAX_MESSAGE_NESTING_DEPTH: usize = 32;

/// Parse result with collected errors and warnings.
pub struct ParseResult {
    pub file: FileDescriptorProto,
    pub errors: Vec<ParseError>,
    pub warnings: Vec<ParseError>,
}

struct Parser {
    tokens: Vec<Token>,
    comments: Vec<Token>,
    pos: usize,
    is_proto3: bool,
    is_editions: bool,
    edition: Option<String>,
    nesting_depth: usize,
    errors: Vec<ParseError>,
    warnings: Vec<ParseError>,
    source_info: SourceInfoBuilder,
    /// Index of the next unconsumed comment in `comments`.
    comment_cursor: usize,
}

/// Parse a `.proto` file source into a `FileDescriptorProto`.
/// Returns the first error encountered; for all errors use `parse_collecting`.
pub fn parse(source: &str) -> Result<FileDescriptorProto, ParseError> {
    let result = parse_collecting(source)?;
    if let Some(err) = result.errors.into_iter().next() {
        return Err(err);
    }
    Ok(result.file)
}

/// Parse a `.proto` file, collecting all errors and warnings instead of
/// stopping at the first one. The parser uses error recovery (skip to next
/// statement) inside message bodies to find multiple issues in a single pass.
pub fn parse_collecting(source: &str) -> Result<ParseResult, ParseError> {
    let mut lexer = Lexer::new(source);
    let (tokens, comments) = lexer.tokenize()?;
    let mut parser = Parser {
        tokens,
        comments,
        pos: 0,
        is_proto3: false,
        is_editions: false,
        edition: None,
        nesting_depth: 0,
        errors: Vec::new(),
        warnings: Vec::new(),
        source_info: SourceInfoBuilder::new(),
        comment_cursor: 0,
    };
    let file = parser.parse_file()?;
    Ok(ParseResult {
        file,
        errors: parser.errors,
        warnings: parser.warnings,
    })
}

impl Parser {
    // -- Token navigation --

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    fn at_eof(&self) -> bool {
        self.at(&TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    /// Get the span of the previously consumed token.
    fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            Span::default()
        }
    }

    /// Collect leading and detached comments before the declaration at `decl_start_line`
    /// (0-based). Advances `comment_cursor` past consumed comments.
    fn collect_leading_comments(
        &mut self,
        decl_start_line: usize,
    ) -> (Option<String>, Vec<String>) {
        let mut leading = None;
        let mut detached = Vec::new();
        let mut pending: Vec<String> = Vec::new();
        let mut prev_comment_end_line: Option<usize> = None;

        while self.comment_cursor < self.comments.len() {
            let c = &self.comments[self.comment_cursor];
            let c_start_line = c.span.start.line - 1; // convert to 0-based
            let c_end_line = c.span.end.line - 1;

            // Comment must start before the declaration start line
            if c_start_line >= decl_start_line {
                break;
            }

            // Check for blank line gap between previous comment and this one
            if let Some(prev_end) = prev_comment_end_line {
                if c_start_line > prev_end + 1 {
                    detached.append(&mut pending);
                }
            }

            let text = format_comment(&c.text, c.kind == TokenKind::BlockComment);
            pending.push(text);
            prev_comment_end_line = Some(c_end_line);
            self.comment_cursor += 1;
        }

        if !pending.is_empty() {
            if let Some(prev_end) = prev_comment_end_line {
                if decl_start_line > prev_end + 1 {
                    detached.append(&mut pending);
                } else {
                    leading = Some(pending.join(""));
                }
            }
        }

        (leading, detached)
    }

    /// Collect a trailing comment on the same line as `decl_end_line` (0-based).
    fn collect_trailing_comment(&mut self, decl_end_line: usize) -> Option<String> {
        if self.comment_cursor < self.comments.len() {
            let c = &self.comments[self.comment_cursor];
            let c_start_line = c.span.start.line - 1;
            if c_start_line == decl_end_line && c.kind == TokenKind::LineComment {
                self.comment_cursor += 1;
                return Some(format_comment(&c.text, false));
            }
        }
        None
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Token, ParseError> {
        if self.at(kind) {
            Ok(self.advance().clone())
        } else {
            Err(ParseError {
                message: format!("expected {:?}, found {:?}", kind, self.peek().kind),
                span: self.peek().span,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        // Allow keywords to be used as identifiers in field/message names
        let tok = self.peek();
        if tok.kind == TokenKind::Ident || TokenKind::from_keyword(&tok.text).is_some() {
            let text = tok.text.clone();
            self.advance();
            Ok(text)
        } else {
            Err(ParseError {
                message: format!("expected identifier, found {:?}", tok.kind),
                span: tok.span,
            })
        }
    }

    fn expect_semi(&mut self) -> Result<(), ParseError> {
        self.expect(&TokenKind::Semicolon)?;
        Ok(())
    }

    /// Record a non-fatal error (used during error recovery).
    fn record_error(&mut self, err: ParseError) {
        self.errors.push(err);
    }

    /// Record a warning.
    fn record_warning(&mut self, warn: ParseError) {
        self.warnings.push(warn);
    }

    /// Check if the current edition is >= the given edition string.
    fn edition_ge(&self, min: &str) -> bool {
        match &self.edition {
            Some(e) => e.as_str() >= min,
            None => false,
        }
    }

    /// Check if the current edition is UNSTABLE.
    fn is_unstable(&self) -> bool {
        self.edition.as_deref() == Some("UNSTABLE")
    }

    /// Try to consume a visibility modifier (`export` or `local`).
    /// Returns the visibility if the current token is a visibility keyword in
    /// editions 2024+ context. In editions < 2024, these are just normal identifiers.
    fn try_parse_visibility(&mut self) -> Result<Option<Visibility>, ParseError> {
        if !self.is_editions || !self.edition_ge("2024") {
            return Ok(None);
        }
        let tok = self.peek();
        if tok.kind != TokenKind::Ident {
            return Ok(None);
        }
        let vis = match tok.text.as_str() {
            "export" => Visibility::Export,
            "local" => Visibility::Local,
            _ => return Ok(None),
        };
        let vis_span = tok.span;
        self.advance();
        // Visibility is only valid before `message` and `enum`
        if !self.at(&TokenKind::Message) && !self.at(&TokenKind::Enum) {
            return Err(ParseError {
                message: "'local' and 'export' visibility modifiers are valid only on 'message' and 'enum'".to_string(),
                span: vis_span,
            });
        }
        Ok(Some(vis))
    }

    /// Check if current token is `export`/`local` followed by a declaration keyword
    /// (message, enum, service, extend) in edition 2024+. Does NOT consume.
    fn at_visibility_keyword(&self) -> bool {
        if !self.is_editions || !self.edition_ge("2024") {
            return false;
        }
        let tok = self.peek();
        if tok.kind != TokenKind::Ident || (tok.text != "export" && tok.text != "local") {
            return false;
        }
        let next_pos = self.pos + 1;
        if next_pos < self.tokens.len() {
            let next = &self.tokens[next_pos];
            return matches!(
                next.kind,
                TokenKind::Message | TokenKind::Enum | TokenKind::Service | TokenKind::Extend
            );
        }
        false
    }

    /// Skip tokens until we reach a `;` or a matching `}` at the current brace
    /// depth. Used for error recovery so we can continue parsing after errors.
    fn skip_statement(&mut self) {
        let mut brace_depth: u32 = 0;
        loop {
            if self.at_eof() {
                return;
            }
            match self.peek().kind {
                TokenKind::LBrace => {
                    brace_depth += 1;
                    self.advance();
                }
                TokenKind::RBrace => {
                    if brace_depth == 0 {
                        // Don't consume the '}' -- it belongs to the enclosing scope
                        return;
                    }
                    brace_depth -= 1;
                    self.advance();
                    if brace_depth == 0 {
                        return;
                    }
                }
                TokenKind::Semicolon => {
                    if brace_depth == 0 {
                        self.advance(); // consume the ';'
                        return;
                    }
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    // -- Top-level --

    fn parse_file(&mut self) -> Result<FileDescriptorProto, ParseError> {
        let mut file = FileDescriptorProto::default();

        // Optional syntax or edition statement (must be first non-comment statement)
        if self.at(&TokenKind::Syntax) {
            let start = self.peek().span;
            let syntax = self.parse_syntax()?;
            self.is_proto3 = syntax == "proto3";
            file.syntax = Some(syntax);
            let end = self.prev_span();
            self.source_info.push(fnum::FILE_SYNTAX);
            self.source_info
                .add_location(Span::new(start.start, end.end));
            self.source_info.pop();
        } else if self.at(&TokenKind::Edition) {
            let start = self.peek().span;
            let edition = self.parse_edition()?;
            self.is_editions = true;
            self.edition = Some(edition.clone());
            file.syntax = Some("editions".to_string());
            file.edition = Some(edition);
            let end = self.prev_span();
            self.source_info.push(fnum::FILE_EDITION);
            self.source_info
                .add_location(Span::new(start.start, end.end));
            self.source_info.pop();
        }

        let mut has_option_import = false;
        let mut pub_dep_count: i32 = 0;
        let mut weak_dep_count: i32 = 0;
        while !self.at_eof() {
            // Handle visibility modifiers (export/local) in editions 2024+
            if self.at_visibility_keyword() {
                let vis = self.try_parse_visibility()?;
                if self.at(&TokenKind::Message) {
                    let idx = file.message_type.len() as i32;
                    let start = self.peek().span;
                    self.source_info.push_index(fnum::FILE_MESSAGE_TYPE, idx);
                    let mut msg = self.parse_message()?;
                    msg.visibility = vis;
                    let end = self.prev_span();
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                    file.message_type.push(msg);
                } else if self.at(&TokenKind::Enum) {
                    let idx = file.enum_type.len() as i32;
                    let start = self.peek().span;
                    self.source_info.push_index(fnum::FILE_ENUM_TYPE, idx);
                    let mut e = self.parse_enum()?;
                    e.visibility = vis;
                    let end = self.prev_span();
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                    file.enum_type.push(e);
                }
                // try_parse_visibility already errors for service/extend
                continue;
            }
            match &self.peek().kind {
                TokenKind::Import => {
                    let start = self.peek().span;
                    let import_span = self.peek().span;
                    let (dep, is_public, is_weak, is_option) = self.parse_import()?;
                    let end = self.prev_span();
                    if is_option {
                        has_option_import = true;
                        file.option_dependency.push(dep);
                    } else {
                        if has_option_import {
                            return Err(ParseError {
                                message: "imports should precede any option imports to ensure proto files can roundtrip.".to_string(),
                                span: import_span,
                            });
                        }
                        let idx = file.dependency.len() as i32;
                        // Record dependency location
                        self.source_info.push_index(fnum::FILE_DEPENDENCY, idx);
                        self.source_info
                            .add_location(Span::new(start.start, end.end));
                        self.source_info.pop_index();
                        if is_public {
                            self.source_info
                                .push_index(fnum::FILE_PUBLIC_DEPENDENCY, pub_dep_count);
                            self.source_info
                                .add_location(Span::new(start.start, end.end));
                            self.source_info.pop_index();
                            pub_dep_count += 1;
                            file.public_dependency.push(idx);
                        }
                        if is_weak {
                            self.source_info
                                .push_index(fnum::FILE_WEAK_DEPENDENCY, weak_dep_count);
                            self.source_info
                                .add_location(Span::new(start.start, end.end));
                            self.source_info.pop_index();
                            weak_dep_count += 1;
                            file.weak_dependency.push(idx);
                        }
                        file.dependency.push(dep);
                    }
                }
                TokenKind::Package => {
                    if file.package.is_some() {
                        return Err(ParseError {
                            message: "multiple package declarations".to_string(),
                            span: self.peek().span,
                        });
                    }
                    let start = self.peek().span;
                    file.package = Some(self.parse_package()?);
                    let end = self.prev_span();
                    self.source_info.push(fnum::FILE_PACKAGE);
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop();
                }
                TokenKind::Option => {
                    let start = self.peek().span;
                    match self.parse_option_statement() {
                        Ok(opt) => {
                            let end = self.prev_span();
                            // Record file options span
                            self.source_info.push(fnum::FILE_OPTIONS);
                            self.source_info
                                .add_location(Span::new(start.start, end.end));
                            self.source_info.pop();
                            let options = file.options.get_or_insert_with(FileOptions::default);
                            apply_file_option(options, opt)?;
                        }
                        Err(err) => {
                            self.record_error(err);
                            self.skip_statement();
                        }
                    }
                }
                TokenKind::Message => {
                    let idx = file.message_type.len() as i32;
                    let start = self.peek().span;
                    let start_line = start.start.line - 1;
                    let (leading, detached) = self.collect_leading_comments(start_line);
                    self.source_info.push_index(fnum::FILE_MESSAGE_TYPE, idx);
                    file.message_type.push(self.parse_message()?);
                    let end = self.prev_span();
                    let trailing = self.collect_trailing_comment(end.end.line - 1);
                    self.source_info.add_location_with_comments(
                        Span::new(start.start, end.end),
                        leading,
                        trailing,
                        detached,
                    );
                    self.source_info.pop_index();
                }
                TokenKind::Enum => {
                    let idx = file.enum_type.len() as i32;
                    let start = self.peek().span;
                    let start_line = start.start.line - 1;
                    let (leading, detached) = self.collect_leading_comments(start_line);
                    self.source_info.push_index(fnum::FILE_ENUM_TYPE, idx);
                    file.enum_type.push(self.parse_enum()?);
                    let end = self.prev_span();
                    let trailing = self.collect_trailing_comment(end.end.line - 1);
                    self.source_info.add_location_with_comments(
                        Span::new(start.start, end.end),
                        leading,
                        trailing,
                        detached,
                    );
                    self.source_info.pop_index();
                }
                TokenKind::Service => {
                    let idx = file.service.len() as i32;
                    let start = self.peek().span;
                    let start_line = start.start.line - 1;
                    let (leading, detached) = self.collect_leading_comments(start_line);
                    self.source_info.push_index(fnum::FILE_SERVICE, idx);
                    file.service.push(self.parse_service()?);
                    let end = self.prev_span();
                    let trailing = self.collect_trailing_comment(end.end.line - 1);
                    self.source_info.add_location_with_comments(
                        Span::new(start.start, end.end),
                        leading,
                        trailing,
                        detached,
                    );
                    self.source_info.pop_index();
                }
                TokenKind::Extend => {
                    let ext_start_idx = file.extension.len() as i32;
                    let extensions =
                        self.parse_extend_with_context(fnum::FILE_EXTENSION, ext_start_idx)?;
                    file.extension.extend(extensions);
                }
                TokenKind::Semicolon => {
                    self.advance(); // empty statement
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected top-level statement (e.g. \"message\").".to_string(),
                        span: self.peek().span,
                    });
                }
            }
        }

        // Build source_code_info from collected locations
        let source_info = std::mem::take(&mut self.source_info);
        file.source_code_info = Some(source_info.build());

        Ok(file)
    }

    // -- syntax --

    fn parse_syntax(&mut self) -> Result<String, ParseError> {
        self.expect(&TokenKind::Syntax)?;
        self.expect(&TokenKind::Equals)?;
        let tok = self.expect(&TokenKind::StringLiteral)?;
        if tok.text != "proto2" && tok.text != "proto3" {
            return Err(ParseError {
                message: format!(
                    "Unrecognized syntax identifier \"{}\". This parser only recognizes \"proto2\" and \"proto3\".",
                    tok.text
                ),
                span: tok.span,
            });
        }
        self.expect_semi()?;
        Ok(tok.text)
    }

    /// Known valid edition strings.
    fn is_valid_edition(edition: &str) -> bool {
        matches!(
            edition,
            "2023"
                | "2024"
                | "99997_TEST_ONLY"
                | "99998_TEST_ONLY"
                | "99999_TEST_ONLY"
                | "UNSTABLE"
        )
    }

    fn parse_edition(&mut self) -> Result<String, ParseError> {
        self.expect(&TokenKind::Edition)?;
        self.expect(&TokenKind::Equals)?;
        let tok = self.expect(&TokenKind::StringLiteral)?;
        if !Self::is_valid_edition(&tok.text) {
            return Err(ParseError {
                message: format!("Unknown edition \"{}\".", tok.text),
                span: tok.span,
            });
        }
        self.expect_semi()?;
        Ok(tok.text)
    }

    // -- import --

    /// Parse an import statement. Returns (path, is_public, is_weak, is_option).
    fn parse_import(&mut self) -> Result<(String, bool, bool, bool), ParseError> {
        self.expect(&TokenKind::Import)?;
        let mut is_public = false;
        let mut is_weak = false;
        let mut is_option = false;
        if self.at(&TokenKind::Public) {
            self.advance();
            is_public = true;
        } else if self.at(&TokenKind::Weak) {
            let weak_span = self.peek().span;
            self.advance();
            is_weak = true;
            // weak import is not supported in edition 2024+
            if self.is_editions && self.edition_ge("2024") {
                return Err(ParseError {
                    message: "weak import is not supported in edition 2024 and above. Consider using option import instead.".to_string(),
                    span: weak_span,
                });
            }
        } else if self.at(&TokenKind::Option) {
            let option_span = self.peek().span;
            self.advance();
            is_option = true;
            // option import is only supported in edition 2024+
            if !self.is_editions || !self.edition_ge("2024") {
                return Err(ParseError {
                    message: "option import is not supported before edition 2024.".to_string(),
                    span: option_span,
                });
            }
        }
        let path = self.expect(&TokenKind::StringLiteral)?;
        self.expect_semi()?;
        Ok((path.text, is_public, is_weak, is_option))
    }

    // -- package --

    fn parse_package(&mut self) -> Result<String, ParseError> {
        self.expect(&TokenKind::Package)?;
        let name = self.parse_full_ident()?;
        self.expect_semi()?;
        Ok(name)
    }

    // -- option --

    fn parse_option_statement(&mut self) -> Result<(String, OptionValue), ParseError> {
        let option_span = self.peek().span;
        self.expect(&TokenKind::Option)?;
        let name = self.parse_option_name()?;
        self.check_features_option(&name, option_span)?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_option_value()?;
        self.expect_semi()?;
        Ok((name, value))
    }

    /// Check that `features.*` options are only used in editions mode.
    fn check_features_option(&self, name: &str, span: Span) -> Result<(), ParseError> {
        if name.starts_with("features.") && !self.is_editions {
            return Err(ParseError {
                message: "Features are only valid under editions.".to_string(),
                span,
            });
        }
        Ok(())
    }

    fn parse_option_name(&mut self) -> Result<String, ParseError> {
        let mut name = String::new();
        if self.at(&TokenKind::LParen) {
            // Extension option: (com.example.foo) or (.com.example.foo)
            self.advance();
            name.push('(');
            if self.at(&TokenKind::Dot) {
                self.advance();
                name.push('.');
            }
            name.push_str(&self.parse_full_ident()?);
            self.expect(&TokenKind::RParen)?;
            name.push(')');
        } else {
            name = self.expect_ident()?;
        }
        // Handle dotted sub-options: option (foo).bar = ...
        // Also handles chained extensions: option (foo).(.bar.baz) = ...
        while self.at(&TokenKind::Dot) {
            self.advance();
            name.push('.');
            if self.at(&TokenKind::LParen) {
                // Extension in the middle of a path: .(ext.name)
                self.advance();
                name.push('(');
                if self.at(&TokenKind::Dot) {
                    self.advance();
                    name.push('.');
                }
                name.push_str(&self.parse_full_ident()?);
                self.expect(&TokenKind::RParen)?;
                name.push(')');
            } else {
                name.push_str(&self.expect_ident()?);
            }
        }
        Ok(name)
    }

    fn parse_option_value(&mut self) -> Result<OptionValue, ParseError> {
        match &self.peek().kind {
            TokenKind::StringLiteral => {
                let tok = self.advance().clone();
                Ok(OptionValue::String(tok.text))
            }
            TokenKind::IntLiteral => {
                let tok = self.advance().clone();
                Ok(OptionValue::Int(tok.text))
            }
            TokenKind::FloatLiteral => {
                let tok = self.advance().clone();
                Ok(OptionValue::Float(tok.text))
            }
            TokenKind::True => {
                self.advance();
                Ok(OptionValue::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(OptionValue::Bool(false))
            }
            TokenKind::Ident | TokenKind::Inf | TokenKind::Nan => {
                let tok = self.advance().clone();
                Ok(OptionValue::Ident(tok.text))
            }
            TokenKind::Minus => {
                self.advance();
                match &self.peek().kind {
                    TokenKind::IntLiteral => {
                        let tok = self.advance().clone();
                        Ok(OptionValue::Int(format!("-{}", tok.text)))
                    }
                    TokenKind::FloatLiteral => {
                        let tok = self.advance().clone();
                        Ok(OptionValue::Float(format!("-{}", tok.text)))
                    }
                    TokenKind::Inf => {
                        self.advance();
                        Ok(OptionValue::Float("-inf".to_string()))
                    }
                    TokenKind::Nan => {
                        self.advance();
                        Ok(OptionValue::Float("-nan".to_string()))
                    }
                    _ => Err(ParseError {
                        message: "expected number after '-'".to_string(),
                        span: self.peek().span,
                    }),
                }
            }
            TokenKind::LBrace => {
                // Aggregate option value: { key: value, ... }
                self.advance();
                let mut text = String::from("{");
                let mut depth = 1u32;
                while depth > 0 {
                    if self.at_eof() {
                        return Err(ParseError {
                            message: "unterminated aggregate option value".to_string(),
                            span: self.peek().span,
                        });
                    }
                    let tok = self.advance();
                    if tok.kind == TokenKind::LBrace {
                        depth += 1;
                    } else if tok.kind == TokenKind::RBrace {
                        depth -= 1;
                    }
                    text.push_str(&tok.text);
                    text.push(' ');
                }
                Ok(OptionValue::Aggregate(text.trim().to_string()))
            }
            _ => Err(ParseError {
                message: format!("expected option value, found {:?}", self.peek().kind),
                span: self.peek().span,
            }),
        }
    }

    /// Parse inline field options: [default = 5, deprecated = true]
    fn parse_field_options(&mut self) -> Result<Vec<(String, OptionValue)>, ParseError> {
        self.expect(&TokenKind::LBracket)?;
        let mut opts = Vec::new();
        loop {
            let opt_span = self.peek().span;
            let name = self.parse_option_name()?;
            self.check_features_option(&name, opt_span)?;
            self.expect(&TokenKind::Equals)?;
            let value = self.parse_option_value()?;
            opts.push((name, value));
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(opts)
    }

    // -- message --

    fn parse_message(&mut self) -> Result<DescriptorProto, ParseError> {
        let start_span = self.peek().span;
        self.expect(&TokenKind::Message)?;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record message name span
        self.source_info.push(fnum::MSG_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();
        self.expect(&TokenKind::LBrace)?;

        self.nesting_depth += 1;
        if self.nesting_depth > MAX_MESSAGE_NESTING_DEPTH {
            return Err(ParseError {
                message: "Reached maximum recursion limit for nested messages.".to_string(),
                span: start_span,
            });
        }

        let mut msg = DescriptorProto {
            name: Some(name),
            source_span: Some(start_span),
            ..Default::default()
        };

        self.parse_message_body(&mut msg)?;

        self.expect(&TokenKind::RBrace)?;
        self.nesting_depth -= 1;
        Ok(msg)
    }

    /// Parse the body of a message (between { and }). Shared by message and group.
    /// Uses error recovery: on parse error in simple statements, records the
    /// error and skips to the next statement so multiple errors can be reported.
    fn parse_message_body(&mut self, msg: &mut DescriptorProto) -> Result<(), ParseError> {
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.at(&TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            // Handle visibility modifiers (export/local) on nested messages/enums
            if self.at_visibility_keyword() {
                let vis = self.try_parse_visibility()?;
                if self.at(&TokenKind::Message) {
                    let idx = msg.nested_type.len() as i32;
                    let start = self.peek().span;
                    self.source_info.push_index(fnum::MSG_NESTED_TYPE, idx);
                    let mut nested = self.parse_message()?;
                    nested.visibility = vis;
                    let end = self.prev_span();
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                    msg.nested_type.push(nested);
                } else if self.at(&TokenKind::Enum) {
                    let idx = msg.enum_type.len() as i32;
                    let start = self.peek().span;
                    self.source_info.push_index(fnum::MSG_ENUM_TYPE, idx);
                    let mut e = self.parse_enum()?;
                    e.visibility = vis;
                    let end = self.prev_span();
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                    msg.enum_type.push(e);
                }
                continue;
            }
            // Structural constructs (message, enum, oneof, extend) have their
            // own brace-delimited bodies. Errors from these are NOT recoverable
            // because skip_statement would corrupt brace depth tracking.
            match self.peek().kind {
                TokenKind::Message | TokenKind::Enum | TokenKind::Oneof | TokenKind::Extend => {
                    self.parse_message_statement(msg)?;
                    continue;
                }
                _ => {}
            }
            // All other statements (fields, maps, reserved, extensions, options)
            // support error recovery.
            let result = self.parse_message_statement(msg);
            if let Err(err) = result {
                self.record_error(err);
                self.skip_statement();
            }
        }
        Ok(())
    }

    /// Parse a single statement inside a message body.
    /// Returns Ok(true) if the error is recoverable and should be handled
    /// by the caller with skip_statement, or propagates non-recoverable errors.
    fn parse_message_statement(&mut self, msg: &mut DescriptorProto) -> Result<(), ParseError> {
        match &self.peek().kind {
            // Structural constructs with braces -- errors are NOT recoverable
            // because brace depth tracking would be corrupted
            TokenKind::Message => {
                let idx = msg.nested_type.len() as i32;
                let start = self.peek().span;
                self.source_info.push_index(fnum::MSG_NESTED_TYPE, idx);
                let nested = self.parse_message()?;
                let end = self.prev_span();
                self.source_info
                    .add_location(Span::new(start.start, end.end));
                self.source_info.pop_index();
                msg.nested_type.push(nested);
            }
            TokenKind::Enum => {
                let idx = msg.enum_type.len() as i32;
                let start = self.peek().span;
                self.source_info.push_index(fnum::MSG_ENUM_TYPE, idx);
                let e = self.parse_enum()?;
                let end = self.prev_span();
                self.source_info
                    .add_location(Span::new(start.start, end.end));
                self.source_info.pop_index();
                msg.enum_type.push(e);
            }
            TokenKind::Oneof => {
                let oneof_idx = msg.oneof_decl.len() as i32;
                let field_offset = msg.field.len() as i32;
                let start = self.peek().span;
                self.source_info.push_index(fnum::MSG_ONEOF_DECL, oneof_idx);
                let (oneof, fields, nested_msgs) = self.parse_oneof(oneof_idx)?;
                let end = self.prev_span();
                self.source_info
                    .add_location(Span::new(start.start, end.end));
                self.source_info.pop_index();
                // Record field spans for oneof fields (at message level, not oneof level)
                for (i, f) in fields.iter().enumerate() {
                    if let Some(sp) = f.source_span {
                        self.source_info
                            .push_index(fnum::MSG_FIELD, field_offset + i as i32);
                        self.source_info.add_location(sp);
                        self.source_info.pop_index();
                    }
                }
                msg.oneof_decl.push(oneof);
                msg.field.extend(fields);
                msg.nested_type.extend(nested_msgs);
            }
            TokenKind::Extend => {
                let ext_start_idx = msg.extension.len() as i32;
                let exts = self.parse_extend_with_context(fnum::MSG_EXTENSION, ext_start_idx)?;
                msg.extension.extend(exts);
            }
            // Simple statement-level constructs -- errors ARE recoverable
            TokenKind::Map => {
                let field_idx = msg.field.len() as i32;
                let start = self.peek().span;
                self.source_info.push_index(fnum::MSG_FIELD, field_idx);
                let (field, nested_msg) = self.parse_map_field()?;
                let end = self.prev_span();
                self.source_info
                    .add_location(Span::new(start.start, end.end));
                self.source_info.pop_index();
                msg.field.push(field);
                msg.nested_type.push(nested_msg);
            }
            TokenKind::Reserved => {
                let range_start_idx = msg.reserved_range.len();
                let name_start_idx = msg.reserved_name.len();
                let start = self.peek().span;
                let (ranges, names) = self.parse_reserved()?;
                let end = self.prev_span();
                for (i, _) in ranges.iter().enumerate() {
                    self.source_info
                        .push_index(fnum::MSG_RESERVED_RANGE, (range_start_idx + i) as i32);
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                }
                for (i, _) in names.iter().enumerate() {
                    self.source_info
                        .push_index(fnum::MSG_RESERVED_NAME, (name_start_idx + i) as i32);
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                }
                msg.reserved_range.extend(ranges);
                msg.reserved_name.extend(names);
            }
            TokenKind::Extensions => {
                let ext_start_idx = msg.extension_range.len();
                let start = self.peek().span;
                let ranges = self.parse_extensions()?;
                let end = self.prev_span();
                for (i, _) in ranges.iter().enumerate() {
                    self.source_info
                        .push_index(fnum::MSG_EXTENSION_RANGE, (ext_start_idx + i) as i32);
                    self.source_info
                        .add_location(Span::new(start.start, end.end));
                    self.source_info.pop_index();
                }
                msg.extension_range.extend(ranges);
            }
            TokenKind::Option => {
                let opt = self.parse_option_statement()?;
                let options = msg.options.get_or_insert_with(MessageOptions::default);
                apply_message_option(options, opt)?;
            }
            _ => {
                let field_idx = msg.field.len() as i32;
                let start = self.peek().span;
                let start_line = start.start.line - 1;
                let (leading, detached) = self.collect_leading_comments(start_line);
                self.source_info.push_index(fnum::MSG_FIELD, field_idx);
                let (mut field, nested) = self.parse_field_or_group()?;
                let end = self.prev_span();
                let trailing = self.collect_trailing_comment(end.end.line - 1);
                self.source_info.add_location_with_comments(
                    Span::new(start.start, end.end),
                    leading,
                    trailing,
                    detached,
                );
                self.source_info.pop_index();
                if field.proto3_optional == Some(true) {
                    // Create synthetic oneof for proto3 optional field
                    let field_name = field.name.as_deref().unwrap_or("");
                    let mut oneof_name = format!("_{}", field_name);
                    // Handle collisions with existing oneofs
                    let existing_names: Vec<_> = msg
                        .oneof_decl
                        .iter()
                        .map(|o| o.name.as_deref().unwrap_or(""))
                        .collect();
                    while existing_names.contains(&oneof_name.as_str()) {
                        oneof_name = format!("X{}", oneof_name);
                    }
                    let oneof_idx = msg.oneof_decl.len() as i32;
                    msg.oneof_decl.push(OneofDescriptorProto {
                        name: Some(oneof_name),
                        options: None,
                    });
                    field.oneof_index = Some(oneof_idx);
                }
                msg.field.push(field);
                if let Some(n) = nested {
                    msg.nested_type.push(n);
                }
            }
        }
        Ok(())
    }

    // -- field --

    /// Parse a field, which may be a regular field or a group.
    /// Returns the field descriptor and optionally a nested message (for groups).
    fn parse_field_or_group(
        &mut self,
    ) -> Result<(FieldDescriptorProto, Option<DescriptorProto>), ParseError> {
        let mut field = FieldDescriptorProto {
            source_span: Some(self.peek().span),
            ..Default::default()
        };

        // Label: optional, required, repeated
        // In editions mode, `optional` and `required` are banned.
        if self.is_editions && self.at(&TokenKind::Optional) {
            return Err(ParseError {
                message: "Label \"optional\" is not supported in editions. By default, all singular fields have presence unless features.field_presence is set.".to_string(),
                span: self.peek().span,
            });
        }
        if self.is_editions && self.at(&TokenKind::Required) {
            return Err(ParseError {
                message: "Label \"required\" is not supported in editions, use features.field_presence = LEGACY_REQUIRED.".to_string(),
                span: self.peek().span,
            });
        }
        let explicit_optional = self.at(&TokenKind::Optional);
        let label_tok = self.peek().clone();
        let label = match &self.peek().kind {
            TokenKind::Optional => {
                self.advance();
                Some(FieldLabel::Optional)
            }
            TokenKind::Required => {
                self.advance();
                Some(FieldLabel::Required)
            }
            TokenKind::Repeated => {
                self.advance();
                Some(FieldLabel::Repeated)
            }
            _ => None,
        };
        field.label = label;
        // Record label span
        if label.is_some() {
            self.source_info.push(fnum::FIELD_LABEL);
            self.source_info.add_location(label_tok.span);
            self.source_info.pop();
        }
        if explicit_optional && self.is_proto3 {
            field.proto3_optional = Some(true);
        }
        // In editions mode, fields without explicit label default to LABEL_OPTIONAL
        if self.is_editions && label.is_none() {
            field.label = Some(FieldLabel::Optional);
        }

        // Label + map<K,V> is not allowed (but `optional map` as a message type IS ok)
        if label.is_some() && self.at(&TokenKind::Map) {
            // Peek ahead to see if this is map<K,V> syntax vs "map" as a message type name
            let next_pos = self.pos + 1;
            if next_pos < self.tokens.len() && self.tokens[next_pos].kind == TokenKind::LAngle {
                return Err(ParseError {
                    message:
                        "Field labels (required/optional/repeated) are not allowed on map fields."
                            .to_string(),
                    span: field.source_span.unwrap(),
                });
            }
        }

        // Proto2 requires explicit label (optional/required/repeated) on fields.
        // Editions and proto3 allow implicit labels (defaults to LABEL_OPTIONAL).
        if !self.is_proto3 && !self.is_editions && label.is_none() && !self.at(&TokenKind::Group) {
            return Err(ParseError {
                message: "Expected \"required\", \"optional\", or \"repeated\".".to_string(),
                span: self.peek().span,
            });
        }

        // Groups are banned in editions mode.
        if self.is_editions && self.at(&TokenKind::Group) {
            return Err(ParseError {
                message: "Group syntax is no longer supported in editions. To get group behavior you can specify features.message_encoding = DELIMITED on a message field.".to_string(),
                span: self.peek().span,
            });
        }

        // Check for group: `label group Name = number { ... }`
        if self.at(&TokenKind::Group) {
            let type_tok = self.peek().clone();
            self.advance();
            // Record type span for "group" keyword
            self.source_info.push(fnum::FIELD_TYPE);
            self.source_info.add_location(type_tok.span);
            self.source_info.pop();
            let name_tok = self.peek().clone();
            let name = self.expect_ident()?;
            // Record name span
            self.source_info.push(fnum::FIELD_NAME);
            self.source_info.add_location(name_tok.span);
            self.source_info.pop();
            // Group names must start with a capital letter
            if name.chars().next().is_none_or(|c| !c.is_ascii_uppercase()) {
                return Err(ParseError {
                    message: "group names must start with a capital letter".to_string(),
                    span: name_tok.span,
                });
            }
            self.expect(&TokenKind::Equals)?;
            let num_tok = self.expect(&TokenKind::IntLiteral)?;
            let number = parse_field_number(&num_tok)?;
            // Record number span
            self.source_info.push(fnum::FIELD_NUMBER);
            self.source_info.add_location(num_tok.span);
            self.source_info.pop();

            // Group fields can have inline options before the body
            field.r#type = Some(FieldType::Group);
            if self.at(&TokenKind::LBracket) {
                let opts = self.parse_field_options()?;
                let field_opts = field.options.get_or_insert_with(FieldOptions::default);
                for (name, value) in &opts {
                    apply_field_option(field_opts, name, value)?;
                }
                apply_field_special_options(&mut field, opts, true)?;
            }

            // Parse the group body as a message
            if !self.at(&TokenKind::LBrace) {
                return Err(ParseError {
                    message: "Missing group body.".to_string(),
                    span: self.peek().span,
                });
            }
            self.expect(&TokenKind::LBrace)?;
            let mut group_msg = DescriptorProto {
                name: Some(name.clone()),
                ..Default::default()
            };
            self.parse_message_body(&mut group_msg)?;
            self.expect(&TokenKind::RBrace)?;

            // The field references the group message type
            field.name = Some(name.to_ascii_lowercase());
            field.number = Some(number);
            field.type_name = Some(name.clone());

            if field.json_name.is_none() {
                field.json_name = Some(to_camel_case(field.name.as_ref().unwrap()));
            }

            return Ok((field, Some(group_msg)));
        }

        // Regular field
        let type_start = self.peek().span;
        let (field_type, type_name) = self.parse_field_type()?;
        let type_end = self.prev_span();
        field.r#type = field_type;
        field.type_name = type_name;
        // Record type span (FIELD_TYPE for scalars, FIELD_TYPE_NAME for messages/enums)
        if field.type_name.is_some() {
            self.source_info.push(fnum::FIELD_TYPE_NAME);
            self.source_info
                .add_location(Span::new(type_start.start, type_end.end));
            self.source_info.pop();
        } else {
            self.source_info.push(fnum::FIELD_TYPE);
            self.source_info
                .add_location(Span::new(type_start.start, type_end.end));
            self.source_info.pop();
        }

        let name_tok = self.peek().clone();
        field.name = Some(self.expect_ident()?);
        // Record name span
        self.source_info.push(fnum::FIELD_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();

        self.expect(&TokenKind::Equals)?;
        let num_tok = self.expect(&TokenKind::IntLiteral)?;
        field.number = Some(parse_field_number(&num_tok)?);
        // Record number span
        self.source_info.push(fnum::FIELD_NUMBER);
        self.source_info.add_location(num_tok.span);
        self.source_info.pop();

        if self.at(&TokenKind::LBracket) {
            let opts = self.parse_field_options()?;
            let field_opts = field.options.get_or_insert_with(FieldOptions::default);
            for (name, value) in &opts {
                apply_field_option(field_opts, name, value)?;
            }
            apply_field_special_options(&mut field, opts, false)?;
        }

        self.expect_semi()?;

        if field.json_name.is_none() {
            if let Some(ref name) = field.name {
                field.json_name = Some(to_camel_case(name));
            }
        }

        Ok((field, None))
    }

    fn parse_field_type(&mut self) -> Result<(Option<FieldType>, Option<String>), ParseError> {
        let tok = self.peek();
        let span = tok.span;

        // In UNSTABLE edition, check for new type keywords (identifiers, not lexer keywords)
        if self.is_unstable() && tok.kind == TokenKind::Ident {
            if let Some(ft) = match tok.text.as_str() {
                "varint32" => Some(FieldType::Int32),
                "varint64" => Some(FieldType::Int64),
                "uvarint32" => Some(FieldType::Uint32),
                "uvarint64" => Some(FieldType::Uint64),
                "zigzag32" => Some(FieldType::Sint32),
                "zigzag64" => Some(FieldType::Sint64),
                _ => None,
            } {
                self.advance();
                return Ok((Some(ft), None));
            }
        }

        // Check for scalar types
        if tok.kind.is_type_keyword() {
            // In UNSTABLE edition, some types are obsolete and others are remapped
            if self.is_unstable() {
                let obsolete = match &tok.kind {
                    TokenKind::Fixed32 => Some(("fixed32", "uint32")),
                    TokenKind::Fixed64 => Some(("fixed64", "uint64")),
                    TokenKind::Sfixed32 => Some(("sfixed32", "int32")),
                    TokenKind::Sfixed64 => Some(("sfixed64", "int64")),
                    TokenKind::Sint32 => Some(("sint32", "zigzag32")),
                    TokenKind::Sint64 => Some(("sint64", "zigzag64")),
                    _ => None,
                };
                if let Some((old, new)) = obsolete {
                    return Err(ParseError {
                        message: format!(
                            "Type '{}' is obsolete in unstable editions, use '{}' instead.",
                            old, new
                        ),
                        span,
                    });
                }
            }

            let ft = match &tok.kind {
                TokenKind::Double => FieldType::Double,
                TokenKind::Float => FieldType::Float,
                TokenKind::Int32 => {
                    if self.is_unstable() {
                        FieldType::Sfixed32
                    } else {
                        FieldType::Int32
                    }
                }
                TokenKind::Int64 => {
                    if self.is_unstable() {
                        FieldType::Sfixed64
                    } else {
                        FieldType::Int64
                    }
                }
                TokenKind::Uint32 => {
                    if self.is_unstable() {
                        FieldType::Fixed32
                    } else {
                        FieldType::Uint32
                    }
                }
                TokenKind::Uint64 => {
                    if self.is_unstable() {
                        FieldType::Fixed64
                    } else {
                        FieldType::Uint64
                    }
                }
                TokenKind::Sint32 => FieldType::Sint32,
                TokenKind::Sint64 => FieldType::Sint64,
                TokenKind::Fixed32 => FieldType::Fixed32,
                TokenKind::Fixed64 => FieldType::Fixed64,
                TokenKind::Sfixed32 => FieldType::Sfixed32,
                TokenKind::Sfixed64 => FieldType::Sfixed64,
                TokenKind::Bool => FieldType::Bool,
                TokenKind::String_ => FieldType::String,
                TokenKind::Bytes => FieldType::Bytes,
                _ => unreachable!(),
            };
            self.advance();
            Ok((Some(ft), None))
        } else {
            // Message or enum type reference (resolved later by analyzer)
            let type_name = self.parse_type_name()?;
            Ok((None, Some(type_name)))
        }
    }

    /// Parse a type name, possibly with leading dot and package path.
    fn parse_type_name(&mut self) -> Result<String, ParseError> {
        let mut name = String::new();
        if self.at(&TokenKind::Dot) {
            name.push('.');
            self.advance();
        }
        name.push_str(&self.expect_ident()?);
        while self.at(&TokenKind::Dot) {
            self.advance();
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }

    // -- oneof --

    fn parse_oneof(
        &mut self,
        oneof_index: i32,
    ) -> Result<
        (
            OneofDescriptorProto,
            Vec<FieldDescriptorProto>,
            Vec<DescriptorProto>,
        ),
        ParseError,
    > {
        self.expect(&TokenKind::Oneof)?;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record oneof name span
        self.source_info.push(fnum::ONEOF_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();
        self.expect(&TokenKind::LBrace)?;

        let oneof = OneofDescriptorProto {
            name: Some(name),
            options: None,
        };

        let mut fields = Vec::new();
        let mut nested = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.at(&TokenKind::Option) {
                let _opt = self.parse_option_statement()?;
                continue;
            }
            if self.at(&TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            // Labels are not allowed in oneofs
            if matches!(
                self.peek().kind,
                TokenKind::Optional | TokenKind::Required | TokenKind::Repeated
            ) {
                return Err(ParseError {
                    message:
                        "Fields in oneofs must not have labels (required / optional / repeated)."
                            .to_string(),
                    span: self.peek().span,
                });
            }
            // Map fields are not allowed in oneofs (map<K,V> syntax)
            if self.at(&TokenKind::Map) {
                let next_pos = self.pos + 1;
                if next_pos < self.tokens.len() && self.tokens[next_pos].kind == TokenKind::LAngle {
                    return Err(ParseError {
                        message: "Map fields are not allowed in oneofs.".to_string(),
                        span: self.peek().span,
                    });
                }
            }
            // Groups can appear in oneof without a label
            if self.at(&TokenKind::Group) {
                let (mut field, group_msg) = self.parse_field_or_group()?;
                field.oneof_index = Some(oneof_index);
                fields.push(field);
                if let Some(g) = group_msg {
                    nested.push(g);
                }
                continue;
            }
            let mut field = self.parse_oneof_field()?;
            field.oneof_index = Some(oneof_index);
            fields.push(field);
        }

        self.expect(&TokenKind::RBrace)?;
        Ok((oneof, fields, nested))
    }

    fn parse_oneof_field(&mut self) -> Result<FieldDescriptorProto, ParseError> {
        let mut field = FieldDescriptorProto::default();
        let start = self.peek().span;

        let (field_type, type_name) = self.parse_field_type()?;
        field.r#type = field_type;
        field.type_name = type_name;
        field.name = Some(self.expect_ident()?);

        self.expect(&TokenKind::Equals)?;
        let num_tok = self.expect(&TokenKind::IntLiteral)?;
        field.number = Some(parse_field_number(&num_tok)?);

        if self.at(&TokenKind::LBracket) {
            let opts = self.parse_field_options()?;
            let field_opts = field.options.get_or_insert_with(FieldOptions::default);
            for (name, value) in &opts {
                apply_field_option(field_opts, name, value)?;
            }
        }

        self.expect_semi()?;
        let end = self.prev_span();
        field.source_span = Some(Span::new(start.start, end.end));

        if field.json_name.is_none() {
            if let Some(ref name) = field.name {
                field.json_name = Some(to_camel_case(name));
            }
        }

        Ok(field)
    }

    // -- map --

    fn parse_map_field(&mut self) -> Result<(FieldDescriptorProto, DescriptorProto), ParseError> {
        self.expect(&TokenKind::Map)?;
        self.expect(&TokenKind::LAngle)?;

        let (key_type, key_type_name) = self.parse_field_type()?;
        self.expect(&TokenKind::Comma)?;
        let (value_type, value_type_name) = self.parse_field_type()?;

        self.expect(&TokenKind::RAngle)?;

        let field_name = self.expect_ident()?;
        self.expect(&TokenKind::Equals)?;
        let num_tok = self.expect(&TokenKind::IntLiteral)?;
        let field_number = parse_field_number(&num_tok)?;

        // Optional inline options
        let mut field_opts_list = Vec::new();
        if self.at(&TokenKind::LBracket) {
            field_opts_list = self.parse_field_options()?;
        }
        self.expect_semi()?;

        // Desugar map<K,V> into a synthetic nested message + repeated field
        let entry_name = map_entry_name(&field_name);

        // Key field
        let key_field = FieldDescriptorProto {
            name: Some("key".to_string()),
            number: Some(1),
            label: Some(FieldLabel::Optional),
            r#type: key_type,
            type_name: key_type_name,
            json_name: Some("key".to_string()),
            ..Default::default()
        };

        // Value field
        let value_field = FieldDescriptorProto {
            name: Some("value".to_string()),
            number: Some(2),
            label: Some(FieldLabel::Optional),
            r#type: value_type,
            type_name: value_type_name,
            json_name: Some("value".to_string()),
            ..Default::default()
        };

        // Synthetic message
        let nested_msg = DescriptorProto {
            name: Some(entry_name.clone()),
            field: vec![key_field, value_field],
            options: Some(MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        // The field on the parent message
        let mut map_field = FieldDescriptorProto {
            name: Some(field_name.clone()),
            number: Some(field_number),
            label: Some(FieldLabel::Repeated),
            r#type: Some(FieldType::Message),
            type_name: Some(entry_name),
            json_name: Some(to_camel_case(&field_name)),
            ..Default::default()
        };

        if !field_opts_list.is_empty() {
            let fopts = map_field.options.get_or_insert_with(FieldOptions::default);
            for (name, value) in &field_opts_list {
                apply_field_option(fopts, name, value)?;
            }
        }

        Ok((map_field, nested_msg))
    }

    // -- enum --

    fn parse_enum(&mut self) -> Result<EnumDescriptorProto, ParseError> {
        let start_span = self.peek().span;
        self.expect(&TokenKind::Enum)?;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record enum name span
        self.source_info.push(fnum::ENUM_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();
        self.expect(&TokenKind::LBrace)?;

        let mut e = EnumDescriptorProto {
            name: Some(name),
            source_span: Some(start_span),
            ..Default::default()
        };

        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            match &self.peek().kind {
                TokenKind::Option => {
                    let opt = self.parse_option_statement()?;
                    let options = e.options.get_or_insert_with(EnumOptions::default);
                    apply_enum_option(options, opt)?;
                }
                TokenKind::Reserved => {
                    let range_start_idx = e.reserved_range.len();
                    let name_start_idx = e.reserved_name.len();
                    let start = self.peek().span;
                    let (ranges, names) = self.parse_enum_reserved()?;
                    let end = self.prev_span();
                    for (i, _) in ranges.iter().enumerate() {
                        self.source_info
                            .push_index(fnum::ENUM_RESERVED_RANGE, (range_start_idx + i) as i32);
                        self.source_info
                            .add_location(Span::new(start.start, end.end));
                        self.source_info.pop_index();
                    }
                    for (i, _) in names.iter().enumerate() {
                        self.source_info
                            .push_index(fnum::ENUM_RESERVED_NAME, (name_start_idx + i) as i32);
                        self.source_info
                            .add_location(Span::new(start.start, end.end));
                        self.source_info.pop_index();
                    }
                    e.reserved_range.extend(ranges);
                    e.reserved_name.extend(names);
                }
                TokenKind::Semicolon => {
                    self.advance();
                }
                _ => {
                    let val_idx = e.value.len() as i32;
                    let start = self.peek().span;
                    let start_line = start.start.line - 1;
                    let (leading, detached) = self.collect_leading_comments(start_line);
                    self.source_info.push_index(fnum::ENUM_VALUE, val_idx);
                    let val = self.parse_enum_value()?;
                    let end = self.prev_span();
                    let trailing = self.collect_trailing_comment(end.end.line - 1);
                    self.source_info.add_location_with_comments(
                        Span::new(start.start, end.end),
                        leading,
                        trailing,
                        detached,
                    );
                    self.source_info.pop_index();
                    e.value.push(val);
                }
            }
        }

        let end_span = self.peek().span;
        self.expect(&TokenKind::RBrace)?;

        // Validate allow_alias semantics
        if let Some(ref opts) = e.options {
            if let Some(allow_alias) = opts.allow_alias {
                let enum_name = e.name.as_deref().unwrap_or("?");
                if !allow_alias {
                    // allow_alias = false is pointless and C++ errors on it
                    return Err(ParseError {
                        message: format!(
                            "\"{}\" declares 'option allow_alias = false;' which has no effect. \
                             Please remove the declaration.",
                            enum_name
                        ),
                        span: end_span,
                    });
                }
                // allow_alias = true but no aliases: unnecessary
                let numbers: Vec<i32> = e.value.iter().filter_map(|v| v.number).collect();
                let unique_count = {
                    let mut seen = std::collections::HashSet::new();
                    numbers.iter().filter(|n| seen.insert(**n)).count()
                };
                if unique_count == numbers.len() {
                    return Err(ParseError {
                        message: format!(
                            "\"{}\" declares support for enum aliases but no enum values share \
                             field numbers. Please remove the unnecessary \
                             'option allow_alias = true;' declaration.",
                            enum_name
                        ),
                        span: end_span,
                    });
                }
            }
        }

        Ok(e)
    }

    fn parse_enum_value(&mut self) -> Result<EnumValueDescriptorProto, ParseError> {
        let start_span = self.peek().span;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record enum value name span
        self.source_info.push(fnum::ENUM_VALUE_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();
        self.expect(&TokenKind::Equals)?;

        // Enum values can be negative but must fit in i32
        let num_start = self.peek().span;
        let tok = self.peek().clone();
        let number = match tok.kind {
            TokenKind::IntLiteral => {
                self.advance();
                parse_enum_value(&tok)?
            }
            TokenKind::Minus => {
                self.advance();
                let num_tok = self.expect(&TokenKind::IntLiteral)?;
                let val = parse_int(&num_tok.text).map_err(|e| ParseError {
                    message: format!("invalid enum value: {}", e),
                    span: num_tok.span,
                })?;
                // Check for -2147483649 and below
                if val > (i32::MAX as u64) + 1 {
                    return Err(ParseError {
                        message: format!("enum value out of range: -{}", num_tok.text),
                        span: num_tok.span,
                    });
                }
                -(val as i64) as i32
            }
            _ => {
                return Err(ParseError {
                    message: format!("expected enum value number, found {:?}", tok.kind),
                    span: tok.span,
                });
            }
        };
        // Record enum value number span
        let num_end = self.prev_span();
        self.source_info.push(fnum::ENUM_VALUE_NUMBER);
        self.source_info
            .add_location(Span::new(num_start.start, num_end.end));
        self.source_info.pop();

        let mut ev = EnumValueDescriptorProto {
            name: Some(name),
            number: Some(number),
            options: None,
            source_span: Some(start_span),
        };

        if self.at(&TokenKind::LBracket) {
            let opts = self.parse_field_options()?;
            let evopts = ev.options.get_or_insert_with(EnumValueOptions::default);
            for (name, value) in &opts {
                if name == "deprecated" {
                    if let OptionValue::Bool(b) = value {
                        evopts.deprecated = Some(*b);
                    }
                }
            }
        }

        self.expect_semi()?;
        Ok(ev)
    }

    fn parse_enum_reserved(&mut self) -> Result<(Vec<EnumReservedRange>, Vec<String>), ParseError> {
        self.expect(&TokenKind::Reserved)?;
        let mut ranges = Vec::new();
        let mut names = Vec::new();

        if self.at(&TokenKind::StringLiteral) {
            if self.is_editions {
                return Err(ParseError {
                    message: "Reserved names must be identifiers in editions, not string literals."
                        .to_string(),
                    span: self.peek().span,
                });
            }
            // Reserved names (string literals) -- proto2/proto3
            loop {
                let tok = self.expect(&TokenKind::StringLiteral)?;
                // Warn if the string is not a valid identifier
                if !is_valid_identifier(&tok.text) {
                    self.record_warning(ParseError {
                        message: format!(
                            "Reserved name \"{}\" is not a valid identifier.",
                            tok.text
                        ),
                        span: tok.span,
                    });
                }
                names.push(tok.text);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After a string, next must also be a string (no mixing)
                    if !self.at(&TokenKind::StringLiteral) {
                        return Err(ParseError {
                            message: "Expected enum number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        } else if self.at(&TokenKind::Ident) || (self.at(&TokenKind::Max) && !self.is_editions) {
            if !self.is_editions {
                return Err(ParseError {
                    message: "Reserved names must be string literals. (Only editions supports identifiers.)".to_string(),
                    span: self.peek().span,
                });
            }
            // Editions: bare identifiers for reserved names
            loop {
                let tok_text = self.expect_ident()?;
                names.push(tok_text);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After an ident, next must also be an ident (no mixing with numbers)
                    if self.at(&TokenKind::IntLiteral) || self.at(&TokenKind::Minus) {
                        return Err(ParseError {
                            message: "Expected enum number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        } else {
            // Reserved numbers/ranges
            loop {
                let start = self.parse_enum_reserved_int()?;
                let end = if self.at(&TokenKind::To) {
                    self.advance();
                    if self.at(&TokenKind::Max) {
                        self.advance();
                        i32::MAX
                    } else {
                        self.parse_enum_reserved_int()?
                    }
                } else {
                    start
                };
                ranges.push(EnumReservedRange {
                    start: Some(start),
                    end: Some(end),
                });
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After a number, next must also be a number (no mixing)
                    if self.at(&TokenKind::StringLiteral) || self.at(&TokenKind::Ident) {
                        return Err(ParseError {
                            message: "Expected enum number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        }

        self.expect_semi()?;
        Ok((ranges, names))
    }

    /// Parse a signed integer for enum reserved ranges, with i32 range validation.
    fn parse_enum_reserved_int(&mut self) -> Result<i32, ParseError> {
        if self.at(&TokenKind::Minus) {
            self.advance();
            let tok = self.expect(&TokenKind::IntLiteral)?;
            let val = parse_int(&tok.text).map_err(|e| ParseError {
                message: format!("invalid number: {}", e),
                span: tok.span,
            })?;
            if val > (i32::MAX as u64) + 1 {
                return Err(ParseError {
                    message: "Integer out of range.".to_string(),
                    span: tok.span,
                });
            }
            Ok(-(val as i64) as i32)
        } else {
            let tok = self.expect(&TokenKind::IntLiteral)?;
            // Handle negative IntLiteral from lexer (e.g., "-10" as single token)
            if tok.text.starts_with('-') {
                return parse_enum_value(&tok);
            }
            let val = parse_int(&tok.text).map_err(|e| ParseError {
                message: format!("invalid number: {}", e),
                span: tok.span,
            })?;
            if val > i32::MAX as u64 {
                return Err(ParseError {
                    message: "Integer out of range.".to_string(),
                    span: tok.span,
                });
            }
            Ok(val as i32)
        }
    }

    // -- reserved --

    fn parse_reserved(&mut self) -> Result<(Vec<ReservedRange>, Vec<String>), ParseError> {
        self.expect(&TokenKind::Reserved)?;
        let mut ranges = Vec::new();
        let mut names = Vec::new();

        if self.at(&TokenKind::StringLiteral) {
            if self.is_editions {
                return Err(ParseError {
                    message: "Reserved names must be identifiers in editions, not string literals."
                        .to_string(),
                    span: self.peek().span,
                });
            }
            // Reserved names (string literals) -- proto2/proto3
            loop {
                let tok = self.expect(&TokenKind::StringLiteral)?;
                // Warn if the string is not a valid identifier
                if !is_valid_identifier(&tok.text) {
                    self.record_warning(ParseError {
                        message: format!(
                            "Reserved name \"{}\" is not a valid identifier.",
                            tok.text
                        ),
                        span: tok.span,
                    });
                }
                names.push(tok.text);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After a string, next must also be a string (no mixing)
                    if self.at(&TokenKind::IntLiteral) || self.at(&TokenKind::Minus) {
                        return Err(ParseError {
                            message: "Expected field number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        } else if self.at(&TokenKind::Ident) || (self.at(&TokenKind::Max) && !self.is_editions) {
            if !self.is_editions {
                return Err(ParseError {
                    message: "Reserved names must be string literals. (Only editions supports identifiers.)".to_string(),
                    span: self.peek().span,
                });
            }
            // Editions: bare identifiers for reserved names
            loop {
                let tok_text = self.expect_ident()?;
                names.push(tok_text);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After an ident, next must also be an ident (no mixing with numbers)
                    if self.at(&TokenKind::IntLiteral) || self.at(&TokenKind::Minus) {
                        return Err(ParseError {
                            message: "Expected field number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        } else {
            // Reserved numbers/ranges
            loop {
                // Message reserved must be positive integers
                if self.at(&TokenKind::Minus) {
                    return Err(ParseError {
                        message: "expected positive number in reserved".to_string(),
                        span: self.peek().span,
                    });
                }
                let start_tok = self.expect(&TokenKind::IntLiteral)?;
                // Reject negative int literals (lexer may produce "-10" as single token)
                if start_tok.text.starts_with('-') {
                    return Err(ParseError {
                        message: "expected positive number in reserved".to_string(),
                        span: start_tok.span,
                    });
                }
                let start = parse_field_number(&start_tok)?;

                let end = if self.at(&TokenKind::To) {
                    self.advance();
                    if self.at(&TokenKind::Max) {
                        self.advance();
                        536870911 // MAX_FIELD_NUMBER
                    } else {
                        let end_tok = self.expect(&TokenKind::IntLiteral)?;
                        parse_field_number(&end_tok)?
                    }
                } else {
                    start
                };

                ranges.push(ReservedRange {
                    start: Some(start),
                    end: Some(end + 1), // protobuf convention: end is exclusive
                });

                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // After a number, next must also be a number (no mixing)
                    if self.at(&TokenKind::StringLiteral) || self.at(&TokenKind::Ident) {
                        return Err(ParseError {
                            message: "Expected field number range.".to_string(),
                            span: self.peek().span,
                        });
                    }
                } else {
                    break;
                }
            }
        }

        self.expect_semi()?;
        Ok((ranges, names))
    }

    // -- extensions --

    fn parse_extensions(&mut self) -> Result<Vec<ExtensionRange>, ParseError> {
        self.expect(&TokenKind::Extensions)?;
        let mut ranges = Vec::new();

        loop {
            let start_tok = self.expect(&TokenKind::IntLiteral)?;
            let start_u64 = parse_int(&start_tok.text).map_err(|e| ParseError {
                message: format!("invalid extension number: {}", e),
                span: start_tok.span,
            })?;
            if start_u64 > 536870911 {
                return Err(ParseError {
                    message: "Field number out of bounds.".to_string(),
                    span: start_tok.span,
                });
            }
            let start = start_u64 as i32;

            let end = if self.at(&TokenKind::To) {
                self.advance();
                if self.at(&TokenKind::Max) {
                    self.advance();
                    536870912 // MAX_FIELD_NUMBER + 1
                } else {
                    let end_tok = self.expect(&TokenKind::IntLiteral)?;
                    let end_u64 = parse_int(&end_tok.text).map_err(|e| ParseError {
                        message: format!("invalid extension number: {}", e),
                        span: end_tok.span,
                    })?;
                    if end_u64 > 536870911 {
                        return Err(ParseError {
                            message: "Field number out of bounds.".to_string(),
                            span: end_tok.span,
                        });
                    }
                    end_u64 as i32 + 1 // exclusive
                }
            } else {
                start + 1 // single number range, exclusive end
            };

            // Optional inline options: [verification = UNVERIFIED]
            let ext_opts = if self.at(&TokenKind::LBracket) {
                let opts = self.parse_field_options()?;
                let mut ext_range_opts = ExtensionRangeOptions::default();
                for (name, value) in opts {
                    ext_range_opts
                        .uninterpreted_option
                        .push(make_uninterpreted(&name, &value));
                }
                Some(ext_range_opts)
            } else {
                None
            };

            ranges.push(ExtensionRange {
                start: Some(start),
                end: Some(end),
                options: ext_opts,
            });

            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        self.expect_semi()?;
        Ok(ranges)
    }

    // -- extend --

    /// Parse an `extend` block. `ext_field_num` and `ext_base_idx` allow the
    /// caller to specify the path context for source info recording.
    fn parse_extend_with_context(
        &mut self,
        ext_field_num: i32,
        ext_base_idx: i32,
    ) -> Result<Vec<FieldDescriptorProto>, ParseError> {
        self.expect(&TokenKind::Extend)?;
        // The extendee must be a message type name, not a primitive
        if self.peek().kind.is_type_keyword() {
            return Err(ParseError {
                message: "expected message type for extend, not a primitive type".to_string(),
                span: self.peek().span,
            });
        }
        let extendee_tok = self.peek().clone();
        let extendee = self.parse_type_name()?;
        let extendee_end = self.prev_span();
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.at(&TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            // Map fields are not allowed as extensions
            if self.at(&TokenKind::Map) {
                let next_pos = self.pos + 1;
                if next_pos < self.tokens.len() && self.tokens[next_pos].kind == TokenKind::LAngle {
                    return Err(ParseError {
                        message: "Map fields are not allowed to be extensions.".to_string(),
                        span: self.peek().span,
                    });
                }
            }
            let field_idx = ext_base_idx + fields.len() as i32;
            let start = self.peek().span;
            self.source_info.push_index(ext_field_num, field_idx);
            let (mut field, _nested) = self.parse_field_or_group()?;
            let end = self.prev_span();
            // Record extendee span on each extension field
            self.source_info.push(fnum::FIELD_EXTENDEE);
            self.source_info
                .add_location(Span::new(extendee_tok.span.start, extendee_end.end));
            self.source_info.pop();
            self.source_info
                .add_location(Span::new(start.start, end.end));
            self.source_info.pop_index();
            field.extendee = Some(extendee.clone());
            fields.push(field);
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(fields)
    }

    // -- service --

    fn parse_service(&mut self) -> Result<ServiceDescriptorProto, ParseError> {
        self.expect(&TokenKind::Service)?;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record service name span
        self.source_info.push(fnum::SERVICE_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();
        self.expect(&TokenKind::LBrace)?;

        let mut svc = ServiceDescriptorProto {
            name: Some(name),
            ..Default::default()
        };

        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            match &self.peek().kind {
                TokenKind::Rpc => {
                    let method_idx = svc.method.len() as i32;
                    let start = self.peek().span;
                    let start_line = start.start.line - 1;
                    let (leading, detached) = self.collect_leading_comments(start_line);
                    self.source_info
                        .push_index(fnum::SERVICE_METHOD, method_idx);
                    let method = self.parse_rpc()?;
                    let end = self.prev_span();
                    let trailing = self.collect_trailing_comment(end.end.line - 1);
                    self.source_info.add_location_with_comments(
                        Span::new(start.start, end.end),
                        leading,
                        trailing,
                        detached,
                    );
                    self.source_info.pop_index();
                    svc.method.push(method);
                }
                TokenKind::Option => {
                    let opt = self.parse_option_statement()?;
                    let options = svc.options.get_or_insert_with(ServiceOptions::default);
                    apply_service_option(options, opt)?;
                }
                TokenKind::Semicolon => {
                    self.advance();
                }
                _ => {
                    return Err(ParseError {
                        message: format!("unexpected token in service: {:?}", self.peek().kind),
                        span: self.peek().span,
                    });
                }
            }
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(svc)
    }

    fn parse_rpc(&mut self) -> Result<MethodDescriptorProto, ParseError> {
        self.expect(&TokenKind::Rpc)?;
        let name_tok = self.peek().clone();
        let name = self.expect_ident()?;
        // Record method name span
        self.source_info.push(fnum::METHOD_NAME);
        self.source_info.add_location(name_tok.span);
        self.source_info.pop();

        // Input
        self.expect(&TokenKind::LParen)?;
        let client_streaming = if self.at(&TokenKind::Stream) {
            let stream_tok = self.peek().clone();
            self.advance();
            self.source_info.push(fnum::METHOD_CLIENT_STREAMING);
            self.source_info.add_location(stream_tok.span);
            self.source_info.pop();
            Some(true)
        } else {
            Some(false)
        };
        // RPC params must be message types, not primitives
        if self.peek().kind.is_type_keyword() {
            return Err(ParseError {
                message: "expected message type for RPC parameter, not a primitive type"
                    .to_string(),
                span: self.peek().span,
            });
        }
        let input_tok = self.peek().clone();
        let input_type = Some(self.parse_type_name()?);
        let input_end = self.prev_span();
        // Record input_type span
        self.source_info.push(fnum::METHOD_INPUT_TYPE);
        self.source_info
            .add_location(Span::new(input_tok.span.start, input_end.end));
        self.source_info.pop();
        self.expect(&TokenKind::RParen)?;

        // Returns
        self.expect(&TokenKind::Returns)?;

        // Output
        self.expect(&TokenKind::LParen)?;
        let server_streaming = if self.at(&TokenKind::Stream) {
            let stream_tok = self.peek().clone();
            self.advance();
            self.source_info.push(fnum::METHOD_SERVER_STREAMING);
            self.source_info.add_location(stream_tok.span);
            self.source_info.pop();
            Some(true)
        } else {
            Some(false)
        };
        if self.peek().kind.is_type_keyword() {
            return Err(ParseError {
                message: "expected message type for RPC return, not a primitive type".to_string(),
                span: self.peek().span,
            });
        }
        let output_tok = self.peek().clone();
        let output_type = Some(self.parse_type_name()?);
        let output_end = self.prev_span();
        // Record output_type span
        self.source_info.push(fnum::METHOD_OUTPUT_TYPE);
        self.source_info
            .add_location(Span::new(output_tok.span.start, output_end.end));
        self.source_info.pop();
        self.expect(&TokenKind::RParen)?;

        let mut method = MethodDescriptorProto {
            name: Some(name),
            input_type,
            output_type,
            client_streaming,
            server_streaming,
            options: None,
        };

        // Method can end with ; or { options }
        if self.at(&TokenKind::LBrace) {
            self.advance();
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                if self.at(&TokenKind::Option) {
                    let opt = self.parse_option_statement()?;
                    let options = method.options.get_or_insert_with(MethodOptions::default);
                    apply_method_option(options, opt)?;
                } else if self.at(&TokenKind::Semicolon) {
                    self.advance();
                } else {
                    return Err(ParseError {
                        message: format!("unexpected token in rpc body: {:?}", self.peek().kind),
                        span: self.peek().span,
                    });
                }
            }
            if self.at_eof() {
                return Err(ParseError {
                    message: "Reached end of input in method options (missing '}').".to_string(),
                    span: self.peek().span,
                });
            }
            self.expect(&TokenKind::RBrace)?;
        } else {
            self.expect_semi()?;
        }

        Ok(method)
    }

    // -- Helpers --

    fn parse_full_ident(&mut self) -> Result<String, ParseError> {
        let mut name = self.expect_ident()?;
        while self.at(&TokenKind::Dot) {
            self.advance();
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }
}

// Option types, option application logic, utility functions, and tests are in
// submodules: options.rs, parse_helpers.rs, tests.rs

#[cfg(test)]
mod tests;
