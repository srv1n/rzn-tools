use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Example MCP client that demonstrates how to interact with the MCP server
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting MCP client example...");

    // Start the MCP server as a subprocess
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "mcp_server"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");

    let mut writer = tokio::io::BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);

    // Helper function to send request and read response
    async fn send_request(
        writer: &mut tokio::io::BufWriter<tokio::process::ChildStdin>,
        reader: &mut BufReader<tokio::process::ChildStdout>,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        // Send request
        let request_str = serde_json::to_string(&request)?;
        println!("\nSending: {}", request_str);
        writer.write_all(request_str.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;
        println!("Received: {}", response_line);

        Ok(serde_json::from_str(&response_line)?)
    }

    // 1. Initialize the server
    let init_request = json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "protocolVersion": "0.1.0",
            "capabilities": {},
            "clientInfo": {
                "name": "example_client",
                "version": "0.1.0"
            }
        },
        "id": 1
    });

    let _init_response = send_request(&mut writer, &mut reader, init_request).await?;

    // 2. List available tools
    let list_tools_request = json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "params": {},
        "id": 2
    });

    let tools_response = send_request(&mut writer, &mut reader, list_tools_request).await?;

    if let Some(result) = tools_response.get("result") {
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            println!("\nAvailable tools:");
            for tool in tools {
                if let Some(name) = tool.get("name").and_then(|n| n.as_str()) {
                    if let Some(desc) = tool.get("description").and_then(|d| d.as_str()) {
                        println!("  - {}: {}", name, desc);
                    }
                }
            }
        }
    }

    // 3. Call a tool - let's search HackerNews
    let search_hn_request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "hackernews/search_stories",
            "arguments": {
                "query": "rust",
                "hitsPerPage": 3
            }
        },
        "id": 3
    });

    let search_response = send_request(&mut writer, &mut reader, search_hn_request).await?;

    if let Some(result) = search_response.get("result") {
        println!("\nSearch results:");
        println!("{}", serde_json::to_string_pretty(result)?);
    }

    // 4. List resources
    let list_resources_request = json!({
        "jsonrpc": "2.0",
        "method": "resources/list",
        "params": {},
        "id": 4
    });

    let resources_response = send_request(&mut writer, &mut reader, list_resources_request).await?;

    if let Some(result) = resources_response.get("result") {
        if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
            println!("\nAvailable resources:");
            for resource in resources {
                if let Some(uri) = resource.get("uri").and_then(|u| u.as_str()) {
                    if let Some(name) = resource.get("name").and_then(|n| n.as_str()) {
                        println!("  - {}: {}", name, uri);
                    }
                }
            }
        }
    }

    // 5. Get Wikipedia article
    let wiki_tool_request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "wikipedia/get_article",
            "arguments": {
                "title": "Rust (programming language)"
            }
        },
        "id": 5
    });

    let wiki_response = send_request(&mut writer, &mut reader, wiki_tool_request).await?;

    if let Some(result) = wiki_response.get("result") {
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            if let Some(first) = content.first() {
                if let Some(text) = first.get("text").and_then(|t| t.as_str()) {
                    let parsed: serde_json::Value = serde_json::from_str(text)?;
                    if let Some(extract) = parsed.get("extract").and_then(|e| e.as_str()) {
                        println!("\nWikipedia extract:");
                        println!("{}", &extract[..extract.len().min(500)]);
                        println!("...");
                    }
                }
            }
        }
    }

    // Clean up
    child.kill().await?;

    Ok(())
}
