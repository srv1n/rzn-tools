use rzn_tools_core::connectors::pubmed::PubMedConnector;
use rzn_tools_core::{CallToolRequestParam, Connector, PaginatedRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = PubMedConnector::new().await?;
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
                    "query": "valerian root sleep",
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

    let first_pmid = structured
        .get("articles")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|a0| a0.get("pmid"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(pmid) = first_pmid {
        let get_response = connector
            .call_tool(CallToolRequestParam {
                name: "get".into(),
                arguments: Some(json!({ "pmid": pmid }).as_object().unwrap().clone()),
            })
            .await?;
        let get_structured = get_response.structured_content.unwrap_or_else(|| json!({}));
        println!(
            "First article:\n{}",
            serde_json::to_string_pretty(&get_structured)?
        );
    }

    Ok(())
}
