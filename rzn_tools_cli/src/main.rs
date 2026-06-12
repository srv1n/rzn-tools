use clap::{CommandFactory, Parser};
use owo_colors::OwoColorize;
use std::{io, io::Write, process};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cli;
mod commands;
mod feature_hints;
mod output;

#[cfg(feature = "tui")]
mod tui;

use cli::{Cli, CloudflareConfigureAction, Commands, ConfigureTarget};
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
                // Google connectors
                Some(Commands::Caldav { tool }) => {
                    connectors::handle_caldav(&cli, tool.clone()).await
                }
                Some(Commands::GoogleCalendar { tool }) => {
                    connectors::handle_google_calendar(&cli, tool.clone()).await
                }
                Some(Commands::GoogleDrive { tool }) => {
                    connectors::handle_google_drive(&cli, tool.clone()).await
                }
                Some(Commands::GoogleGmail { tool }) => {
                    connectors::handle_google_gmail(&cli, tool.clone()).await
                }
                Some(Commands::GooglePeople { tool }) => {
                    connectors::handle_google_people(&cli, tool.clone()).await
                }
                Some(Commands::GoogleSearchConsole { tool }) => {
                    connectors::handle_google_search_console(&cli, tool.clone()).await
                }
                Some(Commands::BingWebmasterTools { tool }) => {
                    connectors::handle_bing_webmaster_tools(&cli, tool.clone()).await
                }
                Some(Commands::Linkedin { tool }) => {
                    connectors::handle_linkedin(&cli, tool.clone()).await
                }
                Some(Commands::GoogleScholar { tool }) => {
                    connectors::handle_google_scholar(&cli, tool.clone()).await
                }

                // LLM Search connectors
                Some(Commands::OpenaiSearch { tool }) => {
                    connectors::handle_openai_search(&cli, tool.clone()).await
                }
                Some(Commands::AnthropicSearch { tool }) => {
                    connectors::handle_anthropic_search(&cli, tool.clone()).await
                }
                Some(Commands::GeminiSearch { tool }) => {
                    connectors::handle_gemini_search(&cli, tool.clone()).await
                }
                Some(Commands::PerplexitySearch { tool }) => {
                    connectors::handle_perplexity_search(&cli, tool.clone()).await
                }
                Some(Commands::XaiSearch { tool }) => {
                    connectors::handle_xai_search(&cli, tool.clone()).await
                }
                Some(Commands::Exa { tool }) => connectors::handle_exa(&cli, tool.clone()).await,
                Some(Commands::TavilySearch { tool }) => {
                    connectors::handle_tavily_search(&cli, tool.clone()).await
                }
                Some(Commands::SerperSearch { tool }) => {
                    connectors::handle_serper_search(&cli, tool.clone()).await
                }
                Some(Commands::SerpapiSearch { tool }) => {
                    connectors::handle_serpapi_search(&cli, tool.clone()).await
                }
                Some(Commands::FirecrawlSearch { tool }) => {
                    connectors::handle_firecrawl_search(&cli, tool.clone()).await
                }
                Some(Commands::ParallelSearch { tool }) => {
                    connectors::handle_parallel_search(&cli, tool.clone()).await
                }

                // Productivity connectors
                Some(Commands::Atlassian { tool }) => {
                    connectors::handle_atlassian(&cli, tool.clone()).await
                }
                Some(Commands::MicrosoftGraph { tool }) => {
                    connectors::handle_microsoft_graph(&cli, tool.clone()).await
                }
                Some(Commands::Imap { tool }) => connectors::handle_imap(&cli, tool.clone()).await,
                Some(Commands::Smtp { tool }) => connectors::handle_smtp(&cli, tool.clone()).await,

                // For now, other connectors fall back to the call command
                // Connector-specific subcommands with proper CLI flags
                Some(Commands::Localfs { tool }) => {
                    connectors::handle_localfs(&cli, tool.clone()).await
                }
                Some(Commands::Youtube { args }) => {
                    connectors::handle_youtube(&cli, args.clone()).await
                }
                Some(Commands::Hackernews { tool }) => {
                    connectors::handle_hackernews(&cli, tool.clone()).await
                }
                Some(Commands::Arxiv { tool }) => {
                    connectors::handle_arxiv(&cli, tool.clone()).await
                }
                Some(Commands::Github { tool }) => {
                    connectors::handle_github(&cli, tool.clone()).await
                }
                Some(Commands::Reddit { tool }) => {
                    connectors::handle_reddit(&cli, tool.clone()).await
                }
                Some(Commands::Polymarket { tool }) => {
                    connectors::handle_polymarket(&cli, tool.clone()).await
                }
                Some(Commands::Kalshi { tool }) => {
                    connectors::handle_kalshi(&cli, tool.clone()).await
                }
                Some(Commands::PlayStore { tool }) => {
                    connectors::handle_play_store(&cli, tool.clone()).await
                }
                Some(Commands::AppStore { tool }) => {
                    connectors::handle_app_store(&cli, tool.clone()).await
                }
                Some(Commands::AppStoreConnect { tool }) => {
                    connectors::handle_app_store_connect(&cli, tool.clone()).await
                }
                Some(Commands::AppleSearchAds { tool }) => {
                    connectors::handle_apple_search_ads(&cli, tool.clone()).await
                }
                Some(Commands::Web { tool }) => connectors::handle_web(&cli, tool.clone()).await,
                Some(Commands::Wikipedia { tool }) => {
                    connectors::handle_wikipedia(&cli, tool.clone()).await
                }
                Some(Commands::Pubmed { tool }) => {
                    connectors::handle_pubmed(&cli, tool.clone()).await
                }
                Some(Commands::SemanticScholar { tool }) => {
                    connectors::handle_semantic_scholar(&cli, tool.clone()).await
                }
                Some(Commands::Slack { tool }) => {
                    connectors::handle_slack(&cli, tool.clone()).await
                }
                Some(Commands::X { tool }) => connectors::handle_x_api(&cli, tool.clone()).await,
                Some(Commands::XBrowser { tool }) => connectors::handle_x(&cli, tool.clone()).await,
                Some(Commands::Discord { tool }) => {
                    connectors::handle_discord(&cli, tool.clone()).await
                }
                Some(Commands::Rss { tool }) => connectors::handle_rss(&cli, tool.clone()).await,
                Some(Commands::Biorxiv { tool }) => {
                    connectors::handle_biorxiv(&cli, tool.clone()).await
                }
                Some(Commands::Scihub { tool }) => {
                    connectors::handle_scihub(&cli, tool.clone()).await
                }
                Some(Commands::Macos { tool }) => {
                    connectors::handle_macos(&cli, tool.clone()).await
                }
                Some(Commands::Spotlight { tool }) => {
                    connectors::handle_spotlight(&cli, tool.clone()).await
                }
                Some(Commands::AppleMessages { tool }) => {
                    connectors::handle_apple_messages(&cli, tool.clone()).await
                }
            }
        })
        .await;

    if let Err(e) = result {
        eprintln!("{}: {}", "Error".red().bold(), e.format_error());
        process::exit(1);
    }
}
