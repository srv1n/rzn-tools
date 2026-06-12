pub mod config;
pub mod connectors;
pub mod fetch;
pub mod get;
pub mod ingest;
pub mod list;
pub mod pricing;
pub mod report;
pub mod search;
pub mod serve;
pub mod setup;
pub mod skills;
pub mod tool_mappings;
pub mod tools;
pub mod usage;
pub mod usage_helpers;
pub mod workflows;

#[cfg(test)]
mod tool_mapping_audit;

use owo_colors::OwoColorize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Connector '{0}' not found")]
    ConnectorNotFound(String),

    #[error("Tool '{0}' not found for connector '{1}'")]
    ToolNotFound(String, String),

    #[error("Authentication required for connector '{0}'")]
    #[allow(dead_code)]
    AuthenticationRequired(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Core library error: {0}")]
    Core(#[from] rzn_tools_core::error::ConnectorError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CommandError>;

/// Copy text to the system clipboard and display a confirmation message
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    use arboard::Clipboard;

    let mut clipboard = Clipboard::new()
        .map_err(|e| CommandError::Clipboard(format!("Failed to access clipboard: {}", e)))?;

    clipboard
        .set_text(text.to_string())
        .map_err(|e| CommandError::Clipboard(format!("Failed to copy to clipboard: {}", e)))?;

    eprintln!(
        "{} Output copied to clipboard ({} chars)",
        "✓".green().bold(),
        text.len()
    );

    Ok(())
}
