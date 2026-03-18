//! gRPC service code generation for protobuf schemas.
//!
//! Bridges protobuf-rs schema types to grpc-codegen for generating
//! server traits and client stubs.

use grpc_codegen::protobuf::service_from_proto;
use grpc_codegen::{client_gen, server_gen};
use protoc_rs_schema::FileDescriptorProto;

/// Generate gRPC service code (server + client) for all services in a file.
///
/// Returns the generated Rust source code as a string, or empty string if
/// no services are defined.
pub fn generate_services(file: &FileDescriptorProto) -> Result<String, crate::CodeGenError> {
    let package = file.package.as_deref().unwrap_or("");

    if file.service.is_empty() {
        return Ok(String::new());
    }

    let mut tokens = proc_macro2::TokenStream::new();

    for svc_proto in &file.service {
        let svc_def = service_from_proto(svc_proto, package, "super");
        tokens.extend(server_gen::generate(&svc_def));
        tokens.extend(client_gen::generate(&svc_def));
    }

    let file = syn::parse2::<syn::File>(tokens)
        .map_err(|e| crate::CodeGenError::ServiceGen(e.to_string()))?;
    Ok(prettyplease::unparse(&file))
}
