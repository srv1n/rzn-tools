use rzn_tools_core::connectors::hackernews::HackerNewsConnector;
use rzn_tools_core::{CallToolRequestParam, Connector};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = HackerNewsConnector::new();
    connector.test_auth().await?;

    let search_response = connector
        .call_tool(CallToolRequestParam {
            name: "search".into(),
            arguments: Some(
                json!({
                    "query": "rust cli",
                    "limit": 5
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

    // Try to fetch the first result (if present)
    let first_id = structured
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(|v| v.as_i64());

    if let Some(id) = first_id {
        let post_response = connector
            .call_tool(CallToolRequestParam {
                name: "get_thread".into(),
                arguments: Some(
                    json!({ "id": id, "max_comments": 10, "response_format": "compact" })
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            })
            .await?;
        let post_structured = post_response
            .structured_content
            .unwrap_or_else(|| json!({}));
        println!(
            "First post:\n{}",
            serde_json::to_string_pretty(&post_structured)?
        );
    }

    Ok(())
}
