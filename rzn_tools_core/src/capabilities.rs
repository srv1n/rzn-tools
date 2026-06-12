use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ConnectorConfigSchema {
    pub fields: Vec<Field>, // Single field type for everything
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Field {
    pub name: String,
    pub label: String,
    pub field_type: FieldType,
    pub required: bool,
    pub description: Option<String>,
    pub options: Option<Vec<String>>, // For select fields
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum FieldType {
    Text,
    Secret, // Use for API keys, passwords, cookies – anything sensitive
    Number,
    Boolean,
    Select { options: Vec<String> },
}
