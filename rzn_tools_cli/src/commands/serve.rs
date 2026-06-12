use std::{
    collections::HashSet,
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command},
};

use owo_colors::OwoColorize;
use rzn_tools_core::{auth_store::FileAuthStore, build_registry_enabled_only};
use rzn_tools_mcp::{run_http_server, HttpConfig};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};

use crate::{
    cli::Cli,
    commands::{CommandError, Result},
};

const DEFAULT_BIND: &str = "127.0.0.1:8000";
const CURRENT_CONNECTOR_DEFAULTS_VERSION: u8 = 2;
const DEFAULT_EXPOSED_CONNECTORS: &[&str] = &["youtube", "hackernews", "pubmed", "reddit"];
const LEGACY_DEFAULT_EXPOSED_CONNECTORS: &[&str] = &[
    "youtube",
    "hackernews",
    "pubmed",
    "parallel-search",
    "exa",
    "reddit",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ServeConfigFile {
    #[serde(default)]
    bind: Option<String>,
    #[serde(default)]
    allowed_hosts: Vec<String>,
    #[serde(default)]
    exposed_connectors: Vec<String>,
    #[serde(default)]
    connector_defaults_version: Option<u8>,
    #[serde(default)]
    expose_all_connectors: bool,
    #[serde(default)]
    cloudflare: Option<CloudflareTunnelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudflareTunnelConfig {
    hostname: String,
    tunnel_name: Option<String>,
}

pub async fn run(
    _cli: &Cli,
    bind: Option<&str>,
    allowed_hosts: &[String],
    connectors: &[String],
    add_connectors: &[String],
    remove_connectors: &[String],
    all_connectors: bool,
    list_connectors: bool,
    local_only: bool,
) -> Result<()> {
    let mut config = load_config()?;
    let config_path = serve_config_path()?;
    if let Some(bind) = bind {
        config.bind = Some(bind.to_string());
    }

    if !allowed_hosts.is_empty() {
        for host in allowed_hosts {
            push_unique_host(&mut config.allowed_hosts, host);
        }
    }

    apply_connector_changes(
        &mut config,
        connectors,
        add_connectors,
        remove_connectors,
        all_connectors,
    )?;
    let effective_exposed_connectors = effective_exposed_connectors(&config);
    let available_connectors = available_connector_names().await?;
    let available_connector_set = available_connectors.iter().cloned().collect::<HashSet<_>>();

    if connectors_changed(
        connectors,
        add_connectors,
        remove_connectors,
        all_connectors,
    ) {
        save_config(&config_path, &config)?;
        println!("{}", "Saved serve config".green().bold());
        println!("Config: {}", config_path.display().to_string().dimmed());
        println!();
    }

    if list_connectors {
        print_connector_inventory(
            &config,
            &effective_exposed_connectors,
            &available_connectors,
        );
        return Ok(());
    }

    let bind = config
        .bind
        .clone()
        .unwrap_or_else(|| DEFAULT_BIND.to_string());
    let bind_addr = parse_bind_addr(&bind)?;
    let allowed_hosts = normalized_hosts(&config.allowed_hosts);
    let unavailable_exposed = effective_exposed_connectors
        .iter()
        .filter(|name| !available_connector_set.contains(*name))
        .cloned()
        .collect::<Vec<_>>();

    println!("{}", "rzn-tools MCP HTTP server".bold().cyan());
    println!("Bind: {}", bind_addr.to_string().green());
    println!("Endpoint: {}", format!("http://{bind_addr}/mcp").green());

    let should_autostart_tunnel = should_autostart_tunnel(&config, local_only);
    if let Some(cloudflare) = &config.cloudflare {
        println!(
            "Tunnel hostname: {}",
            format!("https://{}/mcp", cloudflare.hostname).green()
        );
        match &cloudflare.tunnel_name {
            Some(tunnel_name) => {
                println!("Tunnel name: {}", tunnel_name.green());
                if should_autostart_tunnel {
                    println!(
                        "Tunnel mode: {}",
                        "auto-start via `rzn-tools serve`".green()
                    );
                } else if local_only {
                    println!("Tunnel mode: {}", "disabled (`--local-only`)".yellow());
                } else {
                    println!(
                        "Tunnel mode: {}",
                        "manual until a tunnel name is configured".yellow()
                    );
                }
            }
            None => println!(
                "Tunnel name: {}",
                "not set (recommended for `doctor`)".yellow()
            ),
        }
    } else {
        println!(
            "Cloudflare: {}",
            "not configured (`rzn-tools configure cloudflare guide`)".yellow()
        );
    }

    if let Some(hosts) = &allowed_hosts {
        let mut hosts = hosts.iter().cloned().collect::<Vec<_>>();
        hosts.sort();
        println!("Allowed hosts: {}", hosts.join(", ").dimmed());
    } else {
        println!("Allowed hosts: {}", "disabled".yellow());
    }

    print_exposed_connector_status(
        &config,
        &effective_exposed_connectors,
        &available_connectors,
        &unavailable_exposed,
    );

    println!();

    let mut managed_tunnel = maybe_start_cloudflared_tunnel(&config, local_only).await?;
    let server_result = run_http_server(HttpConfig {
        bind: bind_addr,
        allowed_hosts,
        exposed_connectors: normalized_connectors_for_http(&config),
    })
    .await
    .map_err(|error| CommandError::Other(error.to_string()));

    if let Some(tunnel) = managed_tunnel.as_mut() {
        stop_cloudflared_tunnel(tunnel)?;
    }

    server_result
}

pub async fn configure_cloudflare_tunnel(
    _cli: &Cli,
    hostname: &str,
    tunnel_name: Option<&str>,
    bind: Option<&str>,
) -> Result<()> {
    let hostname = normalize_hostname(hostname)?;
    let mut config = load_config()?;
    let bind = bind
        .map(ToOwned::to_owned)
        .or_else(|| config.bind.clone())
        .unwrap_or_else(|| DEFAULT_BIND.to_string());
    let _ = parse_bind_addr(&bind)?;

    config.bind = Some(bind.clone());
    config.cloudflare = Some(CloudflareTunnelConfig {
        hostname: hostname.clone(),
        tunnel_name: tunnel_name.map(normalize_tunnel_name).transpose()?,
    });

    push_unique_host(&mut config.allowed_hosts, "localhost");
    push_unique_host(&mut config.allowed_hosts, "127.0.0.1");
    push_unique_host(&mut config.allowed_hosts, &hostname);

    let config_path = serve_config_path()?;
    save_config(&config_path, &config)?;

    println!("{}", "Saved Cloudflare tunnel config".green().bold());
    println!("Config: {}", config_path.display().to_string().dimmed());
    println!("Bind: {}", bind.cyan());
    println!(
        "Origin endpoint: {}",
        format!("https://{hostname}/mcp").cyan()
    );
    match config
        .cloudflare
        .as_ref()
        .and_then(|entry| entry.tunnel_name.as_ref())
    {
        Some(tunnel_name) => println!("Tunnel name: {}", tunnel_name.cyan()),
        None => println!(
            "Tunnel name: {}",
            "not set (add `--tunnel-name` so rzn-tools can verify it)".yellow()
        ),
    }
    println!();
    println!("{}", "Next".bold());
    println!("  1. {}", "rzn-tools serve".cyan());
    if let Some(tunnel_name) = config
        .cloudflare
        .as_ref()
        .and_then(|entry| entry.tunnel_name.as_ref())
    {
        println!(
            "  2. {}",
            format!("`rzn-tools serve` will auto-start cloudflared tunnel run {tunnel_name}")
                .cyan()
        );
    } else {
        println!(
            "  2. Add a tunnel name so `rzn-tools serve` can auto-start {}",
            "`cloudflared`".cyan()
        );
    }
    println!(
        "  3. Point that tunnel at {}",
        format!("http://{bind}").cyan()
    );
    println!(
        "  4. Proxy external traffic to {}",
        format!("https://{hostname}/mcp").cyan()
    );
    println!(
        "  Optional: {}",
        "rzn-tools configure cloudflare doctor".cyan()
    );
    println!();
    println!("{}", "Requirements".bold());
    println!("  - {}", "cloudflared is required for tunnel mode".cyan());
    println!(
        "  - {}",
        "wrangler is only needed if you also deploy a Worker".cyan()
    );

    Ok(())
}

pub async fn cloudflare_guide(_cli: &Cli) -> Result<()> {
    let serve_config = serve_config_path()?;
    let cloudflared_dir = cloudflared_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.cloudflared".to_string());
    let cloudflared_config = cloudflared_config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| format!("{cloudflared_dir}/config.yml"));

    println!("{}", "rzn-tools + Cloudflare Tunnel".bold().cyan());
    println!();
    println!("{}", "What rzn-tools Owns".bold());
    println!(
        "  - Local MCP HTTP server at {}",
        format!("http://{DEFAULT_BIND}/mcp").cyan()
    );
    println!(
        "  - Local defaults in {}",
        serve_config.display().to_string().dimmed()
    );
    println!();
    println!("{}", "What Cloudflare owns".bold());
    println!("  - Tunnel identity and credentials");
    println!("  - Public hostname -> tunnel mapping");
    println!("  - Local tunnel state under {}", cloudflared_dir.dimmed());
    println!("  - Usually {}", cloudflared_config.dimmed());
    println!();
    println!("{}", "Requirements".bold());
    println!("  - Required: {}", "cloudflared".cyan());
    println!("  - Required: a Cloudflare hostname you control");
    println!("  - Required: a named tunnel mapped to that hostname");
    println!(
        "  - Optional: {}",
        "wrangler, but only if you also deploy a Worker".cyan()
    );
    println!();
    println!("{}", "Recommended Flow".bold());
    println!("  1. Install {}", "cloudflared".cyan());
    println!("  2. Create or pick a named tunnel in Cloudflare");
    println!("  3. Route your hostname to that tunnel");
    println!(
        "  4. Save rzn-tools defaults with {}",
        "rzn-tools configure cloudflare tunnel --hostname <host> --tunnel-name <name>".cyan()
    );
    println!(
        "  5. Optional sanity check with {}",
        "rzn-tools configure cloudflare doctor".cyan()
    );
    println!("  6. Start everything with {}", "rzn-tools serve".cyan());
    println!();
    println!("{}", "Notes".bold());
    println!("  - rzn-tools does not create the tunnel for you.");
    println!("  - rzn-tools does not need Wrangler unless a Worker is in the picture.");
    println!("  - If a tunnel name is configured, `rzn-tools serve` starts `cloudflared` for you.");
    println!("  - Use `rzn-tools serve --local-only` if you only want localhost.");

    Ok(())
}

pub async fn cloudflare_doctor(_cli: &Cli, tunnel_name: Option<&str>) -> Result<()> {
    let config = load_config()?;
    let config_path = serve_config_path()?;
    let configured_cloudflare = config.cloudflare.clone();
    let bind = config
        .bind
        .clone()
        .unwrap_or_else(|| DEFAULT_BIND.to_string());
    let bind_addr = parse_bind_addr(&bind).ok();
    let configured_tunnel_name =
        tunnel_name
            .map(normalize_tunnel_name)
            .transpose()?
            .or_else(|| {
                configured_cloudflare
                    .as_ref()
                    .and_then(|entry| entry.tunnel_name.clone())
            });

    println!("{}", "rzn-tools Cloudflare Doctor".bold().cyan());
    println!();

    print_status(
        "serve config",
        config_path.exists(),
        if config_path.exists() {
            format!("found {}", config_path.display())
        } else {
            format!(
                "missing {}; run `rzn-tools configure cloudflare tunnel ...`",
                config_path.display()
            )
        },
    );
    print_status(
        "bind",
        bind_addr.is_some(),
        bind_addr.map_or_else(
            || format!("invalid bind address `{bind}`"),
            |addr| addr.to_string(),
        ),
    );

    match &configured_cloudflare {
        Some(cloudflare) => {
            print_status(
                "hostname",
                true,
                format!("configured for https://{}/mcp", cloudflare.hostname),
            );
            if let Some(tunnel_name) = &cloudflare.tunnel_name {
                print_status("tunnel name", true, format!("configured as {tunnel_name}"));
            } else {
                print_status(
                    "tunnel name",
                    false,
                    "not set; add `--tunnel-name` so rzn-tools can show the exact run command"
                        .to_string(),
                );
            }
        }
        None => {
            print_status(
                "hostname",
                false,
                "not configured; run `rzn-tools configure cloudflare tunnel --hostname ...`"
                    .to_string(),
            );
        }
    }

    let cloudflared = command_status("cloudflared", &["--version"]);
    print_status("cloudflared", cloudflared.ok, cloudflared.message);

    if let Some(cloudflared_config_path) = cloudflared_config_path() {
        let exists = cloudflared_config_path.exists();
        let detail = if exists {
            format!("found {}", cloudflared_config_path.display())
        } else {
            format!("not found at {}", cloudflared_config_path.display())
        };
        print_status("cloudflared config", exists, detail);
    } else {
        print_status(
            "cloudflared config",
            false,
            "home directory not available; could not derive ~/.cloudflared/config.yml".to_string(),
        );
    }

    if let Some(cloudflare) = &configured_cloudflare {
        match resolve_hostname(&cloudflare.hostname).await {
            Some(addresses) => print_status(
                "dns lookup",
                true,
                format!("resolved {}", addresses.join(", ")),
            ),
            None => print_status(
                "dns lookup",
                false,
                format!(
                    "could not resolve `{}`; your public hostname is not live yet",
                    cloudflare.hostname
                ),
            ),
        }
    } else {
        print_optional("dns lookup", "skipped; no hostname configured".to_string());
    }

    if let (Some(cloudflare), Some(bind_addr), Some(path)) = (
        configured_cloudflare.as_ref(),
        bind_addr,
        cloudflared_config_path(),
    ) {
        if path.exists() {
            match load_cloudflared_config(&path) {
                Ok(parsed) => {
                    if let Some(service) =
                        ingress_service_for_hostname(&parsed, cloudflare.hostname.as_str())
                    {
                        print_status(
                            "ingress hostname",
                            true,
                            format!("matched `{}` in {}", cloudflare.hostname, path.display()),
                        );
                        let matches_bind = service_targets_bind(service, bind_addr);
                        let expected = format!("http://{bind_addr}");
                        let message = if matches_bind {
                            format!("routes `{}` to {}", cloudflare.hostname, service)
                        } else {
                            format!(
                                "routes `{}` to {}, expected {}",
                                cloudflare.hostname, service, expected
                            )
                        };
                        print_status("ingress service", matches_bind, message);
                    } else {
                        let available = configured_ingress_hostnames(&parsed);
                        let detail = if available.is_empty() {
                            format!(
                                "`{}` is not present in {}; no hostname-specific ingress rules were found",
                                cloudflare.hostname,
                                path.display()
                            )
                        } else {
                            format!(
                                "`{}` is not present in {}; found {}",
                                cloudflare.hostname,
                                path.display(),
                                available.join(", ")
                            )
                        };
                        print_status("ingress hostname", false, detail);
                    }
                }
                Err(error) => print_status(
                    "ingress config",
                    false,
                    format!("failed to parse {}: {}", path.display(), error),
                ),
            }
        } else {
            print_optional(
                "ingress hostname",
                "skipped; ~/.cloudflared/config.yml is missing".to_string(),
            );
        }
    } else {
        print_optional(
            "ingress hostname",
            "skipped; need a configured hostname, valid bind, and cloudflared config".to_string(),
        );
    }

    if let (Some(cloudflare), Some(bind_addr)) = (configured_cloudflare.as_ref(), bind_addr) {
        match probe_origin(bind_addr, cloudflare.hostname.as_str()).await {
            Ok(message) => print_status("origin probe", true, message),
            Err(message) => print_status("origin probe", false, message),
        }
    } else {
        print_optional(
            "origin probe",
            "skipped; need a configured hostname and valid bind".to_string(),
        );
    }

    let wrangler = command_status("wrangler", &["--version"]);
    if wrangler.ok {
        print_status("wrangler", true, wrangler.message);
    } else {
        print_optional(
            "wrangler",
            "not installed; fine unless you also deploy a Worker".to_string(),
        );
    }

    if let Some(tunnel_name) = configured_tunnel_name {
        if cloudflared.ok {
            let tunnel = command_status("cloudflared", &["tunnel", "info", &tunnel_name]);
            if tunnel.ok {
                print_status("tunnel lookup", true, format!("verified `{tunnel_name}`"));
            } else {
                print_optional(
                    "tunnel lookup",
                    format!(
                        "could not verify `{tunnel_name}` via `cloudflared tunnel info`; {}",
                        tunnel.message
                    ),
                );
            }
        } else {
            print_optional(
                "tunnel lookup",
                format!("skipped `{tunnel_name}` because cloudflared is not installed"),
            );
        }
    } else {
        print_optional(
            "tunnel lookup",
            "skipped; no tunnel name configured".to_string(),
        );
    }

    println!();
    println!("{}", "Reality Check".bold());
    println!("  - {}", "cloudflared is required for tunnel mode".cyan());
    println!(
        "  - {}",
        "wrangler is only required if you put a Worker in front".cyan()
    );
    println!("  - The tunnel still lives in Cloudflare and cloudflared; rzn-tools just starts the local process");

    Ok(())
}

fn connectors_changed(
    connectors: &[String],
    add_connectors: &[String],
    remove_connectors: &[String],
    all_connectors: bool,
) -> bool {
    all_connectors
        || !connectors.is_empty()
        || !add_connectors.is_empty()
        || !remove_connectors.is_empty()
}

fn apply_connector_changes(
    config: &mut ServeConfigFile,
    connectors: &[String],
    add_connectors: &[String],
    remove_connectors: &[String],
    all_connectors: bool,
) -> Result<()> {
    if all_connectors && !connectors.is_empty() {
        return Err(CommandError::InvalidInput(
            "cannot combine --all-connectors with --connectors".to_string(),
        ));
    }

    if all_connectors {
        config.expose_all_connectors = true;
        config.exposed_connectors.clear();
    }

    if !connectors.is_empty() {
        config.expose_all_connectors = false;
        config.exposed_connectors = parse_connector_names(connectors)?;
    }

    if config.expose_all_connectors && (!add_connectors.is_empty() || !remove_connectors.is_empty())
    {
        return Err(CommandError::InvalidInput(
            "cannot use --add-connectors or --remove-connectors while --all-connectors is active; set --connectors first".to_string(),
        ));
    }

    if !add_connectors.is_empty() {
        let additions = parse_connector_names(add_connectors)?;
        if config.exposed_connectors.is_empty() {
            config.exposed_connectors = default_exposed_connectors();
        }
        for connector in additions {
            push_unique_connector(&mut config.exposed_connectors, &connector);
        }
    }

    if !remove_connectors.is_empty() {
        let removals = parse_connector_names(remove_connectors)?;
        if config.exposed_connectors.is_empty() {
            config.exposed_connectors = default_exposed_connectors();
        }
        config
            .exposed_connectors
            .retain(|connector| !removals.contains(connector));
    }

    Ok(())
}

fn parse_connector_names(values: &[String]) -> Result<Vec<String>> {
    let mut connectors = Vec::new();
    for value in values {
        let normalized = normalize_connector_name(value)?;
        push_unique_connector(&mut connectors, &normalized);
    }
    Ok(connectors)
}

fn normalize_connector_name(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.is_empty() {
        return Err(CommandError::InvalidInput(
            "connector name cannot be empty".to_string(),
        ));
    }

    Ok(canonical_connector_name(&normalized).to_string())
}

fn push_unique_connector(connectors: &mut Vec<String>, connector: &str) {
    if !connectors.iter().any(|existing| existing == connector) {
        connectors.push(connector.to_string());
    }
}

fn default_exposed_connectors() -> Vec<String> {
    DEFAULT_EXPOSED_CONNECTORS
        .iter()
        .map(|connector| (*connector).to_string())
        .collect()
}

fn legacy_default_exposed_connectors() -> Vec<String> {
    LEGACY_DEFAULT_EXPOSED_CONNECTORS
        .iter()
        .map(|connector| (*connector).to_string())
        .collect()
}

fn migrate_legacy_default_connectors(config: &mut ServeConfigFile) -> bool {
    if config.expose_all_connectors || config.connector_defaults_version.is_some() {
        return false;
    }

    let configured = config
        .exposed_connectors
        .iter()
        .map(|connector| canonical_connector_name(connector).to_string())
        .collect::<Vec<_>>();

    if configured != legacy_default_exposed_connectors() {
        return false;
    }

    config.exposed_connectors = default_exposed_connectors();
    config.connector_defaults_version = Some(CURRENT_CONNECTOR_DEFAULTS_VERSION);
    true
}

fn effective_exposed_connectors(config: &ServeConfigFile) -> Vec<String> {
    if config.expose_all_connectors {
        return Vec::new();
    }
    if config.exposed_connectors.is_empty() {
        return default_exposed_connectors();
    }
    config
        .exposed_connectors
        .iter()
        .map(|connector| canonical_connector_name(connector).to_string())
        .collect()
}

fn normalized_connectors_for_http(config: &ServeConfigFile) -> Option<HashSet<String>> {
    if config.expose_all_connectors {
        return None;
    }

    let connectors = effective_exposed_connectors(config)
        .into_iter()
        .collect::<HashSet<_>>();
    Some(connectors)
}

fn canonical_connector_name(value: &str) -> &str {
    match value {
        "exa-search" => "exa",
        "youtube-transcripts" => "youtube",
        "youtube-transcript" => "youtube",
        "parallel" => "parallel-search",
        other => other,
    }
}

async fn available_connector_names() -> Result<Vec<String>> {
    let registry = build_registry_enabled_only().await;
    let mut connectors = registry.providers.keys().cloned().collect::<Vec<_>>();
    connectors.sort();
    Ok(connectors)
}

fn print_connector_inventory(
    config: &ServeConfigFile,
    effective_exposed_connectors: &[String],
    available_connectors: &[String],
) {
    println!("{}", "Connector inventory".bold().cyan());
    if config.expose_all_connectors {
        println!("Configured mode: {}", "all connectors".green());
    } else {
        println!(
            "Configured connectors: {}",
            effective_exposed_connectors.join(", ").green()
        );
    }
    println!(
        "Available compiled connectors: {}",
        available_connectors.join(", ").dimmed()
    );
    println!();
    println!("{}", "Manage connectors".bold());
    println!("  {}", "rzn-tools serve --add-connectors wikipedia".cyan());
    println!("  {}", "rzn-tools serve --remove-connectors reddit".cyan());
    println!(
        "  {}",
        "rzn-tools serve --connectors youtube,hackernews,pubmed,reddit".cyan()
    );
    println!("  {}", "rzn-tools serve --all-connectors".cyan());
}

fn print_exposed_connector_status(
    config: &ServeConfigFile,
    effective_exposed_connectors: &[String],
    available_connectors: &[String],
    unavailable_exposed: &[String],
) {
    if config.expose_all_connectors {
        println!(
            "Exposed connectors: {}",
            format!("all compiled ({})", available_connectors.len()).green()
        );
    } else {
        println!(
            "Exposed connectors: {}",
            effective_exposed_connectors.join(", ").green()
        );
    }

    if !unavailable_exposed.is_empty() {
        println!(
            "Unavailable connectors: {}",
            unavailable_exposed.join(", ").yellow()
        );
    }

    println!("{}", "Manage exposed connectors".bold());
    println!(
        "  add: {}",
        "rzn-tools serve --add-connectors <connector>".cyan()
    );
    println!(
        "  remove: {}",
        "rzn-tools serve --remove-connectors <connector>".cyan()
    );
    println!("  list: {}", "rzn-tools serve --list-connectors".cyan());
}

fn load_config() -> Result<ServeConfigFile> {
    let path = serve_config_path()?;
    if !path.exists() {
        return Ok(ServeConfigFile::default());
    }
    let data = fs::read_to_string(&path)?;
    let mut config: ServeConfigFile = serde_json::from_str(&data)
        .map_err(|error| CommandError::InvalidConfig(format!("{}: {}", path.display(), error)))?;

    if migrate_legacy_default_connectors(&mut config) {
        save_config(&path, &config)?;
    }

    Ok(config)
}

fn save_config(path: &Path, config: &ServeConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut config = config.clone();
    config
        .connector_defaults_version
        .get_or_insert(CURRENT_CONNECTOR_DEFAULTS_VERSION);
    let payload = serde_json::to_string_pretty(&config)
        .map_err(|error| CommandError::InvalidConfig(error.to_string()))?;
    fs::write(path, payload)?;
    Ok(())
}

fn serve_config_path() -> Result<PathBuf> {
    let store = FileAuthStore::new_default();
    let config_path = PathBuf::from(store.config_path());
    let base = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base.join("serve.json"))
}

fn parse_bind_addr(bind: &str) -> Result<SocketAddr> {
    bind.parse().map_err(|error| {
        CommandError::InvalidInput(format!("invalid bind address '{}': {}", bind, error))
    })
}

fn normalized_hosts(hosts: &[String]) -> Option<HashSet<String>> {
    let set = hosts
        .iter()
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect::<HashSet<_>>();
    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

fn push_unique_host(hosts: &mut Vec<String>, host: &str) {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return;
    }
    if !hosts.iter().any(|existing| existing == &host) {
        hosts.push(host);
    }
}

fn normalize_tunnel_name(tunnel_name: &str) -> Result<String> {
    let value = tunnel_name.trim();
    if value.is_empty() {
        return Err(CommandError::InvalidInput(
            "tunnel name cannot be empty".to_string(),
        ));
    }
    Ok(value.to_string())
}

fn should_autostart_tunnel(config: &ServeConfigFile, local_only: bool) -> bool {
    !local_only
        && config
            .cloudflare
            .as_ref()
            .and_then(|entry| entry.tunnel_name.as_ref())
            .is_some()
}

fn normalize_hostname(hostname: &str) -> Result<String> {
    let mut value = hostname.trim().to_ascii_lowercase();
    value = value
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();

    if value.is_empty() || value.contains('/') || value.contains(' ') {
        return Err(CommandError::InvalidInput(format!(
            "invalid hostname '{}'",
            hostname
        )));
    }

    Ok(value)
}

fn cloudflared_dir() -> Option<PathBuf> {
    let base = env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))?;
    Some(base.join(".cloudflared"))
}

fn cloudflared_config_path() -> Option<PathBuf> {
    cloudflared_dir().map(|path| path.join("config.yml"))
}

fn command_status(program: &str, args: &[&str]) -> CommandStatus {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => CommandStatus {
            ok: true,
            message: first_non_empty_line(&output.stdout)
                .or_else(|| first_non_empty_line(&output.stderr))
                .unwrap_or_else(|| "ok".to_string()),
        },
        Ok(output) => CommandStatus {
            ok: false,
            message: first_non_empty_line(&output.stderr)
                .or_else(|| first_non_empty_line(&output.stdout))
                .unwrap_or_else(|| format!("command exited with {}", output.status)),
        },
        Err(error) => CommandStatus {
            ok: false,
            message: error.to_string(),
        },
    }
}

async fn maybe_start_cloudflared_tunnel(
    config: &ServeConfigFile,
    local_only: bool,
) -> Result<Option<ManagedTunnel>> {
    if !should_autostart_tunnel(config, local_only) {
        return Ok(None);
    }

    let tunnel_name = config
        .cloudflare
        .as_ref()
        .and_then(|entry| entry.tunnel_name.as_ref())
        .cloned()
        .ok_or_else(|| CommandError::Other("missing tunnel name".to_string()))?;

    println!(
        "{} {}",
        "Starting Cloudflare tunnel:".bold(),
        format!("cloudflared tunnel run {tunnel_name}").cyan()
    );

    let mut child = Command::new("cloudflared")
        .args(["tunnel", "run", tunnel_name.as_str()])
        .spawn()
        .map_err(|error| {
            CommandError::Other(format!(
                "failed to start cloudflared: {}. Install cloudflared or run `rzn-tools serve --local-only`",
                error
            ))
        })?;

    sleep(Duration::from_millis(500)).await;
    if let Some(status) = child
        .try_wait()
        .map_err(|error| CommandError::Other(format!("failed to inspect cloudflared: {error}")))?
    {
        return Err(CommandError::Other(format!(
            "cloudflared tunnel `{}` exited immediately with status {}. Run `rzn-tools configure cloudflare doctor`.",
            tunnel_name, status
        )));
    }

    Ok(Some(ManagedTunnel { tunnel_name, child }))
}

fn stop_cloudflared_tunnel(tunnel: &mut ManagedTunnel) -> Result<()> {
    if tunnel
        .child
        .try_wait()
        .map_err(|error| CommandError::Other(format!("failed to inspect cloudflared: {error}")))?
        .is_some()
    {
        return Ok(());
    }

    println!(
        "{} {}",
        "Stopping Cloudflare tunnel:".bold(),
        tunnel.tunnel_name.cyan()
    );
    tunnel
        .child
        .kill()
        .map_err(|error| CommandError::Other(format!("failed to stop cloudflared: {error}")))?;
    let _ = tunnel.child.wait();
    Ok(())
}

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn print_status(label: &str, ok: bool, message: String) {
    if ok {
        println!("{} {label}: {message}", "[ok]".green());
    } else {
        println!("{} {label}: {message}", "[warn]".yellow());
    }
}

fn print_optional(label: &str, message: String) {
    println!("{} {label}: {message}", "[info]".cyan());
}

struct CommandStatus {
    ok: bool,
    message: String,
}

struct ManagedTunnel {
    tunnel_name: String,
    child: Child,
}

#[derive(Debug, Deserialize)]
struct CloudflaredConfigFile {
    ingress: Option<Vec<CloudflaredIngressRule>>,
}

#[derive(Debug, Deserialize)]
struct CloudflaredIngressRule {
    hostname: Option<String>,
    service: Option<String>,
}

fn load_cloudflared_config(path: &Path) -> std::result::Result<CloudflaredConfigFile, String> {
    let data = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    serde_yaml::from_str(&data).map_err(|error| error.to_string())
}

fn configured_ingress_hostnames(config: &CloudflaredConfigFile) -> Vec<String> {
    let Some(rules) = config.ingress.as_ref() else {
        return Vec::new();
    };

    let mut hosts = rules
        .iter()
        .filter_map(|rule| rule.hostname.as_ref())
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect::<Vec<_>>();
    hosts.sort();
    hosts.dedup();
    hosts
}

fn ingress_service_for_hostname<'a>(
    config: &'a CloudflaredConfigFile,
    hostname: &str,
) -> Option<&'a str> {
    let hostname = hostname.trim().to_ascii_lowercase();
    config
        .ingress
        .as_ref()?
        .iter()
        .find(|rule| {
            rule.hostname
                .as_ref()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case(&hostname))
        })
        .and_then(|rule| rule.service.as_deref())
}

fn service_targets_bind(service: &str, bind_addr: SocketAddr) -> bool {
    let Ok(url) = reqwest::Url::parse(service) else {
        return false;
    };

    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    if port != bind_addr.port() {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return bind_addr.ip().is_loopback();
    }

    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip == bind_addr.ip() || (ip.is_loopback() && bind_addr.ip().is_loopback()))
}

async fn resolve_hostname(hostname: &str) -> Option<Vec<String>> {
    let mut addresses = tokio::net::lookup_host((hostname, 443))
        .await
        .ok()?
        .map(|addr| addr.ip().to_string())
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    (!addresses.is_empty()).then_some(addresses)
}

async fn probe_origin(
    bind_addr: SocketAddr,
    hostname: &str,
) -> std::result::Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;

    let readyz_url = format!("http://{bind_addr}/readyz");
    let readyz = client
        .get(&readyz_url)
        .send()
        .await
        .map_err(|error| format!("nothing healthy is answering on {bind_addr}: {error}"))?;
    if !readyz.status().is_success() {
        return Err(format!(
            "origin at {bind_addr} answered {} for /readyz",
            readyz.status()
        ));
    }

    let mcp_url = format!("http://{bind_addr}/mcp");
    let response = client
        .get(&mcp_url)
        .header("Host", hostname)
        .send()
        .await
        .map_err(|error| format!("failed to probe /mcp on {bind_addr}: {error}"))?;

    match response.status() {
        reqwest::StatusCode::METHOD_NOT_ALLOWED => Ok(format!(
            "origin accepted Host `{hostname}` on {bind_addr}"
        )),
        reqwest::StatusCode::MISDIRECTED_REQUEST => Err(format!(
            "origin rejected Host `{hostname}` with 421; restart any stale `rzn-tools serve` process after changing hostname"
        )),
        status => {
            let body = response
                .text()
                .await
                .ok()
                .map(|text| text.trim().replace('\n', " "))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "empty response body".to_string());
            Err(format!(
                "origin answered {status} for Host `{hostname}` on /mcp: {body}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_exposed_connectors, effective_exposed_connectors, ingress_service_for_hostname,
        legacy_default_exposed_connectors, migrate_legacy_default_connectors, normalize_hostname,
        normalize_tunnel_name, normalized_hosts, service_targets_bind, should_autostart_tunnel,
        CloudflareTunnelConfig, CloudflaredConfigFile, ServeConfigFile,
    };

    #[test]
    fn defaults_to_only_the_shipped_public_connectors() {
        assert_eq!(
            default_exposed_connectors(),
            vec!["youtube", "hackernews", "pubmed", "reddit"]
        );
    }

    #[test]
    fn empty_config_uses_default_exposed_connectors() {
        let config = ServeConfigFile::default();

        assert_eq!(
            effective_exposed_connectors(&config),
            vec!["youtube", "hackernews", "pubmed", "reddit"]
        );
    }

    #[test]
    fn migrates_legacy_persisted_default_connectors() {
        let mut config = ServeConfigFile {
            bind: None,
            allowed_hosts: Vec::new(),
            exposed_connectors: legacy_default_exposed_connectors(),
            connector_defaults_version: None,
            expose_all_connectors: false,
            cloudflare: None,
        };

        assert!(migrate_legacy_default_connectors(&mut config));
        assert_eq!(
            effective_exposed_connectors(&config),
            vec!["youtube", "hackernews", "pubmed", "reddit"]
        );
        assert_eq!(config.connector_defaults_version, Some(2));
    }

    #[test]
    fn preserves_explicit_current_connector_selection() {
        let mut config = ServeConfigFile {
            bind: None,
            allowed_hosts: Vec::new(),
            exposed_connectors: legacy_default_exposed_connectors(),
            connector_defaults_version: Some(2),
            expose_all_connectors: false,
            cloudflare: None,
        };

        assert!(!migrate_legacy_default_connectors(&mut config));
        assert_eq!(
            effective_exposed_connectors(&config),
            vec![
                "youtube",
                "hackernews",
                "pubmed",
                "parallel-search",
                "exa",
                "reddit"
            ]
        );
    }

    #[test]
    fn normalizes_cloudflare_hostnames() {
        assert_eq!(
            normalize_hostname("https://Rzn-Tools-Origin.Example.com/").unwrap(),
            "rzn-tools-origin.example.com"
        );
    }

    #[test]
    fn dedupes_and_lowercases_allowed_hosts() {
        let hosts = normalized_hosts(&[
            "LOCALHOST".to_string(),
            "localhost".to_string(),
            "Example.com".to_string(),
        ])
        .unwrap();

        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains("localhost"));
        assert!(hosts.contains("example.com"));
    }

    #[test]
    fn trims_tunnel_name_without_lowercasing_it() {
        assert_eq!(
            normalize_tunnel_name("  Rzn-Tools-Mcp  ").unwrap(),
            "Rzn-Tools-Mcp"
        );
    }

    #[test]
    fn autostarts_tunnel_only_when_configured_and_not_local_only() {
        let config = ServeConfigFile {
            bind: None,
            allowed_hosts: Vec::new(),
            exposed_connectors: Vec::new(),
            connector_defaults_version: None,
            expose_all_connectors: false,
            cloudflare: Some(CloudflareTunnelConfig {
                hostname: "example.com".to_string(),
                tunnel_name: Some("rzn-tools-mcp".to_string()),
            }),
        };

        assert!(should_autostart_tunnel(&config, false));
        assert!(!should_autostart_tunnel(&config, true));
    }

    #[test]
    fn finds_ingress_service_for_matching_hostname() {
        let config: CloudflaredConfigFile = serde_yaml::from_str(
            r#"
ingress:
  - hostname: rzn-tools.sarav.xyz
    service: http://localhost:8000
  - service: http_status:404
"#,
        )
        .unwrap();

        assert_eq!(
            ingress_service_for_hostname(&config, "RZN-TOOLS.SARAV.XYZ"),
            Some("http://localhost:8000")
        );
    }

    #[test]
    fn matches_loopback_service_targets_against_bind() {
        let bind_addr = "127.0.0.1:8000".parse().unwrap();

        assert!(service_targets_bind("http://localhost:8000", bind_addr));
        assert!(service_targets_bind("http://127.0.0.1:8000", bind_addr));
        assert!(!service_targets_bind("http://localhost:8787", bind_addr));
        assert!(!service_targets_bind("http_status:404", bind_addr));
    }
}
