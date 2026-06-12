use crate::cli::Cli;
use crate::commands::{CommandError, Result};
use crate::output::{format_output, OutputData};
use rzn_tools_core::usage::{BillingCategory, PricingEntry, PricingModel};
use rzn_tools_core::PricingCatalog;
use serde_json::{json, Map, Value};

pub async fn run(
    cli: &Cli,
    connector: Option<&str>,
    tool: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    let catalog = PricingCatalog::load_default()
        .map_err(|e| CommandError::Other(format!("pricing catalog error: {}", e)))?;

    let mut rows: Vec<Value> = catalog
        .entries
        .iter()
        .filter(|entry| entry_matches(entry, connector, tool, model))
        .map(pricing_entry_to_value)
        .collect();

    rows.sort_by(|a, b| {
        let ap = a.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let bp = b.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        ap.cmp(bp)
    });

    let report = json!({
        "version": catalog.version,
        "filters": {
            "connector": connector,
            "tool": tool,
            "model": model,
        },
        "entries": rows,
    });

    let data = OutputData::PricingInfo { report };
    format_output(&data, &cli.output)?;
    Ok(())
}

fn pricing_entry_to_value(entry: &PricingEntry) -> Value {
    let mut obj = Map::new();
    obj.insert("pattern".to_string(), Value::from(entry.pattern.clone()));
    if let Some(model_pattern) = &entry.model_pattern {
        obj.insert(
            "model_pattern".to_string(),
            Value::from(model_pattern.clone()),
        );
    }
    obj.insert(
        "category".to_string(),
        Value::from(category_label(&entry.category)),
    );
    obj.insert("currency".to_string(), Value::from(entry.currency.clone()));
    obj.insert("pricing".to_string(), pricing_model_to_value(&entry.model));
    Value::Object(obj)
}

fn pricing_model_to_value(model: &PricingModel) -> Value {
    match model {
        PricingModel::PerRequest { unit_cost_usd } => json!({
            "type": "per_request",
            "unit_cost_usd": unit_cost_usd,
        }),
        PricingModel::PerResult { unit_cost_usd } => json!({
            "type": "per_result",
            "unit_cost_usd": unit_cost_usd,
        }),
        PricingModel::PerToken {
            input_cost_usd,
            output_cost_usd,
        } => json!({
            "type": "per_token",
            "input_cost_usd": input_cost_usd,
            "output_cost_usd": output_cost_usd,
        }),
        PricingModel::PerTokenPlusRequest {
            input_cost_usd,
            output_cost_usd,
            request_cost_usd,
        } => json!({
            "type": "per_token_plus_request",
            "input_cost_usd": input_cost_usd,
            "output_cost_usd": output_cost_usd,
            "request_cost_usd": request_cost_usd,
        }),
        PricingModel::PerRequestPlusResults {
            base_cost_usd,
            included_results,
            per_result_usd,
        } => json!({
            "type": "per_request_plus_results",
            "base_cost_usd": base_cost_usd,
            "included_results": included_results,
            "per_result_usd": per_result_usd,
        }),
        PricingModel::ProviderReported => json!({
            "type": "provider_reported",
        }),
        PricingModel::Unknown => json!({
            "type": "unknown",
        }),
    }
}

fn category_label(category: &BillingCategory) -> &'static str {
    match category {
        BillingCategory::AuthOnly => "auth_only",
        BillingCategory::Metered => "metered",
    }
}

fn entry_matches(
    entry: &PricingEntry,
    connector: Option<&str>,
    tool: Option<&str>,
    model: Option<&str>,
) -> bool {
    let (pattern_connector, pattern_tool) = split_pattern(&entry.pattern);

    if let Some(conn) = connector {
        if pattern_connector == "*" {
            return false;
        }
        if !wildcard_match(pattern_connector, conn) {
            return false;
        }
    }

    if let Some(t) = tool {
        if !wildcard_match(pattern_tool, t) {
            return false;
        }
    }

    if let Some(m) = model {
        match entry.model_pattern.as_deref() {
            Some(pat) if wildcard_match(pat, m) => {}
            _ => return false,
        }
    }

    true
}

fn split_pattern(pattern: &str) -> (&str, &str) {
    match pattern.split_once('.') {
        Some((conn, tool)) => (conn, tool),
        None => (pattern, "*"),
    }
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
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
