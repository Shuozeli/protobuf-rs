use protoc_rs_schema::*;

use crate::lexer::{LexError, Lexer, Token, TokenKind};
use crate::source_info_builder::{field_num as fnum, SourceInfoBuilder};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

/// Maximum nesting depth for messages (matches C++ protoc).
const MAX_MESSAGE_NESTING_DEPTH: usize = 32;

/// Parse result with collected errors and warnings.
pub struct ParseResult {
    pub file: FileDescriptorProto,
    pub errors: Vec<ParseError>,
    pub warnings: Vec<ParseError>,
}

#[allow(dead_code)]
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

    #[allow(dead_code)]
    fn parse_extend(&mut self) -> Result<Vec<FieldDescriptorProto>, ParseError> {
        self.parse_extend_with_context(fnum::FILE_EXTENSION, 0)
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

// ---------------------------------------------------------------------------
// Option value (intermediate representation before applying to specific option types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum OptionValue {
    String(String),
    Int(String),
    Float(String),
    Bool(bool),
    Ident(String),
    Aggregate(String),
}

fn option_value_to_string(v: &OptionValue) -> String {
    match v {
        OptionValue::String(s) => s.clone(),
        OptionValue::Int(s) | OptionValue::Float(s) => s.clone(),
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Ident(s) => s.clone(),
        OptionValue::Aggregate(s) => s.clone(),
    }
}

fn expect_string_option(
    name: &str,
    value: &OptionValue,
    parent: &str,
) -> Result<String, ParseError> {
    if let OptionValue::String(s) = value {
        Ok(s.clone())
    } else {
        Err(ParseError {
            message: format!(
                "Value must be quoted string for string option \"{}.{}\".",
                parent, name
            ),
            span: Span::default(),
        })
    }
}

fn expect_enum_option(name: &str, value: &OptionValue, parent: &str) -> Result<String, ParseError> {
    if let OptionValue::Ident(s) = value {
        Ok(s.clone())
    } else {
        Err(ParseError {
            message: format!(
                "Value must be identifier for enum-valued option \"{}.{}\".",
                parent, name
            ),
            span: Span::default(),
        })
    }
}

fn apply_file_option(
    opts: &mut FileOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    const P: &str = "google.protobuf.FileOptions";
    match name.as_str() {
        "java_package" => opts.java_package = Some(expect_string_option(&name, &value, P)?),
        "java_outer_classname" => {
            opts.java_outer_classname = Some(expect_string_option(&name, &value, P)?)
        }
        "go_package" => opts.go_package = Some(expect_string_option(&name, &value, P)?),
        "objc_class_prefix" => {
            opts.objc_class_prefix = Some(expect_string_option(&name, &value, P)?)
        }
        "csharp_namespace" => opts.csharp_namespace = Some(expect_string_option(&name, &value, P)?),
        "swift_prefix" => opts.swift_prefix = Some(expect_string_option(&name, &value, P)?),
        "php_class_prefix" => opts.php_class_prefix = Some(expect_string_option(&name, &value, P)?),
        "php_namespace" => opts.php_namespace = Some(expect_string_option(&name, &value, P)?),
        "php_metadata_namespace" => {
            opts.php_metadata_namespace = Some(expect_string_option(&name, &value, P)?)
        }
        "ruby_package" => opts.ruby_package = Some(expect_string_option(&name, &value, P)?),
        "java_multiple_files" => {
            if let OptionValue::Bool(b) = value {
                opts.java_multiple_files = Some(b);
            }
        }
        "java_string_check_utf8" => {
            if let OptionValue::Bool(b) = value {
                opts.java_string_check_utf8 = Some(b);
            }
        }
        "java_generate_equals_and_hash" => {
            if let OptionValue::Bool(b) = value {
                opts.java_generate_equals_and_hash = Some(b);
            }
        }
        "optimize_for" => {
            let s = expect_enum_option(&name, &value, P)?;
            opts.optimize_for = match s.as_str() {
                "SPEED" => Some(OptimizeMode::Speed),
                "CODE_SIZE" => Some(OptimizeMode::CodeSize),
                "LITE_RUNTIME" => Some(OptimizeMode::LiteRuntime),
                _ => None,
            };
        }
        "cc_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.cc_generic_services = Some(b);
            }
        }
        "java_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.java_generic_services = Some(b);
            }
        }
        "py_generic_services" => {
            if let OptionValue::Bool(b) = value {
                opts.py_generic_services = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "cc_enable_arenas" => {
            if let OptionValue::Bool(b) = value {
                opts.cc_enable_arenas = Some(b);
            }
        }
        _ => {
            // Unknown/custom option -- store as uninterpreted
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

fn apply_message_option(
    opts: &mut MessageOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "message_set_wire_format" => {
            if let OptionValue::Bool(b) = value {
                opts.message_set_wire_format = Some(b);
            }
        }
        "no_standard_descriptor_accessor" => {
            if let OptionValue::Bool(b) = value {
                opts.no_standard_descriptor_accessor = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "map_entry" => {
            if let OptionValue::Bool(b) = value {
                opts.map_entry = Some(b);
                if b {
                    return Err(ParseError {
                        message: "map_entry should not be set explicitly. Use map<KeyType, ValueType> instead.".to_string(),
                        span: Span::default(),
                    });
                }
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

fn apply_field_option(
    opts: &mut FieldOptions,
    name: &str,
    value: &OptionValue,
) -> Result<(), ParseError> {
    match name {
        "packed" => {
            if let OptionValue::Bool(b) = value {
                opts.packed = Some(*b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(*b);
            }
        }
        "lazy" => {
            if let OptionValue::Bool(b) = value {
                opts.lazy = Some(*b);
            }
        }
        "weak" => {
            if let OptionValue::Bool(b) = value {
                opts.weak = Some(*b);
            }
        }
        "jstype" => {
            let s = expect_enum_option(name, value, "google.protobuf.FieldOptions")?;
            opts.jstype = match s.as_str() {
                "JS_NORMAL" => Some(FieldJsType::JsNormal),
                "JS_STRING" => Some(FieldJsType::JsString),
                "JS_NUMBER" => Some(FieldJsType::JsNumber),
                _ => None,
            };
        }
        "ctype" => {
            let s = expect_enum_option(name, value, "google.protobuf.FieldOptions")?;
            opts.ctype = match s.as_str() {
                "STRING" => Some(FieldCType::String),
                "CORD" => Some(FieldCType::Cord),
                "STRING_PIECE" => Some(FieldCType::StringPiece),
                _ => None,
            };
        }
        "default" | "json_name" => {
            // Handled by caller, not stored in FieldOptions
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(name, value));
        }
    }
    Ok(())
}

/// Process default= and json_name= options on a field, with type validation
/// and duplicate detection. `is_group` indicates this is a group field.
fn apply_field_special_options(
    field: &mut FieldDescriptorProto,
    opts: Vec<(String, OptionValue)>,
    is_group: bool,
) -> Result<(), ParseError> {
    let span = field.source_span.unwrap_or_default();
    let mut has_default = false;
    let mut has_json_name = false;

    for (name, value) in opts {
        if name == "default" {
            if has_default {
                return Err(ParseError {
                    message: "Already set option \"default\".".to_string(),
                    span,
                });
            }
            has_default = true;

            if is_group || field.r#type == Some(FieldType::Group) {
                return Err(ParseError {
                    message: "Messages can't have default values.".to_string(),
                    span,
                });
            }

            // Type-check the default value against the field type
            if let Some(ref ft) = field.r#type {
                validate_default_value(ft, &value, span)?;
            }

            field.default_value = Some(option_value_to_string(&value));
        } else if name == "json_name" {
            if has_json_name {
                return Err(ParseError {
                    message: "Already set option \"json_name\".".to_string(),
                    span,
                });
            }
            has_json_name = true;

            match value {
                OptionValue::String(s) => {
                    field.json_name = Some(s);
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected string for JSON name.".to_string(),
                        span,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Validate that a default value matches the expected field type.
fn validate_default_value(
    field_type: &FieldType,
    value: &OptionValue,
    span: Span,
) -> Result<(), ParseError> {
    match field_type {
        // Signed integer types need int values with range check
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => match value {
            OptionValue::Int(s) => validate_int_range(s, i32::MIN as i64, i32::MAX as i64, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => match value {
            OptionValue::Int(s) => validate_int_range(s, i64::MIN, i64::MAX, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        // Unsigned integer types need non-negative int values with range check
        FieldType::Uint32 | FieldType::Fixed32 => match value {
            OptionValue::Int(s) if s.starts_with('-') => Err(ParseError {
                message: "Unsigned field can't have negative default value.".to_string(),
                span,
            }),
            OptionValue::Int(s) => validate_uint_range(s, u32::MAX as u64, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        FieldType::Uint64 | FieldType::Fixed64 => match value {
            OptionValue::Int(s) if s.starts_with('-') => Err(ParseError {
                message: "Unsigned field can't have negative default value.".to_string(),
                span,
            }),
            OptionValue::Int(s) => validate_uint_range(s, u64::MAX, span),
            _ => Err(ParseError {
                message: "Expected integer for field default value.".to_string(),
                span,
            }),
        },
        // Bool needs true/false
        FieldType::Bool => match value {
            OptionValue::Bool(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected \"true\" or \"false\".".to_string(),
                span,
            }),
        },
        // String/bytes need string literal
        FieldType::String | FieldType::Bytes => match value {
            OptionValue::String(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected string for field default value.".to_string(),
                span,
            }),
        },
        // Float/double accept int, float, or special idents (inf, nan)
        FieldType::Float | FieldType::Double => match value {
            OptionValue::Float(_) | OptionValue::Int(_) => Ok(()),
            OptionValue::Ident(s) if s == "inf" || s == "nan" => Ok(()),
            _ => Err(ParseError {
                message: "Expected number for field default value.".to_string(),
                span,
            }),
        },
        // Enum default is an ident
        FieldType::Enum => match value {
            OptionValue::Ident(_) => Ok(()),
            _ => Err(ParseError {
                message: "Expected enum value for field default value.".to_string(),
                span,
            }),
        },
        // Group/message can't have defaults (handled by caller)
        _ => Ok(()),
    }
}

/// Check that a signed integer string fits in the range [min, max].
fn validate_int_range(s: &str, min: i64, max: i64, span: Span) -> Result<(), ParseError> {
    let is_negative = s.starts_with('-');
    if is_negative {
        let abs_text = &s[1..];
        let abs_val = parse_int(abs_text).map_err(|_| ParseError {
            message: "Integer out of range.".to_string(),
            span,
        })?;
        // Check if -(abs_val) can be represented
        // abs_val as i64 might overflow if abs_val > i64::MAX
        if abs_val > (i64::MAX as u64) + 1 {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
        let signed = -(abs_val as i128) as i64;
        if signed < min || signed > max {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
    } else {
        let val = parse_int(s).map_err(|_| ParseError {
            message: "Integer out of range.".to_string(),
            span,
        })?;
        if val > max as u64 {
            return Err(ParseError {
                message: "Integer out of range.".to_string(),
                span,
            });
        }
    }
    Ok(())
}

/// Check that an unsigned integer string fits in [0, max].
fn validate_uint_range(s: &str, max: u64, span: Span) -> Result<(), ParseError> {
    let val = parse_int(s).map_err(|_| ParseError {
        message: "Integer out of range.".to_string(),
        span,
    })?;
    if val > max {
        return Err(ParseError {
            message: "Integer out of range.".to_string(),
            span,
        });
    }
    Ok(())
}

fn apply_enum_option(
    opts: &mut EnumOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "allow_alias" => {
            if let OptionValue::Bool(b) = value {
                opts.allow_alias = Some(b);
            }
        }
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

fn apply_service_option(
    opts: &mut ServiceOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

fn apply_method_option(
    opts: &mut MethodOptions,
    (name, value): (String, OptionValue),
) -> Result<(), ParseError> {
    match name.as_str() {
        "deprecated" => {
            if let OptionValue::Bool(b) = value {
                opts.deprecated = Some(b);
            }
        }
        "idempotency_level" => {
            if let OptionValue::Ident(s) = &value {
                match s.as_str() {
                    "IDEMPOTENCY_UNKNOWN" => {
                        opts.idempotency_level = Some(IdempotencyLevel::IdempotencyUnknown)
                    }
                    "NO_SIDE_EFFECTS" => {
                        opts.idempotency_level = Some(IdempotencyLevel::NoSideEffects)
                    }
                    "IDEMPOTENT" => opts.idempotency_level = Some(IdempotencyLevel::Idempotent),
                    _ => {
                        opts.uninterpreted_option
                            .push(make_uninterpreted(&name, &value));
                    }
                }
            }
        }
        _ => {
            opts.uninterpreted_option
                .push(make_uninterpreted(&name, &value));
        }
    }
    Ok(())
}

/// Parse an option name string like "(baz.bar).foo" into NamePart list.
/// Parenthesized groups are extension references; bare identifiers are field names.
fn parse_option_name_parts(name: &str) -> Vec<NamePart> {
    let mut parts = Vec::new();
    let mut rest = name;
    while !rest.is_empty() {
        // Skip leading dot separator
        if rest.starts_with('.') {
            rest = &rest[1..];
        }
        if rest.starts_with('(') {
            // Extension part: find matching ')'
            if let Some(close) = rest.find(')') {
                let inner = &rest[1..close];
                parts.push(NamePart {
                    name_part: inner.to_string(),
                    is_extension: true,
                });
                rest = &rest[close + 1..];
            } else {
                // Malformed -- treat rest as plain name
                parts.push(NamePart {
                    name_part: rest.to_string(),
                    is_extension: false,
                });
                break;
            }
        } else {
            // Plain identifier: take until next '.' or '('
            let end = rest.find(['.', '(']).unwrap_or(rest.len());
            if end > 0 {
                parts.push(NamePart {
                    name_part: rest[..end].to_string(),
                    is_extension: false,
                });
            }
            rest = &rest[end..];
        }
    }
    parts
}

fn make_uninterpreted(name: &str, value: &OptionValue) -> UninterpretedOption {
    let name_parts = parse_option_name_parts(name);

    let mut opt = UninterpretedOption {
        name: name_parts,
        ..Default::default()
    };

    match value {
        OptionValue::String(s) => opt.string_value = Some(s.as_bytes().to_vec()),
        OptionValue::Int(s) => {
            if s.starts_with('-') {
                opt.negative_int_value = s.parse().ok();
            } else {
                opt.positive_int_value = s.parse().ok();
            }
        }
        OptionValue::Float(s) => opt.double_value = Some(s.clone()),
        OptionValue::Bool(b) => opt.identifier_value = Some(b.to_string()),
        OptionValue::Ident(s) => opt.identifier_value = Some(s.clone()),
        OptionValue::Aggregate(s) => opt.aggregate_value = Some(s.clone()),
    }

    opt
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Parse and validate an enum value from a token. Must fit in i32.
/// Handles both positive and negative IntLiteral tokens (lexer may produce "-1" as a single token).
fn parse_enum_value(tok: &Token) -> Result<i32, ParseError> {
    let text = &tok.text;
    if let Some(abs_text) = text.strip_prefix('-') {
        // Negative: parse absolute value and check range
        let abs_val = parse_int(abs_text).map_err(|e| ParseError {
            message: format!("invalid enum value: {}", e),
            span: tok.span,
        })?;
        if abs_val > (i32::MAX as u64) + 1 {
            return Err(ParseError {
                message: format!("enum value out of range: {}", text),
                span: tok.span,
            });
        }
        Ok(-(abs_val as i64) as i32)
    } else {
        let val = parse_int(text).map_err(|e| ParseError {
            message: format!("invalid enum value: {}", e),
            span: tok.span,
        })?;
        if val > i32::MAX as u64 {
            return Err(ParseError {
                message: format!("enum value out of range: {}", text),
                span: tok.span,
            });
        }
        Ok(val as i32)
    }
}

/// Format a comment's text for SourceCodeInfo.
/// Line comments have their leading space stripped. Block comments are returned as-is.
fn format_comment(text: &str, is_block: bool) -> String {
    if is_block {
        // Block comment: text is between /* and */, return with leading space stripped
        let mut result = String::new();
        // Add leading space for consistency with protoc
        if let Some(stripped) = text.strip_prefix(' ') {
            result.push_str(stripped);
        } else {
            result.push_str(text);
        }
        result.push('\n');
        result
    } else {
        // Line comment: strip one leading space if present
        let trimmed = text.strip_prefix(' ').unwrap_or(text);
        format!("{}\n", trimmed)
    }
}

/// Parse and validate a field number from a token. Must fit in i32 and be > 0.
fn parse_field_number(tok: &Token) -> Result<i32, ParseError> {
    let val = parse_int(&tok.text).map_err(|e| ParseError {
        message: format!("invalid field number: {}", e),
        span: tok.span,
    })?;
    if val > i32::MAX as u64 {
        return Err(ParseError {
            message: format!("field number out of range: {}", tok.text),
            span: tok.span,
        });
    }
    Ok(val as i32)
}

fn parse_int(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('-') {
        // Negative numbers: parse as i64 then cast
        let abs = parse_int(rest)?;
        return Ok((-(abs as i64)) as u64);
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else if let Some(oct) = s
        .strip_prefix('0')
        .filter(|r| !r.is_empty() && !r.contains(['8', '9']))
    {
        u64::from_str_radix(oct, 8).map_err(|e| e.to_string())
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())
    }
}

/// Check if a string is a valid protobuf identifier ([a-zA-Z_][a-zA-Z0-9_]*).
fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Convert snake_case to camelCase (protobuf json_name default).
fn to_camel_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate the synthetic map entry message name: "FooEntry" for field "foo".
fn map_entry_name(field_name: &str) -> String {
    let mut name = String::with_capacity(field_name.len() + 5);
    let mut chars = field_name.chars();
    if let Some(c) = chars.next() {
        name.extend(c.to_uppercase());
    }
    let mut capitalize_next = false;
    for c in chars {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            name.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            name.push(c);
        }
    }
    name.push_str("Entry");
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_proto3() {
        let source = r#"syntax = "proto3";"#;
        let file = parse(source).unwrap();
        assert_eq!(file.syntax, Some("proto3".to_string()));
        assert_eq!(file.syntax_enum(), Syntax::Proto3);
    }

    #[test]
    fn test_parse_minimal_proto2() {
        let source = r#"syntax = "proto2";"#;
        let file = parse(source).unwrap();
        assert_eq!(file.syntax, Some("proto2".to_string()));
        assert_eq!(file.syntax_enum(), Syntax::Proto2);
    }

    #[test]
    fn test_parse_package() {
        let source = r#"
            syntax = "proto3";
            package example.foo;
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.package, Some("example.foo".to_string()));
    }

    #[test]
    fn test_parse_import() {
        let source = r#"
            syntax = "proto3";
            import "other.proto";
            import public "public_dep.proto";
            import weak "weak_dep.proto";
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.dependency.len(), 3);
        assert_eq!(file.dependency[0], "other.proto");
        assert_eq!(file.dependency[1], "public_dep.proto");
        assert_eq!(file.dependency[2], "weak_dep.proto");
        assert_eq!(file.public_dependency, vec![1]);
        assert_eq!(file.weak_dependency, vec![2]);
    }

    #[test]
    fn test_parse_import_option_editions() {
        let source = r#"
            edition = "2024";
            import option "unloaded.proto";
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.dependency.len(), 0);
        assert_eq!(file.option_dependency.len(), 1);
        assert_eq!(file.option_dependency[0], "unloaded.proto");
    }

    #[test]
    fn test_parse_import_option_not_editions_error() {
        let source = r#"
            syntax = "proto3";
            import option "unloaded.proto";
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.contains("not supported before edition 2024"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_parse_simple_message() {
        let source = r#"
            syntax = "proto3";

            message Person {
                string name = 1;
                int32 age = 2;
                repeated string emails = 3;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.message_type.len(), 1);
        let msg = &file.message_type[0];
        assert_eq!(msg.name, Some("Person".to_string()));
        assert_eq!(msg.field.len(), 3);

        let name_field = &msg.field[0];
        assert_eq!(name_field.name, Some("name".to_string()));
        assert_eq!(name_field.r#type, Some(FieldType::String));
        assert_eq!(name_field.number, Some(1));

        let age_field = &msg.field[1];
        assert_eq!(age_field.name, Some("age".to_string()));
        assert_eq!(age_field.r#type, Some(FieldType::Int32));
        assert_eq!(age_field.number, Some(2));

        let emails_field = &msg.field[2];
        assert_eq!(emails_field.name, Some("emails".to_string()));
        assert_eq!(emails_field.r#type, Some(FieldType::String));
        assert_eq!(emails_field.label, Some(FieldLabel::Repeated));
        assert_eq!(emails_field.number, Some(3));
    }

    #[test]
    fn test_parse_nested_message() {
        let source = r#"
            syntax = "proto3";

            message Outer {
                message Inner {
                    int32 x = 1;
                }
                Inner inner = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let outer = &file.message_type[0];
        assert_eq!(outer.nested_type.len(), 1);
        assert_eq!(outer.nested_type[0].name, Some("Inner".to_string()));
        assert_eq!(outer.field.len(), 1);
        assert_eq!(outer.field[0].type_name, Some("Inner".to_string()));
    }

    #[test]
    fn test_parse_enum() {
        let source = r#"
            syntax = "proto3";

            enum Status {
                UNKNOWN = 0;
                ACTIVE = 1;
                INACTIVE = 2;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.enum_type.len(), 1);
        let e = &file.enum_type[0];
        assert_eq!(e.name, Some("Status".to_string()));
        assert_eq!(e.value.len(), 3);
        assert_eq!(e.value[0].name, Some("UNKNOWN".to_string()));
        assert_eq!(e.value[0].number, Some(0));
        assert_eq!(e.value[1].name, Some("ACTIVE".to_string()));
        assert_eq!(e.value[1].number, Some(1));
    }

    #[test]
    fn test_parse_oneof() {
        let source = r#"
            syntax = "proto3";

            message Msg {
                oneof test_oneof {
                    string name = 1;
                    int32 id = 2;
                }
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.oneof_decl.len(), 1);
        assert_eq!(msg.oneof_decl[0].name, Some("test_oneof".to_string()));
        assert_eq!(msg.field.len(), 2);
        assert_eq!(msg.field[0].oneof_index, Some(0));
        assert_eq!(msg.field[1].oneof_index, Some(0));
    }

    #[test]
    fn test_parse_map() {
        let source = r#"
            syntax = "proto3";

            message Msg {
                map<string, int32> tags = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.field.len(), 1);
        assert_eq!(msg.field[0].name, Some("tags".to_string()));
        assert_eq!(msg.field[0].label, Some(FieldLabel::Repeated));
        assert_eq!(msg.field[0].r#type, Some(FieldType::Message));

        // Synthetic nested message
        assert_eq!(msg.nested_type.len(), 1);
        let entry = &msg.nested_type[0];
        assert_eq!(entry.name, Some("TagsEntry".to_string()));
        assert_eq!(entry.options.as_ref().unwrap().map_entry, Some(true));
        assert_eq!(entry.field.len(), 2);
        assert_eq!(entry.field[0].name, Some("key".to_string()));
        assert_eq!(entry.field[1].name, Some("value".to_string()));
    }

    #[test]
    fn test_parse_service() {
        let source = r#"
            syntax = "proto3";

            service Greeter {
                rpc SayHello (HelloRequest) returns (HelloReply);
                rpc StreamHello (stream HelloRequest) returns (stream HelloReply) {}
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.service.len(), 1);
        let svc = &file.service[0];
        assert_eq!(svc.name, Some("Greeter".to_string()));
        assert_eq!(svc.method.len(), 2);

        let m1 = &svc.method[0];
        assert_eq!(m1.name, Some("SayHello".to_string()));
        assert_eq!(m1.input_type, Some("HelloRequest".to_string()));
        assert_eq!(m1.output_type, Some("HelloReply".to_string()));
        assert_eq!(m1.client_streaming, Some(false));
        assert_eq!(m1.server_streaming, Some(false));

        let m2 = &svc.method[1];
        assert_eq!(m2.client_streaming, Some(true));
        assert_eq!(m2.server_streaming, Some(true));
    }

    #[test]
    fn test_parse_reserved() {
        let source = r#"
            syntax = "proto3";

            message Msg {
                reserved 2, 15, 9 to 11;
                reserved "foo", "bar";
                int32 x = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.reserved_range.len(), 3);
        assert_eq!(msg.reserved_range[0].start, Some(2));
        assert_eq!(msg.reserved_range[0].end, Some(3)); // exclusive
        assert_eq!(msg.reserved_range[1].start, Some(15));
        assert_eq!(msg.reserved_range[2].start, Some(9));
        assert_eq!(msg.reserved_range[2].end, Some(12)); // exclusive
        assert_eq!(msg.reserved_name, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_field_options() {
        let source = r#"
            syntax = "proto2";

            message Msg {
                optional int32 old_field = 1 [deprecated = true];
                repeated int32 values = 2 [packed = true];
                optional string name = 3 [default = "hello"];
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];

        let f1 = &msg.field[0];
        assert_eq!(f1.options.as_ref().unwrap().deprecated, Some(true));

        let f2 = &msg.field[1];
        assert_eq!(f2.options.as_ref().unwrap().packed, Some(true));

        let f3 = &msg.field[2];
        assert_eq!(f3.default_value, Some("hello".to_string()));
    }

    #[test]
    fn test_parse_file_options() {
        let source = r#"
            syntax = "proto3";
            option java_package = "com.example";
            option java_multiple_files = true;
            option optimize_for = SPEED;
            option go_package = "example.com/pkg";
        "#;
        let file = parse(source).unwrap();
        let opts = file.options.as_ref().unwrap();
        assert_eq!(opts.java_package, Some("com.example".to_string()));
        assert_eq!(opts.java_multiple_files, Some(true));
        assert_eq!(opts.optimize_for, Some(OptimizeMode::Speed));
        assert_eq!(opts.go_package, Some("example.com/pkg".to_string()));
    }

    #[test]
    fn test_parse_extensions() {
        let source = r#"
            syntax = "proto2";

            message Msg {
                extensions 100 to 199;
                extensions 1000 to max;
                optional int32 x = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.extension_range.len(), 2);
        assert_eq!(msg.extension_range[0].start, Some(100));
        assert_eq!(msg.extension_range[0].end, Some(200));
        assert_eq!(msg.extension_range[1].start, Some(1000));
    }

    #[test]
    fn test_parse_extend() {
        let source = r#"
            syntax = "proto2";

            message Foo {
                extensions 100 to 200;
            }

            extend Foo {
                optional int32 bar = 100;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.extension.len(), 1);
        assert_eq!(file.extension[0].name, Some("bar".to_string()));
        assert_eq!(file.extension[0].extendee, Some("Foo".to_string()));
        assert_eq!(file.extension[0].number, Some(100));
    }

    #[test]
    fn test_json_name_default() {
        let source = r#"
            syntax = "proto3";
            message Msg {
                int32 my_field_name = 1;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(
            file.message_type[0].field[0].json_name,
            Some("myFieldName".to_string())
        );
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("my_field"), "myField");
        assert_eq!(to_camel_case("some_long_name"), "someLongName");
        assert_eq!(to_camel_case("already"), "already");
        assert_eq!(to_camel_case("a_b_c"), "aBC");
    }

    #[test]
    fn test_parse_enum_with_options() {
        let source = r#"
            syntax = "proto3";

            enum Corpus {
                option allow_alias = true;
                CORPUS_UNSPECIFIED = 0;
                CORPUS_UNIVERSAL = 1;
                CORPUS_WEB = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let e = &file.enum_type[0];
        assert_eq!(e.options.as_ref().unwrap().allow_alias, Some(true));
        assert_eq!(e.value.len(), 3);
    }

    #[test]
    fn test_parse_enum_reserved() {
        let source = r#"
            syntax = "proto3";

            enum Foo {
                reserved 2, 15, 9 to 11;
                reserved "FOO", "BAR";
                UNSPECIFIED = 0;
            }
        "#;
        let file = parse(source).unwrap();
        let e = &file.enum_type[0];
        assert_eq!(e.reserved_range.len(), 3);
        assert_eq!(e.reserved_name, vec!["FOO", "BAR"]);
    }

    #[test]
    fn test_parse_comprehensive() {
        let source = r#"
            syntax = "proto3";

            package example.v1;

            import "google/protobuf/timestamp.proto";

            option java_package = "com.example.v1";

            enum Status {
                STATUS_UNSPECIFIED = 0;
                STATUS_ACTIVE = 1;
                STATUS_INACTIVE = 2;
            }

            message User {
                string id = 1;
                string name = 2;
                string email = 3;
                Status status = 4;
                google.protobuf.Timestamp created_at = 5;
                repeated string tags = 6;
                map<string, string> metadata = 7;

                message Address {
                    string street = 1;
                    string city = 2;
                    string country = 3;
                }

                repeated Address addresses = 8;

                oneof contact {
                    string phone = 9;
                    string fax = 10;
                }
            }

            service UserService {
                rpc GetUser (GetUserRequest) returns (User);
                rpc ListUsers (ListUsersRequest) returns (stream User);
            }

            message GetUserRequest {
                string id = 1;
            }

            message ListUsersRequest {
                int32 page_size = 1;
                string page_token = 2;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.syntax, Some("proto3".to_string()));
        assert_eq!(file.package, Some("example.v1".to_string()));
        assert_eq!(file.dependency.len(), 1);
        assert_eq!(file.enum_type.len(), 1);
        assert_eq!(file.message_type.len(), 3); // User, GetUserRequest, ListUsersRequest
        assert_eq!(file.service.len(), 1);

        let user = &file.message_type[0];
        assert_eq!(user.field.len(), 10); // 8 regular + 2 oneof
        assert_eq!(user.nested_type.len(), 2); // Address + MetadataEntry (map)
        assert_eq!(user.oneof_decl.len(), 1);
    }

    // =======================================================================
    // ParseMessageTest parity (C++ parser_unittest.cc)
    // =======================================================================

    #[test]
    fn cpp_ignore_bom() {
        // UTF-8 BOM (\xEF\xBB\xBF) at start should be silently skipped
        let mut input = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(
            b"syntax = \"proto2\";\nmessage Foo {\n  required int32 bar = 1;\n}\n",
        );
        let source = String::from_utf8(input).unwrap();
        let file = parse(&source).unwrap();
        assert_eq!(file.message_type.len(), 1);
        assert_eq!(file.message_type[0].name.as_deref(), Some("Foo"));
    }

    #[test]
    fn cpp_bom_error() {
        // 0xEF without full BOM should error
        let mut input = vec![0xEF];
        input.extend_from_slice(b" message Foo {}\n");
        let source = String::from_utf8_lossy(&input).to_string();
        let err = parse(&source).unwrap_err();
        assert!(
            err.message.contains("0xEF") && err.message.contains("BOM"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_parse_group() {
        // Group field creates nested type + field with TYPE_GROUP
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                optional group TestGroup = 1 {}
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.nested_type.len(), 1);
        assert_eq!(msg.nested_type[0].name.as_deref(), Some("TestGroup"));
        assert_eq!(msg.field.len(), 1);
        let f = &msg.field[0];
        assert_eq!(f.name.as_deref(), Some("testgroup"));
        assert_eq!(f.r#type, Some(FieldType::Group));
        assert_eq!(f.type_name.as_deref(), Some("TestGroup"));
        assert_eq!(f.number, Some(1));
        assert_eq!(f.label, Some(FieldLabel::Optional));
    }

    #[test]
    fn cpp_reserved_range_on_message_set() {
        // reserved with max in message with message_set_wire_format option
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                option message_set_wire_format = true;
                reserved 20 to max;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.reserved_range.len(), 1);
        assert_eq!(msg.reserved_range[0].start, Some(20));
        // "max" maps to MAX_FIELD_NUMBER (536870911), end is exclusive (+1 = 536870912)
        assert_eq!(msg.reserved_range[0].end, Some(536870912));
        assert!(msg.options.is_some());
    }

    #[test]
    fn cpp_max_int_extension_does_not_overflow() {
        // extensions 2147483647 should error (out of bounds)
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                extensions 2147483647;
            }
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.to_lowercase().contains("out of") || err.message.contains("bound"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_max_int_extension_range_overflow() {
        // extensions 1 to 2147483647 should error
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                extensions 1 to 2147483647;
            }
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.to_lowercase().contains("out of") || err.message.contains("bound"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_larger_max_for_message_set() {
        // message_set_wire_format allows extensions 4 to max
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                extensions 4 to max;
                option message_set_wire_format = true;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.extension_range.len(), 1);
        assert_eq!(msg.extension_range[0].start, Some(4));
        // "max" maps to 0x7fffffff = 2147483647
        assert!(msg.extension_range[0].end.unwrap() >= 536870912);
    }

    #[test]
    fn cpp_field_options_large_decimal() {
        // Large decimal literal > uint64 max should parse as double default
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                optional double a = 1 [default = 18446744073709551616];
                optional double b = 2 [default = -18446744073709551616];
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert!(msg.field[0].default_value.is_some());
        assert!(msg.field[1].default_value.is_some());
    }

    #[test]
    fn cpp_field_options_inf_nan() {
        // inf and nan as option values should parse as identifiers
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                optional double a = 1 [default = inf];
                optional double b = 2 [default = -inf];
                optional double c = 3 [default = nan];
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.field[0].default_value.as_deref(), Some("inf"));
        assert_eq!(msg.field[1].default_value.as_deref(), Some("-inf"));
        assert_eq!(msg.field[2].default_value.as_deref(), Some("nan"));
    }

    #[test]
    fn cpp_multiple_extensions_one_extendee() {
        // Multiple extension fields in one extend block
        let source = r#"
            syntax = "proto2";
            extend Extendee1 {
                optional int32 foo = 12;
                repeated string bar = 22;
            }
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.extension.len(), 2);
        assert_eq!(file.extension[0].extendee.as_deref(), Some("Extendee1"));
        assert_eq!(file.extension[1].extendee.as_deref(), Some("Extendee1"));
        assert_eq!(file.extension[0].name.as_deref(), Some("foo"));
        assert_eq!(file.extension[1].name.as_deref(), Some("bar"));
    }

    #[test]
    fn cpp_extensions_in_message_scope() {
        // extend inside a message body
        let source = r#"
            syntax = "proto2";
            message TestMessage {
                extend Extendee1 { optional int32 foo = 12; }
                extend Extendee2 { repeated int32 bar = 22; }
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.extension.len(), 2);
        assert_eq!(msg.extension[0].extendee.as_deref(), Some("Extendee1"));
        assert_eq!(msg.extension[1].extendee.as_deref(), Some("Extendee2"));
    }

    #[test]
    fn cpp_explicit_optional_label_proto3() {
        // proto3 explicit optional creates synthetic oneof
        let source = r#"
            syntax = "proto3";
            message TestMessage {
                optional int32 foo = 1;
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        assert_eq!(msg.field[0].proto3_optional, Some(true));
        assert_eq!(msg.field[0].oneof_index, Some(0));
        assert_eq!(msg.oneof_decl.len(), 1);
        assert_eq!(msg.oneof_decl[0].name.as_deref(), Some("_foo"));
    }

    #[test]
    fn cpp_explicit_optional_proto3_collision() {
        // Synthetic oneof name collision with existing oneof
        let source = r#"
            syntax = "proto3";
            message TestMessage {
                optional int32 foo = 1;
                oneof _foo {
                    int32 bar = 2;
                }
            }
        "#;
        let file = parse(source).unwrap();
        let msg = &file.message_type[0];
        // _foo is taken, so synthetic oneof should be X_foo
        assert_eq!(msg.oneof_decl.len(), 2);
        // The real oneof comes first
        let oneof_names: Vec<_> = msg.oneof_decl.iter().map(|o| o.name.as_deref()).collect();
        assert!(
            oneof_names.contains(&Some("_foo")),
            "missing _foo: {:?}",
            oneof_names
        );
    }

    #[test]
    fn cpp_can_handle_error_on_first_token() {
        // Invalid first token should produce a clear error
        let source = "/";
        let err = parse(source).unwrap_err();
        assert!(
            err.message.contains("Expected")
                || err.message.contains("expected")
                || err.message.contains("unexpected"),
            "got: {}",
            err.message
        );
    }

    // =======================================================================
    // ParseImportTest parity
    // =======================================================================

    #[test]
    fn cpp_option_import_before_import_bad_order() {
        // option import before regular import should error
        let source = r#"
            edition = "2024";
            import option "baz.proto";
            import "foo.proto";
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.contains("imports should precede"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_option_import_before_public_import_bad_order() {
        // option import before public import should error
        let source = r#"
            edition = "2024";
            import option "baz.proto";
            import public "bar.proto";
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.contains("imports should precede"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_option_import_before_multiple_imports_bad_order() {
        // Multiple regular imports after option import: each should error
        // (we stop at first error, so just check the first one)
        let source = r#"
            edition = "2024";
            import option "baz.proto";
            import "foo.proto";
        "#;
        let err = parse(source).unwrap_err();
        assert!(
            err.message.contains("imports should precede"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn cpp_option_and_public_imports() {
        // Mixing public and option imports in proper order
        let source = r#"
            edition = "2024";
            import "foo.proto";
            import public "bar.proto";
            import option "baz.proto";
            import option "qux.proto";
        "#;
        let file = parse(source).unwrap();
        assert_eq!(file.dependency, vec!["foo.proto", "bar.proto"]);
        assert_eq!(file.public_dependency, vec![1]);
        assert_eq!(file.option_dependency, vec!["baz.proto", "qux.proto"]);
    }

    // =======================================================================
    // ParseMiscTest parity
    // =======================================================================

    #[test]
    fn cpp_parse_package_with_spaces() {
        // Package name with whitespace between dot-separated identifiers
        let source = "syntax = \"proto2\";\npackage foo   .   bar.\n  baz;\n";
        let file = parse(source).unwrap();
        assert_eq!(file.package.as_deref(), Some("foo.bar.baz"));
    }

    // =======================================================================
    // SourceInfoTest parity (C++ parser_unittest.cc)
    // =======================================================================

    /// Helper: find a SourceLocation by path in SourceCodeInfo.
    fn find_location(file: &FileDescriptorProto, path: &[i32]) -> Option<Vec<i32>> {
        file.source_code_info
            .as_ref()?
            .location
            .iter()
            .find(|loc| loc.path == path)
            .map(|loc| loc.span.clone())
    }

    fn find_location_full<'a>(
        file: &'a FileDescriptorProto,
        path: &[i32],
    ) -> Option<&'a protoc_rs_schema::SourceLocation> {
        file.source_code_info
            .as_ref()?
            .location
            .iter()
            .find(|loc| loc.path == path)
    }

    #[test]
    fn source_info_basic_file_decls() {
        let source = concat!(
            "syntax = \"proto2\";\n",  // line 0 (0-based)
            "package foo.bar;\n",      // line 1
            "import \"baz.proto\";\n", // line 2
            "import \"qux.proto\";\n", // line 3
        );
        let file = parse(source).unwrap();
        let sci = file
            .source_code_info
            .as_ref()
            .expect("source_code_info should be populated");
        assert!(!sci.location.is_empty(), "should have locations");

        // syntax statement: path=[12], spans line 0
        let span = find_location(&file, &[12]).expect("syntax location");
        assert_eq!(span[0], 0, "syntax start line"); // line 0

        // package statement: path=[2], spans line 1
        let span = find_location(&file, &[2]).expect("package location");
        assert_eq!(span[0], 1, "package start line"); // line 1

        // dependency[0]: path=[3, 0], spans line 2
        let span = find_location(&file, &[3, 0]).expect("dependency[0] location");
        assert_eq!(span[0], 2, "dep[0] start line");

        // dependency[1]: path=[3, 1], spans line 3
        let span = find_location(&file, &[3, 1]).expect("dependency[1] location");
        assert_eq!(span[0], 3, "dep[1] start line");
    }

    #[test]
    fn source_info_messages() {
        let source = concat!(
            "syntax = \"proto3\";\n", // line 0
            "message Foo {}\n",       // line 1
            "message Bar {}\n",       // line 2
        );
        let file = parse(source).unwrap();

        // message_type[0]: path=[4, 0]
        let span = find_location(&file, &[4, 0]).expect("message[0] location");
        assert_eq!(span[0], 1, "message[0] start line");

        // message_type[1]: path=[4, 1]
        let span = find_location(&file, &[4, 1]).expect("message[1] location");
        assert_eq!(span[0], 2, "message[1] start line");
    }

    #[test]
    fn source_info_enums() {
        let source = concat!(
            "syntax = \"proto3\";\n", // line 0
            "enum Color {\n",         // line 1
            "  RED = 0;\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // enum_type[0]: path=[5, 0]
        let span = find_location(&file, &[5, 0]).expect("enum[0] location");
        assert_eq!(span[0], 1, "enum[0] start line");
    }

    #[test]
    fn source_info_services() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Req {}\n",
            "message Resp {}\n",
            "service MyService {\n", // line 3
            "  rpc DoThing(Req) returns (Resp);\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // service[0]: path=[6, 0]
        let span = find_location(&file, &[6, 0]).expect("service[0] location");
        assert_eq!(span[0], 3, "service[0] start line");
    }

    #[test]
    fn source_info_imports_with_public() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "import \"foo.proto\";\n",        // line 1: dependency[0]
            "import public \"bar.proto\";\n", // line 2: dependency[1], public_dependency[0]
        );
        let file = parse(source).unwrap();

        // dependency[0]: path=[3, 0]
        assert!(find_location(&file, &[3, 0]).is_some(), "dependency[0]");

        // dependency[1]: path=[3, 1]
        assert!(find_location(&file, &[3, 1]).is_some(), "dependency[1]");

        // public_dependency[0]: path=[10, 0]
        assert!(
            find_location(&file, &[10, 0]).is_some(),
            "public_dependency[0]"
        );
    }

    #[test]
    fn source_info_populated_after_parse() {
        // Verify source_code_info is always populated (even for empty files)
        let source = "syntax = \"proto3\";\n";
        let file = parse(source).unwrap();
        assert!(file.source_code_info.is_some());
        let sci = file.source_code_info.unwrap();
        // At minimum, we should have the syntax location
        assert!(!sci.location.is_empty());
    }

    #[test]
    fn source_info_message_name() {
        // path [4, 0, 1] = FILE_MESSAGE_TYPE[0].MSG_NAME
        let source = "syntax = \"proto3\";\nmessage Foo {}\n";
        let file = parse(source).unwrap();
        let span = find_location(&file, &[4, 0, 1]).expect("message name location");
        assert_eq!(span[0], 1, "name on line 1");
        // "Foo" starts at col 8
        assert_eq!(span[1], 8, "name start col");
    }

    #[test]
    fn source_info_message_fields() {
        // Tests field container span and field sub-elements
        let source = concat!(
            "syntax = \"proto3\";\n", // line 0
            "message Foo {\n",        // line 1
            "  int32 bar = 1;\n",     // line 2
            "  string baz = 2;\n",    // line 3
            "}\n",
        );
        let file = parse(source).unwrap();

        // field[0] container: path=[4, 0, 2, 0]
        let span = find_location(&file, &[4, 0, 2, 0]).expect("field[0] location");
        assert_eq!(span[0], 2, "field[0] on line 2");

        // field[0] type: path=[4, 0, 2, 0, 5] (FIELD_TYPE=5 for scalar)
        let span = find_location(&file, &[4, 0, 2, 0, 5]).expect("field[0] type");
        assert_eq!(span[0], 2, "type on line 2");
        assert_eq!(span[1], 2, "type start col"); // "  int32" -> col 2

        // field[0] name: path=[4, 0, 2, 0, 1] (FIELD_NAME=1)
        let span = find_location(&file, &[4, 0, 2, 0, 1]).expect("field[0] name");
        assert_eq!(span[0], 2, "name on line 2");
        assert_eq!(span[1], 8, "name start col"); // "  int32 bar" -> col 8

        // field[0] number: path=[4, 0, 2, 0, 3] (FIELD_NUMBER=3)
        let span = find_location(&file, &[4, 0, 2, 0, 3]).expect("field[0] number");
        assert_eq!(span[0], 2, "number on line 2");

        // field[1] container: path=[4, 0, 2, 1]
        let span = find_location(&file, &[4, 0, 2, 1]).expect("field[1] location");
        assert_eq!(span[0], 3, "field[1] on line 3");
    }

    #[test]
    fn source_info_field_with_label() {
        let source = concat!(
            "syntax = \"proto2\";\n",     // line 0
            "message Foo {\n",            // line 1
            "  optional int32 x = 1;\n",  // line 2
            "  repeated string y = 2;\n", // line 3
            "}\n",
        );
        let file = parse(source).unwrap();

        // field[0] label: path=[4, 0, 2, 0, 4] (FIELD_LABEL=4)
        let span = find_location(&file, &[4, 0, 2, 0, 4]).expect("field[0] label");
        assert_eq!(span[0], 2, "label on line 2");
        assert_eq!(span[1], 2, "label start col"); // "  optional" -> col 2

        // field[1] label
        let span = find_location(&file, &[4, 0, 2, 1, 4]).expect("field[1] label");
        assert_eq!(span[0], 3, "field[1] label on line 3");
    }

    #[test]
    fn source_info_field_message_type() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Bar {}\n",
            "message Foo {\n", // line 2
            "  Bar b = 1;\n",  // line 3
            "}\n",
        );
        let file = parse(source).unwrap();

        // field type_name: path=[4, 1, 2, 0, 6] (FIELD_TYPE_NAME=6 for message ref)
        let span = find_location(&file, &[4, 1, 2, 0, 6]).expect("field type_name");
        assert_eq!(span[0], 3, "type_name on line 3");
        assert_eq!(span[1], 2, "type_name start col"); // "  Bar" -> col 2
    }

    #[test]
    fn source_info_group_sub_elements() {
        let source = concat!(
            "syntax = \"proto2\";\n",
            "message Foo {\n",               // line 1
            "  optional group Bar = 1 {}\n", // line 2
            "}\n",
        );
        let file = parse(source).unwrap();

        // field[0] label: path=[4, 0, 2, 0, 4]
        let span = find_location(&file, &[4, 0, 2, 0, 4]).expect("group label");
        assert_eq!(span[0], 2, "group label on line 2");

        // field[0] type (group keyword): path=[4, 0, 2, 0, 5]
        let span = find_location(&file, &[4, 0, 2, 0, 5]).expect("group type");
        assert_eq!(span[0], 2, "group type on line 2");

        // field[0] name: path=[4, 0, 2, 0, 1]
        let span = find_location(&file, &[4, 0, 2, 0, 1]).expect("group name");
        assert_eq!(span[0], 2, "group name on line 2");

        // field[0] number: path=[4, 0, 2, 0, 3]
        let span = find_location(&file, &[4, 0, 2, 0, 3]).expect("group number");
        assert_eq!(span[0], 2, "group number on line 2");
    }

    #[test]
    fn source_info_nested_message() {
        // path [4, 0, 3, 0] = FILE_MESSAGE_TYPE[0].MSG_NESTED_TYPE[0]
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Outer {\n",    // line 1
            "  message Inner {}\n", // line 2
            "}\n",
        );
        let file = parse(source).unwrap();

        // nested_type[0]: path=[4, 0, 3, 0]
        let span = find_location(&file, &[4, 0, 3, 0]).expect("nested message location");
        assert_eq!(span[0], 2, "nested message on line 2");

        // nested message name: path=[4, 0, 3, 0, 1]
        let span = find_location(&file, &[4, 0, 3, 0, 1]).expect("nested message name");
        assert_eq!(span[0], 2, "nested name on line 2");
    }

    #[test]
    fn source_info_enum_name_and_values() {
        let source = concat!(
            "syntax = \"proto3\";\n", // line 0
            "enum Color {\n",         // line 1
            "  RED = 0;\n",           // line 2
            "  GREEN = 1;\n",         // line 3
            "}\n",
        );
        let file = parse(source).unwrap();

        // enum name: path=[5, 0, 1]
        let span = find_location(&file, &[5, 0, 1]).expect("enum name location");
        assert_eq!(span[0], 1, "enum name on line 1");

        // enum value[0]: path=[5, 0, 2, 0]
        let span = find_location(&file, &[5, 0, 2, 0]).expect("enum value[0] location");
        assert_eq!(span[0], 2, "value[0] on line 2");

        // enum value[0] name: path=[5, 0, 2, 0, 1]
        let span = find_location(&file, &[5, 0, 2, 0, 1]).expect("enum value[0] name");
        assert_eq!(span[0], 2, "value name on line 2");

        // enum value[0] number: path=[5, 0, 2, 0, 2]
        let span = find_location(&file, &[5, 0, 2, 0, 2]).expect("enum value[0] number");
        assert_eq!(span[0], 2, "value number on line 2");

        // enum value[1]: path=[5, 0, 2, 1]
        let span = find_location(&file, &[5, 0, 2, 1]).expect("enum value[1] location");
        assert_eq!(span[0], 3, "value[1] on line 3");
    }

    #[test]
    fn source_info_service_name_and_methods() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Req {}\n",
            "message Resp {}\n",
            "service Svc {\n",                 // line 3
            "  rpc Do(Req) returns (Resp);\n", // line 4
            "}\n",
        );
        let file = parse(source).unwrap();

        // service name: path=[6, 0, 1]
        let span = find_location(&file, &[6, 0, 1]).expect("service name location");
        assert_eq!(span[0], 3, "service name on line 3");

        // method[0]: path=[6, 0, 2, 0]
        let span = find_location(&file, &[6, 0, 2, 0]).expect("method[0] location");
        assert_eq!(span[0], 4, "method on line 4");

        // method name: path=[6, 0, 2, 0, 1]
        let span = find_location(&file, &[6, 0, 2, 0, 1]).expect("method name location");
        assert_eq!(span[0], 4, "method name on line 4");

        // method input_type: path=[6, 0, 2, 0, 2]
        let span = find_location(&file, &[6, 0, 2, 0, 2]).expect("method input_type location");
        assert_eq!(span[0], 4, "input_type on line 4");

        // method output_type: path=[6, 0, 2, 0, 3]
        let span = find_location(&file, &[6, 0, 2, 0, 3]).expect("method output_type location");
        assert_eq!(span[0], 4, "output_type on line 4");
    }

    #[test]
    fn source_info_oneof() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Foo {\n",     // line 1
            "  oneof bar {\n",     // line 2
            "    int32 x = 1;\n",  // line 3
            "    string y = 2;\n", // line 4
            "  }\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // oneof[0]: path=[4, 0, 8, 0]
        let span = find_location(&file, &[4, 0, 8, 0]).expect("oneof[0] location");
        assert_eq!(span[0], 2, "oneof on line 2");

        // oneof name: path=[4, 0, 8, 0, 1]
        let span = find_location(&file, &[4, 0, 8, 0, 1]).expect("oneof name location");
        assert_eq!(span[0], 2, "oneof name on line 2");
    }

    #[test]
    fn source_info_nested_enum_in_message() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Foo {\n", // line 1
            "  enum Bar {\n",  // line 2
            "    X = 0;\n",    // line 3
            "  }\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // nested enum: path=[4, 0, 4, 0]
        let span = find_location(&file, &[4, 0, 4, 0]).expect("nested enum location");
        assert_eq!(span[0], 2, "nested enum on line 2");

        // nested enum name: path=[4, 0, 4, 0, 1]
        let span = find_location(&file, &[4, 0, 4, 0, 1]).expect("nested enum name");
        assert_eq!(span[0], 2, "nested enum name on line 2");

        // nested enum value[0]: path=[4, 0, 4, 0, 2, 0]
        let span = find_location(&file, &[4, 0, 4, 0, 2, 0]).expect("nested enum value");
        assert_eq!(span[0], 3, "nested enum value on line 3");
    }

    #[test]
    fn source_info_reserved_and_extensions() {
        let source = concat!(
            "syntax = \"proto2\";\n",
            "message Foo {\n",            // line 1
            "  reserved 1, 2 to 5;\n",    // line 2
            "  extensions 100 to 200;\n", // line 3
            "  optional int32 x = 10;\n", // line 4
            "}\n",
        );
        let file = parse(source).unwrap();

        // reserved_range[0]: path=[4, 0, 9, 0]
        let span = find_location(&file, &[4, 0, 9, 0]).expect("reserved_range[0]");
        assert_eq!(span[0], 2, "reserved on line 2");

        // reserved_range[1]: path=[4, 0, 9, 1]
        let span = find_location(&file, &[4, 0, 9, 1]).expect("reserved_range[1]");
        assert_eq!(span[0], 2, "reserved range 2 on line 2");

        // extension_range[0]: path=[4, 0, 5, 0]
        let span = find_location(&file, &[4, 0, 5, 0]).expect("extension_range[0]");
        assert_eq!(span[0], 3, "extension range on line 3");
    }

    #[test]
    fn source_info_map_field() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Foo {\n",
            "  map<string, int32> tags = 1;\n", // line 2
            "}\n",
        );
        let file = parse(source).unwrap();

        // map field: path=[4, 0, 2, 0]
        let span = find_location(&file, &[4, 0, 2, 0]).expect("map field location");
        assert_eq!(span[0], 2, "map field on line 2");
    }

    #[test]
    fn source_info_file_options() {
        let source = concat!(
            "syntax = \"proto2\";\n",
            "option java_package = \"com.example\";\n", // line 1
        );
        let file = parse(source).unwrap();

        // file options: path=[8]
        let span = find_location(&file, &[8]).expect("file options location");
        assert_eq!(span[0], 1, "file option on line 1");
    }

    #[test]
    fn source_info_file_extensions() {
        let source = concat!(
            "syntax = \"proto2\";\n",
            "message Foo {\n",
            "  extensions 100 to 200;\n",
            "  optional int32 x = 1;\n",
            "}\n",
            "extend Foo {\n",                // line 5
            "  optional int32 bar = 100;\n", // line 6 - the actual extension field
            "}\n",
        );
        let file = parse(source).unwrap();

        // file extension[0]: path=[7, 0] - span covers the field, not the extend keyword
        let span = find_location(&file, &[7, 0]).expect("file extension[0]");
        assert_eq!(span[0], 6, "extension field on line 6");

        // extension field extendee: path=[7, 0, 2] (FIELD_EXTENDEE)
        let span = find_location(&file, &[7, 0, 2]).expect("extension extendee");
        assert_eq!(span[0], 5, "extendee on line 5"); // "Foo" after "extend"

        // extension field label: path=[7, 0, 4]
        let span = find_location(&file, &[7, 0, 4]).expect("extension label");
        assert_eq!(span[0], 6, "extension label on line 6");

        // extension field name: path=[7, 0, 1]
        let span = find_location(&file, &[7, 0, 1]).expect("extension field name");
        assert_eq!(span[0], 6, "extension name on line 6");
    }

    #[test]
    fn source_info_enum_reserved() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "enum Foo {\n",          // line 1
            "  X = 0;\n",            // line 2
            "  reserved 1 to 5;\n",  // line 3
            "  reserved \"BAR\";\n", // line 4
            "}\n",
        );
        let file = parse(source).unwrap();

        // enum reserved_range[0]: path=[5, 0, 4, 0]
        let span = find_location(&file, &[5, 0, 4, 0]).expect("enum reserved_range[0]");
        assert_eq!(span[0], 3, "enum reserved range on line 3");

        // enum reserved_name[0]: path=[5, 0, 5, 0]
        let span = find_location(&file, &[5, 0, 5, 0]).expect("enum reserved_name[0]");
        assert_eq!(span[0], 4, "enum reserved name on line 4");
    }

    #[test]
    fn source_info_streaming_methods() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Req {}\n",
            "message Resp {}\n",
            "service Svc {\n",                                 // line 3
            "  rpc Bidi(stream Req) returns (stream Resp);\n", // line 4
            "}\n",
        );
        let file = parse(source).unwrap();

        // method[0] client_streaming: path=[6, 0, 2, 0, 5]
        let span = find_location(&file, &[6, 0, 2, 0, 5]).expect("client_streaming");
        assert_eq!(span[0], 4, "client streaming on line 4");

        // method[0] server_streaming: path=[6, 0, 2, 0, 6]
        let span = find_location(&file, &[6, 0, 2, 0, 6]).expect("server_streaming");
        assert_eq!(span[0], 4, "server streaming on line 4");
    }

    #[test]
    fn source_info_message_options() {
        let source = concat!(
            "syntax = \"proto2\";\n",
            "message Foo {\n",                            // line 1
            "  option message_set_wire_format = true;\n", // line 2
            "  optional int32 x = 1;\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // Verify the message has the option set
        assert!(file.message_type[0]
            .options
            .as_ref()
            .unwrap()
            .message_set_wire_format
            .is_some());
    }

    #[test]
    fn source_info_leading_comments() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "// Leading comment for Foo\n",
            "message Foo {\n",
            "  // Leading comment for bar\n",
            "  int32 bar = 1;\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // message[0]: path=[4, 0]
        let loc = find_location_full(&file, &[4, 0]).expect("message location");
        assert_eq!(
            loc.leading_comments.as_deref(),
            Some("Leading comment for Foo\n"),
            "message leading comment"
        );

        // field[0]: path=[4, 0, 2, 0]
        let loc = find_location_full(&file, &[4, 0, 2, 0]).expect("field location");
        assert_eq!(
            loc.leading_comments.as_deref(),
            Some("Leading comment for bar\n"),
            "field leading comment"
        );
    }

    #[test]
    fn source_info_trailing_comments() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "enum Color {\n",
            "  RED = 0; // The color red\n",
            "  GREEN = 1; // The color green\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // enum value[0]: path=[5, 0, 2, 0]
        let loc = find_location_full(&file, &[5, 0, 2, 0]).expect("value[0]");
        assert_eq!(
            loc.trailing_comments.as_deref(),
            Some("The color red\n"),
            "trailing comment on RED"
        );

        // enum value[1]: path=[5, 0, 2, 1]
        let loc = find_location_full(&file, &[5, 0, 2, 1]).expect("value[1]");
        assert_eq!(
            loc.trailing_comments.as_deref(),
            Some("The color green\n"),
            "trailing comment on GREEN"
        );
    }

    #[test]
    fn source_info_detached_comments() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "\n",
            "// Detached comment 1\n",
            "\n",
            "// Detached comment 2\n",
            "\n",
            "// Leading comment\n",
            "message Foo {}\n",
        );
        let file = parse(source).unwrap();

        // message[0]: path=[4, 0]
        let loc = find_location_full(&file, &[4, 0]).expect("message location");
        assert_eq!(
            loc.leading_comments.as_deref(),
            Some("Leading comment\n"),
            "leading comment"
        );
        assert_eq!(
            loc.leading_detached_comments.len(),
            2,
            "should have 2 detached comments"
        );
        assert_eq!(loc.leading_detached_comments[0], "Detached comment 1\n");
        assert_eq!(loc.leading_detached_comments[1], "Detached comment 2\n");
    }

    #[test]
    fn source_info_block_comment() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "/* Block leading */\n",
            "message Foo {}\n",
        );
        let file = parse(source).unwrap();

        let loc = find_location_full(&file, &[4, 0]).expect("message location");
        assert_eq!(
            loc.leading_comments.as_deref(),
            Some("Block leading \n"),
            "block comment as leading"
        );
    }

    #[test]
    fn source_info_method_comments() {
        let source = concat!(
            "syntax = \"proto3\";\n",
            "message Req {}\n",
            "message Resp {}\n",
            "service Svc {\n",
            "  // The main RPC\n",
            "  rpc Do(Req) returns (Resp);\n",
            "}\n",
        );
        let file = parse(source).unwrap();

        // method[0]: path=[6, 0, 2, 0]
        let loc = find_location_full(&file, &[6, 0, 2, 0]).expect("method location");
        assert_eq!(
            loc.leading_comments.as_deref(),
            Some("The main RPC\n"),
            "method leading comment"
        );
    }
}
