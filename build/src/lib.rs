//! Build-time code generation for protoc-rs runtime backend.
//!
//! Add this to your `build.rs` to generate runtime-backed Rust code
//! from `.proto` files at compile time:
//!
//! ```no_run
//! // build.rs
//! protoc_rs_build::Config::new()
//!     .include("proto/")
//!     .compile(&["proto/my_service.proto"])
//!     .expect("protobuf codegen failed");
//! ```
//!
//! Then include the generated code in your crate:
//!
//! ```ignore
//! // src/lib.rs
//! include!(concat!(env!("OUT_DIR"), "/my.package.rs"));
//! ```

use protoc_rs_analyzer::{analyze_files, AnalyzeError};
use protoc_rs_codegen::RuntimeCodeGenOptions;
use protoc_rs_compiler::resolver::{relative_proto_path, FsResolver};
use std::path::{Path, PathBuf};

/// Configuration for protobuf code generation.
pub struct Config {
    include_paths: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    emit_serde: bool,
}

impl Config {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self {
            include_paths: Vec::new(),
            out_dir: None,
            emit_serde: false,
        }
    }

    /// Add a proto import search path.
    pub fn include(mut self, path: impl AsRef<Path>) -> Self {
        self.include_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Set the output directory. Defaults to `OUT_DIR` environment variable.
    pub fn out_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Enable serde JSON derives in generated code.
    pub fn emit_serde(mut self, enable: bool) -> Self {
        self.emit_serde = enable;
        self
    }

    /// Compile the given `.proto` files and write generated Rust code.
    pub fn compile(self, proto_files: &[impl AsRef<Path>]) -> Result<(), CompileError> {
        let out_dir = self.out_dir.unwrap_or_else(|| {
            PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set; run from build.rs"))
        });

        let include_paths = if self.include_paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.include_paths
        };

        let resolver = FsResolver::new(include_paths.clone());

        // Compute relative proto names
        let root_names: Vec<String> = proto_files
            .iter()
            .map(|p| {
                let p = p.as_ref();
                let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                let canon_includes: Vec<PathBuf> = include_paths
                    .iter()
                    .map(|ip| std::fs::canonicalize(ip).unwrap_or_else(|_| ip.clone()))
                    .collect();
                relative_proto_path(&abs, &canon_includes)
            })
            .collect();

        let root_refs: Vec<&str> = root_names.iter().map(|s| s.as_str()).collect();
        let fds = analyze_files(&root_refs, &resolver).map_err(CompileError::Analyze)?;

        let options = RuntimeCodeGenOptions {
            emit_serde: self.emit_serde,
        };
        let files = protoc_rs_codegen::generate_rust_runtime_with_options(&fds, &options)
            .map_err(|e| CompileError::CodeGen(format!("{e}")))?;

        if !out_dir.exists() {
            std::fs::create_dir_all(&out_dir)
                .map_err(|e| CompileError::Io(format!("create {}: {e}", out_dir.display())))?;
        }

        for (filename, content) in &files {
            let path = out_dir.join(filename);
            std::fs::write(&path, content)
                .map_err(|e| CompileError::Io(format!("write {}: {e}", path.display())))?;
        }

        // Emit cargo:rerun-if-changed for each input file
        for proto_file in proto_files {
            println!("cargo:rerun-if-changed={}", proto_file.as_ref().display());
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from the compile process.
#[derive(Debug)]
pub enum CompileError {
    Analyze(AnalyzeError),
    CodeGen(String),
    Io(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Analyze(e) => write!(f, "analysis error: {e}"),
            CompileError::CodeGen(e) => write!(f, "codegen error: {e}"),
            CompileError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for CompileError {}
