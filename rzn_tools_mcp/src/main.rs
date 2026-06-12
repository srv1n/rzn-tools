use std::{collections::HashSet, env, net::SocketAddr};
use tracing::{error, info};

use rzn_tools_mcp::{run_http_server, run_stdio_server, HttpConfig};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportMode {
    Stdio,
    Http,
}

#[derive(Debug)]
struct Config {
    transport: TransportMode,
    http: HttpConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().init();

    let config = Config::parse()?;
    info!("Starting rzn-tools MCP Server");

    match config.transport {
        TransportMode::Stdio => {
            info!("MCP Server ready, listening on stdio");
            if let Err(e) = run_stdio_server().await {
                error!("Transport error: {}", e);
                return Err(e);
            }
        }
        TransportMode::Http => {
            if let Err(e) = run_http_server(config.http).await {
                error!("Transport error: {}", e);
                return Err(e);
            }
        }
    }

    Ok(())
}

impl Config {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut transport = TransportMode::Stdio;
        let mut bind = env::var("RZN_TOOLS_MCP_BIND")
            .ok()
            .or_else(|| env::var("BIND").ok())
            .or_else(|| {
                env::var("PORT")
                    .ok()
                    .map(|port| format!("127.0.0.1:{port}"))
            })
            .unwrap_or_else(|| "127.0.0.1:8000".to_string());
        let mut allowed_hosts = env::var("ALLOWED_HOSTS")
            .ok()
            .map(|value| parse_allowed_hosts(&value));
        let mut exposed_connectors = env::var("RZN_TOOLS_MCP_CONNECTORS")
            .ok()
            .map(|value| parse_connector_allowlist(&value));

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                "http" | "--http" => transport = TransportMode::Http,
                "stdio" => transport = TransportMode::Stdio,
                "--transport" => {
                    let Some(value) = args.next() else {
                        return Err("missing value for --transport".into());
                    };
                    transport = match value.as_str() {
                        "stdio" => TransportMode::Stdio,
                        "http" => TransportMode::Http,
                        _ => return Err(format!("unsupported transport: {value}").into()),
                    };
                }
                "--bind" => {
                    let Some(value) = args.next() else {
                        return Err("missing value for --bind".into());
                    };
                    bind = value;
                }
                "--allowed-hosts" => {
                    let Some(value) = args.next() else {
                        return Err("missing value for --allowed-hosts".into());
                    };
                    allowed_hosts = Some(parse_allowed_hosts(&value));
                }
                "--connectors" => {
                    let Some(value) = args.next() else {
                        return Err("missing value for --connectors".into());
                    };
                    exposed_connectors = Some(parse_connector_allowlist(&value));
                }
                "--all-connectors" => exposed_connectors = None,
                unknown => return Err(format!("unknown argument: {unknown}").into()),
            }
        }

        let bind: SocketAddr = bind.parse()?;
        Ok(Self {
            transport,
            http: HttpConfig {
                bind,
                allowed_hosts,
                exposed_connectors,
            },
        })
    }
}

fn parse_allowed_hosts(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_ascii_lowercase())
        .collect()
}

fn parse_connector_allowlist(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_ascii_lowercase().replace('_', "-"))
        .map(|entry| match entry.as_str() {
            "exa-search" => "exa".to_string(),
            "youtube-transcripts" => "youtube".to_string(),
            "parallel" => "parallel-search".to_string(),
            _ => entry,
        })
        .collect()
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  rzn-tools-mcp
  rzn-tools-mcp http [--bind 127.0.0.1:8000] [--allowed-hosts host1,host2] [--connectors youtube,reddit]
  rzn-tools-mcp --transport http [--bind 127.0.0.1:8000]

Environment:
  PORT              Set HTTP port when using http transport
  RZN_TOOLS_MCP_BIND Set full bind address, for example 127.0.0.1:8000
  ALLOWED_HOSTS     Optional comma-separated host allowlist for HTTP mode
  RZN_TOOLS_MCP_CONNECTORS  Optional comma-separated connector allowlist
"
    );
}
