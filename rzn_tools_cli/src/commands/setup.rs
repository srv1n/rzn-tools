use crate::cli::Cli;
use crate::commands::{CommandError, Result};
use owo_colors::OwoColorize;
use rzn_tools_core::{
    auth::AuthDetails,
    auth_store::{AuthStore, FileAuthStore},
    capabilities::{ConnectorConfigSchema, FieldType},
};
use std::io::{self, Write};

pub async fn run(cli: &Cli, connector: Option<&str>) -> Result<()> {
    match connector {
        Some(name) => setup_connector(cli, name).await,
        None => run_setup_wizard(cli).await,
    }
}

fn selected_auth_profile(cli: &Cli) -> &str {
    cli.auth_profile.as_deref().unwrap_or("default")
}

async fn registry(cli: &Cli) -> Result<rzn_tools_core::ProviderRegistry> {
    crate::commands::list::create_registry(cli.auth_profile.as_deref()).await
}

async fn run_setup_wizard(cli: &Cli) -> Result<()> {
    let registry = registry(cli).await?;
    let mut available = Vec::new();
    let mut needs_setup = Vec::new();

    for provider in registry.list_providers() {
        let Some(connector) = registry.get_provider(&provider.name) else {
            continue;
        };
        let connector = connector.lock().await;
        let schema = connector.config_schema();
        let row = format!("{} - {}", provider.name, provider.description);
        if schema.fields.iter().any(|field| field.required) {
            needs_setup.push(row);
        } else {
            available.push(row);
        }
    }

    println!("\n{}\n", "rzn-tools Setup".bold().cyan());
    print_group("Ready to use", &available, "green");
    print_group("Needs setup", &needs_setup, "yellow");
    print!("Configure connector (or q to quit): ");
    io::stdout().flush()?;

    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim();
    if name.is_empty() || matches!(name, "q" | "quit") {
        return Ok(());
    }
    setup_connector(cli, name).await
}

fn print_group(title: &str, rows: &[String], color: &str) {
    let title = match color {
        "green" => title.green().bold().to_string(),
        _ => title.yellow().bold().to_string(),
    };
    println!("{title}:");
    for row in rows {
        println!("  {row}");
    }
    println!();
}

async fn setup_connector(cli: &Cli, name: &str) -> Result<()> {
    let registry = registry(cli).await?;
    let provider = registry
        .get_provider(name)
        .ok_or_else(|| CommandError::ConnectorNotFound(name.to_string()))?
        .clone();
    let connector = provider.lock().await;
    let display_name = connector.display_name().to_string();
    let description = connector.description().to_string();
    let schema = connector.config_schema();
    drop(connector);

    println!("\n{} {}", "Setting up".bold().cyan(), display_name.bold());
    println!("{}", description.dimmed());
    if schema.fields.is_empty() {
        println!(
            "{} This connector has no configuration fields.",
            "Ready!".green().bold()
        );
        return Ok(());
    }
    configure_schema(cli, name, &schema).await
}

async fn configure_schema(cli: &Cli, name: &str, schema: &ConnectorConfigSchema) -> Result<()> {
    let profile = selected_auth_profile(cli);
    println!("\n{}", "Configuration fields:".bold());
    for field in &schema.fields {
        let required = if field.required {
            " (required)"
        } else {
            " (optional)"
        };
        println!(
            "  {}{}{}",
            field.name.cyan(),
            required.dimmed(),
            field
                .description
                .as_deref()
                .map(|description| format!(" - {description}"))
                .unwrap_or_default()
                .dimmed()
        );
    }
    println!(
        "\n{}",
        "Use env vars if your deployment provides them, or save a profile now.".dimmed()
    );
    print!("Save credentials for profile '{profile}'? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Run {} later.", format!("rzn-tools setup {name}").cyan());
        return Ok(());
    }

    let mut auth = AuthDetails::new();
    for field in &schema.fields {
        print!(
            "{}{}: ",
            field.label.bold(),
            if field.required { " *" } else { "" }
        );
        io::stdout().flush()?;
        let value = if matches!(&field.field_type, FieldType::Secret) {
            read_secret()?
        } else {
            read_line()?
        };
        if !value.is_empty() {
            auth.insert(field.name.clone(), value);
        }
    }
    let missing = schema
        .fields
        .iter()
        .filter(|field| field.required && !auth.contains_key(&field.name))
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(CommandError::InvalidInput(format!(
            "Missing required configuration fields: {}",
            missing.join(", ")
        )));
    }
    FileAuthStore::new_default()
        .save_profile(name, profile, &auth)
        .map_err(|error| {
            CommandError::InvalidConfig(format!("Failed to save credentials: {error}"))
        })?;
    println!(
        "{} Credentials saved. Verify with {}.",
        "Saved!".green().bold(),
        format!("rzn-tools config test {name}").cyan()
    );
    Ok(())
}

fn read_line() -> Result<String> {
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}

fn read_secret() -> Result<String> {
    rpassword::read_password()
        .map(|value| value.trim().to_string())
        .or_else(|_| read_line())
}

#[cfg(test)]
mod tests {
    use super::selected_auth_profile;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn setup_uses_default_profile() {
        let cli = Cli::try_parse_from(["rzn-tools", "setup"]).expect("CLI parses");
        assert_eq!(selected_auth_profile(&cli), "default");
    }
}
