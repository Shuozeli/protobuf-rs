mod resolve;
mod validate;
mod well_known;

use protoc_rs_parser::{parse, ParseError};
use protoc_rs_schema::*;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Error type for analysis failures.
#[derive(Debug, Clone)]
pub struct AnalyzeError {
    pub message: String,
    pub file: Option<String>,
    pub span: Option<Span>,
}

impl std::fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.file, &self.span) {
            (Some(file), Some(span)) => write!(f, "{}:{}: {}", file, span, self.message),
            (Some(file), None) => write!(f, "{}: {}", file, self.message),
            (None, Some(span)) => write!(f, "{}: {}", span, self.message),
            (None, None) => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for AnalyzeError {}

impl From<ParseError> for AnalyzeError {
    fn from(e: ParseError) -> Self {
        AnalyzeError {
            message: e.message.clone(),
            file: None,
            span: Some(e.span),
        }
    }
}

/// Trait for resolving import paths to file contents.
pub trait FileResolver {
    fn resolve(&self, name: &str) -> Option<String>;
}

/// Analyze a single proto source string (no imports except well-known types).
pub fn analyze(source: &str) -> Result<FileDescriptorSet, AnalyzeError> {
    let resolver = well_known::WellKnownResolver;
    let mut file = parse(source)?;
    file.name = Some("<input>".to_string());

    let mut ctx = AnalyzeContext::new();
    ctx.load_file("<input>".to_string(), file, &resolver)?;
    ctx.resolve_all_types()?;
    ctx.validate_all()?;
    Ok(ctx.into_descriptor_set())
}

/// Analyze multiple proto files with a custom resolver.
pub fn analyze_files(
    root_files: &[&str],
    resolver: &dyn FileResolver,
) -> Result<FileDescriptorSet, AnalyzeError> {
    let combined = well_known::CombinedResolver::new(resolver);
    let mut ctx = AnalyzeContext::new();

    for name in root_files {
        if ctx.loaded.contains_key(*name) {
            continue;
        }
        let source = combined.resolve(name).ok_or_else(|| AnalyzeError {
            message: format!("file not found: {}", name),
            file: None,
            span: None,
        })?;
        let mut file = parse(&source).map_err(|e| AnalyzeError {
            message: e.to_string(),
            file: Some(name.to_string()),
            span: Some(e.span),
        })?;
        file.name = Some(name.to_string());
        ctx.load_file(name.to_string(), file, &combined)?;
    }

    ctx.resolve_all_types()?;
    ctx.validate_all()?;
    Ok(ctx.into_descriptor_set())
}

// ---------------------------------------------------------------------------
// Internal: Analysis context
// ---------------------------------------------------------------------------

/// Tracks all loaded files and the global symbol table.
struct AnalyzeContext {
    /// All loaded files, keyed by file name.
    loaded: HashMap<String, FileDescriptorProto>,
    /// Load order (for deterministic output).
    load_order: Vec<String>,
    /// Global symbol table: fully qualified name -> SymbolKind.
    symbols: HashMap<String, SymbolKind>,
    /// Files currently being loaded (for circular import detection).
    visiting: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Message,
    Enum,
    Extension,
    Package,
}

impl AnalyzeContext {
    fn new() -> Self {
        Self {
            loaded: HashMap::new(),
            load_order: Vec::new(),
            symbols: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    /// Load a file and all its transitive imports.
    fn load_file(
        &mut self,
        name: String,
        file: FileDescriptorProto,
        resolver: &dyn FileResolver,
    ) -> Result<(), AnalyzeError> {
        if self.loaded.contains_key(&name) {
            return Ok(());
        }

        // Detect circular imports
        if self.visiting.contains(&name) {
            return Err(AnalyzeError {
                message: format!("File recursively imports itself: {}", name),
                file: Some(name),
                span: None,
            });
        }

        self.visiting.insert(name.clone());

        // Load imports first (depth-first), BEFORE inserting into loaded
        let deps: Vec<String> = file.dependency.clone();

        for dep in &deps {
            if self.loaded.contains_key(dep.as_str()) {
                continue;
            }
            let dep_source = resolver.resolve(dep).ok_or_else(|| AnalyzeError {
                message: format!("imported file not found: {}", dep),
                file: Some(name.clone()),
                span: None,
            })?;
            let mut dep_file = parse(&dep_source).map_err(|e| AnalyzeError {
                message: format!("error parsing {}: {}", dep, e),
                file: Some(name.clone()),
                span: None,
            })?;
            dep_file.name = Some(dep.clone());
            self.load_file(dep.clone(), dep_file, resolver)?;
        }

        // Insert into loaded AFTER deps are processed (deps go first in load_order)
        self.visiting.remove(&name);
        self.loaded.insert(name.clone(), file);
        self.load_order.push(name.clone());

        // Register symbols from this file
        let file = self.loaded.get(&name).unwrap().clone();
        let pkg = file.package.as_deref().unwrap_or("");

        // Register package components and check for conflicts with non-package symbols
        if !pkg.is_empty() {
            let mut pkg_path = String::from(".");
            for component in pkg.split('.') {
                pkg_path.push_str(component);
                if let Some(kind) = self.symbols.get(&pkg_path) {
                    if *kind != SymbolKind::Package {
                        return Err(AnalyzeError {
                            message: format!(
                                "\"{}\" is already defined (as something other than a package).",
                                component
                            ),
                            file: Some(name),
                            span: None,
                        });
                    }
                } else {
                    self.symbols.insert(pkg_path.clone(), SymbolKind::Package);
                }
                pkg_path.push('.');
            }
        }

        let prefix = if pkg.is_empty() {
            ".".to_string()
        } else {
            format!(".{}.", pkg)
        };

        self.register_messages(&prefix, &file.message_type)?;
        self.register_enums(&prefix, &file.enum_type)?;
        self.register_extensions(&prefix, &file.extension)?;

        Ok(())
    }

    fn register_messages(
        &mut self,
        prefix: &str,
        messages: &[DescriptorProto],
    ) -> Result<(), AnalyzeError> {
        for msg in messages {
            let name = msg.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            if self.symbols.contains_key(&fqn) {
                return Err(AnalyzeError {
                    message: format!("duplicate symbol: {}", fqn),
                    file: None,
                    span: None,
                });
            }
            self.symbols.insert(fqn.clone(), SymbolKind::Message);

            // Register nested types
            let nested_prefix = format!("{}.", fqn);
            self.register_messages(&nested_prefix, &msg.nested_type)?;
            self.register_enums(&nested_prefix, &msg.enum_type)?;
        }
        Ok(())
    }

    fn register_enums(
        &mut self,
        prefix: &str,
        enums: &[EnumDescriptorProto],
    ) -> Result<(), AnalyzeError> {
        for e in enums {
            let name = e.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            if self.symbols.contains_key(&fqn) {
                return Err(AnalyzeError {
                    message: format!("duplicate symbol: {}", fqn),
                    file: None,
                    span: None,
                });
            }
            self.symbols.insert(fqn, SymbolKind::Enum);
        }
        Ok(())
    }

    fn register_extensions(
        &mut self,
        prefix: &str,
        extensions: &[FieldDescriptorProto],
    ) -> Result<(), AnalyzeError> {
        for ext in extensions {
            let name = ext.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            // Extensions can shadow -- don't error on duplicate, just register
            self.symbols.insert(fqn, SymbolKind::Extension);
        }
        Ok(())
    }

    /// Resolve all type references in all loaded files.
    fn resolve_all_types(&mut self) -> Result<(), AnalyzeError> {
        // Collect files to process (clone to avoid borrow issues)
        let file_names: Vec<String> = self.load_order.clone();

        for file_name in &file_names {
            let file = self.loaded.get(file_name).unwrap().clone();
            let pkg = file.package.as_deref().unwrap_or("");

            // Collect visible symbols for this file (direct imports + public transitive)
            let visible = self.collect_visible_symbols(&file);

            let mut resolved_file = file.clone();

            // Resolve message fields
            for msg in &mut resolved_file.message_type {
                let msg_fqn = resolve::make_fqn(pkg, msg.name.as_deref().unwrap_or(""));
                self.resolve_message_types(msg, &msg_fqn, pkg, &visible)?;
            }

            // Resolve extension fields (type + extendee)
            let pkg_scope = resolve::make_fqn_scope(pkg);
            for ext in &mut resolved_file.extension {
                self.resolve_field_type(ext, pkg, pkg, &visible)?;
                if let Some(ref extendee) = ext.extendee {
                    let resolved = self.resolve_type_name(extendee, &pkg_scope, pkg, &visible)?;
                    // Extendee must be a message type
                    if let Some(&kind) = self.symbols.get(&resolved) {
                        if kind != SymbolKind::Message {
                            return Err(AnalyzeError {
                                message: format!("\"{}\" is not a message type.", extendee),
                                file: Some(file_name.clone()),
                                span: ext.source_span,
                            });
                        }
                    }
                    ext.extendee = Some(resolved);
                }
            }

            // Resolve service method types
            for svc in &mut resolved_file.service {
                let svc_name = svc.name.as_deref().unwrap_or("<unknown>");
                for method in &mut svc.method {
                    let method_name = method.name.as_deref().unwrap_or("<unknown>");
                    if let Some(ref input) = method.input_type {
                        let resolved = self.resolve_type_name(input, &pkg_scope, pkg, &visible)?;
                        // Input type must be a message
                        if let Some(&kind) = self.symbols.get(&resolved) {
                            if kind != SymbolKind::Message {
                                return Err(AnalyzeError {
                                    message: format!(
                                        "\"{}\" is not a message type (input of {}.{}).",
                                        input, svc_name, method_name
                                    ),
                                    file: Some(file_name.clone()),
                                    span: None,
                                });
                            }
                        }
                        method.input_type = Some(resolved);
                    }
                    if let Some(ref output) = method.output_type {
                        let resolved = self.resolve_type_name(output, &pkg_scope, pkg, &visible)?;
                        // Output type must be a message
                        if let Some(&kind) = self.symbols.get(&resolved) {
                            if kind != SymbolKind::Message {
                                return Err(AnalyzeError {
                                    message: format!(
                                        "\"{}\" is not a message type (output of {}.{}).",
                                        output, svc_name, method_name
                                    ),
                                    file: Some(file_name.clone()),
                                    span: None,
                                });
                            }
                        }
                        method.output_type = Some(resolved);
                    }
                }
            }

            self.loaded.insert(file_name.clone(), resolved_file);
        }

        Ok(())
    }

    fn resolve_message_types(
        &self,
        msg: &mut DescriptorProto,
        msg_fqn: &str,
        file_pkg: &str,
        visible: &HashSet<String>,
    ) -> Result<(), AnalyzeError> {
        for field in &mut msg.field {
            self.resolve_field_type(field, msg_fqn, file_pkg, visible)?;
        }
        for ext in &mut msg.extension {
            self.resolve_field_type(ext, msg_fqn, file_pkg, visible)?;
            // Resolve extendee and check it's a message
            if let Some(ref extendee) = ext.extendee {
                let pkg_scope = resolve::make_fqn_scope(file_pkg);
                let resolved = self.resolve_type_name(extendee, &pkg_scope, file_pkg, visible)?;
                if let Some(&kind) = self.symbols.get(&resolved) {
                    if kind != SymbolKind::Message {
                        return Err(AnalyzeError {
                            message: format!("\"{}\" is not a message type.", extendee),
                            file: None,
                            span: ext.source_span,
                        });
                    }
                }
                ext.extendee = Some(resolved);
            }
        }
        for nested in &mut msg.nested_type {
            let nested_fqn = format!("{}.{}", msg_fqn, nested.name.as_deref().unwrap_or(""));
            self.resolve_message_types(nested, &nested_fqn, file_pkg, visible)?;
        }
        Ok(())
    }

    fn resolve_field_type(
        &self,
        field: &mut FieldDescriptorProto,
        scope: &str,
        file_pkg: &str,
        visible: &HashSet<String>,
    ) -> Result<(), AnalyzeError> {
        // Only resolve if type_name is set (message/enum references)
        if let Some(ref type_name) = field.type_name {
            // Skip if already a scalar type with no type_name to resolve
            if field.r#type.is_some()
                && field.r#type != Some(FieldType::Message)
                && field.r#type != Some(FieldType::Enum)
                && field.r#type != Some(FieldType::Group)
            {
                return Ok(());
            }
            let resolved = self.resolve_type_name(type_name, scope, file_pkg, visible)?;
            let kind = self
                .symbols
                .get(&resolved)
                .copied()
                .ok_or_else(|| AnalyzeError {
                    message: format!("unresolved type: {} (resolved to {})", type_name, resolved),
                    file: None,
                    span: None,
                })?;
            field.type_name = Some(resolved);
            match kind {
                SymbolKind::Message => {
                    if field.r#type != Some(FieldType::Group) {
                        field.r#type = Some(FieldType::Message);
                    }
                }
                SymbolKind::Enum => {
                    field.r#type = Some(FieldType::Enum);
                }
                SymbolKind::Extension | SymbolKind::Package => {
                    // Extensions and packages shouldn't be referenced as field types
                    return Err(AnalyzeError {
                        message: format!(
                            "type not found: {}",
                            field.type_name.as_deref().unwrap_or("")
                        ),
                        file: None,
                        span: field.source_span,
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve a type name to a fully-qualified name.
    /// Follows protobuf scoping rules: search from innermost scope outward.
    fn resolve_type_name(
        &self,
        name: &str,
        scope: &str,
        file_pkg: &str,
        visible: &HashSet<String>,
    ) -> Result<String, AnalyzeError> {
        resolve::resolve_type_name(name, scope, file_pkg, &self.symbols, visible)
    }

    /// Collect all symbols visible to a file (from its imports).
    fn collect_visible_symbols(&self, file: &FileDescriptorProto) -> HashSet<String> {
        let mut visible = HashSet::new();

        // All package symbols are always visible (needed for dotted-name resolution)
        for (sym, kind) in &self.symbols {
            if *kind == SymbolKind::Package {
                visible.insert(sym.clone());
            }
        }

        // All symbols from the file itself are visible
        let pkg = file.package.as_deref().unwrap_or("");
        let prefix = if pkg.is_empty() {
            ".".to_string()
        } else {
            format!(".{}.", pkg)
        };
        self.collect_symbols_from_prefix(
            &prefix,
            &file.message_type,
            &file.enum_type,
            &mut visible,
        );

        // Symbols from direct imports
        for dep_name in file.dependency.iter() {
            if let Some(dep_file) = self.loaded.get(dep_name) {
                let dep_pkg = dep_file.package.as_deref().unwrap_or("");
                let dep_prefix = if dep_pkg.is_empty() {
                    ".".to_string()
                } else {
                    format!(".{}.", dep_pkg)
                };
                self.collect_symbols_from_prefix(
                    &dep_prefix,
                    &dep_file.message_type,
                    &dep_file.enum_type,
                    &mut visible,
                );

                // Follow public imports transitively
                self.collect_public_transitive(dep_file, &mut visible);
            }
        }

        visible
    }

    fn collect_public_transitive(&self, file: &FileDescriptorProto, visible: &mut HashSet<String>) {
        for &pub_idx in &file.public_dependency {
            if let Some(dep_name) = file.dependency.get(pub_idx as usize) {
                if let Some(dep_file) = self.loaded.get(dep_name) {
                    let dep_pkg = dep_file.package.as_deref().unwrap_or("");
                    let dep_prefix = if dep_pkg.is_empty() {
                        ".".to_string()
                    } else {
                        format!(".{}.", dep_pkg)
                    };
                    self.collect_symbols_from_prefix(
                        &dep_prefix,
                        &dep_file.message_type,
                        &dep_file.enum_type,
                        visible,
                    );
                    // Recurse for chained public imports
                    self.collect_public_transitive(dep_file, visible);
                }
            }
        }
    }

    fn collect_symbols_from_prefix(
        &self,
        prefix: &str,
        messages: &[DescriptorProto],
        enums: &[EnumDescriptorProto],
        visible: &mut HashSet<String>,
    ) {
        for msg in messages {
            let name = msg.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            visible.insert(fqn.clone());
            let nested_prefix = format!("{}.", fqn);
            self.collect_symbols_from_prefix(
                &nested_prefix,
                &msg.nested_type,
                &msg.enum_type,
                visible,
            );
        }
        for e in enums {
            let name = e.name.as_deref().unwrap_or("");
            let fqn = format!("{}{}", prefix, name);
            visible.insert(fqn.clone());
        }
    }

    /// Run validation passes on all files.
    fn validate_all(&self) -> Result<(), AnalyzeError> {
        for file_name in &self.load_order {
            let file = self.loaded.get(file_name).unwrap();
            validate::validate_file(file)?;
        }
        // Post-resolve validation: check lite runtime import restrictions
        self.validate_lite_imports()?;
        // Post-resolve validation: check enum default values
        self.validate_enum_defaults()?;
        // Post-resolve validation: check extension option names
        self.validate_extension_options()?;
        // Post-resolve validation: check editions feature options
        self.validate_features()?;
        Ok(())
    }

    /// Validate that non-lite files do not import lite runtime files.
    fn validate_lite_imports(&self) -> Result<(), AnalyzeError> {
        for file_name in &self.load_order {
            let file = self.loaded.get(file_name).unwrap();
            let self_is_lite = file
                .options
                .as_ref()
                .and_then(|o| o.optimize_for)
                .map(|m| m == OptimizeMode::LiteRuntime)
                .unwrap_or(false);
            if self_is_lite {
                continue;
            }
            for dep in &file.dependency {
                if let Some(dep_file) = self.loaded.get(dep.as_str()) {
                    let dep_is_lite = dep_file
                        .options
                        .as_ref()
                        .and_then(|o| o.optimize_for)
                        .map(|m| m == OptimizeMode::LiteRuntime)
                        .unwrap_or(false);
                    if dep_is_lite {
                        return Err(AnalyzeError {
                            message: format!(
                                "Files that do not use optimize_for = LITE_RUNTIME cannot \
                                 import files which do use this option.  This file is not \
                                 lite, but it imports \"{}\" which is.",
                                dep
                            ),
                            file: Some(file_name.clone()),
                            span: None,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate that enum field default values reference valid enum value names.
    fn validate_enum_defaults(&self) -> Result<(), AnalyzeError> {
        for file_name in &self.load_order {
            let file = self.loaded.get(file_name).unwrap();
            for msg in &file.message_type {
                self.validate_enum_defaults_in_message(msg, file_name)?;
            }
        }
        Ok(())
    }

    fn validate_enum_defaults_in_message(
        &self,
        msg: &DescriptorProto,
        file_name: &str,
    ) -> Result<(), AnalyzeError> {
        for field in &msg.field {
            if field.r#type == Some(FieldType::Enum) {
                if let (Some(ref default_val), Some(ref type_name)) =
                    (&field.default_value, &field.type_name)
                {
                    // Look up the enum in loaded files
                    if let Some(enum_desc) = self.find_enum_by_fqn(type_name) {
                        let has_value = enum_desc
                            .value
                            .iter()
                            .any(|v| v.name.as_deref() == Some(default_val.as_str()));
                        if !has_value {
                            let enum_name = type_name.strip_prefix('.').unwrap_or(type_name);
                            return Err(AnalyzeError {
                                message: format!(
                                    "Enum type \"{}\" has no value named \"{}\".",
                                    enum_name, default_val
                                ),
                                file: Some(file_name.to_string()),
                                span: field.source_span,
                            });
                        }
                    }
                }
            }
        }
        for nested in &msg.nested_type {
            self.validate_enum_defaults_in_message(nested, file_name)?;
        }
        Ok(())
    }

    /// Find an enum descriptor by its fully-qualified name.
    fn find_enum_by_fqn(&self, fqn: &str) -> Option<&EnumDescriptorProto> {
        for file in self.loaded.values() {
            if let Some(e) = Self::find_enum_in_file(file, fqn) {
                return Some(e);
            }
        }
        None
    }

    fn find_enum_in_file<'a>(
        file: &'a FileDescriptorProto,
        fqn: &str,
    ) -> Option<&'a EnumDescriptorProto> {
        let pkg = file.package.as_deref().unwrap_or("");
        let prefix = if pkg.is_empty() {
            ".".to_string()
        } else {
            format!(".{}.", pkg)
        };
        for e in &file.enum_type {
            let name = format!("{}{}", prefix, e.name.as_deref().unwrap_or(""));
            if name == fqn {
                return Some(e);
            }
        }
        for msg in &file.message_type {
            if let Some(e) = Self::find_enum_in_msg(msg, &prefix, fqn) {
                return Some(e);
            }
        }
        None
    }

    fn find_enum_in_msg<'a>(
        msg: &'a DescriptorProto,
        prefix: &str,
        fqn: &str,
    ) -> Option<&'a EnumDescriptorProto> {
        let msg_prefix = format!("{}{}.", prefix, msg.name.as_deref().unwrap_or(""));
        for e in &msg.enum_type {
            let name = format!("{}{}", msg_prefix, e.name.as_deref().unwrap_or(""));
            if name == fqn {
                return Some(e);
            }
        }
        for nested in &msg.nested_type {
            if let Some(e) = Self::find_enum_in_msg(nested, &msg_prefix, fqn) {
                return Some(e);
            }
        }
        None
    }

    /// Validate extension option names resolve to actual extensions.
    fn validate_extension_options(&self) -> Result<(), AnalyzeError> {
        for file_name in &self.load_order {
            let file = self.loaded.get(file_name).unwrap();
            let pkg = file.package.as_deref().unwrap_or("");
            let scope = resolve::make_fqn_scope(pkg);

            // Check file-level options
            if let Some(ref opts) = file.options {
                self.validate_extension_option_names(
                    &opts.uninterpreted_option,
                    &scope,
                    file_name,
                )?;
            }
        }
        Ok(())
    }

    /// Validate that extension option names (those with is_extension=true) resolve
    /// to defined extensions. Produces an error with scope resolution hint if the
    /// name resolves to a different scope than intended.
    fn validate_extension_option_names(
        &self,
        options: &[UninterpretedOption],
        scope: &str,
        file_name: &str,
    ) -> Result<(), AnalyzeError> {
        for opt in options {
            if opt.name.is_empty() {
                continue;
            }
            let first = &opt.name[0];
            if !first.is_extension {
                continue;
            }
            let ext_name = &first.name_part;
            // Try to resolve using our standard resolution
            match self.resolve_extension_name(ext_name, scope) {
                Ok(_) => {} // resolved fine
                Err(resolved_to) => {
                    // Name resolved to something in a different scope
                    let msg = if let Some(resolved) = resolved_to {
                        format!(
                            "Option \"({})\" is resolved to \"({})\", which is not \
                             defined. The innermost scope is searched first in name \
                             resolution. Consider using a leading '.'(i.e., \
                             \"(.{})\") to start from the outermost scope.",
                            ext_name, resolved, ext_name
                        )
                    } else {
                        format!("Option \"({})\" unknown.", ext_name)
                    };
                    return Err(AnalyzeError {
                        message: msg,
                        file: Some(file_name.to_string()),
                        span: None,
                    });
                }
            }
        }
        Ok(())
    }

    /// Try to resolve an extension name from the given scope.
    /// Returns Ok(fqn) if found as an extension, Err(Some(candidate)) if a
    /// non-extension match was found (e.g., package prefix), Err(None) if not found at all.
    fn resolve_extension_name(&self, name: &str, scope: &str) -> Result<String, Option<String>> {
        // Fully qualified
        if name.starts_with('.') {
            let fqn = name.to_string();
            if matches!(self.symbols.get(&fqn), Some(SymbolKind::Extension)) {
                return Ok(fqn);
            }
            return Err(None);
        }

        // Search from innermost scope outward, checking first component
        let first_component = name.split('.').next().unwrap_or(name);
        let mut current_scope = scope.to_string();

        loop {
            let candidate_prefix = format!("{}.{}", current_scope, first_component);

            // Check if first component matches something in this scope
            if let Some(kind) = self.symbols.get(&candidate_prefix) {
                // First component matches -- now check full name
                let full_candidate = format!("{}.{}", current_scope, name);
                if matches!(
                    self.symbols.get(&full_candidate),
                    Some(SymbolKind::Extension)
                ) {
                    return Ok(full_candidate);
                }
                // First component matched but full name doesn't exist as extension
                if *kind == SymbolKind::Package {
                    // Package prefix matched -- this is the "resolved to X which is not defined" case
                    let resolved = full_candidate
                        .strip_prefix('.')
                        .unwrap_or(&full_candidate)
                        .to_string();
                    return Err(Some(resolved));
                }
            }

            // Move up one scope level
            if let Some(pos) = current_scope.rfind('.') {
                if pos == 0 {
                    // Try global scope
                    let global = format!(".{}", name);
                    if matches!(self.symbols.get(&global), Some(SymbolKind::Extension)) {
                        return Ok(global);
                    }
                    break;
                }
                current_scope.truncate(pos);
            } else {
                break;
            }
        }

        Err(None)
    }

    /// Validate editions feature options.
    /// - UNKNOWN feature values are rejected.
    /// - Implicit presence fields cannot have defaults.
    fn validate_features(&self) -> Result<(), AnalyzeError> {
        for file_name in &self.load_order {
            let file = self.loaded.get(file_name).unwrap();
            let is_editions = file.syntax.as_deref() == Some("editions");

            if !is_editions {
                continue;
            }

            // Check for UNKNOWN feature values at file level
            if let Some(ref opts) = file.options {
                self.validate_feature_values(&opts.uninterpreted_option, file_name)?;
            }

            // Determine file-level field_presence
            let file_field_presence = file
                .options
                .as_ref()
                .and_then(|opts| get_feature_value(&opts.uninterpreted_option, "field_presence"));

            for msg in &file.message_type {
                self.validate_features_in_message(msg, file_name, file_field_presence.as_deref())?;
            }
        }
        Ok(())
    }

    fn validate_features_in_message(
        &self,
        msg: &DescriptorProto,
        file_name: &str,
        parent_field_presence: Option<&str>,
    ) -> Result<(), AnalyzeError> {
        for field in &msg.field {
            let field_opts = field.options.as_ref();

            // Check for UNKNOWN feature values at field level
            if let Some(opts) = field_opts {
                self.validate_feature_values(&opts.uninterpreted_option, file_name)?;
            }

            // Determine effective field_presence (field-level overrides parent)
            let field_presence = field_opts
                .and_then(|opts| get_feature_value(&opts.uninterpreted_option, "field_presence"))
                .or_else(|| parent_field_presence.map(|s| s.to_string()));

            // Implicit presence fields can't specify defaults
            if field_presence.as_deref() == Some("IMPLICIT") && field.default_value.is_some() {
                return Err(AnalyzeError {
                    message: "Implicit presence fields can't specify defaults.".to_string(),
                    file: Some(file_name.to_string()),
                    span: None,
                });
            }
        }

        // Recurse into nested messages
        for nested in &msg.nested_type {
            self.validate_features_in_message(nested, file_name, parent_field_presence)?;
        }

        Ok(())
    }

    fn validate_feature_values(
        &self,
        options: &[UninterpretedOption],
        file_name: &str,
    ) -> Result<(), AnalyzeError> {
        for opt in options {
            if opt.name.len() >= 2
                && opt.name[0].name_part == "features"
                && !opt.name[0].is_extension
            {
                let feature_name = &opt.name[1].name_part;
                if let Some(ref val) = opt.identifier_value {
                    // Check for _UNKNOWN suffix (e.g., FIELD_PRESENCE_UNKNOWN)
                    let expected_unknown = format!("{}_UNKNOWN", feature_name.to_uppercase());
                    if *val == expected_unknown {
                        return Err(AnalyzeError {
                            message: format!(
                                "Feature field `{}` must resolve to a known value, found {}",
                                feature_name, val
                            ),
                            file: Some(file_name.to_string()),
                            span: None,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Consume context and produce the final FileDescriptorSet.
    fn into_descriptor_set(self) -> FileDescriptorSet {
        let mut files = Vec::new();
        for name in &self.load_order {
            if let Some(file) = self.loaded.get(name) {
                let mut file = file.clone();
                // Normalize labels: protoc always sets LABEL_OPTIONAL on fields
                // that have no explicit label. This includes:
                // - proto3 implicit fields (no label keyword)
                // - editions implicit fields
                // - oneof fields (labels forbidden in syntax, but protoc still sets LABEL_OPTIONAL)
                Self::normalize_labels(&mut file.message_type);
                for ext in &mut file.extension {
                    if ext.label.is_none() {
                        ext.label = Some(FieldLabel::Optional);
                    }
                }
                files.push(file);
            }
        }
        FileDescriptorSet { file: files }
    }

    /// Set LABEL_OPTIONAL on fields that have no explicit label (proto3/editions).
    fn normalize_labels(messages: &mut [DescriptorProto]) {
        for msg in messages {
            for field in &mut msg.field {
                if field.label.is_none() {
                    field.label = Some(FieldLabel::Optional);
                }
            }
            for ext in &mut msg.extension {
                if ext.label.is_none() {
                    ext.label = Some(FieldLabel::Optional);
                }
            }
            Self::normalize_labels(&mut msg.nested_type);
        }
    }
}

/// Extract the identifier value of a `features.<name>` uninterpreted option.
fn get_feature_value(options: &[UninterpretedOption], feature_name: &str) -> Option<String> {
    for opt in options {
        if opt.name.len() >= 2
            && opt.name[0].name_part == "features"
            && !opt.name[0].is_extension
            && opt.name[1].name_part == feature_name
        {
            return opt.identifier_value.clone();
        }
    }
    None
}
