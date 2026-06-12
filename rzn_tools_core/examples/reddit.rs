use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::reddit::RedditConnector;
use rzn_tools_core::CallToolRequestParam;
use rzn_tools_core::Connector;
use serde_json::{json, Value};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Default search parameters
    let mut query = "sampler tone map";
    let mut limit = 5;
    let mut advanced_params: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // Process command line arguments
    if args.len() > 1 {
        // First argument is the query
        query = &args[1];

        // Process additional arguments in the format key=value
        for arg in args.iter().skip(2) {
            if let Some((key, value)) = arg.split_once('=') {
                match key {
                    "limit" => {
                        if let Ok(val) = value.parse::<i64>() {
                            limit = val as usize;
                        }
                    }
                    "author" | "subreddit" | "flair" | "title" | "selftext" | "site" | "url" => {
                        advanced_params.insert(key.to_string(), json!(value));
                    }
                    "self" => {
                        if let Ok(val) = value.parse::<bool>() {
                            advanced_params.insert(key.to_string(), json!(val));
                        }
                    }
                    "include_nsfw" => {
                        if let Ok(val) = value.parse::<bool>() {
                            advanced_params.insert(key.to_string(), json!(val));
                        }
                    }
                    _ => println!("Unknown parameter: {}", key),
                }
            }
        }
    }

    println!("🔍 Searching Reddit for: '{}'", query);
    if !advanced_params.is_empty() {
        println!("With advanced filters:");
        for (key, value) in &advanced_params {
            println!("  - {}: {}", key, value);
        }
    }

    // Create an anonymous Reddit connector
    let auth_details = AuthDetails::new();
    let reddit_connector = RedditConnector::new(auth_details).await?;

    // Step 1: Search Reddit for the query with advanced parameters
    let mut search_args = json!({ "query": query, "limit": limit });

    // Add advanced parameters to the search request
    if let Some(obj) = search_args.as_object_mut() {
        for (key, value) in advanced_params.clone() {
            obj.insert(key, value);
        }
    }

    // Call the search_reddit tool
    let search_response = reddit_connector
        .call_tool(CallToolRequestParam {
            name: "search".into(),
            arguments: Some(search_args.as_object().unwrap().clone()),
        })
        .await?;

    let mut search_results = Vec::new();
    let structured = search_response
        .structured_content
        .unwrap_or_else(|| json!({}));
    let results_value = structured.get("data").cloned().unwrap_or(structured);
    let results_array = results_value.as_array().cloned().unwrap_or_default();

    println!("Found {} results", results_array.len());
    for result in results_array {
        if let (Some(url), Some(title)) = (
            result.get("url").and_then(|v| v.as_str()),
            result.get("title").and_then(|v| v.as_str()),
        ) {
            search_results.push((url.to_string(), title.to_string()));
        }
    }

    // Step 2: For each search result, get post details
    for (i, (url, title)) in search_results.iter().enumerate() {
        println!(
            "\n📄 Processing result {}/{}: {}",
            i + 1,
            search_results.len(),
            title
        );

        // Call the get_post_details tool
        match reddit_connector
            .call_tool(CallToolRequestParam {
                name: "get".into(),
                arguments: Some(
                    json!({
                        "post_url": url,
                        "comment_limit": 10,
                        "comment_sort": "best"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            })
            .await
        {
            Ok(post_response) => {
                let post_data = post_response
                    .structured_content
                    .unwrap_or_else(|| json!({}));
                print_post_details(&post_data);
                print_comments(&post_data);
            }
            Err(e) => {
                println!("Error fetching post details: {}", e);
                continue;
            }
        }

        // Add a separator between posts
        println!("\n{}", "=".repeat(80));

        // Optional: Add a small delay between requests to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Print usage information
    println!("\nUsage:");
    println!("  cargo run --example reddit [QUERY] [PARAMETERS]");
    println!("\nParameters (optional):");
    println!("  limit=NUMBER           - Maximum number of results to return");
    println!("  author=USERNAME        - Filter by post author");
    println!("  subreddit=NAME         - Filter by subreddit");
    println!("  flair=TEXT             - Filter by post flair");
    println!("  title=TEXT             - Search within post titles only");
    println!("  selftext=TEXT          - Search within post body text only");
    println!("  site=DOMAIN            - Filter by domain of submitted URL");
    println!("  url=TEXT               - Filter by URL content");
    println!(
        "  self=BOOLEAN           - Filter to text posts only (true) or link posts only (false)"
    );
    println!("  include_nsfw=BOOLEAN   - Include NSFW results in search");
    println!("\nExample:");
    println!("  cargo run --example reddit \"rust programming\" subreddit=rust limit=3");

    Ok(())
}

// Helper function to print post details
fn print_post_details(post_data: &Value) {
    if let Some(post) = post_data.get("post") {
        // Print title
        if let Some(title) = post.get("title").and_then(|v| v.as_str()) {
            println!("Title: {}", title);
        }

        // Print author and subreddit
        let author = post
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let subreddit = post
            .get("subreddit")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        println!("Posted by u/{} in r/{}", author, subreddit);

        // Print score and other metadata
        let score = post.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let comments = post
            .get("num_comments")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        println!("Score: {} | Comments: {}", score, comments);

        // Print post content
        if let Some(selftext) = post.get("selftext").and_then(|v| v.as_str()) {
            if !selftext.is_empty() {
                println!("\nPost Content:");
                // Truncate very long posts
                let content = if selftext.len() > 1000 {
                    format!(
                        "{}...\n[Content truncated, too long to display fully]",
                        &selftext[0..1000]
                    )
                } else {
                    selftext.to_string()
                };
                println!("{}", content);
            }
        }

        // Print URL if it's a link post
        if let Some(url) = post.get("url").and_then(|v| v.as_str()) {
            if let Some(is_self) = post.get("is_self").and_then(|v| v.as_bool()) {
                if !is_self {
                    println!("Link: {}", url);
                }
            }
        }
    }
}

// Helper function to print comments recursively
fn print_comments(post_data: &Value) {
    if let Some(comments) = post_data.get("comments").and_then(|v| v.as_array()) {
        if !comments.is_empty() {
            println!("\nTop Comments:");
            for comment in comments {
                print_comment(comment, 0);
            }
        } else {
            println!("\nNo comments found.");
        }
    }
}

// Helper function to print a single comment with proper indentation
fn print_comment(comment: &Value, depth: usize) {
    let indent = "  ".repeat(depth);

    // Get comment author
    let author = comment
        .get("author")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let is_op = comment
        .get("is_submitter")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Format author name (highlight OP)
    let author_display = if is_op {
        format!("u/{} [OP]", author)
    } else {
        format!("u/{}", author)
    };

    // Get score
    let score = comment.get("score").and_then(|v| v.as_i64()).unwrap_or(0);

    // Print comment header
    println!("\n{}{}  ({})", indent, author_display, score);

    // Print comment body
    if let Some(body) = comment.get("body").and_then(|v| v.as_str()) {
        // Split by lines and add indentation
        for line in body.lines() {
            println!("{}│ {}", indent, line);
        }
    }

    // Recursively print replies
    if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
        for reply in replies {
            print_comment(reply, depth + 1);
        }
    }
}
