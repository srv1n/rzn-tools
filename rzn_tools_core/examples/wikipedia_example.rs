use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::wikipedia::WikipediaConnector;
use rzn_tools_core::{
    CallToolRequestParam, Connector, PaginatedRequestParam, ReadResourceRequestParam,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_details = AuthDetails::new();
    auth_details.insert("language".to_string(), "en".to_string());
    let connector = WikipediaConnector::new(auth_details).await?;

    connector.test_auth().await?;

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
                    "query": "Rust programming language",
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

    let resource = connector
        .read_resource(ReadResourceRequestParam {
            uri: "wikipedia://article/Rust (programming language)".parse()?,
        })
        .await?;
    println!("Resource:\n{:#?}", resource);

    Ok(())
}
