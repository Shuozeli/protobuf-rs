//! Indentation-aware string builder for generating source code.

/// Indentation-aware string builder.
pub struct CodeWriter {
    buf: String,
    indent: usize,
}

impl CodeWriter {
    /// Create a new CodeWriter.
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(8192),
            indent: 0,
        }
    }

    /// Write an indented line (with trailing newline).
    pub fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.buf.push_str("  ");
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    /// Write a blank line.
    pub fn blank(&mut self) {
        self.buf.push('\n');
    }

    /// Increase indentation by one level.
    pub fn indent(&mut self) {
        self.indent += 1;
    }

    /// Decrease indentation by one level.
    pub fn dedent(&mut self) {
        debug_assert!(self.indent > 0, "dedent below zero");
        self.indent -= 1;
    }

    /// Write `header {`, execute the closure (indented), then write `}`.
    pub fn block<F>(&mut self, header: &str, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.line(&format!("{header} {{"));
        self.indent();
        f(self);
        self.dedent();
        self.line("}");
    }

    /// Like [`block`](Self::block) but the closure may return an error.
    /// The closing `}` is emitted even on error (partial output is still
    /// well-formed structurally), but the error is propagated to the caller.
    pub fn try_block<F, E>(&mut self, header: &str, f: F) -> Result<(), E>
    where
        F: FnOnce(&mut Self) -> Result<(), E>,
    {
        self.line(&format!("{header} {{"));
        self.indent();
        let result = f(self);
        self.dedent();
        self.line("}");
        result
    }

    /// Consume the writer and return the generated source code.
    /// Trailing blank lines are trimmed so the output ends with exactly one newline.
    pub fn finish(self) -> String {
        let trimmed = self.buf.trim_end_matches('\n');
        format!("{trimmed}\n")
    }

    /// Get a reference to the current buffer (for debugging).
    pub fn buffer(&self) -> &str {
        &self.buf
    }
}

impl Default for CodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_output() {
        let mut w = CodeWriter::new();
        w.line("use foo::bar;");
        w.blank();
        w.block("pub struct Foo", |w| {
            w.line("pub x: i32,");
        });
        let out = w.finish();
        assert_eq!(out, "use foo::bar;\n\npub struct Foo {\n  pub x: i32,\n}\n");
    }

    #[test]
    fn nested_blocks() {
        let mut w = CodeWriter::new();
        w.block("impl Foo", |w| {
            w.block("pub fn bar(&self)", |w| {
                w.line("42");
            });
        });
        let out = w.finish();
        assert!(out.contains("    42\n"));
    }
}
