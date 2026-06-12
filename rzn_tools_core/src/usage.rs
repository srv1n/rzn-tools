use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("other: {0}")]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BillingCategory {
    AuthOnly,
    Metered,
}

#[derive(Debug, Clone)]
pub enum PricingModel {
    PerRequest {
        unit_cost_usd: f64,
    },
    PerToken {
        input_cost_usd: f64,
        output_cost_usd: f64,
    },
    PerTokenPlusRequest {
        input_cost_usd: f64,
        output_cost_usd: f64,
        request_cost_usd: f64,
    },
    PerResult {
        unit_cost_usd: f64,
    },
    PerRequestPlusResults {
        base_cost_usd: f64,
        included_results: u64,
        per_result_usd: f64,
    },
    ProviderReported,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageUnits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub event_id: String,
    pub run_id: String,
    pub request_id: String,
    pub connector: String,
    pub tool: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    pub category: BillingCategory,
    pub units: UsageUnits,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub currency: String,
    pub estimated: bool,
    pub pricing_version: String,
    pub status: String,
    pub duration_ms: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunSummary {
    pub run_id: String,
    pub total_cost_usd: f64,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_results: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub total_cost_usd: f64,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_results: u64,
    pub runs: HashMap<String, RunSummary>,
}

pub trait UsageStore: Send + Sync {
    fn record(&self, event: &UsageEvent) -> Result<(), UsageError>;
    fn load_all(&self) -> Result<Vec<UsageEvent>, UsageError>;
}

pub struct InMemoryUsageStore {
    events: Mutex<Vec<UsageEvent>>,
}

impl InMemoryUsageStore {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl Default for InMemoryUsageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageStore for InMemoryUsageStore {
    fn record(&self, event: &UsageEvent) -> Result<(), UsageError> {
        let mut guard = self.events.lock().expect("usage store poisoned");
        guard.push(event.clone());
        Ok(())
    }

    fn load_all(&self) -> Result<Vec<UsageEvent>, UsageError> {
        let guard = self.events.lock().expect("usage store poisoned");
        Ok(guard.clone())
    }
}

pub struct FileUsageStore {
    path: PathBuf,
    file: Mutex<File>,
}

impl FileUsageStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, UsageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    pub fn new_default() -> Result<Self, UsageError> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = home.join(".rzn-tools");
        let path = dir.join("usage.jsonl");
        Self::new(path)
    }
}

impl UsageStore for FileUsageStore {
    fn record(&self, event: &UsageEvent) -> Result<(), UsageError> {
        let mut file = self.file.lock().expect("usage store poisoned");
        let line = serde_json::to_string(event)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn load_all(&self) -> Result<Vec<UsageEvent>, UsageError> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: UsageEvent = serde_json::from_str(&line)?;
            out.push(event);
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct PricingEntry {
    pub pattern: String,
    pub model_pattern: Option<String>,
    pub category: BillingCategory,
    pub model: PricingModel,
    pub currency: String,
}

#[derive(Debug, Clone)]
pub struct PricingCatalog {
    pub version: String,
    pub entries: Vec<PricingEntry>,
    pub default_currency: String,
    pub default_category: BillingCategory,
}

impl PricingCatalog {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, UsageError> {
        let content = fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    pub fn load_default() -> Result<Self, UsageError> {
        if let Ok(path) = std::env::var("RZN_TOOLS_PRICING_PATH") {
            return Self::load_from_path(path);
        }
        let embedded = include_str!("../pricing.toml");
        Self::from_toml_str(embedded)
    }

    fn from_toml_str(content: &str) -> Result<Self, UsageError> {
        let cfg: PricingConfig = toml::from_str(content)?;
        Ok(cfg.into_catalog())
    }

    pub fn match_entry(&self, connector: &str, tool: &str, model: Option<&str>) -> PricingEntry {
        let name = format!("{}.{}", connector, tool);
        if let Some(model_name) = model {
            for entry in &self.entries {
                if wildcard_match(&entry.pattern, &name) {
                    if let Some(pattern) = &entry.model_pattern {
                        if wildcard_match(pattern, model_name) {
                            return entry.clone();
                        }
                    }
                }
            }
        }
        for entry in &self.entries {
            if wildcard_match(&entry.pattern, &name) && entry.model_pattern.is_none() {
                return entry.clone();
            }
        }
        PricingEntry {
            pattern: "*".to_string(),
            model_pattern: None,
            category: self.default_category.clone(),
            model: PricingModel::Unknown,
            currency: self.default_currency.clone(),
        }
    }
}

#[derive(Clone)]
pub struct UsageManager {
    pub store: std::sync::Arc<dyn UsageStore>,
    pub catalog: PricingCatalog,
}

impl std::fmt::Debug for UsageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageManager")
            .field("store", &"<dyn UsageStore>")
            .field("catalog", &self.catalog)
            .finish()
    }
}

impl UsageManager {
    pub fn new(store: std::sync::Arc<dyn UsageStore>, catalog: PricingCatalog) -> Self {
        Self { store, catalog }
    }

    pub fn new_default() -> Result<Self, UsageError> {
        let store = std::sync::Arc::new(FileUsageStore::new_default()?);
        let catalog = PricingCatalog::load_default()?;
        Ok(Self { store, catalog })
    }

    pub fn pricing_version(&self) -> &str {
        &self.catalog.version
    }

    #[allow(clippy::too_many_arguments)]
    pub fn estimate_event(
        &self,
        connector: &str,
        tool: &str,
        provider: &str,
        run_id: &str,
        request_id: &str,
        key_id: Option<String>,
        status: &str,
        duration_ms: u64,
        structured: Option<&Value>,
        model: Option<&str>,
    ) -> (UsageEvent, Value) {
        let entry = self.catalog.match_entry(connector, tool, model);
        let (units, mut estimated) = extract_units(structured);
        let mut cost_usd = None;
        let provider_cost = extract_cost_usd(structured);

        match entry.model {
            PricingModel::ProviderReported => {
                if let Some(cost) = provider_cost {
                    cost_usd = Some(cost);
                    estimated = false;
                }
            }
            PricingModel::PerRequest { unit_cost_usd } => {
                let count = units.requests.unwrap_or(1);
                cost_usd = Some(unit_cost_usd * count as f64);
                estimated = true;
            }
            PricingModel::PerResult { unit_cost_usd } => {
                let count = units.results.unwrap_or(0);
                if count > 0 {
                    cost_usd = Some(unit_cost_usd * count as f64);
                }
                estimated = true;
            }
            PricingModel::PerToken {
                input_cost_usd,
                output_cost_usd,
            } => {
                let input = units.input_tokens.unwrap_or(0) as f64;
                let output = units.output_tokens.unwrap_or(0) as f64;
                cost_usd = Some(input_cost_usd * input + output_cost_usd * output);
                estimated = true;
            }
            PricingModel::PerTokenPlusRequest {
                input_cost_usd,
                output_cost_usd,
                request_cost_usd,
            } => {
                let input = units.input_tokens.unwrap_or(0) as f64;
                let output = units.output_tokens.unwrap_or(0) as f64;
                let requests = units.requests.unwrap_or(1) as f64;
                cost_usd = Some(
                    input_cost_usd * input + output_cost_usd * output + request_cost_usd * requests,
                );
                estimated = true;
            }
            PricingModel::PerRequestPlusResults {
                base_cost_usd,
                included_results,
                per_result_usd,
            } => {
                let count = units.results.unwrap_or(included_results);
                let extra = count.saturating_sub(included_results) as f64;
                cost_usd = Some(base_cost_usd + per_result_usd * extra);
                estimated = true;
            }
            PricingModel::Unknown => {
                estimated = true;
            }
        }

        if let Some(cost) = provider_cost {
            cost_usd = Some(cost);
            estimated = false;
        }

        let timestamp = Utc::now().to_rfc3339();
        let event = UsageEvent {
            event_id: new_id("evt"),
            run_id: run_id.to_string(),
            request_id: request_id.to_string(),
            connector: connector.to_string(),
            tool: tool.to_string(),
            provider: provider.to_string(),
            key_id,
            category: entry.category.clone(),
            units: units.clone(),
            cost_usd,
            currency: entry.currency.clone(),
            estimated,
            pricing_version: self.catalog.version.clone(),
            status: status.to_string(),
            duration_ms,
            timestamp,
        };

        let meta = build_meta(&event, &units);
        (event, meta)
    }

    pub fn summarize_all(&self) -> Result<UsageSummary, UsageError> {
        let events = self.store.load_all()?;
        Ok(summarize_events(events.iter()))
    }

    pub fn summarize_run(&self, run_id: &str) -> Result<RunSummary, UsageError> {
        let events = self.store.load_all()?;
        let mut summary = RunSummary {
            run_id: run_id.to_string(),
            ..Default::default()
        };
        for event in events.iter().filter(|e| e.run_id == run_id) {
            apply_event_to_run(&mut summary, event);
        }
        Ok(summary)
    }
}

#[derive(Debug, Deserialize)]
struct PricingDefaults {
    currency: Option<String>,
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PricingEntryConfig {
    pattern: String,
    category: Option<String>,
    pricing_model: Option<String>,
    model: Option<String>,
    unit_cost_usd: Option<f64>,
    input_cost_usd: Option<f64>,
    output_cost_usd: Option<f64>,
    request_cost_usd: Option<f64>,
    base_cost_usd: Option<f64>,
    included_results: Option<u64>,
    per_result_usd: Option<f64>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PricingConfig {
    version: Option<String>,
    defaults: Option<PricingDefaults>,
    entries: Vec<PricingEntryConfig>,
}

impl PricingConfig {
    fn into_catalog(self) -> PricingCatalog {
        let default_currency = self
            .defaults
            .as_ref()
            .and_then(|d| d.currency.clone())
            .unwrap_or_else(|| "USD".to_string());
        let default_category = self
            .defaults
            .as_ref()
            .and_then(|d| d.category.as_ref())
            .and_then(|s| parse_category(s).ok())
            .unwrap_or(BillingCategory::AuthOnly);

        let mut entries = Vec::new();
        for entry in self.entries {
            let category = entry
                .category
                .as_deref()
                .and_then(|s| parse_category(s).ok())
                .unwrap_or(BillingCategory::AuthOnly);
            let currency = entry
                .currency
                .clone()
                .unwrap_or_else(|| default_currency.clone());
            let model = parse_model(&entry).unwrap_or(PricingModel::Unknown);
            entries.push(PricingEntry {
                pattern: entry.pattern,
                model_pattern: entry.model,
                category,
                model,
                currency,
            });
        }

        PricingCatalog {
            version: self
                .version
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
            entries,
            default_currency,
            default_category,
        }
    }
}

fn parse_category(input: &str) -> Result<BillingCategory, UsageError> {
    match input.to_lowercase().as_str() {
        "auth" | "auth_only" | "auth-only" => Ok(BillingCategory::AuthOnly),
        "metered" | "billable" => Ok(BillingCategory::Metered),
        other => Err(UsageError::Other(format!(
            "unknown billing category: {}",
            other
        ))),
    }
}

fn parse_model(entry: &PricingEntryConfig) -> Result<PricingModel, UsageError> {
    let model = entry
        .pricing_model
        .as_deref()
        .unwrap_or("unknown")
        .to_lowercase();
    Ok(match model.as_str() {
        "per_request" | "per-request" => PricingModel::PerRequest {
            unit_cost_usd: entry.unit_cost_usd.unwrap_or(0.0),
        },
        "per_result" | "per-result" => PricingModel::PerResult {
            unit_cost_usd: entry.unit_cost_usd.unwrap_or(0.0),
        },
        "per_token" | "per-token" => PricingModel::PerToken {
            input_cost_usd: entry.input_cost_usd.unwrap_or(0.0),
            output_cost_usd: entry.output_cost_usd.unwrap_or(0.0),
        },
        "per_token_plus_request" | "per-token-plus-request" => PricingModel::PerTokenPlusRequest {
            input_cost_usd: entry.input_cost_usd.unwrap_or(0.0),
            output_cost_usd: entry.output_cost_usd.unwrap_or(0.0),
            request_cost_usd: entry.request_cost_usd.unwrap_or(0.0),
        },
        "per_request_plus_results" | "per-request-plus-results" => {
            PricingModel::PerRequestPlusResults {
                base_cost_usd: entry.base_cost_usd.unwrap_or(0.0),
                included_results: entry.included_results.unwrap_or(0),
                per_result_usd: entry.per_result_usd.unwrap_or(0.0),
            }
        }
        "provider_reported" | "provider-reported" => PricingModel::ProviderReported,
        _ => PricingModel::Unknown,
    })
}

fn extract_units(structured: Option<&Value>) -> (UsageUnits, bool) {
    let mut units = UsageUnits {
        requests: Some(1),
        ..Default::default()
    };
    let mut estimated = true;

    if let Some(value) = structured {
        let usage = find_usage_object(value);
        if let Some(obj) = usage {
            let input = obj
                .get("input_tokens")
                .or_else(|| obj.get("prompt_tokens"))
                .or_else(|| obj.get("input"))
                .or_else(|| obj.get("prompt"))
                .and_then(|v| v.as_u64());
            let output = obj
                .get("output_tokens")
                .or_else(|| obj.get("completion_tokens"))
                .or_else(|| obj.get("output"))
                .or_else(|| obj.get("completion"))
                .and_then(|v| v.as_u64());
            let total = obj
                .get("total_tokens")
                .or_else(|| obj.get("tokens"))
                .and_then(|v| v.as_u64());

            if input.is_some() || output.is_some() {
                units.input_tokens = input;
                units.output_tokens = output;
                estimated = false;
            } else if let Some(total) = total {
                units.input_tokens = Some(total);
                units.output_tokens = Some(0);
                estimated = true;
            }
        }

        if let Some(results) = find_result_count(value) {
            units.results = Some(results);
        }
    }

    (units, estimated)
}

fn find_usage_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value
        .get("usage")
        .and_then(|v| v.as_object())
        .or_else(|| value.get("token_usage").and_then(|v| v.as_object()))
        .or_else(|| {
            value
                .get("raw")
                .and_then(|raw| raw.get("usage"))
                .and_then(|v| v.as_object())
        })
}

fn extract_cost_usd(structured: Option<&Value>) -> Option<f64> {
    let value = structured?;
    if let Some(usage) = value.get("usage") {
        if let Some(cost) = usage.get("cost") {
            if let Some(amount) = cost_value_to_f64(cost) {
                return Some(amount);
            }
        }
    }
    value.get("cost").and_then(cost_value_to_f64).or_else(|| {
        value
            .get("raw")
            .and_then(|raw| raw.get("cost"))
            .and_then(cost_value_to_f64)
            .or_else(|| {
                value
                    .get("raw")
                    .and_then(|raw| raw.get("usage"))
                    .and_then(|usage| usage.get("cost"))
                    .and_then(cost_value_to_f64)
            })
    })
}

fn cost_value_to_f64(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    if let Some(obj) = value.as_object() {
        for key in ["total", "amount", "usd", "total_cost", "cost"] {
            if let Some(n) = obj.get(key).and_then(|v| v.as_f64()) {
                return Some(n);
            }
        }
    }
    None
}

fn find_result_count(value: &Value) -> Option<u64> {
    const RESULT_KEYS: &[&str] = &[
        "results",
        "articles",
        "items",
        "entries",
        "documents",
        "records",
        "posts",
        "stories",
        "videos",
        "papers",
        "messages",
        "mailboxes",
        "conversations",
        "threads",
        "hits",
        "search_results",
        "content",
        "data",
    ];

    for key in RESULT_KEYS {
        if let Some(arr) = value.get(*key).and_then(|v| v.as_array()) {
            return Some(arr.len() as u64);
        }
    }
    None
}

fn build_meta(event: &UsageEvent, units: &UsageUnits) -> Value {
    let mut usage = serde_json::Map::new();
    if let Some(v) = units.requests {
        usage.insert("requests".to_string(), Value::from(v));
    }
    if let Some(v) = units.input_tokens {
        usage.insert("input_tokens".to_string(), Value::from(v));
    }
    if let Some(v) = units.output_tokens {
        usage.insert("output_tokens".to_string(), Value::from(v));
    }
    if let Some(v) = units.results {
        usage.insert("results".to_string(), Value::from(v));
    }

    let mut cost = serde_json::Map::new();
    if let Some(amount) = event.cost_usd {
        cost.insert("amount".to_string(), Value::from(amount));
    }
    cost.insert("currency".to_string(), Value::from(event.currency.clone()));
    cost.insert("estimated".to_string(), Value::from(event.estimated));
    cost.insert(
        "pricing_version".to_string(),
        Value::from(event.pricing_version.clone()),
    );

    let mut meta = serde_json::Map::new();
    meta.insert("run_id".to_string(), Value::from(event.run_id.clone()));
    meta.insert(
        "request_id".to_string(),
        Value::from(event.request_id.clone()),
    );
    meta.insert(
        "connector".to_string(),
        Value::from(event.connector.clone()),
    );
    meta.insert("tool".to_string(), Value::from(event.tool.clone()));
    meta.insert("provider".to_string(), Value::from(event.provider.clone()));
    meta.insert(
        "category".to_string(),
        Value::from(category_label(&event.category)),
    );
    meta.insert("usage".to_string(), Value::Object(usage));
    meta.insert("cost".to_string(), Value::Object(cost));
    Value::Object(meta)
}

fn category_label(category: &BillingCategory) -> &'static str {
    match category {
        BillingCategory::AuthOnly => "auth_only",
        BillingCategory::Metered => "metered",
    }
}

fn summarize_events<'a>(events: impl Iterator<Item = &'a UsageEvent>) -> UsageSummary {
    let mut summary = UsageSummary::default();
    for event in events {
        apply_event(&mut summary, event);
    }
    summary
}

fn apply_event(summary: &mut UsageSummary, event: &UsageEvent) {
    let cost = event.cost_usd.unwrap_or(0.0);
    summary.total_cost_usd += cost;
    summary.total_requests += event.units.requests.unwrap_or(0);
    summary.total_input_tokens += event.units.input_tokens.unwrap_or(0);
    summary.total_output_tokens += event.units.output_tokens.unwrap_or(0);
    summary.total_results += event.units.results.unwrap_or(0);

    let run_entry = summary
        .runs
        .entry(event.run_id.clone())
        .or_insert_with(|| RunSummary {
            run_id: event.run_id.clone(),
            ..Default::default()
        });
    apply_event_to_run(run_entry, event);
}

fn apply_event_to_run(summary: &mut RunSummary, event: &UsageEvent) {
    summary.total_cost_usd += event.cost_usd.unwrap_or(0.0);
    summary.total_requests += event.units.requests.unwrap_or(0);
    summary.total_input_tokens += event.units.input_tokens.unwrap_or(0);
    summary.total_output_tokens += event.units.output_tokens.unwrap_or(0);
    summary.total_results += event.units.results.unwrap_or(0);
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Simple glob matcher supporting '*' and '?'
    let (mut p_idx, mut t_idx, mut star_idx, mut match_idx) = (0, 0, None, 0);
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();

    while t_idx < t.len() {
        if p_idx < p.len() && (p[p_idx] == '?' || p[p_idx] == t[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p.len() && p[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p.len() && p[p_idx] == '*' {
        p_idx += 1;
    }
    p_idx == p.len()
}

fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let ts = Utc::now().timestamp_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{}-{}-{}-{}", prefix, ts, pid, seq)
}
