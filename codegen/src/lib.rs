pub mod rust_gen;
#[cfg(feature = "grpc")]
mod service_gen;
pub use rust_gen::{generate_rust, CodeGenError};
