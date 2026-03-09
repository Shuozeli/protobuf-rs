use protoc_rs_analyzer::FileResolver;
use std::path::{Path, PathBuf};

/// Resolves proto import paths by searching a list of include directories.
pub struct FsResolver {
    include_paths: Vec<PathBuf>,
}

impl FsResolver {
    pub fn new(include_paths: Vec<PathBuf>) -> Self {
        Self { include_paths }
    }
}

impl FileResolver for FsResolver {
    fn resolve(&self, name: &str) -> Option<String> {
        for dir in &self.include_paths {
            let path = dir.join(name);
            if path.is_file() {
                return std::fs::read_to_string(&path).ok();
            }
        }
        None
    }
}

/// Extract the relative proto path from an absolute file path and include dirs.
/// Returns the path relative to the first matching include dir, or the filename.
pub fn relative_proto_path(file_path: &Path, include_paths: &[PathBuf]) -> String {
    for dir in include_paths {
        if let Ok(rel) = file_path.strip_prefix(dir) {
            return rel.to_string_lossy().to_string();
        }
    }
    // Fallback: just the file name
    file_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_finds_file_in_include_path() {
        let tmp = tempfile::tempdir().unwrap();
        let proto_dir = tmp.path().join("protos");
        fs::create_dir_all(&proto_dir).unwrap();
        fs::write(proto_dir.join("test.proto"), "syntax = \"proto3\";").unwrap();

        let resolver = FsResolver::new(vec![proto_dir]);
        let result = resolver.resolve("test.proto");
        assert_eq!(result, Some("syntax = \"proto3\";".to_string()));
    }

    #[test]
    fn resolve_returns_none_for_missing_file() {
        let resolver = FsResolver::new(vec![PathBuf::from("/nonexistent")]);
        assert_eq!(resolver.resolve("foo.proto"), None);
    }

    #[test]
    fn resolve_searches_multiple_paths_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        fs::write(dir_a.join("shared.proto"), "// from a").unwrap();
        fs::write(dir_b.join("shared.proto"), "// from b").unwrap();

        let resolver = FsResolver::new(vec![dir_a, dir_b]);
        // Should find the first one
        assert_eq!(
            resolver.resolve("shared.proto"),
            Some("// from a".to_string())
        );
    }

    #[test]
    fn resolve_finds_nested_path() {
        let tmp = tempfile::tempdir().unwrap();
        let proto_dir = tmp.path().join("protos");
        fs::create_dir_all(proto_dir.join("google/protobuf")).unwrap();
        fs::write(
            proto_dir.join("google/protobuf/any.proto"),
            "syntax = \"proto3\";",
        )
        .unwrap();

        let resolver = FsResolver::new(vec![proto_dir]);
        assert!(resolver.resolve("google/protobuf/any.proto").is_some());
    }

    #[test]
    fn relative_proto_path_strips_include_dir() {
        let include = PathBuf::from("/home/user/protos");
        let file = Path::new("/home/user/protos/google/api/http.proto");
        assert_eq!(
            relative_proto_path(file, &[include]),
            "google/api/http.proto"
        );
    }

    #[test]
    fn relative_proto_path_falls_back_to_filename() {
        let file = Path::new("/some/other/path/test.proto");
        assert_eq!(
            relative_proto_path(file, &[PathBuf::from("/not/matching")]),
            "test.proto"
        );
    }
}
