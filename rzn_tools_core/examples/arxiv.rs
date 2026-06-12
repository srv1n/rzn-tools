use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::arxiv::ArxivConnector;
use rzn_tools_core::Connector;
use rzn_tools_core::{CallToolRequestParam, PaginatedRequestParam};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the arXiv connector
    let auth_details = AuthDetails::new(); // arXiv doesn't require authentication
    let arxiv_connector = ArxivConnector::new(auth_details).await?;

    println!("Initialized arXiv connector: {}", arxiv_connector.name());

    // List available tools
    let tools_response = arxiv_connector
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?;

    println!("Available tools:");
    for tool in tools_response.tools {
        println!(
            "  - {}: {}",
            tool.name,
            tool.description.unwrap_or_default()
        );
    }

    // Search for papers about "quantum computing"
    println!("\nSearching for papers about 'quantum computing'...");
    let search_response = arxiv_connector
        .call_tool(CallToolRequestParam {
            name: "search".into(),
            arguments: Some(
                json!({
                    "query": "quantum computing",
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
    let papers_value = structured.get("data").cloned().unwrap_or(structured);
    let papers: Vec<Value> = papers_value.as_array().cloned().unwrap_or_default();

    println!("\nFound {} papers:", papers.len());
    for (i, paper) in papers.iter().enumerate() {
        let title = paper.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let id = paper.get("id").and_then(|v| v.as_str()).unwrap_or("");
        println!("{}. {}", i + 1, title);
        println!("   ID: {}", id);
    }

    // Get details for the first paper
    if let Some(first) = papers.first() {
        let first_paper_id = first.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if !first_paper_id.is_empty() {
            println!("\nGetting details for paper ID: {}", first_paper_id);
            let detail_response = arxiv_connector
                .call_tool(CallToolRequestParam {
                    name: "get".into(),
                    arguments: Some(json!({ "id": first_paper_id }).as_object().unwrap().clone()),
                })
                .await?;

            let detail = detail_response
                .structured_content
                .unwrap_or_else(|| json!({}));
            println!("{}", serde_json::to_string_pretty(&detail)?);
        }
    }

    Ok(())
}
