use crate::cli::{Cli, IngestAction};
use crate::commands::{CommandError, Result};
use crate::output::{format_output, OutputData};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, ContentArrangement, Table};
use owo_colors::OwoColorize;
use rzn_tools_core::auth_store::FileAuthStore;
use rzn_tools_core::ingest::{
    now_rfc3339, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, Partial, Source,
    NORMALIZED_ITEM_V1_TYPE, NORMALIZED_PAGE_V1_TYPE,
};
use rzn_tools_core::mcp_server::{IngestSource, ListIngestSourcesParams, McpServer};
use rzn_tools_core::{CallToolRequestParam, ProviderRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

const DEFAULT_TENANT: &str = "default";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct IngestSourceConfig {
    id: String,
    connector: String,
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    auth_required: bool,
    #[serde(default)]
    default_args: JsonMap<String, Value>,
    #[serde(default)]
    args: JsonMap<String, Value>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    cadence_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct IngestConfigFile {
    version: u32,
    #[serde(default)]
    sources: Vec<IngestSourceConfig>,
}

impl Default for IngestConfigFile {
    fn default() -> Self {
        Self {
            version: 1,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct IngestRunSummary {
    tenant: String,
    ran_at: String,
    sources: Vec<IngestSourceRunSummary>,
}

#[derive(Debug, Serialize)]
struct IngestSourceRunSummary {
    id: String,
    tool: String,
    pages: u32,
    items: u64,
    blocks: u64,
    next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct IndexWriter {
    items_path: PathBuf,
    blocks_path: PathBuf,
    seen_items_path: PathBuf,
    seen_blocks_path: PathBuf,
    seen_items: HashSet<String>,
    seen_blocks: HashSet<String>,
}

impl IndexWriter {
    fn new(base_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&base_dir)?;
        let items_path = base_dir.join("items.jsonl");
        let blocks_path = base_dir.join("blocks.jsonl");
        let seen_items_path = base_dir.join("seen_items.txt");
        let seen_blocks_path = base_dir.join("seen_blocks.txt");
        let seen_items = load_seen_set(&seen_items_path)?;
        let seen_blocks = load_seen_set(&seen_blocks_path)?;
        Ok(Self {
            items_path,
            blocks_path,
            seen_items_path,
            seen_blocks_path,
            seen_items,
            seen_blocks,
        })
    }

    fn write_item(
        &mut self,
        item: &ContentItem,
        source: &Source,
        partial: &Partial,
    ) -> Result<bool> {
        if self.seen_items.contains(&item.item_ref) {
            return Ok(false);
        }
        let record = json!({
            "item": item,
            "source": source,
            "partial": partial,
        });
        append_jsonl(&self.items_path, &record)?;
        append_seen(&self.seen_items_path, &item.item_ref)?;
        self.seen_items.insert(item.item_ref.clone());
        Ok(true)
    }

    fn write_block(
        &mut self,
        item_ref: &str,
        block: &ContentBlock,
        source: &Source,
    ) -> Result<bool> {
        if self.seen_blocks.contains(&block.block_ref) {
            return Ok(false);
        }
        let record = json!({
            "item_ref": item_ref,
            "block": block,
            "source": source,
        });
        append_jsonl(&self.blocks_path, &record)?;
        append_seen(&self.seen_blocks_path, &block.block_ref)?;
        self.seen_blocks.insert(block.block_ref.clone());
        Ok(true)
    }
}

pub async fn run(cli: &Cli, action: IngestAction) -> Result<()> {
    match action {
        IngestAction::Sources {
            connectors,
            categories,
            include_read,
            include_fetch,
        } => list_sources(cli, connectors, categories, include_read, include_fetch).await,
        IngestAction::Add {
            id,
            args,
            tenant,
            disabled,
            cadence_seconds,
            include_fetch,
        } => {
            add_source(
                cli,
                &id,
                args.as_deref(),
                tenant.as_deref(),
                disabled,
                cadence_seconds,
                include_fetch,
            )
            .await
        }
        IngestAction::List { tenant } => list_configured(cli, tenant.as_deref()).await,
        IngestAction::Remove { id, tenant } => remove_source(cli, &id, tenant.as_deref()).await,
        IngestAction::Run {
            tenant,
            id,
            max_pages,
            max_items,
            interval_seconds,
            include_disabled,
        } => {
            run_ingest_loop(
                cli,
                tenant.as_deref(),
                id.as_deref(),
                max_pages,
                max_items,
                interval_seconds,
                include_disabled,
            )
            .await
        }
    }
}

async fn list_sources(
    cli: &Cli,
    connectors: Option<String>,
    categories: Option<String>,
    include_read: bool,
    include_fetch: bool,
) -> Result<()> {
    let registry = create_registry(cli.auth_profile.as_deref()).await?;
    let server = McpServer::new(std::sync::Arc::new(tokio::sync::Mutex::new(registry)));

    let params = ListIngestSourcesParams {
        connectors: split_csv(connectors),
        categories: split_csv(categories),
        include_read,
        include_fetch,
    };

    let result = server
        .handle_list_ingest_sources_with_params(Some(params))
        .await?;

    if matches!(cli.output, crate::cli::OutputFormat::Pretty) {
        format_pretty_ingest_sources(&result.ingest_sources)?;
        return Ok(());
    }

    let output = OutputData::ToolResult(serde_json::to_value(result)?);
    format_output(&output, &cli.output)?;
    Ok(())
}

async fn add_source(
    cli: &Cli,
    id: &str,
    args: Option<&str>,
    tenant: Option<&str>,
    disabled: bool,
    cadence_seconds: Option<u64>,
    include_fetch: bool,
) -> Result<()> {
    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let ingest_source = find_ingest_source(cli.auth_profile.as_deref(), id, include_fetch).await?;

    let args_provided = args.is_some();
    let args_map = match args {
        Some(raw) => parse_args_json(raw)?,
        None => JsonMap::new(),
    };

    let config_path = ingest_config_path(tenant)?;
    let mut config = load_config(&config_path)?;

    let mut existing_cursor: Option<String> = None;
    let mut existing_last_run: Option<String> = None;
    let mut existing_last_error: Option<String> = None;
    let mut existing_enabled: Option<bool> = None;
    let mut existing_args: Option<JsonMap<String, Value>> = None;

    if let Some(existing) = config.sources.iter().find(|s| s.id == ingest_source.id) {
        existing_cursor = existing.last_cursor.clone();
        existing_last_run = existing.last_run_at.clone();
        existing_last_error = existing.last_error.clone();
        existing_enabled = Some(existing.enabled);
        existing_args = Some(existing.args.clone());
    }

    let final_args = if args_provided {
        args_map
    } else {
        existing_args.unwrap_or_default()
    };

    let enabled = if disabled {
        false
    } else {
        existing_enabled.unwrap_or(true)
    };

    let new_config = IngestSourceConfig {
        id: ingest_source.id.clone(),
        connector: ingest_source.connector.clone(),
        display_name: ingest_source.display_name.clone(),
        description: ingest_source.description.clone(),
        tool: ingest_source.tool.clone(),
        category: ingest_source.category.clone(),
        tags: ingest_source.tags.clone(),
        auth_required: ingest_source.auth_required,
        default_args: ingest_source.default_args.clone(),
        args: final_args,
        enabled,
        cadence_seconds,
        last_cursor: existing_cursor,
        last_run_at: existing_last_run,
        last_error: existing_last_error,
    };

    config.sources.retain(|s| s.id != ingest_source.id);
    config.sources.push(new_config);
    config
        .sources
        .sort_by(|a, b| a.display_name.cmp(&b.display_name));

    save_config(&config_path, &config)?;

    if matches!(cli.output, crate::cli::OutputFormat::Pretty) {
        println!(
            "{} Added ingest source {} for tenant {}",
            "✓".green().bold(),
            ingest_source.id.cyan(),
            tenant.yellow()
        );
        println!("Config: {}", config_path.display().to_string().dimmed());
        return Ok(());
    }

    let output = OutputData::ToolResult(json!({
        "added": ingest_source.id,
        "tenant": tenant,
        "config_path": config_path.display().to_string(),
    }));
    format_output(&output, &cli.output)?;
    Ok(())
}

async fn list_configured(cli: &Cli, tenant: Option<&str>) -> Result<()> {
    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let config_path = ingest_config_path(tenant)?;
    let config = load_config(&config_path)?;

    if matches!(cli.output, crate::cli::OutputFormat::Pretty) {
        format_pretty_ingest_config(&config, tenant, &config_path)?;
        return Ok(());
    }

    let output = OutputData::ToolResult(json!({
        "tenant": tenant,
        "config_path": config_path.display().to_string(),
        "sources": config.sources,
    }));
    format_output(&output, &cli.output)?;
    Ok(())
}

async fn remove_source(cli: &Cli, id: &str, tenant: Option<&str>) -> Result<()> {
    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let config_path = ingest_config_path(tenant)?;
    let mut config = load_config(&config_path)?;
    let before = config.sources.len();
    config.sources.retain(|s| s.id != id);

    if before == config.sources.len() {
        return Err(CommandError::InvalidInput(format!(
            "Ingest source not found: {}",
            id
        )));
    }

    save_config(&config_path, &config)?;

    if matches!(cli.output, crate::cli::OutputFormat::Pretty) {
        println!(
            "{} Removed ingest source {} for tenant {}",
            "✓".green().bold(),
            id.cyan(),
            tenant.yellow()
        );
        return Ok(());
    }

    let output = OutputData::ToolResult(json!({
        "removed": id,
        "tenant": tenant,
        "config_path": config_path.display().to_string(),
    }));
    format_output(&output, &cli.output)?;
    Ok(())
}

async fn run_ingest_loop(
    cli: &Cli,
    tenant: Option<&str>,
    id: Option<&str>,
    max_pages: u32,
    max_items: Option<u32>,
    interval_seconds: Option<u64>,
    include_disabled: bool,
) -> Result<()> {
    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let config_path = ingest_config_path(tenant)?;

    loop {
        let mut config = load_config(&config_path)?;
        if config.sources.is_empty() {
            return Err(CommandError::InvalidInput(format!(
                "No ingest sources configured for tenant '{}'",
                tenant
            )));
        }

        let registry = create_registry(cli.auth_profile.as_deref()).await?;
        let registry = std::sync::Arc::new(tokio::sync::Mutex::new(registry));
        let server = McpServer::new(registry.clone());

        let index_dir = ingest_index_dir(tenant)?;
        let mut index = IndexWriter::new(index_dir)?;

        let mut summaries = Vec::new();
        let run_at = now_rfc3339();

        for source in config.sources.iter_mut() {
            if let Some(filter_id) = id {
                if source.id != filter_id {
                    continue;
                }
            }

            if !include_disabled && !source.enabled {
                continue;
            }

            let summary = run_ingest_source(
                &server,
                source,
                max_pages,
                max_items.map(|v| v as u64),
                &mut index,
            )
            .await;
            summaries.push(summary);
        }

        if id.is_some() && summaries.is_empty() {
            return Err(CommandError::InvalidInput(format!(
                "Ingest source not found or disabled: {}",
                id.unwrap_or_default()
            )));
        }

        save_config(&config_path, &config)?;

        let run_summary = IngestRunSummary {
            tenant: tenant.to_string(),
            ran_at: run_at,
            sources: summaries,
        };

        if matches!(cli.output, crate::cli::OutputFormat::Pretty) {
            format_pretty_ingest_run(&run_summary)?;
        } else {
            let output = OutputData::ToolResult(serde_json::to_value(&run_summary)?);
            format_output(&output, &cli.output)?;
        }

        if let Some(interval) = interval_seconds {
            sleep(Duration::from_secs(interval)).await;
            continue;
        }
        break;
    }

    Ok(())
}

async fn run_ingest_source(
    server: &McpServer,
    source: &mut IngestSourceConfig,
    max_pages: u32,
    max_items: Option<u64>,
    index: &mut IndexWriter,
) -> IngestSourceRunSummary {
    let mut pages = 0u32;
    let mut items = 0u64;
    let mut blocks = 0u64;
    let mut last_error: Option<String> = None;
    let mut cursor = source.last_cursor.clone();
    let mut saw_success = false;

    while pages < max_pages {
        let args = build_args(&source.default_args, &source.args, cursor.as_deref());
        let request = CallToolRequestParam {
            name: source.tool.clone().into(),
            arguments: Some(args),
        };

        match server.handle_call_tool(request).await {
            Ok(result) => {
                let payload = match result.structured_content {
                    Some(value) => value,
                    None => {
                        last_error = Some("Missing structured_content in tool result".to_string());
                        break;
                    }
                };

                let type_field = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if type_field == NORMALIZED_PAGE_V1_TYPE {
                    match serde_json::from_value::<NormalizedPageV1>(payload) {
                        Ok(page) => {
                            let mut page_has_more = page.has_more;
                            let mut next_cursor = page.next_cursor.clone();
                            match index_page(index, &page.items, &page.source, &page.partial) {
                                Ok((item_count, block_count)) => {
                                    items += item_count;
                                    blocks += block_count;
                                }
                                Err(err) => {
                                    last_error = Some(err.to_string());
                                    break;
                                }
                            }
                            pages += 1;
                            saw_success = true;

                            if let Some(limit) = max_items {
                                if items >= limit {
                                    page_has_more = false;
                                    next_cursor = None;
                                }
                            }

                            if !page_has_more || next_cursor.is_none() {
                                cursor = None;
                                break;
                            }

                            cursor = next_cursor;
                        }
                        Err(err) => {
                            last_error = Some(format!("Failed to parse normalized page: {}", err));
                            break;
                        }
                    }
                } else if type_field == NORMALIZED_ITEM_V1_TYPE {
                    match serde_json::from_value::<NormalizedItemV1>(payload) {
                        Ok(item) => {
                            match index_item(index, &item.item, &item.source, &item.partial) {
                                Ok((item_added, block_added)) => {
                                    items += item_added;
                                    blocks += block_added;
                                    pages += 1;
                                    saw_success = true;
                                }
                                Err(err) => {
                                    last_error = Some(err.to_string());
                                    break;
                                }
                            }
                            cursor = None;
                            break;
                        }
                        Err(err) => {
                            last_error = Some(format!("Failed to parse normalized item: {}", err));
                            break;
                        }
                    }
                } else {
                    last_error = Some(format!(
                        "Unsupported normalized payload type: {}",
                        type_field
                    ));
                    break;
                }
            }
            Err(err) => {
                last_error = Some(err.to_string());
                break;
            }
        }
    }

    source.last_run_at = Some(now_rfc3339());
    if saw_success {
        source.last_cursor = cursor.clone();
    }
    source.last_error = last_error.clone();

    IngestSourceRunSummary {
        id: source.id.clone(),
        tool: source.tool.clone(),
        pages,
        items,
        blocks,
        next_cursor: cursor,
        error: last_error,
    }
}

fn index_page(
    index: &mut IndexWriter,
    items: &[ContentItem],
    source: &Source,
    partial: &Partial,
) -> std::result::Result<(u64, u64), CommandError> {
    let mut item_count = 0u64;
    let mut block_count = 0u64;
    for item in items {
        let (item_added, block_added) = index_item(index, item, source, partial)?;
        item_count += item_added;
        block_count += block_added;
    }
    Ok((item_count, block_count))
}

fn index_item(
    index: &mut IndexWriter,
    item: &ContentItem,
    source: &Source,
    partial: &Partial,
) -> std::result::Result<(u64, u64), CommandError> {
    let mut item_added = 0u64;
    let mut block_added = 0u64;
    if index.write_item(item, source, partial)? {
        item_added = 1;
    }
    for block in &item.blocks {
        if index.write_block(&item.item_ref, block, source)? {
            block_added += 1;
        }
    }
    Ok((item_added, block_added))
}

fn build_args(
    default_args: &JsonMap<String, Value>,
    overrides: &JsonMap<String, Value>,
    cursor: Option<&str>,
) -> JsonMap<String, Value> {
    let mut args = default_args.clone();
    for (key, value) in overrides {
        args.insert(key.clone(), value.clone());
    }
    args.insert("output_format".to_string(), json!("normalized_v1"));
    if let Some(cursor) = cursor {
        args.insert("cursor".to_string(), json!(cursor));
    } else {
        args.remove("cursor");
    }
    args
}

async fn find_ingest_source(
    auth_profile: Option<&str>,
    id: &str,
    include_fetch: bool,
) -> Result<IngestSource> {
    let registry = create_registry(auth_profile).await?;
    let server = McpServer::new(std::sync::Arc::new(tokio::sync::Mutex::new(registry)));
    let params = ListIngestSourcesParams {
        connectors: Vec::new(),
        categories: Vec::new(),
        include_read: true,
        include_fetch,
    };
    let result = server
        .handle_list_ingest_sources_with_params(Some(params))
        .await?;

    let needle = id.trim();
    result
        .ingest_sources
        .into_iter()
        .find(|s| matches_source(s, needle))
        .ok_or_else(|| CommandError::InvalidInput(format!("Unknown ingest source: {}", id)))
}

fn matches_source(source: &IngestSource, needle: &str) -> bool {
    if source.id == needle {
        return true;
    }
    if source.tool == needle {
        return true;
    }
    if let Some((connector, tool)) = needle.split_once(':') {
        if source.connector == connector && source.tool.ends_with(tool) {
            return true;
        }
    }
    false
}

fn parse_args_json(raw: &str) -> Result<JsonMap<String, Value>> {
    let value: Value = serde_json::from_str(raw)?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(CommandError::InvalidInput(
            "Arguments must be a JSON object".to_string(),
        )),
    }
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn ingest_config_path(tenant: &str) -> Result<PathBuf> {
    let base = base_config_dir()?;
    let tenant = sanitize_tenant(tenant);
    let path = if tenant == DEFAULT_TENANT {
        base.join("ingest_sources.json")
    } else {
        base.join("tenants")
            .join(tenant)
            .join("ingest_sources.json")
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn ingest_index_dir(tenant: &str) -> Result<PathBuf> {
    let base = base_config_dir()?;
    let tenant = sanitize_tenant(tenant);
    let path = if tenant == DEFAULT_TENANT {
        base.join("ingest")
    } else {
        base.join("tenants").join(tenant).join("ingest")
    };
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn base_config_dir() -> Result<PathBuf> {
    let store = FileAuthStore::new_default();
    let config_path = PathBuf::from(store.config_path());
    let base = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base)
}

fn sanitize_tenant(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        DEFAULT_TENANT.to_string()
    } else {
        out
    }
}

fn load_config(path: &Path) -> Result<IngestConfigFile> {
    if !path.exists() {
        return Ok(IngestConfigFile::default());
    }
    let data = fs::read_to_string(path)?;
    let parsed: IngestConfigFile = serde_json::from_str(&data)?;
    Ok(parsed)
}

fn save_config(path: &Path, config: &IngestConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string_pretty(config)?;
    fs::write(path, payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn load_seen_set(path: &Path) -> Result<HashSet<String>> {
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut set = HashSet::new();
    for line in reader.lines().map_while(std::result::Result::ok) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }
    Ok(set)
}

fn append_seen(path: &Path, id: &str) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", id)?;
    Ok(())
}

fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}

async fn create_registry(auth_profile: Option<&str>) -> Result<ProviderRegistry> {
    crate::commands::list::create_registry(auth_profile).await
}

fn format_pretty_ingest_sources(sources: &[IngestSource]) -> Result<()> {
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(100) as usize;
    let desc_width = term_width.saturating_sub(70).max(20);

    println!("{}", "Ingest Sources".bold().cyan());
    println!();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec![
            "ID",
            "Connector",
            "Tool",
            "Category",
            "Auth",
            "Description",
        ]);

    for source in sources {
        table.add_row(vec![
            source.id.clone(),
            source.connector.clone(),
            source.tool.clone(),
            source.category.clone().unwrap_or_else(|| "-".to_string()),
            if source.auth_required {
                "required".to_string()
            } else {
                "none".to_string()
            },
            truncate_text(source.description.as_deref().unwrap_or(""), desc_width),
        ]);
    }

    println!("{}", table);
    println!();
    println!(
        "{} Add one with {}",
        "Tip:".green().bold(),
        "rzn-tools ingest add <id> --args '{\"key\":\"value\"}'".cyan()
    );
    Ok(())
}

fn format_pretty_ingest_config(
    config: &IngestConfigFile,
    tenant: &str,
    config_path: &Path,
) -> Result<()> {
    println!("{}", "Configured Ingest Sources".bold().cyan());
    println!();
    println!("Tenant: {}", tenant.yellow());
    println!("Config: {}", config_path.display().to_string().dimmed());
    println!();

    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(100) as usize;
    let desc_width = term_width.saturating_sub(80).max(20);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec![
            "ID",
            "Tool",
            "Enabled",
            "Last Run",
            "Last Cursor",
            "Last Error",
        ]);

    for source in &config.sources {
        table.add_row(vec![
            source.id.clone(),
            source.tool.clone(),
            if source.enabled { "yes" } else { "no" }.to_string(),
            source
                .last_run_at
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            source
                .last_cursor
                .clone()
                .map(|c| truncate_text(&c, 24))
                .unwrap_or_else(|| "-".to_string()),
            truncate_text(source.last_error.as_deref().unwrap_or(""), desc_width),
        ]);
    }

    println!("{}", table);
    Ok(())
}

fn format_pretty_ingest_run(summary: &IngestRunSummary) -> Result<()> {
    println!("{}", "Ingest Run Summary".bold().cyan());
    println!();
    println!("Tenant: {}", summary.tenant.yellow());
    println!("Ran at: {}", summary.ran_at.dimmed());
    println!();

    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(100) as usize;
    let desc_width = term_width.saturating_sub(70).max(20);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec!["ID", "Tool", "Pages", "Items", "Blocks", "Status"]);

    for source in &summary.sources {
        let status = if let Some(err) = &source.error {
            truncate_text(err, desc_width)
        } else {
            "ok".to_string()
        };
        table.add_row(vec![
            source.id.clone(),
            source.tool.clone(),
            source.pages.to_string(),
            source.items.to_string(),
            source.blocks.to_string(),
            status,
        ]);
    }

    println!("{}", table);
    Ok(())
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if text.len() <= max_width {
        text.to_string()
    } else if max_width > 3 {
        format!("{}...", &text[..max_width - 3])
    } else {
        text.chars().take(max_width).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_args_and_cursor() {
        let mut defaults = JsonMap::new();
        defaults.insert("limit".to_string(), json!(10));
        let mut overrides = JsonMap::new();
        overrides.insert("query".to_string(), json!("rust"));
        let args = build_args(&defaults, &overrides, Some("abc"));
        assert_eq!(args.get("limit").and_then(|v| v.as_i64()), Some(10));
        assert_eq!(args.get("query").and_then(|v| v.as_str()), Some("rust"));
        assert_eq!(args.get("cursor").and_then(|v| v.as_str()), Some("abc"));
        assert_eq!(
            args.get("output_format").and_then(|v| v.as_str()),
            Some("normalized_v1")
        );
    }

    #[test]
    fn parses_csv() {
        let values = split_csv(Some("a, b ,c".to_string()));
        assert_eq!(values, vec!["a", "b", "c"]);
    }

    #[test]
    fn sanitizes_tenant() {
        assert_eq!(sanitize_tenant("team-1"), "team-1");
        assert_eq!(sanitize_tenant("team 1"), "team_1");
        assert_eq!(sanitize_tenant(""), DEFAULT_TENANT);
    }
}
