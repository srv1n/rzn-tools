use crate::cli::Cli;
use crate::commands::{CommandError, Result};
use crate::output::{format_output, OutputData};
use rzn_tools_core::{UsageEvent, UsageManager};
use serde_json::{json, Map, Value};

pub async fn run(
    cli: &Cli,
    connector: Option<&str>,
    tool: Option<&str>,
    run: Option<&str>,
    last: bool,
) -> Result<()> {
    let usage = UsageManager::new_default()
        .map_err(|e| CommandError::Other(format!("usage store error: {}", e)))?;
    let events = usage
        .store
        .load_all()
        .map_err(|e| CommandError::Other(format!("usage load error: {}", e)))?;

    let run_filter = if last {
        find_last_run(&events)
    } else {
        run.map(|s| s.to_string())
    };

    let filtered: Vec<&UsageEvent> = events
        .iter()
        .filter(|event| {
            if let Some(run_id) = run_filter.as_deref() {
                if event.run_id != run_id {
                    return false;
                }
            }
            if let Some(conn) = connector {
                if event.connector != conn {
                    return false;
                }
            }
            if let Some(t) = tool {
                if event.tool != t {
                    return false;
                }
            }
            true
        })
        .collect();

    let report = build_report(&filtered, connector, tool, run_filter.as_deref());

    let data = OutputData::UsageReport { report };
    format_output(&data, &cli.output)?;
    Ok(())
}

fn build_report(
    events: &[&UsageEvent],
    connector: Option<&str>,
    tool: Option<&str>,
    run_id: Option<&str>,
) -> Value {
    let mut totals = Totals::default();
    let mut start_ts: Option<&str> = None;
    let mut end_ts: Option<&str> = None;

    for event in events {
        totals.events += 1;
        totals.total_cost_usd += event.cost_usd.unwrap_or(0.0);
        totals.total_requests += event.units.requests.unwrap_or(0);
        totals.total_results += event.units.results.unwrap_or(0);
        totals.total_input_tokens += event.units.input_tokens.unwrap_or(0);
        totals.total_output_tokens += event.units.output_tokens.unwrap_or(0);
        if event.estimated {
            totals.estimated_events += 1;
        }

        let ts = event.timestamp.as_str();
        start_ts = match start_ts {
            None => Some(ts),
            Some(prev) if ts < prev => Some(ts),
            Some(prev) => Some(prev),
        };
        end_ts = match end_ts {
            None => Some(ts),
            Some(prev) if ts > prev => Some(ts),
            Some(prev) => Some(prev),
        };
    }

    let mut filters = Map::new();
    if let Some(conn) = connector {
        filters.insert("connector".to_string(), Value::from(conn));
    }
    if let Some(t) = tool {
        filters.insert("tool".to_string(), Value::from(t));
    }
    if let Some(run) = run_id {
        filters.insert("run_id".to_string(), Value::from(run));
    }

    let mut report = Map::new();
    report.insert("filters".to_string(), Value::Object(filters));
    report.insert(
        "totals".to_string(),
        json!({
            "events": totals.events,
            "estimated_events": totals.estimated_events,
            "total_cost_usd": totals.total_cost_usd,
            "total_requests": totals.total_requests,
            "total_results": totals.total_results,
            "total_input_tokens": totals.total_input_tokens,
            "total_output_tokens": totals.total_output_tokens,
        }),
    );
    if let (Some(start), Some(end)) = (start_ts, end_ts) {
        report.insert(
            "range".to_string(),
            json!({
                "start": start,
                "end": end,
            }),
        );
    }
    Value::Object(report)
}

fn find_last_run(events: &[UsageEvent]) -> Option<String> {
    events
        .iter()
        .max_by(|a, b| a.timestamp.cmp(&b.timestamp))
        .map(|event| event.run_id.clone())
}

#[derive(Default)]
struct Totals {
    total_cost_usd: f64,
    total_requests: u64,
    total_results: u64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    events: usize,
    estimated_events: usize,
}
