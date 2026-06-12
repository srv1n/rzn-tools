use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::semantic_scholar::SemanticScholarConnector;
use rzn_tools_core::CallToolRequestParam;
use rzn_tools_core::Connector;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = "machine learning";
    let page_size = 1;
    println!("Searching Semantic Scholar for: '{}'", query);
    // Create a Semantic Scholar connector (no auth required)
    let auth_details = AuthDetails::new();
    let semantic_scholar_connector = SemanticScholarConnector::new(auth_details).await?;

    println!("Creating request");
    // Create a request to search for papers
    let request = CallToolRequestParam {
        name: "search".into(),
        arguments: Some(
            json!({
                "query": query,
                "limit": page_size,
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
    };

    println!("Calling tool");
    // Call the search_papers tool
    let response = semantic_scholar_connector.call_tool(request).await?;
    let structured = response.structured_content.unwrap_or_else(|| json!({}));
    println!("{}", serde_json::to_string_pretty(&structured)?);
    // let papers: Vec<Paper> = serde_json::from_str(response.content.first().unwrap().text.as_str())?;

    // // Process and print the results
    // if let Some(ToolResponseContent::Text { text }) = response.content.first() {
    //     let papers: Vec<serde_json::Value> = serde_json::from_str(text)?;

    //     println!("Found {} papers about '{}':\n", papers.len(), query);

    //     for (i, paper) in papers.iter().enumerate() {
    //         println!("{}. {}", i + 1, paper["title"]);
    //         println!("   Authors: {}", paper["authors"].as_array().map_or("Unknown", |a| {
    //             if a.is_empty() { "Unknown" } else { a[0].as_str().unwrap_or("Unknown") }
    //         }));
    //         println!("   URL: {}", paper["url"]);

    //         if let Some(content) = paper["content"].as_str() {
    //             if !content.is_empty() {
    //                 let summary = if content.len() > 200 {
    //                     format!("{}...", &content[0..200])
    //                 } else {
    //                     content.to_string()
    //                 };
    //                 println!("   Abstract: {:#?}", summary);
    //             }
    //         }

    //         if let Some(journal) = paper["journal"].as_str() {
    //             if !journal.is_empty() {
    //                 println!("   Journal: {:#?}", journal);
    //             }
    //         }

    //         if let Some(comments) = paper["comments"].as_str() {
    //             println!("   Citations: {:#?}", comments);
    //         }

    //         println!("--------------------------------\n");
    //     }
    // } else {
    //     println!("No results found or error in response");
    // }

    Ok(())
}
