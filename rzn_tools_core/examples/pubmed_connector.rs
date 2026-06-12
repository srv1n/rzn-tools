use rzn_tools_core::connectors::pubmed::PubMedConnector;
use rzn_tools_core::{CallToolRequestParam, Connector, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = PubMedConnector::new().await?;

    let prompts = connector
        .list_prompts(Some(PaginatedRequestParam { cursor: None }))
        .await?
        .prompts;
    println!("Prompts:");
    for p in prompts {
        println!("  - {}: {}", p.name, p.description.unwrap_or_default());
    }

    let response = connector
        .call_tool(CallToolRequestParam {
            name: "search".into(),
            arguments: Some(
                json!({
                    "query": "CRISPR gene therapy",
                    "limit": 3
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
