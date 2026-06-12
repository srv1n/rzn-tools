use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LogMessage {
    pub level: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
    pub data: Value,
}
