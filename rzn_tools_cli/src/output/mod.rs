use crate::cli::OutputFormat;
use crate::commands::{CommandError, Result};
use rzn_tools_core::resolver::PatternInfo;
use rzn_tools_core::ServerInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod pretty;
pub use pretty::format_pretty;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum OutputData {
    ConnectorList(Vec<ServerInfo>),
    SearchResults {
        connector: String,
        query: String,
        results: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    FederatedResults {
        query: String,
        profile: Option<String>,
        results: Value,
    },
    ResourceData {
        connector: String,
        id: String,
        data: Value,
    },
    ToolsList {
        connector: Option<String>,
        tools: Value,
    },
    CallResult {
        connector: String,
        tool: String,
        result: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    ToolResult(Value),
    PricingInfo {
        report: Value,
    },
    UsageReport {
        report: Value,
    },
    Patterns(Vec<PatternInfo>),
    ConfigInfo(Value),
    ErrorMessage(String),
}

pub fn format_output(data: &OutputData, format: &OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputFormat::Yaml => {
            println!("{}", serde_yaml::to_string(data)?);
        }
        OutputFormat::Text => {
            format_text_output(data)?;
        }
        OutputFormat::Markdown => {
            format_markdown_output(data)?;
        }
        OutputFormat::Pretty => {
            format_pretty_output(data)?;
        }
    }
    Ok(())
}

fn format_text_output(data: &OutputData) -> Result<()> {
    match data {
        OutputData::ConnectorList(connectors) => {
            for connector in connectors {
                println!("{}: {}", connector.name, connector.description);
            }
        }
        OutputData::SearchResults {
            connector,
            query,
            results,
            meta,
        } => {
            println!("Search results for '{}' using {}:", query, connector);
            println!("{}", serde_json::to_string_pretty(results)?);
            if let Some(meta) = meta {
                println!("Meta:");
                println!("{}", serde_json::to_string_pretty(meta)?);
            }
        }
        OutputData::FederatedResults {
            query,
            profile,
            results,
        } => {
            if let Some(profile) = profile {
                println!(
                    "Federated search for '{}' using profile '{}':",
                    query, profile
                );
            } else {
                println!("Federated search for '{}':", query);
            }
            println!("{}", serde_json::to_string_pretty(results)?);
        }
        OutputData::ResourceData {
            connector,
            id,
            data,
        } => {
            println!("Resource '{}' from {}:", id, connector);
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputData::ToolsList { connector, tools } => {
            if let Some(connector) = connector {
                println!("Tools for {}:", connector);
            } else {
                println!("Available tools:");
            }
            println!("{}", serde_json::to_string_pretty(tools)?);
        }
        OutputData::CallResult {
            connector,
            tool,
            result,
            meta,
        } => {
            println!("Tool {}.{}", connector, tool);
            println!("{}", serde_json::to_string_pretty(result)?);
            if let Some(meta) = meta {
                println!("Meta:");
                println!("{}", serde_json::to_string_pretty(meta)?);
            }
        }
        OutputData::ConfigInfo(config) => {
            println!("Configuration:");
            println!("{}", serde_json::to_string_pretty(config)?);
        }
        OutputData::ErrorMessage(msg) => {
            eprintln!("Error: {}", msg);
        }
        OutputData::ToolResult(result) => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
        OutputData::PricingInfo { report } => {
            println!("Pricing:");
            println!("{}", serde_json::to_string_pretty(report)?);
        }
        OutputData::UsageReport { report } => {
            println!("Usage:");
            println!("{}", serde_json::to_string_pretty(report)?);
        }
        OutputData::Patterns(patterns) => {
            for p in patterns {
                println!(
                    "{}\t{}\t{}\t{}",
                    p.connector, p.tool, p.example, p.description
                );
            }
        }
    }
    Ok(())
}

fn format_pretty_output(data: &OutputData) -> Result<()> {
    use owo_colors::OwoColorize;

    match data {
        OutputData::ConnectorList(connectors) => {
            println!("{}", "Available Connectors".cyan().bold());
            println!();
            let value = serde_json::to_value(connectors)?;
            println!("{}", format_pretty(&value));
        }
        OutputData::SearchResults {
            connector,
            query,
            results,
            meta,
        } => {
            println!(
                "{} {} {} {}",
                "Search:".dimmed(),
                query.cyan().bold(),
                "via".dimmed(),
                connector.green()
            );
            println!();
            println!("{}", format_pretty(results));
            if let Some(meta) = meta {
                println!();
                println!("{}", "Meta".dimmed());
                println!("{}", format_pretty(meta));
            }
        }
        OutputData::FederatedResults {
            query,
            profile,
            results,
        } => {
            if let Some(profile) = profile {
                println!(
                    "{} {} {} {}",
                    "Federated search:".dimmed(),
                    query.cyan().bold(),
                    "profile:".dimmed(),
                    profile.green()
                );
            } else {
                println!("{} {}", "Federated search:".dimmed(), query.cyan().bold());
            }
            println!();
            println!("{}", format_pretty(results));
        }
        OutputData::ResourceData {
            connector,
            id,
            data,
        } => {
            println!(
                "{} {} {} {}",
                "Resource:".dimmed(),
                id.cyan().bold(),
                "from".dimmed(),
                connector.green()
            );
            println!();
            println!("{}", format_pretty(data));
        }
        OutputData::ToolsList { connector, tools } => {
            if let Some(connector) = connector {
                println!("{} {}", "Tools for".dimmed(), connector.green().bold());
            } else {
                println!("{}", "Available Tools".cyan().bold());
            }
            println!();
            println!("{}", format_pretty(tools));
        }
        OutputData::CallResult {
            connector,
            tool,
            result,
            meta,
        } => {
            println!(
                "{} {}.{}",
                "Result:".dimmed(),
                connector.green(),
                tool.cyan().bold()
            );
            println!();
            println!("{}", format_pretty(result));
            if let Some(meta) = meta {
                println!();
                println!("{}", "Meta".dimmed());
                println!("{}", format_pretty(meta));
            }
        }
        OutputData::ToolResult(result) => {
            println!("{}", format_pretty(result));
        }
        OutputData::PricingInfo { report } => {
            println!("{}", "Pricing".cyan().bold());
            println!();
            println!("{}", format_pretty(report));
        }
        OutputData::UsageReport { report } => {
            println!("{}", "Usage".cyan().bold());
            println!();
            println!("{}", format_pretty(report));
        }
        OutputData::ConfigInfo(config) => {
            println!("{}", "Configuration".cyan().bold());
            println!();
            println!("{}", format_pretty(config));
        }
        OutputData::ErrorMessage(msg) => {
            eprintln!("{} {}", "Error:".red().bold(), msg);
        }
        OutputData::Patterns(patterns) => {
            println!("{}", "Supported Patterns".cyan().bold());
            println!();
            let value = serde_json::to_value(patterns)?;
            println!("{}", format_pretty(&value));
        }
    }
    Ok(())
}

fn format_markdown_output(data: &OutputData) -> Result<()> {
    match data {
        OutputData::ConnectorList(connectors) => {
            println!("# Available Connectors\n");
            for connector in connectors {
                println!("## {}\n", connector.name);
                println!("{}\n", connector.description);
            }
        }
        OutputData::SearchResults {
            connector,
            query,
            results,
            meta,
        } => {
            println!("# Search Results\n");
            println!("**Connector:** {}\n", connector);
            println!("**Query:** {}\n", query);
            println!("```json");
            println!("{}", serde_json::to_string_pretty(results)?);
            println!("```\n");
            if let Some(meta) = meta {
                println!("**Meta:**\n");
                println!("```json");
                println!("{}", serde_json::to_string_pretty(meta)?);
                println!("```\n");
            }
        }
        OutputData::FederatedResults {
            query,
            profile,
            results,
        } => {
            println!("# Federated Search Results\n");
            println!("**Query:** {}\n", query);
            if let Some(profile) = profile {
                println!("**Profile:** {}\n", profile);
            }
            println!("```json");
            println!("{}", serde_json::to_string_pretty(results)?);
            println!("```\n");
        }
        OutputData::ResourceData {
            connector,
            id,
            data,
        } => {
            println!("# Resource Data\n");
            println!("**Connector:** {}\n", connector);
            println!("**ID:** {}\n", id);
            println!("```json");
            println!("{}", serde_json::to_string_pretty(data)?);
            println!("```\n");
        }
        OutputData::ToolsList { connector, tools } => {
            if let Some(connector) = connector {
                println!("# Tools for {}\n", connector);
            } else {
                println!("# Available Tools\n");
            }
            println!("```json");
            println!("{}", serde_json::to_string_pretty(tools)?);
            println!("```\n");
        }
        OutputData::CallResult {
            connector,
            tool,
            result,
            meta,
        } => {
            println!("# Tool Result\n");
            println!("**Connector:** {}\n", connector);
            println!("**Tool:** {}\n", tool);
            println!("```json");
            println!("{}", serde_json::to_string_pretty(result)?);
            println!("```\n");
            if let Some(meta) = meta {
                println!("**Meta:**\n");
                println!("```json");
                println!("{}", serde_json::to_string_pretty(meta)?);
                println!("```\n");
            }
        }
        OutputData::ConfigInfo(config) => {
            println!("# Configuration\n");
            println!("```json");
            println!("{}", serde_json::to_string_pretty(config)?);
            println!("```\n");
        }
        OutputData::ErrorMessage(msg) => {
            println!("# Error\n");
            println!("{}\n", msg);
        }
        OutputData::ToolResult(result) => {
            println!("# Result\n");
            println!("```json");
            println!("{}", serde_json::to_string_pretty(result)?);
            println!("```\n");
        }
        OutputData::PricingInfo { report } => {
            println!("# Pricing\n");
            println!("```json");
            println!("{}", serde_json::to_string_pretty(report)?);
            println!("```\n");
        }
        OutputData::UsageReport { report } => {
            println!("# Usage\n");
            println!("```json");
            println!("{}", serde_json::to_string_pretty(report)?);
            println!("```\n");
        }
        OutputData::Patterns(patterns) => {
            println!("# Supported Patterns\n");
            println!("| Connector | Tool | Example | Description |");
            println!("|-----------|------|---------|-------------|");
            for p in patterns {
                println!(
                    "| {} | {} | `{}` | {} |",
                    p.connector, p.tool, p.example, p.description
                );
            }
            println!();
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub trait FormatError {
    fn format_error(&self) -> String;
}

impl FormatError for CommandError {
    fn format_error(&self) -> String {
        match self {
            CommandError::ConnectorNotFound(name) => {
                if let Some(hint) = crate::feature_hints::hint_for_connector(name) {
                    if hint.enabled {
                        format!(
                            "Connector '{}' not found (it appears enabled in this build). Use 'rzn-tools list' to see available connectors.",
                            hint.canonical
                        )
                    } else {
                        format!(
                            "Connector '{}' is not enabled in this build.\n\nRebuild/install with a connector feature, e.g.:\n  cargo build --release -p rzn_tools_cli --features {}\n  # or: --features full\n\nThen re-run:\n  rzn-tools list",
                            hint.canonical, hint.cargo_feature
                        )
                    }
                } else {
                    format!(
                        "Connector '{}' not found. Use 'rzn-tools list' to see available connectors.",
                        name
                    )
                }
            }
            CommandError::ToolNotFound(tool, connector) => {
                format!("Tool '{}' not found for connector '{}'. Use 'rzn-tools tools {}' to see available tools.", tool, connector, connector)
            }
            CommandError::AuthenticationRequired(connector) => {
                format!("Authentication required for connector '{}'. Use 'rzn-tools config set {}' to configure.", connector, connector)
            }
            _ => self.to_string(),
        }
    }
}
