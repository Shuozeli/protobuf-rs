//! Random `.proto` schema generation (proto3 syntax).

use rand::rngs::StdRng;
use rand::Rng;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GenConfig {
    pub max_messages: usize,
    pub max_enums: usize,
    pub max_fields_per_message: usize,
    pub max_enum_values: usize,
    pub max_nesting_depth: usize,
    /// Probability that a field is `repeated`.
    pub prob_repeated: f64,
    /// Probability that a field references an enum type (when enums exist).
    pub prob_enum_field: f64,
    /// Probability that a field references a nested message.
    pub prob_message_field: f64,
    /// Probability that a message has a nested message definition.
    pub prob_nested_message: f64,
}

impl Default for GenConfig {
    fn default() -> Self {
        Self {
            max_messages: 4,
            max_enums: 2,
            max_fields_per_message: 6,
            max_enum_values: 5,
            max_nesting_depth: 3,
            prob_repeated: 0.25,
            prob_enum_field: 0.15,
            prob_message_field: 0.3,
            prob_nested_message: 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Schema IR (internal representation for generation)
// ---------------------------------------------------------------------------

pub struct SchemaIR {
    pub package: Option<String>,
    pub root_message: String,
    pub messages: Vec<MessageDef>,
    pub enums: Vec<EnumDef>,
}

pub struct MessageDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub nested_messages: Vec<MessageDef>,
    pub nested_enums: Vec<EnumDef>,
}

pub struct FieldDef {
    pub name: String,
    pub number: i32,
    pub field_type: FieldTypeDef,
    pub repeated: bool,
}

#[derive(Clone)]
pub enum FieldTypeDef {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
    Enum(String),
    Message(String),
}

impl FieldTypeDef {
    fn proto_name(&self) -> &str {
        match self {
            Self::Double => "double",
            Self::Float => "float",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Uint32 => "uint32",
            Self::Uint64 => "uint64",
            Self::Sint32 => "sint32",
            Self::Sint64 => "sint64",
            Self::Fixed32 => "fixed32",
            Self::Fixed64 => "fixed64",
            Self::Sfixed32 => "sfixed32",
            Self::Sfixed64 => "sfixed64",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Bytes => "bytes",
            Self::Enum(name) | Self::Message(name) => name,
        }
    }
}

pub struct EnumDef {
    pub name: String,
    pub values: Vec<(String, i32)>,
}

// ---------------------------------------------------------------------------
// Name generation
// ---------------------------------------------------------------------------

const MSG_NAMES: &[&str] = &[
    "Request", "Response", "Event", "Record", "Entry", "Item", "Status", "Config", "Payload",
    "Header", "Detail", "Summary", "Report", "Action", "Update", "Result", "Metadata", "Info",
    "Params", "Context",
];

const ENUM_NAMES: &[&str] = &[
    "Kind",
    "Type",
    "State",
    "Level",
    "Mode",
    "Category",
    "Priority",
    "Direction",
    "Phase",
    "Color",
];

const FIELD_NAMES: &[&str] = &[
    "id",
    "name",
    "value",
    "count",
    "data",
    "label",
    "code",
    "score",
    "flag",
    "size",
    "index",
    "offset",
    "ratio",
    "weight",
    "tag",
    "timestamp",
    "duration",
    "message",
    "description",
    "version",
    "key",
    "status",
    "level",
    "amount",
    "total",
    "rate",
    "limit",
    "position",
    "rank",
    "quantity",
];

const ENUM_VALUE_NAMES: &[&str] = &[
    "UNKNOWN",
    "DEFAULT",
    "ACTIVE",
    "INACTIVE",
    "PENDING",
    "COMPLETED",
    "CANCELLED",
    "ERROR",
    "WARNING",
    "INFO",
    "DEBUG",
    "CRITICAL",
    "LOW",
    "MEDIUM",
    "HIGH",
    "URGENT",
    "NORMAL",
    "SPECIAL",
];

fn pick_unique_name(rng: &mut StdRng, names: &[&str], used: &[String]) -> String {
    for _ in 0..100 {
        let name = names[rng.gen_range(0..names.len())];
        if !used.iter().any(|u| u == name) {
            return name.to_string();
        }
    }
    // Fallback: append a number
    let base = names[rng.gen_range(0..names.len())];
    let suffix = rng.gen_range(100..999);
    format!("{base}{suffix}")
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

const SCALAR_TYPES: &[FieldTypeDef] = &[
    FieldTypeDef::Double,
    FieldTypeDef::Float,
    FieldTypeDef::Int32,
    FieldTypeDef::Int64,
    FieldTypeDef::Uint32,
    FieldTypeDef::Uint64,
    FieldTypeDef::Sint32,
    FieldTypeDef::Sint64,
    FieldTypeDef::Fixed32,
    FieldTypeDef::Fixed64,
    FieldTypeDef::Sfixed32,
    FieldTypeDef::Sfixed64,
    FieldTypeDef::Bool,
    FieldTypeDef::String,
    FieldTypeDef::Bytes,
];

pub fn generate_schema(rng: &mut StdRng, config: &GenConfig) -> SchemaIR {
    let mut used_enum_names = Vec::new();
    let mut enums = Vec::new();
    let num_enums = rng.gen_range(1..=config.max_enums);
    for _ in 0..num_enums {
        let name = pick_unique_name(rng, ENUM_NAMES, &used_enum_names);
        used_enum_names.push(name.clone());
        enums.push(generate_enum(rng, &name, config));
    }

    let mut used_msg_names = Vec::new();
    let mut messages = Vec::new();
    let num_messages = rng.gen_range(1..=config.max_messages);
    for _ in 0..num_messages {
        let name = pick_unique_name(rng, MSG_NAMES, &used_msg_names);
        used_msg_names.push(name.clone());
        let enum_names: Vec<String> = enums.iter().map(|e| e.name.clone()).collect();
        let msg_names: Vec<String> = messages
            .iter()
            .map(|m: &MessageDef| m.name.clone())
            .collect();
        messages.push(generate_message(
            rng,
            &name,
            config,
            &enum_names,
            &msg_names,
            0,
        ));
    }

    let root_message = messages[0].name.clone();

    SchemaIR {
        package: None,
        root_message,
        messages,
        enums,
    }
}

fn generate_enum(rng: &mut StdRng, name: &str, config: &GenConfig) -> EnumDef {
    let num_values = rng.gen_range(2..=config.max_enum_values);
    let mut used_names = Vec::new();
    let mut values = Vec::new();

    // Proto3 requires first value to be 0
    let prefix = name.to_uppercase();
    let zero_name = format!("{prefix}_UNKNOWN");
    values.push((zero_name.clone(), 0));
    used_names.push(zero_name);

    for i in 1..num_values {
        let base = pick_unique_name(rng, ENUM_VALUE_NAMES, &used_names);
        let full_name = format!("{prefix}_{base}");
        used_names.push(full_name.clone());
        values.push((full_name, i as i32));
    }

    EnumDef {
        name: name.to_string(),
        values,
    }
}

fn generate_message(
    rng: &mut StdRng,
    name: &str,
    config: &GenConfig,
    enum_names: &[String],
    msg_names: &[String],
    depth: usize,
) -> MessageDef {
    let num_fields = rng.gen_range(1..=config.max_fields_per_message.max(1));
    let mut used_field_names = Vec::new();
    let mut fields = Vec::new();

    // Generate nested messages (more likely at shallow depths)
    let mut nested_messages = Vec::new();
    let nested_enums = Vec::new();
    let all_enum_names: Vec<String> = enum_names.to_vec();
    let mut all_msg_names: Vec<String> = msg_names.to_vec();

    if depth < config.max_nesting_depth && config.max_nesting_depth > 0 {
        // At depth 0, always generate at least one nested message
        let should_nest = depth == 0 || rng.gen_bool(config.prob_nested_message.min(1.0));
        if should_nest {
            // Generate 1-2 nested messages
            let num_nested = if depth == 0 { rng.gen_range(1..=2) } else { 1 };
            for _ in 0..num_nested {
                let nested_name = pick_unique_name(rng, MSG_NAMES, &all_msg_names);
                all_msg_names.push(nested_name.clone());
                nested_messages.push(generate_message(
                    rng,
                    &nested_name,
                    config,
                    &all_enum_names,
                    &all_msg_names,
                    depth + 1,
                ));
            }
        }
    }

    // Collect nested message names for forced references
    let nested_msg_names: Vec<String> = nested_messages.iter().map(|m| m.name.clone()).collect();
    let mut forced_nested_ref = !nested_msg_names.is_empty();

    for i in 0..num_fields {
        let field_name = pick_unique_name(rng, FIELD_NAMES, &used_field_names);
        used_field_names.push(field_name.clone());

        // Force at least one field to reference a nested message if available
        let field_type = if forced_nested_ref && i == num_fields - 1 {
            // Last field: force a nested message reference if we haven't used one yet
            forced_nested_ref = false;
            let name = &nested_msg_names[rng.gen_range(0..nested_msg_names.len())];
            FieldTypeDef::Message(name.clone())
        } else {
            let ft = choose_field_type(rng, config, &all_enum_names, &all_msg_names, depth);
            if matches!(&ft, FieldTypeDef::Message(n) if nested_msg_names.contains(n)) {
                forced_nested_ref = false;
            }
            ft
        };
        let repeated = rng.gen_bool(config.prob_repeated.min(1.0));

        fields.push(FieldDef {
            name: field_name,
            number: (i + 1) as i32,
            field_type,
            repeated,
        });
    }

    MessageDef {
        name: name.to_string(),
        fields,
        nested_messages,
        nested_enums,
    }
}

fn choose_field_type(
    rng: &mut StdRng,
    config: &GenConfig,
    enum_names: &[String],
    msg_names: &[String],
    depth: usize,
) -> FieldTypeDef {
    let roll: f64 = rng.gen();
    let mut threshold = 0.0;

    // Try enum field
    if !enum_names.is_empty() {
        threshold += config.prob_enum_field;
        if roll < threshold {
            let name = &enum_names[rng.gen_range(0..enum_names.len())];
            return FieldTypeDef::Enum(name.clone());
        }
    }

    // Try message field (only if not too deep)
    if !msg_names.is_empty() && depth < config.max_nesting_depth {
        threshold += config.prob_message_field;
        if roll < threshold {
            let name = &msg_names[rng.gen_range(0..msg_names.len())];
            return FieldTypeDef::Message(name.clone());
        }
    }

    // Scalar type
    SCALAR_TYPES[rng.gen_range(0..SCALAR_TYPES.len())].clone()
}

// ---------------------------------------------------------------------------
// .proto text output
// ---------------------------------------------------------------------------

impl SchemaIR {
    pub fn to_proto_text(&self) -> String {
        let mut out = String::new();
        out.push_str("syntax = \"proto3\";\n\n");

        if let Some(ref pkg) = self.package {
            out.push_str(&format!("package {pkg};\n\n"));
        }

        for e in &self.enums {
            write_enum(&mut out, e, 0);
            out.push('\n');
        }

        for m in &self.messages {
            write_message(&mut out, m, 0);
            out.push('\n');
        }

        out
    }
}

fn write_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_enum(out: &mut String, e: &EnumDef, indent: usize) {
    write_indent(out, indent);
    out.push_str(&format!("enum {} {{\n", e.name));
    for (name, number) in &e.values {
        write_indent(out, indent + 1);
        out.push_str(&format!("{name} = {number};\n"));
    }
    write_indent(out, indent);
    out.push_str("}\n");
}

fn write_message(out: &mut String, m: &MessageDef, indent: usize) {
    write_indent(out, indent);
    out.push_str(&format!("message {} {{\n", m.name));

    for ne in &m.nested_enums {
        write_enum(out, ne, indent + 1);
    }
    for nm in &m.nested_messages {
        write_message(out, nm, indent + 1);
    }

    for f in &m.fields {
        write_indent(out, indent + 1);
        let repeated = if f.repeated { "repeated " } else { "" };
        out.push_str(&format!(
            "{}{} {} = {};\n",
            repeated,
            f.field_type.proto_name(),
            f.name,
            f.number
        ));
    }

    write_indent(out, indent);
    out.push_str("}\n");
}
