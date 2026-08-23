use clap::{CommandFactory, Parser};
use owo_colors::OwoColorize;
use std::{io, io::Write, process};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cli;
mod commands;
mod output;

#[cfg(feature = "tui")]
mod tui;

use cli::{Cli, Commands};
#[cfg(feature = "serve")]
use cli::{CloudflareConfigureAction, ConfigureTarget};
use commands::*;
use output::FormatError;
use rzn_tools_core::UsageContext;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rzn_tools_cli=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    // Handle TUI mode
    #[cfg(feature = "tui")]
    if cli.tui {
        if let Err(e) = tui::run().await {
            eprintln!("{}: {}", "Error".red().bold(), e);
            process::exit(1);
        }
        return;
    }

    // Handle regular CLI commands
    let usage_ctx = match std::env::var("RZN_TOOLS_RUN_ID") {
        Ok(id) => UsageContext::new(id),
        Err(_) => UsageContext::new_random(),
    };

    let result = usage_ctx
        .scope(|| async {
            match &cli.command {
                None => {
                    // Keep bare invocation fast: render static clap help instead of
                    // constructing the connector registry and enumerating tools.
                    let mut cmd = Cli::command();
                    cmd.print_long_help()
                        .map_err(|err| CommandError::Other(err.to_string()))?;
                    println!();
                    io::stdout()
                        .flush()
                        .map_err(|err| CommandError::Other(err.to_string()))?;
                    Ok(())
                }
                Some(Commands::List) => list::run(&cli).await,
                Some(Commands::Setup { connector }) => setup::run(&cli, connector.as_deref()).await,
                #[cfg(feature = "serve")]
                Some(Commands::Configure { target }) => match target {
                    ConfigureTarget::Cloudflare { action } => match action {
                        CloudflareConfigureAction::Guide => serve::cloudflare_guide(&cli).await,
                        CloudflareConfigureAction::Doctor { tunnel_name } => {
                            serve::cloudflare_doctor(&cli, tunnel_name.as_deref()).await
                        }
                        CloudflareConfigureAction::Tunnel {
                            hostname,
                            tunnel_name,
                            bind,
                        } => {
                            serve::configure_cloudflare_tunnel(
                                &cli,
                                hostname,
                                tunnel_name.as_deref(),
                                bind.as_deref(),
                            )
                            .await
                        }
                    },
                },
                #[cfg(feature = "serve")]
                Some(Commands::Serve {
                    bind,
                    allow_hosts,
                    connectors,
                    add_connectors,
                    remove_connectors,
                    all_connectors,
                    list_connectors,
                    local_only,
                }) => {
                    serve::run(
                        &cli,
                        bind.as_deref(),
                        allow_hosts,
                        connectors,
                        add_connectors,
                        remove_connectors,
                        *all_connectors,
                        *list_connectors,
                        *local_only,
                    )
                    .await
                }
                Some(Commands::Search {
                    connector_or_query,
                    query,
                    limit,
                    profile,
                    connectors,
                    merge,
                    add,
                    exclude,
                }) => {
                    search::run(
                        &cli,
                        connector_or_query,
                        query.as_deref(),
                        *limit,
                        profile.as_deref(),
                        connectors.as_deref(),
                        merge,
                        add.as_deref(),
                        exclude.as_deref(),
                        false, // web flag removed
                    )
                    .await
                }
                Some(Commands::Get {
                    connector,
                    id,
                    field,
                }) => get::run(&cli, connector, id, field.as_deref()).await,
                Some(Commands::Fetch {
                    input,
                    output_format,
                }) => fetch::run(&cli, input, output_format).await,
                Some(Commands::Formats) => fetch::show_formats(&cli).await,
                Some(Commands::Config { action }) => config::run(&cli, action.clone()).await,
                Some(Commands::Connectors) => connectors::run(&cli).await,
                Some(Commands::Tools { connector }) => tools::run(&cli, connector.as_deref()).await,
                Some(Commands::Call {
                    connector,
                    tool,
                    args,
                }) => connectors::call(&cli, connector, tool, args).await,
                Some(Commands::Ingest { action }) => ingest::run(&cli, action.clone()).await,
                Some(Commands::Pricing {
                    connector,
                    tool,
                    model,
                }) => {
                    pricing::run(
                        &cli,
                        connector.as_deref(),
                        tool.as_deref(),
                        model.as_deref(),
                    )
                    .await
                }
                Some(Commands::Usage {
                    connector,
                    tool,
                    run,
                    last,
                }) => {
                    usage::run(
                        &cli,
                        connector.as_deref(),
                        tool.as_deref(),
                        run.as_deref(),
                        *last,
                    )
                    .await
                }
                Some(Commands::Report { action }) => report::run(action.clone()).await,
                Some(Commands::Workflows { action }) => workflows::run(&cli, action.clone()).await,
                Some(Commands::Skills { action }) => skills::run(&cli, action.clone()).await,
                Some(Commands::Youtube { args }) => {
                    connectors::handle_youtube(&cli, args.clone()).await
                }
            }
        })
        .await;

    if let Err(e) = result {
        eprintln!("{}: {}", "Error".red().bold(), e.format_error());
        process::exit(1);
    }
}
