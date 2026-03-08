use crate::FileResolver;

/// Well-known Google protobuf types, embedded at compile time.
static WELL_KNOWN_PROTOS: &[(&str, &str)] = &[
    (
        "google/protobuf/any.proto",
        include_str!("well_known_protos/google/protobuf/any.proto"),
    ),
    (
        "google/protobuf/api.proto",
        include_str!("well_known_protos/google/protobuf/api.proto"),
    ),
    (
        "google/protobuf/duration.proto",
        include_str!("well_known_protos/google/protobuf/duration.proto"),
    ),
    (
        "google/protobuf/empty.proto",
        include_str!("well_known_protos/google/protobuf/empty.proto"),
    ),
    (
        "google/protobuf/field_mask.proto",
        include_str!("well_known_protos/google/protobuf/field_mask.proto"),
    ),
    (
        "google/protobuf/source_context.proto",
        include_str!("well_known_protos/google/protobuf/source_context.proto"),
    ),
    (
        "google/protobuf/struct.proto",
        include_str!("well_known_protos/google/protobuf/struct.proto"),
    ),
    (
        "google/protobuf/timestamp.proto",
        include_str!("well_known_protos/google/protobuf/timestamp.proto"),
    ),
    (
        "google/protobuf/type.proto",
        include_str!("well_known_protos/google/protobuf/type.proto"),
    ),
    (
        "google/protobuf/wrappers.proto",
        include_str!("well_known_protos/google/protobuf/wrappers.proto"),
    ),
];

/// Resolver that knows about well-known Google protobuf types.
pub struct WellKnownResolver;

impl FileResolver for WellKnownResolver {
    fn resolve(&self, name: &str) -> Option<String> {
        WELL_KNOWN_PROTOS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, content)| content.to_string())
    }
}

/// Combines a user-provided resolver with well-known types.
/// User resolver takes priority.
pub struct CombinedResolver<'a> {
    user: &'a dyn FileResolver,
}

impl<'a> CombinedResolver<'a> {
    pub fn new(user: &'a dyn FileResolver) -> Self {
        Self { user }
    }
}

impl<'a> FileResolver for CombinedResolver<'a> {
    fn resolve(&self, name: &str) -> Option<String> {
        self.user
            .resolve(name)
            .or_else(|| WellKnownResolver.resolve(name))
    }
}
