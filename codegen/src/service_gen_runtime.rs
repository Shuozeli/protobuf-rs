//! gRPC service code generation for the runtime backend.
//!
//! Generates service traits and client stubs that use `protoc_rs_runtime::Message`
//! instead of `prost::Message`.

use heck::{ToSnakeCase, ToUpperCamelCase};
use protoc_rs_schema::{FileDescriptorProto, ServiceDescriptorProto};

use crate::rust_gen::fqn_to_rust_path;

/// Generate runtime-backed gRPC service code for all services in a file.
///
/// Returns Rust source code with server traits and client stubs.
pub fn generate_runtime_services(file: &FileDescriptorProto, package: &str) -> String {
    let mut buf = String::new();

    if file.service.is_empty() {
        return buf;
    }

    for svc in &file.service {
        buf.push_str(&gen_service_trait(svc, package));
        buf.push('\n');
        buf.push_str(&gen_client_stub(svc, package));
        buf.push('\n');
    }

    buf
}

fn gen_service_trait(svc: &ServiceDescriptorProto, package: &str) -> String {
    let name = svc.name.as_deref().unwrap_or("Unknown");
    let trait_name = format!("{}Server", name.to_upper_camel_case());

    let mut buf = String::new();
    buf.push_str(&format!("/// Server trait for the `{name}` service.\n"));
    buf.push_str("#[async_trait::async_trait]\n");
    buf.push_str(&format!(
        "pub trait {trait_name}: Send + Sync + 'static {{\n"
    ));

    for method in &svc.method {
        let method_name = method.name.as_deref().unwrap_or("unknown").to_snake_case();
        let input = resolve_type(method.input_type.as_deref().unwrap_or(""), package);
        let output = resolve_type(method.output_type.as_deref().unwrap_or(""), package);

        let client_streaming = method.client_streaming.unwrap_or(false);
        let server_streaming = method.server_streaming.unwrap_or(false);

        match (client_streaming, server_streaming) {
            (false, false) => {
                // Unary
                buf.push_str(&format!(
                    "    async fn {method_name}(&self, request: {input}) -> ::core::result::Result<{output}, ServiceError>;\n"
                ));
            }
            (false, true) => {
                // Server streaming
                buf.push_str(&format!(
                    "    async fn {method_name}(&self, request: {input}) -> ::core::result::Result<::std::vec::Vec<{output}>, ServiceError>;\n"
                ));
            }
            (true, false) => {
                // Client streaming
                buf.push_str(&format!(
                    "    async fn {method_name}(&self, requests: ::std::vec::Vec<{input}>) -> ::core::result::Result<{output}, ServiceError>;\n"
                ));
            }
            (true, true) => {
                // Bidirectional streaming
                buf.push_str(&format!(
                    "    async fn {method_name}(&self, requests: ::std::vec::Vec<{input}>) -> ::core::result::Result<::std::vec::Vec<{output}>, ServiceError>;\n"
                ));
            }
        }
    }

    buf.push_str("}\n");
    buf
}

fn gen_client_stub(svc: &ServiceDescriptorProto, package: &str) -> String {
    let name = svc.name.as_deref().unwrap_or("Unknown");
    let client_name = format!("{}Client", name.to_upper_camel_case());

    let mut buf = String::new();
    buf.push_str(&format!("/// Client for the `{name}` service.\n"));
    buf.push_str(&format!("pub struct {client_name} {{\n"));
    buf.push_str("    _endpoint: ::std::string::String,\n");
    buf.push_str("}\n\n");

    buf.push_str(&format!("impl {client_name} {{\n"));
    buf.push_str("    pub fn new(endpoint: impl Into<::std::string::String>) -> Self {\n");
    buf.push_str("        Self { _endpoint: endpoint.into() }\n");
    buf.push_str("    }\n");

    for method in &svc.method {
        let method_name = method.name.as_deref().unwrap_or("unknown").to_snake_case();
        let input = resolve_type(method.input_type.as_deref().unwrap_or(""), package);
        let output = resolve_type(method.output_type.as_deref().unwrap_or(""), package);
        let _proto_method_path = format!(
            "/{}.{}/{}",
            package,
            name,
            method.name.as_deref().unwrap_or("Unknown")
        );

        buf.push_str(&format!(
            "\n    /// Stub for `{method_name}` -- implement transport layer to connect.\n"
        ));
        buf.push_str(&format!(
            "    pub async fn {method_name}(&self, _request: {input}) -> ::core::result::Result<{output}, ServiceError> {{\n"
        ));
        buf.push_str("        todo!(\"connect to gRPC endpoint\")\n");
        buf.push_str("    }\n");
    }

    buf.push_str("}\n");
    buf
}

fn resolve_type(fqn: &str, current_scope: &str) -> String {
    fqn_to_rust_path(fqn, current_scope)
}

/// Error type for service operations.
/// Generated once at the top of the file.
pub fn service_error_type() -> &'static str {
    r#"
/// Error type for gRPC service operations.
#[derive(Debug)]
pub struct ServiceError {
    pub message: ::std::string::String,
}

impl ::std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl ::std::error::Error for ServiceError {}
"#
}
