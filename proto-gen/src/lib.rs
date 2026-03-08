//! Random protobuf schema and binary data generator.
//!
//! Produces valid `.proto` text (proto3 syntax) and corresponding binary-encoded
//! protobuf data. Uses a deterministic seed so the same seed always produces the
//! same schema and data.

mod data_gen;
mod schema_gen;

use rand::rngs::StdRng;
use rand::SeedableRng;

pub use schema_gen::GenConfig;

/// Result of random generation: a `.proto` schema and matching binary data.
pub struct Generated {
    pub schema_text: String,
    pub binary_data: Vec<u8>,
    pub root_message: String,
}

/// Generate a random `.proto` schema and conforming binary data.
pub fn generate(seed: u64, config: GenConfig) -> Generated {
    let mut rng = StdRng::seed_from_u64(seed);
    let schema = schema_gen::generate_schema(&mut rng, &config);
    let binary = data_gen::generate_data(&mut rng, &schema, &config);
    Generated {
        root_message: schema.root_message.clone(),
        schema_text: schema.to_proto_text(),
        binary_data: binary,
    }
}

#[cfg(test)]
mod tests;
