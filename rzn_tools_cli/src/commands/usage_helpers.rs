use crate::cli::OutputFormat;
use rzn_tools_core::usage_context::current_context;
use rzn_tools_core::UsageManager;
use serde_json::Value;

#[derive(Debug, Default)]
struct RunTotals {
    total_cost_usd: f64,
    total_requests: u64,
    total_results: u64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    events: usize,
    estimated_events: usize,
}

#[derive(Debug)]
struct CostInfo {
    amount: Option<f64>,
    estimated: Option<bool>,
    pricing_version: Option<String>,
    category: Option<String>,
    run_id: Option<String>,
}

pub fn print_cost_summary(output: &OutputFormat, meta: Option<&Value>) {
    if !should_print_cost(output) {
        return;
    }

    let cost_info = meta.and_then(parse_cost_info);
    let run_id = cost_info
        .as_ref()
        .and_then(|info| info.run_id.clone())
        .or_else(|| current_context().map(|ctx| ctx.run_id));
    let run_totals = run_id.as_deref().and_then(load_run_totals);

    if cost_info.is_none() && run_totals.is_none() {
        return;
    }

    println!();
    if let Some(info) = cost_info {
        println!("{}", format_cost_line(&info));
    }
    if let Some(totals) = run_totals {
        println!("{}", format_run_total_line(&totals));
    }
}

fn should_print_cost(output: &OutputFormat) -> bool {
    matches!(
        output,
        OutputFormat::Pretty | OutputFormat::Text | OutputFormat::Markdown
    )
}

fn parse_cost_info(meta: &Value) -> Option<CostInfo> {
    let obj = meta.as_object()?;
    let cost_obj = obj.get("cost").and_then(|v| v.as_object());
    let amount = cost_obj.and_then(|c| c.get("amount").and_then(|v| v.as_f64()));
    let estimated = cost_obj.and_then(|c| c.get("estimated").and_then(|v| v.as_bool()));
    let pricing_version = cost_obj
        .and_then(|c| c.get("pricing_version").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let category = obj
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let run_id = obj
        .get("run_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(CostInfo {
        amount,
        estimated,
        pricing_version,
        category,
        run_id,
    })
}

fn format_cost_line(info: &CostInfo) -> String {
    let amount_str = match info.amount {
        Some(amount) => format_usd(amount),
        None => match info.category.as_deref() {
            Some("auth_only") => "$0.00".to_string(),
            _ => "n/a".to_string(),
        },
    };

    let mut suffixes: Vec<String> = Vec::new();
    if info.amount.is_some() {
        if let Some(estimated) = info.estimated {
            suffixes.push(if estimated {
                "estimated".to_string()
            } else {
                "reported".to_string()
            });
        }
    }
    if let Some(category) = info.category.as_deref() {
        if category != "metered" {
            suffixes.push(category.to_string());
        }
    }
    if info.amount.is_some() {
        if let Some(pricing) = info.pricing_version.as_deref() {
            suffixes.push(format!("pricing {}", pricing));
        }
    }

    let suffix = if suffixes.is_empty() {
        String::new()
    } else {
        format!(" ({})", suffixes.join(", "))
    };
    format!("Cost: {}{}", amount_str, suffix)
}

fn format_run_total_line(totals: &RunTotals) -> String {
    let mut line = format!("Run total: {}", format_usd(totals.total_cost_usd));
    if totals.estimated_events > 0 {
        line.push_str(" (estimated)");
    }

    let mut details = Vec::new();
    if totals.total_requests > 0 {
        details.push(format!("{} requests", totals.total_requests));
    }
    if totals.total_results > 0 {
        details.push(format!("{} results", totals.total_results));
    }
    if totals.total_input_tokens > 0 || totals.total_output_tokens > 0 {
        details.push(format!(
            "{} input, {} output tokens",
            totals.total_input_tokens, totals.total_output_tokens
        ));
    }

    if !details.is_empty() {
        line.push_str(&format!(" — {}", details.join(", ")));
    }

    line
}

fn format_usd(amount: f64) -> String {
    if amount.abs() >= 0.01 {
        format!("${:.2}", amount)
    } else {
        format!("${:.4}", amount)
    }
}

fn load_run_totals(run_id: &str) -> Option<RunTotals> {
    let usage = UsageManager::new_default().ok()?;
    let events = usage.store.load_all().ok()?;
    let mut totals = RunTotals::default();
    let mut found = false;

    for event in events.iter().filter(|e| e.run_id == run_id) {
        found = true;
        totals.events += 1;
        totals.total_cost_usd += event.cost_usd.unwrap_or(0.0);
        totals.total_requests += event.units.requests.unwrap_or(0);
        totals.total_results += event.units.results.unwrap_or(0);
        totals.total_input_tokens += event.units.input_tokens.unwrap_or(0);
        totals.total_output_tokens += event.units.output_tokens.unwrap_or(0);
        if event.estimated {
            totals.estimated_events += 1;
        }
    }

    if found {
        Some(totals)
    } else {
        None
    }
}
