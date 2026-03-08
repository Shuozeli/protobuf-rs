use crate::{AnalyzeError, SymbolKind};
use std::collections::{HashMap, HashSet};

/// Build a fully-qualified name from a package and a type name.
pub fn make_fqn(pkg: &str, name: &str) -> String {
    if pkg.is_empty() {
        format!(".{}", name)
    } else {
        format!(".{}.{}", pkg, name)
    }
}

/// Build a scope string from a package (for use as search scope).
/// Returns "." for empty package, ".pkg" for non-empty.
pub fn make_fqn_scope(pkg: &str) -> String {
    if pkg.is_empty() {
        ".".to_string()
    } else {
        format!(".{}", pkg)
    }
}

/// Resolve a type name to a fully-qualified name.
///
/// Protobuf scoping rules:
/// 1. If name starts with `.`, it's already fully qualified
/// 2. Otherwise, search from the innermost scope outward:
///    - current message scope
///    - parent message scope
///    - ...
///    - package scope
///    - global scope
///
/// Additionally:
/// - Only symbols in `visible` (direct + public transitive imports + own file) can be used.
/// - If the first component matches in an inner scope but the full path doesn't exist,
///   that's an error (SearchMostLocalFirst rule).
pub fn resolve_type_name(
    name: &str,
    scope: &str,
    _file_pkg: &str,
    symbols: &HashMap<String, SymbolKind>,
    visible: &HashSet<String>,
) -> Result<String, AnalyzeError> {
    // Already fully qualified
    if name.starts_with('.') {
        if symbols.contains_key(name) && visible.contains(name) {
            return Ok(name.to_string());
        }
        // Check if symbol exists but not visible (defined in non-imported file)
        if symbols.contains_key(name) {
            return Err(AnalyzeError {
                message: format!("type not found: {}", name),
                file: None,
                span: None,
            });
        }
        return Err(AnalyzeError {
            message: format!("type not found: {}", name),
            file: None,
            span: None,
        });
    }

    // For dotted names (e.g., "Bar.Baz"), extract the first component
    let first_component = name.split('.').next().unwrap_or(name);
    let has_dots = name.contains('.');

    // Search from innermost scope outward
    let mut current_scope = scope.to_string();
    loop {
        let candidate = format!("{}.{}", current_scope, name);

        if symbols.contains_key(&candidate) && visible.contains(&candidate) {
            return Ok(candidate);
        }

        // SearchMostLocalFirst: if the name has dots (e.g. "Bar.Baz"), check if
        // the first component exists at this scope level. If it does, but the full
        // path doesn't resolve, that's an error -- the inner scope shadows the outer.
        if has_dots {
            let first_candidate = format!("{}.{}", current_scope, first_component);
            if symbols.contains_key(&first_candidate) && visible.contains(&first_candidate) {
                // First component found here, but full path doesn't exist
                return Err(AnalyzeError {
                    message: format!(
                        "\"{}\" is resolved to \"{}\", which is not defined. \
                         The innermost scope is searched first in name resolution. \
                         Consider using a leading '.'(i.e., \".{}\") to start from the outermost scope.",
                        name, candidate, name
                    ),
                    file: None,
                    span: None,
                });
            }
        }

        // Move up one scope level
        if let Some(pos) = current_scope.rfind('.') {
            if pos == 0 {
                // We're at the root "." -- one more try at global scope
                let global = format!(".{}", name);
                if symbols.contains_key(&global) && visible.contains(&global) {
                    return Ok(global);
                }
                // SearchMostLocalFirst at global scope
                if has_dots {
                    let first_global = format!(".{}", first_component);
                    if symbols.contains_key(&first_global) && visible.contains(&first_global) {
                        return Err(AnalyzeError {
                            message: format!(
                                "\"{}\" is resolved to \"{}\", which is not defined. \
                                 The innermost scope is searched first in name resolution. \
                                 Consider using a leading '.'(i.e., \".{}\") to start from the outermost scope.",
                                name, global, name
                            ),
                            file: None,
                            span: None,
                        });
                    }
                }
                break;
            }
            current_scope.truncate(pos);
        } else {
            break;
        }
    }

    // Try with package prefix if name contains dots (e.g., "dep.DepMessage")
    // This handles cross-package references like "other_pkg.SomeType"
    let dotted = format!(".{}", name);
    if symbols.contains_key(&dotted) && visible.contains(&dotted) {
        return Ok(dotted);
    }

    Err(AnalyzeError {
        message: format!("type not found: {} (searched from scope {})", name, scope),
        file: None,
        span: None,
    })
}
