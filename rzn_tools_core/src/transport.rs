use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::mcp_server::JsonRpcHandler;

/// Stdio transport for MCP server
pub struct StdioTransport {
    handler: JsonRpcHandler,
}

impl StdioTransport {
    pub fn new(handler: JsonRpcHandler) -> Self {
        Self { handler }
    }

    /// Run the stdio transport, reading from stdin and writing to stdout
    pub async fn run(&self) -> io::Result<()> {
        info!("Starting stdio transport");

        // Create channels for communication
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        // Spawn a task to read from stdin
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let mut reader = AsyncBufReader::new(stdin);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF reached
                        debug!("EOF reached on stdin");
                        break;
                    }
                    Ok(_) => {
                        if !line.trim().is_empty() {
                            if let Err(e) = tx_clone.send(line.clone()) {
                                error!("Failed to send line: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading from stdin: {}", e);
                        break;
                    }
                }
            }
        });

        // Process messages
        while let Some(line) = rx.recv().await {
            if let Err(e) = self.process_line(&line).await {
                error!("Error processing line: {}", e);
            }
        }

        Ok(())
    }

    /// Process a single line of input
    async fn process_line(&self, line: &str) -> io::Result<()> {
        debug!("Processing line: {}", line);

        // Parse JSON-RPC request
        match serde_json::from_str::<Value>(line) {
            Ok(request) => {
                // Handle the request
                let response = self.handler.handle_request(request).await;

                // Write response to stdout
                self.write_response(&response).await?;
            }
            Err(e) => {
                error!("Failed to parse JSON-RPC request: {}", e);

                // Send error response
                let error_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": e.to_string()
                    },
                    "id": null
                });

                self.write_response(&error_response).await?;
            }
        }

        Ok(())
    }

    /// Write a response to stdout
    async fn write_response(&self, response: &Value) -> io::Result<()> {
        let mut stdout = tokio::io::stdout();
        let response_str = serde_json::to_string(response)?;

        stdout.write_all(response_str.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;

        debug!("Sent response: {}", response_str);

        Ok(())
    }
}

/// Synchronous stdio transport for environments that don't support async
pub struct SyncStdioTransport {
    handler: JsonRpcHandler,
}

impl SyncStdioTransport {
    pub fn new(handler: JsonRpcHandler) -> Self {
        Self { handler }
    }

    /// Run the synchronous stdio transport
    pub fn run(&self) -> io::Result<()> {
        info!("Starting synchronous stdio transport");

        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            debug!("Processing line: {}", line);

            // Parse JSON-RPC request
            match serde_json::from_str::<Value>(&line) {
                Ok(request) => {
                    // Handle the request using tokio runtime
                    let response = tokio::runtime::Runtime::new()
                        .unwrap()
                        .block_on(self.handler.handle_request(request));

                    // Write response
                    writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                    stdout.flush()?;
                }
                Err(e) => {
                    error!("Failed to parse JSON-RPC request: {}", e);

                    let error_response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32700,
                            "message": "Parse error",
                            "data": e.to_string()
                        },
                        "id": null
                    });

                    writeln!(stdout, "{}", serde_json::to_string(&error_response)?)?;
                    stdout.flush()?;
                }
            }
        }

        Ok(())
    }
}
