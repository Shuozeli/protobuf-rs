use protoc_rs_schema::{Position, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Literals
    Ident,
    IntLiteral,
    FloatLiteral,
    StringLiteral,

    // Keywords
    Syntax,
    Edition,
    Import,
    Weak,
    Public,
    Package,
    Option,
    Message,
    Enum,
    Oneof,
    Map,
    Repeated,
    Optional,
    Required,
    Reserved,
    Extensions,
    Extend,
    Service,
    Rpc,
    Returns,
    Stream,
    Group,
    To, // "to" in reserved ranges
    Max,

    // Scalar type keywords
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String_,
    Bytes,

    // Boolean literals
    True,
    False,

    // Constant keywords
    Inf,
    Nan,

    // Symbols
    LBrace,    // {
    RBrace,    // }
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    LAngle,    // <
    RAngle,    // >
    Semicolon, // ;
    Comma,     // ,
    Dot,       // .
    Equals,    // =
    Minus,     // -
    Plus,      // +
    Slash,     // /
    Colon,     // :

    // Comments (preserved for source info)
    LineComment,
    BlockComment,

    // End of file
    Eof,
}

impl TokenKind {
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(Self::Syntax),
            "edition" => Some(Self::Edition),
            "import" => Some(Self::Import),
            "weak" => Some(Self::Weak),
            "public" => Some(Self::Public),
            "package" => Some(Self::Package),
            "option" => Some(Self::Option),
            "message" => Some(Self::Message),
            "enum" => Some(Self::Enum),
            "oneof" => Some(Self::Oneof),
            "map" => Some(Self::Map),
            "repeated" => Some(Self::Repeated),
            "optional" => Some(Self::Optional),
            "required" => Some(Self::Required),
            "reserved" => Some(Self::Reserved),
            "extensions" => Some(Self::Extensions),
            "extend" => Some(Self::Extend),
            "service" => Some(Self::Service),
            "rpc" => Some(Self::Rpc),
            "returns" => Some(Self::Returns),
            "stream" => Some(Self::Stream),
            "group" => Some(Self::Group),
            "to" => Some(Self::To),
            "max" => Some(Self::Max),
            "double" => Some(Self::Double),
            "float" => Some(Self::Float),
            "int32" => Some(Self::Int32),
            "int64" => Some(Self::Int64),
            "uint32" => Some(Self::Uint32),
            "uint64" => Some(Self::Uint64),
            "sint32" => Some(Self::Sint32),
            "sint64" => Some(Self::Sint64),
            "fixed32" => Some(Self::Fixed32),
            "fixed64" => Some(Self::Fixed64),
            "sfixed32" => Some(Self::Sfixed32),
            "sfixed64" => Some(Self::Sfixed64),
            "bool" => Some(Self::Bool),
            "string" => Some(Self::String_),
            "bytes" => Some(Self::Bytes),
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            "inf" => Some(Self::Inf),
            "nan" => Some(Self::Nan),
            _ => None,
        }
    }

    pub fn is_type_keyword(&self) -> bool {
        matches!(
            self,
            Self::Double
                | Self::Float
                | Self::Int32
                | Self::Int64
                | Self::Uint32
                | Self::Uint64
                | Self::Sint32
                | Self::Sint64
                | Self::Fixed32
                | Self::Fixed64
                | Self::Sfixed32
                | Self::Sfixed64
                | Self::Bool
                | Self::String_
                | Self::Bytes
        )
    }
}

pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn current_position(&self) -> Position {
        Position::new(self.line, self.col, self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_line_comment(&mut self) -> Token {
        let start = self.current_position();
        // Skip the leading //
        self.advance();
        self.advance();
        let text_start = self.pos;
        while let Some(b) = self.peek() {
            if b == b'\n' {
                break;
            }
            self.advance();
        }
        let text = self.source[text_start..self.pos].to_string();
        Token {
            kind: TokenKind::LineComment,
            text,
            span: Span::new(start, self.current_position()),
        }
    }

    fn read_block_comment(&mut self) -> Result<Token, LexError> {
        let start = self.current_position();
        // Skip /*
        self.advance();
        self.advance();
        let text_start = self.pos;
        loop {
            match self.peek() {
                None => {
                    return Err(LexError {
                        message: "unterminated block comment".to_string(),
                        span: Span::new(start, self.current_position()),
                    });
                }
                Some(b'*') if self.peek_at(1) == Some(b'/') => {
                    let text = self.source[text_start..self.pos].to_string();
                    self.advance(); // *
                    self.advance(); // /
                    return Ok(Token {
                        kind: TokenKind::BlockComment,
                        text,
                        span: Span::new(start, self.current_position()),
                    });
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn read_string(&mut self) -> Result<Token, LexError> {
        let start = self.current_position();
        let quote = self.advance().unwrap(); // consume opening quote
        let mut value = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        span: Span::new(start, self.current_position()),
                    });
                }
                Some(b'\\') => {
                    self.advance(); // backslash
                    match self.advance() {
                        Some(b'a') => value.push('\x07'), // bell
                        Some(b'b') => value.push('\x08'), // backspace
                        Some(b'f') => value.push('\x0C'), // form feed
                        Some(b'n') => value.push('\n'),
                        Some(b'r') => value.push('\r'),
                        Some(b't') => value.push('\t'),
                        Some(b'v') => value.push('\x0B'), // vertical tab
                        Some(b'\\') => value.push('\\'),
                        Some(b'\'') => value.push('\''),
                        Some(b'"') => value.push('"'),
                        Some(b'?') => value.push('?'), // trigraph escape
                        Some(b'0') => value.push('\0'),
                        Some(b'x') | Some(b'X') => {
                            let hex = self.read_hex_digits(2)?;
                            value.push(char::from(hex as u8));
                        }
                        Some(c) if c.is_ascii_digit() => {
                            // Octal escape: up to 3 digits
                            let mut oct = (c - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d) if d.is_ascii_digit() && d < b'8' => {
                                        oct = oct * 8 + (d - b'0') as u32;
                                        self.advance();
                                    }
                                    _ => break,
                                }
                            }
                            if let Some(ch) = char::from_u32(oct) {
                                value.push(ch);
                            }
                        }
                        Some(c) => {
                            return Err(LexError {
                                message: format!("invalid escape sequence: \\{}", c as char),
                                span: Span::new(start, self.current_position()),
                            });
                        }
                        None => {
                            return Err(LexError {
                                message: "unterminated escape sequence".to_string(),
                                span: Span::new(start, self.current_position()),
                            });
                        }
                    }
                }
                Some(c) if c == quote => {
                    self.advance(); // closing quote
                    return Ok(Token {
                        kind: TokenKind::StringLiteral,
                        text: value,
                        span: Span::new(start, self.current_position()),
                    });
                }
                Some(_) => {
                    value.push(self.advance().unwrap() as char);
                }
            }
        }
    }

    fn read_hex_digits(&mut self, count: usize) -> Result<u32, LexError> {
        let start = self.current_position();
        let mut val = 0u32;
        for _ in 0..count {
            match self.peek() {
                Some(c) if c.is_ascii_hexdigit() => {
                    val = val * 16 + hex_digit_value(c) as u32;
                    self.advance();
                }
                _ => {
                    return Err(LexError {
                        message: "expected hex digit".to_string(),
                        span: Span::new(start, self.current_position()),
                    });
                }
            }
        }
        Ok(val)
    }

    fn read_number(&mut self) -> Token {
        let start = self.current_position();
        let text_start = self.pos;
        let mut is_float = false;

        // Check for hex: 0x...
        if self.peek() == Some(b'0') && matches!(self.peek_at(1), Some(b'x') | Some(b'X')) {
            self.advance(); // 0
            self.advance(); // x
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    self.advance();
                } else {
                    break;
                }
            }
        } else if self.peek() == Some(b'0') && matches!(self.peek_at(1), Some(b'0'..=b'7')) {
            // Octal
            self.advance(); // leading 0
            while let Some(c) = self.peek() {
                if matches!(c, b'0'..=b'7') {
                    self.advance();
                } else {
                    break;
                }
            }
        } else {
            // Decimal
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            // Fractional part
            if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                self.advance(); // .
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            // Exponent
            if matches!(self.peek(), Some(b'e') | Some(b'E')) {
                is_float = true;
                self.advance();
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    self.advance();
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        let text = self.source[text_start..self.pos].to_string();
        Token {
            kind: if is_float {
                TokenKind::FloatLiteral
            } else {
                TokenKind::IntLiteral
            },
            text,
            span: Span::new(start, self.current_position()),
        }
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let start = self.current_position();
        let text_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let text = &self.source[text_start..self.pos];
        let kind = TokenKind::from_keyword(text).unwrap_or(TokenKind::Ident);
        Token {
            kind,
            text: text.to_string(),
            span: Span::new(start, self.current_position()),
        }
    }

    /// Tokenize the entire source into a Vec of tokens (excluding comments).
    /// Comments are collected separately and returned.
    pub fn tokenize(&mut self) -> Result<(Vec<Token>, Vec<Token>), LexError> {
        // Handle UTF-8 BOM at start of file
        if self.pos == 0
            && self.bytes.len() >= 3
            && self.bytes[0] == 0xEF
            && self.bytes[1] == 0xBB
            && self.bytes[2] == 0xBF
        {
            // Valid UTF-8 BOM -- skip it
            self.pos = 3;
            self.col = 4; // BOM is 3 bytes, so col advances
        } else if self.pos == 0 && !self.bytes.is_empty() && self.bytes[0] == 0xEF {
            // Starts with 0xEF but not a valid UTF-8 BOM
            return Err(LexError {
                message: "Proto file starts with 0xEF but not UTF-8 BOM. Only UTF-8 is accepted for proto file.".to_string(),
                span: Span::new(
                    Position::new(1, 2, 1),
                    Position::new(1, 2, 1),
                ),
            });
        }

        let mut tokens = Vec::new();
        let mut comments = Vec::new();
        loop {
            let tok = self.next_token()?;
            match tok.kind {
                TokenKind::Eof => {
                    tokens.push(tok);
                    break;
                }
                TokenKind::LineComment | TokenKind::BlockComment => {
                    comments.push(tok);
                }
                _ => {
                    tokens.push(tok);
                }
            }
        }
        Ok((tokens, comments))
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace();

        let start = self.current_position();

        let Some(b) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                text: String::new(),
                span: Span::point(start),
            });
        };

        match b {
            b'/' => match self.peek_at(1) {
                Some(b'/') => Ok(self.read_line_comment()),
                Some(b'*') => self.read_block_comment(),
                _ => {
                    self.advance();
                    Ok(Token {
                        kind: TokenKind::Slash,
                        text: "/".to_string(),
                        span: Span::new(start, self.current_position()),
                    })
                }
            },
            b'\'' | b'"' => self.read_string(),
            b'{' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::LBrace,
                    text: "{".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'}' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::RBrace,
                    text: "}".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'(' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::LParen,
                    text: "(".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b')' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::RParen,
                    text: ")".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'[' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::LBracket,
                    text: "[".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b']' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::RBracket,
                    text: "]".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'<' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::LAngle,
                    text: "<".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'>' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::RAngle,
                    text: ">".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b';' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::Semicolon,
                    text: ";".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b',' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::Comma,
                    text: ",".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'.' => {
                // Check if this is the start of a float like .123
                if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
                    Ok(self.read_number())
                } else {
                    self.advance();
                    Ok(Token {
                        kind: TokenKind::Dot,
                        text: ".".into(),
                        span: Span::new(start, self.current_position()),
                    })
                }
            }
            b'=' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::Equals,
                    text: "=".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b'-' => {
                // Negative number or minus sign
                if self
                    .peek_at(1)
                    .is_some_and(|c| c.is_ascii_digit() || c == b'.')
                {
                    self.advance(); // consume '-'
                    let mut num_tok = self.read_number();
                    num_tok.text.insert(0, '-');
                    num_tok.span = Span::new(start, num_tok.span.end);
                    Ok(num_tok)
                } else {
                    self.advance();
                    Ok(Token {
                        kind: TokenKind::Minus,
                        text: "-".into(),
                        span: Span::new(start, self.current_position()),
                    })
                }
            }
            b'+' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::Plus,
                    text: "+".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            b':' => {
                self.advance();
                Ok(Token {
                    kind: TokenKind::Colon,
                    text: ":".into(),
                    span: Span::new(start, self.current_position()),
                })
            }
            c if c.is_ascii_digit() => Ok(self.read_number()),
            c if c.is_ascii_alphabetic() || c == b'_' => Ok(self.read_ident_or_keyword()),
            _ => {
                self.advance();
                Err(LexError {
                    message: format!("unexpected character: {:?}", b as char),
                    span: Span::new(start, self.current_position()),
                })
            }
        }
    }
}

fn hex_digit_value(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.message, self.span)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(source);
        let (tokens, _) = lexer.tokenize().unwrap();
        tokens
            .into_iter()
            .filter(|t| t.kind != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn test_keywords() {
        let tokens = lex("syntax message enum oneof map repeated optional required");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &TokenKind::Syntax,
                &TokenKind::Message,
                &TokenKind::Enum,
                &TokenKind::Oneof,
                &TokenKind::Map,
                &TokenKind::Repeated,
                &TokenKind::Optional,
                &TokenKind::Required,
            ]
        );
    }

    #[test]
    fn test_identifiers() {
        let tokens = lex("MyMessage _private foo123");
        assert_eq!(tokens.len(), 3);
        assert!(tokens.iter().all(|t| t.kind == TokenKind::Ident));
        assert_eq!(tokens[0].text, "MyMessage");
        assert_eq!(tokens[1].text, "_private");
        assert_eq!(tokens[2].text, "foo123");
    }

    #[test]
    fn test_numbers() {
        let tokens = lex("42 0x1F 0777 3.14 1e10 -5");
        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens[0].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[0].text, "42");
        assert_eq!(tokens[1].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[1].text, "0x1F");
        assert_eq!(tokens[2].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[2].text, "0777");
        assert_eq!(tokens[3].kind, TokenKind::FloatLiteral);
        assert_eq!(tokens[3].text, "3.14");
        assert_eq!(tokens[4].kind, TokenKind::FloatLiteral);
        assert_eq!(tokens[4].text, "1e10");
        assert_eq!(tokens[5].kind, TokenKind::IntLiteral);
        assert_eq!(tokens[5].text, "-5");
    }

    #[test]
    fn test_strings() {
        let tokens = lex(r#""hello world" 'single' "esc\n\t""#);
        assert_eq!(tokens.len(), 3);
        assert!(tokens.iter().all(|t| t.kind == TokenKind::StringLiteral));
        assert_eq!(tokens[0].text, "hello world");
        assert_eq!(tokens[1].text, "single");
        assert_eq!(tokens[2].text, "esc\n\t");
    }

    #[test]
    fn test_symbols() {
        let tokens = lex("{ } ( ) [ ] < > ; , . = - +");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &TokenKind::LBrace,
                &TokenKind::RBrace,
                &TokenKind::LParen,
                &TokenKind::RParen,
                &TokenKind::LBracket,
                &TokenKind::RBracket,
                &TokenKind::LAngle,
                &TokenKind::RAngle,
                &TokenKind::Semicolon,
                &TokenKind::Comma,
                &TokenKind::Dot,
                &TokenKind::Equals,
                &TokenKind::Minus,
                &TokenKind::Plus,
            ]
        );
    }

    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("foo // line comment\nbar /* block */");
        let (tokens, comments) = lexer.tokenize().unwrap();
        let token_kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(
            token_kinds,
            vec![&TokenKind::Ident, &TokenKind::Ident, &TokenKind::Eof]
        );
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].kind, TokenKind::LineComment);
        assert_eq!(comments[1].kind, TokenKind::BlockComment);
    }

    #[test]
    fn test_type_keywords() {
        let tokens = lex("int32 uint64 string bytes bool double float");
        assert!(tokens.iter().all(|t| t.kind.is_type_keyword()));
    }

    #[test]
    fn test_syntax_statement() {
        let tokens = lex(r#"syntax = "proto3";"#);
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &TokenKind::Syntax,
                &TokenKind::Equals,
                &TokenKind::StringLiteral,
                &TokenKind::Semicolon,
            ]
        );
        assert_eq!(tokens[2].text, "proto3");
    }

    #[test]
    fn test_span_tracking() {
        let tokens = lex("foo bar");
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.column, 1);
        assert_eq!(tokens[1].span.start.line, 1);
        assert_eq!(tokens[1].span.start.column, 5);
    }

    #[test]
    fn test_multiline_span() {
        let tokens = lex("foo\nbar\nbaz");
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[1].span.start.line, 2);
        assert_eq!(tokens[2].span.start.line, 3);
    }
}
