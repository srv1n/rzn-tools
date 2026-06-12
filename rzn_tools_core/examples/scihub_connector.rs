use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::scihub::SciHubConnector;
use rzn_tools_core::{CallToolRequestParam, Connector, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = SciHubConnector::new(AuthDetails::new()).await?;
    connector.test_auth().await?;

    let tools = connector
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?
        .tools;
    println!("Tools:");
    for t in tools {
        println!("  - {}: {}", t.name, t.description.unwrap_or_default());
    }

    let response = connector
        .call_tool(CallToolRequestParam {
            name: "get".into(),
            arguments: Some(
                json!({
                    "doi": "10.1371/journal.pone.0000308"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        })
        .await?;

    let structured = response.structured_content.unwrap_or_else(|| json!({}));
    println!("{}", serde_json::to_string_pretty(&structured)?);

    Ok(())
}
