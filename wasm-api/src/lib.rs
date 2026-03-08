use std::collections::HashMap;
use std::panic;
use wasm_bindgen::prelude::*;

/// In-memory file resolver for WASM (no filesystem access).
struct MemoryResolver {
    files: HashMap<String, String>,
}

impl protoc_rs_analyzer::FileResolver for MemoryResolver {
    fn resolve(&self, name: &str) -> Option<String> {
        self.files.get(name).cloned()
    }
}

/// Catch panics and convert them to JsError.
fn catch<F, T>(f: F) -> Result<T, JsError>
where
    F: FnOnce() -> Result<T, JsError> + panic::UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(result) => result,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "internal compiler panic".to_string()
            };
            Err(JsError::new(&msg))
        }
    }
}

/// Compile a .proto source and generate prost-compatible Rust code.
///
/// Takes a protobuf source string and returns a JSON object mapping
/// filenames to generated Rust source code (e.g. `{"my.package.rs": "..."}`).
///
/// Does not support imports (single-file only, well-known types are available).
#[wasm_bindgen]
pub fn compile_proto_to_rust(source: &str) -> Result<String, JsError> {
    let source = source.to_string();
    catch(move || {
        let fds = protoc_rs_analyzer::analyze(&source).map_err(|e| JsError::new(&e.to_string()))?;

        let files =
            protoc_rs_codegen::generate_rust(&fds).map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&files).map_err(|e| JsError::new(&e.to_string()))
    })
}

/// Compile multiple .proto sources with import support.
///
/// `sources_json` is a JSON object mapping filenames to source code:
/// ```json
/// {
///   "main.proto": "syntax = \"proto3\"; import \"dep.proto\"; ...",
///   "dep.proto": "syntax = \"proto3\"; ..."
/// }
/// ```
///
/// `root_file` is the entry point filename (must be a key in `sources_json`).
///
/// Returns a JSON object mapping output filenames to generated Rust source code.
#[wasm_bindgen]
pub fn compile_protos_to_rust(root_file: &str, sources_json: &str) -> Result<String, JsError> {
    let root_file = root_file.to_string();
    let sources_json = sources_json.to_string();
    catch(move || {
        let sources: HashMap<String, String> =
            serde_json::from_str(&sources_json).map_err(|e| JsError::new(&e.to_string()))?;

        let resolver = MemoryResolver { files: sources };
        let fds = protoc_rs_analyzer::analyze_files(&[root_file.as_str()], &resolver)
            .map_err(|e| JsError::new(&e.to_string()))?;

        let files =
            protoc_rs_codegen::generate_rust(&fds).map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&files).map_err(|e| JsError::new(&e.to_string()))
    })
}
