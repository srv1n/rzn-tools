use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Prompt {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
    pub messages: Vec<PromptMessage>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PromptMessage {
    pub role: String, // "user" or "assistant"
    pub content: PromptMessageContent,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum PromptMessageContent {
    Text {
        r#type: String,
        text: String,
    },
    Image {
        r#type: String,
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Resource {
        r#type: String,
        resource: crate::resources::Resource,
    },
}
