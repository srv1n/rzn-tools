use crate::cli::{Cli, ConfigAction};
use crate::commands::{CommandError, Result};
use crate::output::{format_output, OutputData};
use owo_colors::OwoColorize;
use rzn_tools_core::auth_store::{AuthStore, FileAuthStore};
use serde_json::{json, Value};
use std::io::{self, Write};

pub async fn run(cli: &Cli, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => show_config(cli).await,
        ConfigAction::Set {
            connector,
            key,
            auth_type,
            value,
            browser,
        } => {
            set_config(
                cli,
                &connector,
                key.as_deref(),
                auth_type.as_deref(),
                value.as_deref(),
                browser.as_deref(),
            )
            .await
        }
        ConfigAction::Remove { connector } => remove_config(cli, &connector).await,
        ConfigAction::Test { connector } => test_config(cli, &connector).await,
    }
}

async fn show_config(cli: &Cli) -> Result<()> {
    let store = FileAuthStore::new_default();
    let providers = store.list_providers();

    let output_data = OutputData::ConfigInfo(get_config_json(&store, &providers));

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            println!();
            println!("{}", "Configured Connectors".bold().cyan());
            println!("{}", "=====================".cyan());
            println!();

            if providers.is_empty() {
                println!("{}", "No connectors configured yet.".yellow());
                println!();
                println!("Run {} to set up a connector.", "rzn-tools setup".cyan());
            } else {
                println!("Config file: {}", store.config_path().dimmed());
                if cli.auth_profile.as_deref().is_some_and(|p| p != "default") {
                    println!(
                        "{} {}",
                        "Active auth profile:".bold(),
                        cli.auth_profile.as_deref().unwrap_or("default").cyan()
                    );
                }
                println!();

                for key in &providers {
                    let (provider, profile) = FileAuthStore::parse_key(key);
                    let auth = store.load(key);
                    let field_count = auth.as_ref().map(|a| a.len()).unwrap_or(0);

                    // Check if it has meaningful auth (not just browser selection)
                    let has_token = auth
                        .as_ref()
                        .map(|a| {
                            a.contains_key("token")
                                || a.contains_key("api_key")
                                || a.contains_key("access_token")
                                || a.contains_key("client_id")
                        })
                        .unwrap_or(false);

                    let status = if has_token {
                        "configured".green().to_string()
                    } else if auth
                        .as_ref()
                        .map(|a| a.contains_key("browser"))
                        .unwrap_or(false)
                    {
                        "browser cookies".blue().to_string()
                    } else {
                        "partial".yellow().to_string()
                    };

                    println!(
                        "  {} - {} ({} fields)",
                        if profile == "default" {
                            provider.cyan().bold().to_string()
                        } else {
                            format!("{} ({})", provider, profile)
                                .cyan()
                                .bold()
                                .to_string()
                        },
                        status,
                        field_count
                    );
                }
                println!();
                println!(
                    "Test a connector: {}",
                    "rzn-tools config test <connector>".cyan()
                );
                println!(
                    "Remove a connector: {}",
                    "rzn-tools config remove <connector>".cyan()
                );
            }
            println!();
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    Ok(())
}

fn get_config_json(store: &FileAuthStore, providers: &[String]) -> Value {
    let mut config = json!({});

    for provider in providers {
        if let Some(auth) = store.load(provider) {
            let mut provider_config = json!({});

            // Show field names but mask values
            for key in auth.keys() {
                provider_config[key] = json!("***");
            }

            provider_config["field_count"] = json!(auth.len());
            config[provider] = provider_config;
        }
    }

    config
}

async fn set_config(
    cli: &Cli,
    connector: &str,
    key: Option<&str>,
    auth_type: Option<&str>,
    value: Option<&str>,
    browser: Option<&str>,
) -> Result<()> {
    // Validate connector exists
    let registry = crate::commands::list::create_registry(cli.auth_profile.as_deref()).await?;
    if registry.get_provider(connector).is_none() {
        return Err(CommandError::ConnectorNotFound(connector.to_string()));
    }

    let store = FileAuthStore::new_default();
    let auth_profile = cli.auth_profile.as_deref().unwrap_or("default");

    // Handle different auth methods
    match (key, auth_type, value, browser) {
        // Explicit field set: --key <field> --value <value>
        (Some(field), _, Some(v), _) => {
            let mut auth = store
                .load_profile(connector, auth_profile)
                .unwrap_or_default();
            auth.insert(field.to_string(), v.to_string());
            store
                .save_profile(connector, auth_profile, &auth)
                .map_err(|e| CommandError::InvalidConfig(format!("Failed to save: {}", e)))?;
            println!(
                "{} Saved {} for {}",
                "Success!".green().bold(),
                field.cyan(),
                connector.cyan()
            );
        }
        // API key with explicit type
        (None, Some("api-key"), Some(key), _) | (None, Some("token"), Some(key), _) => {
            let mut auth = store
                .load_profile(connector, auth_profile)
                .unwrap_or_default();
            auth.insert("token".to_string(), key.to_string());
            store
                .save_profile(connector, auth_profile, &auth)
                .map_err(|e| CommandError::InvalidConfig(format!("Failed to save: {}", e)))?;
            println!(
                "{} API key saved for {}",
                "Success!".green().bold(),
                connector.cyan()
            );
        }
        // Proxy
        (None, Some("proxy"), Some(proxy_url), _)
        | (None, Some("proxy-url"), Some(proxy_url), _) => {
            let mut auth = store
                .load_profile(connector, auth_profile)
                .unwrap_or_default();
            auth.insert("proxy_url".to_string(), proxy_url.to_string());
            store
                .save_profile(connector, auth_profile, &auth)
                .map_err(|e| CommandError::InvalidConfig(format!("Failed to save: {}", e)))?;
            println!(
                "{} Proxy saved for {}",
                "Success!".green().bold(),
                connector.cyan()
            );
        }
        // Browser cookies
        (None, Some("browser"), _, Some(browser_name)) | (None, None, None, Some(browser_name)) => {
            let supported = ["chrome", "firefox", "edge", "safari", "brave"];
            if !supported.contains(&browser_name) {
                return Err(CommandError::InvalidConfig(format!(
                    "Unsupported browser: {}. Use: {}",
                    browser_name,
                    supported.join(", ")
                )));
            }
            let mut auth = store
                .load_profile(connector, auth_profile)
                .unwrap_or_default();
            auth.insert("browser".to_string(), browser_name.to_string());
            store
                .save_profile(connector, auth_profile, &auth)
                .map_err(|e| CommandError::InvalidConfig(format!("Failed to save: {}", e)))?;
            println!(
                "{} Browser set to {} for {}",
                "Success!".green().bold(),
                browser_name.cyan(),
                connector.cyan()
            );
        }
        // Value without explicit type - try to guess
        (None, None, Some(value), None) => {
            let mut auth = store
                .load_profile(connector, auth_profile)
                .unwrap_or_default();
            // Use common field names based on value format
            let field_name = if value.starts_with("xoxb-")
                || value.starts_with("ghp_")
                || value.starts_with("github_pat_")
            {
                "token"
            } else if value.starts_with("sk-ant-")
                || value.starts_with("sk-")
                || value.starts_with("pplx-")
                || value.starts_with("tvly-")
            {
                "api_key"
            } else {
                "token" // default
            };
            auth.insert(field_name.to_string(), value.to_string());
            store
                .save_profile(connector, auth_profile, &auth)
                .map_err(|e| CommandError::InvalidConfig(format!("Failed to save: {}", e)))?;
            println!(
                "{} Credential saved for {}",
                "Success!".green().bold(),
                connector.cyan()
            );
        }
        // OAuth placeholder
        (None, Some("oauth"), _, _) => {
            println!(
                "{} Use {} for OAuth setup",
                "Note:".yellow().bold(),
                format!("rzn-tools setup {}", connector).cyan()
            );
        }
        _ => {
            return Err(CommandError::InvalidConfig(
                "Specify --value <token> or --browser <browser> (or use --key <field> --value <value>)"
                    .to_string(),
            ));
        }
    }

    // Suggest testing
    println!();
    println!(
        "Test with: {}",
        format!("rzn-tools config test {}", connector).cyan()
    );

    Ok(())
}

async fn remove_config(cli: &Cli, connector: &str) -> Result<()> {
    let store = FileAuthStore::new_default();
    let auth_profile = match cli.auth_profile.as_deref() {
        Some(p) => Some(p.to_string()),
        None => store.resolve_profile_for_provider(connector),
    };

    // Check if connector has config
    let Some(auth_profile) = auth_profile.as_deref() else {
        println!(
            "{} No configuration found for {}",
            "Note:".yellow().bold(),
            connector.cyan()
        );
        return Ok(());
    };
    if store.load_profile(connector, auth_profile).is_none() {
        println!(
            "{} No configuration found for {}",
            "Note:".yellow().bold(),
            connector.cyan()
        );
        return Ok(());
    }

    // Confirm removal
    if auth_profile == "default" {
        print!("Remove credentials for {}? [y/N] ", connector.cyan().bold());
    } else {
        print!(
            "Remove credentials for {} (profile: {})? [y/N] ",
            connector.cyan().bold(),
            auth_profile.cyan()
        );
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("Cancelled.");
        return Ok(());
    }

    // Remove
    match store.remove_profile(connector, auth_profile) {
        Ok(true) => {
            println!(
                "{} Removed configuration for {}",
                "Success!".green().bold(),
                connector.cyan()
            );
            if auth_profile != "default" {
                println!("{}", format!("Auth profile: {}", auth_profile).dimmed());
            }
        }
        Ok(false) => {
            println!(
                "{} No configuration found for {}",
                "Note:".yellow().bold(),
                connector.cyan()
            );
        }
        Err(e) => {
            return Err(CommandError::InvalidConfig(format!(
                "Failed to remove: {}",
                e
            )));
        }
    }

    Ok(())
}

async fn test_config(cli: &Cli, connector: &str) -> Result<()> {
    println!();
    print!("{} {} ... ", "Testing".bold().cyan(), connector.cyan());
    io::stdout().flush()?;

    let store = FileAuthStore::new_default();
    let auth_profile = match cli.auth_profile.as_deref() {
        Some(p) => Some(p.to_string()),
        None => store.resolve_profile_for_provider(connector),
    };

    let registry = crate::commands::list::create_registry(auth_profile.as_deref()).await?;
    let provider = registry
        .get_provider(connector)
        .ok_or_else(|| CommandError::ConnectorNotFound(connector.to_string()))?;

    let mut c = provider.lock().await;

    // Load saved credentials and set them on the connector
    if let Some(profile) = auth_profile.as_deref() {
        if let Some(auth) = store.load_profile(connector, profile) {
            if let Err(e) = c.set_auth_details(auth).await {
                println!("{}", "Failed".red().bold());
                println!();
                println!(
                    "{} {}",
                    "Error:".red().bold(),
                    format!("Failed to set credentials: {}", e).red()
                );
                return Ok(());
            }
        }
    }

    match c.test_auth().await {
        Ok(_) => {
            println!("{}", "Success!".green().bold());
            println!();
            println!("{}", "Authentication is working. Try:".bold());
            println!(
                "  {}",
                format!("rzn-tools search {} \"test\"", connector).cyan()
            );
        }
        Err(e) => {
            println!("{}", "Failed".red().bold());
            println!();
            println!("{} {}", "Error:".red().bold(), e.to_string().red());
            println!();
            println!("You can:");
            println!(
                "  - Re-configure with {}",
                format!("rzn-tools setup {}", connector).cyan()
            );
            println!(
                "  - Check credentials in {}",
                FileAuthStore::new_default().config_path().dimmed()
            );
        }
    }
    println!();

    Ok(())
}
