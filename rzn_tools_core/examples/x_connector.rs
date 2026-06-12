use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::x::XApiConnector;
use rzn_tools_core::{CallToolRequestParam, Connector, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // NOTE: The `x` connector uses the official X API v2.
    // This example uses bearer auth for public reads. For user-context reads/writes,
    // import OAuth2 or OAuth1 credentials instead.
    let mut auth = AuthDetails::new();
    let token = std::env::var("X_BEARER_TOKEN")
        .or_else(|_| std::env::var("TWITTER_BEARER_TOKEN"))
        .expect("set X_BEARER_TOKEN or TWITTER_BEARER_TOKEN");
    auth.insert("bearer_token".to_string(), token);

    let connector = XApiConnector::new(auth).await?;
    let tools = connector
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?
        .tools;
    println!("Tools:");
    for t in tools {
        println!("  - {}: {}", t.name, t.description.unwrap_or_default());
    }

    let resp = connector
        .call_tool(CallToolRequestParam {
            name: "search_recent_tweets".into(),
            arguments: Some(
                json!({
                    "query": "rust lang:en",
                    "limit": 5,
                    "since": "24h"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        })
        .await?;

    let structured = resp.structured_content.unwrap_or_else(|| json!({}));
    println!("{}", serde_json::to_string_pretty(&structured)?);

    Ok(())
}
