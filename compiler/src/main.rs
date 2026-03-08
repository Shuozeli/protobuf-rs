use protoc_rs_analyzer::{analyze_files, AnalyzeError};
use protoc_rs_compiler::descriptor_set::serialize_descriptor_set;
use protoc_rs_compiler::resolver::{relative_proto_path, FsResolver};
use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        process::exit(if args.is_empty() { 1 } else { 0 });
    }

    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("protoc-rs: {}", e);
            process::exit(1);
        }
    };

    if let Err(e) = run(parsed) {
        eprintln!("protoc-rs: {}", e);
        process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "Usage: protoc-rs [OPTIONS] <FILES>...

Options:
  -I, --proto_path <PATH>           Import search path (repeatable)
  -o, --descriptor_set_out <FILE>   Write FileDescriptorSet binary to FILE
      --include_imports             Include imported files in descriptor set
      --include_source_info         Include source code info in descriptor set
      --rust_out <DIR>               Generate prost-compatible Rust code to DIR
      --dump-schema                 Dump parsed schema to stderr
  -h, --help                        Show this help message"
    );
}

struct ParsedArgs {
    include_paths: Vec<PathBuf>,
    descriptor_set_out: Option<PathBuf>,
    rust_out: Option<PathBuf>,
    include_imports: bool,
    _include_source_info: bool,
    dump_schema: bool,
    proto_files: Vec<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut include_paths = Vec::new();
    let mut descriptor_set_out = None;
    let mut rust_out = None;
    let mut include_imports = false;
    let mut include_source_info = false;
    let mut dump_schema = false;
    let mut proto_files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-I" | "--proto_path" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| format!("{} requires an argument", arg))?;
                include_paths.push(PathBuf::from(path));
            }
            "-o" | "--descriptor_set_out" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| format!("{} requires an argument", arg))?;
                descriptor_set_out = Some(PathBuf::from(path));
            }
            "--include_imports" => {
                include_imports = true;
            }
            "--include_source_info" => {
                include_source_info = true;
            }
            "--dump-schema" | "--dump_schema" => {
                dump_schema = true;
            }
            "--rust_out" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| format!("{} requires an argument", arg))?;
                rust_out = Some(PathBuf::from(path));
            }
            _ if arg.starts_with("-I") => {
                // Support -I<path> (no space)
                include_paths.push(PathBuf::from(&arg[2..]));
            }
            _ if arg.starts_with("--proto_path=") => {
                include_paths.push(PathBuf::from(&arg["--proto_path=".len()..]));
            }
            _ if arg.starts_with("--descriptor_set_out=") => {
                descriptor_set_out =
                    Some(PathBuf::from(&arg["--descriptor_set_out=".len()..]));
            }
            _ if arg.starts_with("--rust_out=") => {
                rust_out = Some(PathBuf::from(&arg["--rust_out=".len()..]));
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown flag: {}", arg));
            }
            _ => {
                proto_files.push(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    if proto_files.is_empty() {
        return Err("no input files".to_string());
    }

    // If no include paths given, use the current directory
    if include_paths.is_empty() {
        include_paths.push(PathBuf::from("."));
    }

    Ok(ParsedArgs {
        include_paths,
        descriptor_set_out,
        rust_out,
        include_imports,
        _include_source_info: include_source_info,
        dump_schema,
        proto_files,
    })
}

fn run(args: ParsedArgs) -> Result<(), AnalyzeError> {
    let resolver = FsResolver::new(args.include_paths.clone());

    // Compute relative proto paths for the root files
    let root_names: Vec<String> = args
        .proto_files
        .iter()
        .map(|p| {
            let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            let canon_includes: Vec<PathBuf> = args
                .include_paths
                .iter()
                .map(|ip| std::fs::canonicalize(ip).unwrap_or_else(|_| ip.clone()))
                .collect();
            relative_proto_path(&abs, &canon_includes)
        })
        .collect();

    let root_refs: Vec<&str> = root_names.iter().map(|s| s.as_str()).collect();
    let fds = analyze_files(&root_refs, &resolver)?;

    if args.dump_schema {
        for file in &fds.file {
            eprintln!("{:#?}", file);
        }
    }

    if let Some(ref out_path) = args.descriptor_set_out {
        let mut output_fds = fds.clone();

        // If --include_imports is not set, only include the root files
        if !args.include_imports {
            output_fds.file.retain(|f| {
                f.name
                    .as_ref()
                    .map(|n| root_names.contains(n))
                    .unwrap_or(false)
            });
        }

        // Strip source_code_info unless --include_source_info is set
        if !args._include_source_info {
            for file in &mut output_fds.file {
                file.source_code_info = None;
            }
        }

        let bytes = serialize_descriptor_set(&output_fds);

        if out_path.to_str() == Some("/dev/stdout") || out_path.to_str() == Some("-") {
            use std::io::Write;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| AnalyzeError {
                    message: format!("failed to write to stdout: {}", e),
                    file: None,
                    span: None,
                })?;
        } else {
            // Create parent directory if needed
            if let Some(parent) = out_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| AnalyzeError {
                        message: format!(
                            "failed to create directory {}: {}",
                            parent.display(),
                            e
                        ),
                        file: None,
                        span: None,
                    })?;
                }
            }
            std::fs::write(out_path, &bytes).map_err(|e| AnalyzeError {
                message: format!("failed to write {}: {}", out_path.display(), e),
                file: None,
                span: None,
            })?;
        }

        eprintln!("Wrote {} bytes to {}", bytes.len(), out_path.display());
    }

    if let Some(ref out_dir) = args.rust_out {
        let files = protoc_rs_codegen::generate_rust(&fds).map_err(|e| AnalyzeError {
            message: format!("codegen failed: {}", e),
            file: None,
            span: None,
        })?;

        if !out_dir.exists() {
            std::fs::create_dir_all(out_dir).map_err(|e| AnalyzeError {
                message: format!("failed to create directory {}: {}", out_dir.display(), e),
                file: None,
                span: None,
            })?;
        }

        for (filename, content) in &files {
            let path = out_dir.join(filename);
            std::fs::write(&path, content).map_err(|e| AnalyzeError {
                message: format!("failed to write {}: {}", path.display(), e),
                file: None,
                span: None,
            })?;
        }

        eprintln!(
            "Generated {} Rust file(s) in {}",
            files.len(),
            out_dir.display()
        );
    }

    if args.descriptor_set_out.is_none() && !args.dump_schema && args.rust_out.is_none() {
        // Nothing to do -- at least confirm the files parse successfully
        eprintln!(
            "Parsed {} file(s) successfully ({} total with imports)",
            root_names.len(),
            fds.file.len()
        );
    }

    Ok(())
}
