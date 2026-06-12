use rzn_tools_core::connectors::youtube::YouTubeConnector;
use rzn_tools_core::{CallToolRequestParam, Connector, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // YouTube connector does not require an API key (uses public endpoints).
    let connector = YouTubeConnector::new(None).await?;

    let tools = connector
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?
        .tools;
    println!("Tools:");
    for t in tools {
        println!("  - {}: {}", t.name, t.description.unwrap_or_default());
    }

    let search_response = connector
        .call_tool(CallToolRequestParam {
            name: "search".into(),
            arguments: Some(
                json!({
                    "query": "rust programming",
                    "limit": 3,
                    "type": "video"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        })
        .await?;

    let structured = search_response
        .structured_content
        .unwrap_or_else(|| json!({}));
    println!(
        "Search response:\n{}",
        serde_json::to_string_pretty(&structured)?
    );

    // Fetch details for the first result (if present)
    let first_id = structured
        .get("results")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|r0| r0.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(id) = first_id {
        let get_response = connector
            .call_tool(CallToolRequestParam {
                name: "get".into(),
                arguments: Some(json!({ "id": id }).as_object().unwrap().clone()),
            })
            .await?;
        let get_structured = get_response.structured_content.unwrap_or_else(|| json!({}));
        println!(
            "First video:\n{}",
            serde_json::to_string_pretty(&get_structured)?
        );
    }

    Ok(())
}
