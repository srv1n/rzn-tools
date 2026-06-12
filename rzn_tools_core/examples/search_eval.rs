use rzn_tools_core::{build_registry_enabled_only, CallToolRequestParam, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = build_registry_enabled_only().await;
    let connectors = registry.list_providers();
    let selected: Option<Vec<String>> = std::env::var("EVAL_CONNECTORS")
        .ok()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect());

    let scenarios = vec![
        json!({
            "name": "general_update",
            "args": {"query": "Latest developments in LLM safety 2025", "max_results": 3, "response_format": "concise"}
        }),
        json!({
            "name": "news_window",
            "args": {"query": "Company earnings Apple Q4 2025", "since": "2025-09-01", "until": "2025-12-31", "max_results": 3, "response_format": "concise"}
        }),
    ];

    let mut results = vec![];
    for info in connectors {
        let name = info.name;
        if let Some(sel) = &selected {
            if !sel.contains(&name) {
                continue;
            }
        }
        if let Some(provider) = registry.get_provider(&name) {
            let c = provider.lock().await;
            let tools = c
                .list_tools(Some(PaginatedRequestParam { cursor: None }))
                .await?
                .tools;
            let has_search = tools.iter().any(|t| t.name.as_ref() == "search");
            if !has_search {
                continue;
            }
            for sc in &scenarios {
                let args = sc.get("args").unwrap().as_object().unwrap().clone();
                // Run concise and detailed for latency/size comparison
                let run_variant = |fmt: &str| -> Result<
                    (serde_json::Value, u128, usize),
                    Box<dyn std::error::Error>,
                > {
                    let mut a = args.clone();
                    a.insert("response_format".to_string(), json!(fmt));
                    let req = CallToolRequestParam {
                        name: "search".into(),
                        arguments: Some(a),
                    };
                    let t0 = std::time::Instant::now();
                    let resp = futures::executor::block_on(c.call_tool(req))?;
                    let ms = t0.elapsed().as_millis();
                    let structured = resp.structured_content.unwrap_or_else(|| json!({}));
                    let size = serde_json::to_vec(&structured)?.len();
                    Ok((structured, ms, size))
                };

                let (concise_json, ms_concise, bytes_concise) = run_variant("concise")?;
                let (_detailed_json, ms_detailed, bytes_detailed) = run_variant("detailed")?;

                let concise_answer = concise_json
                    .get("answer")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let citations_count = concise_json
                    .get("citations")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let results_count = concise_json
                    .get("results")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);

                results.push(json!({
                    "connector": name,
                    "scenario": sc.get("name").unwrap(),
                    "has_answer": !concise_answer.trim().is_empty(),
                    "citations_count": citations_count,
                    "results_count": results_count,
                    "ms_concise": ms_concise,
                    "bytes_concise": bytes_concise,
                    "ms_detailed": ms_detailed,
                    "bytes_detailed": bytes_detailed
                }));
            }
        }
    }

    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}
