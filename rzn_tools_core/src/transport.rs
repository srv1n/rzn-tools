use std::io;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::mcp_server::JsonRpcHandler;

/// Line-delimited JSON-RPC transport over stdin/stdout.
pub struct StdioTransport {
    handler: JsonRpcHandler,
}

impl StdioTransport {
    pub fn new(handler: JsonRpcHandler) -> Self {
        Self { handler }
    }

    pub async fn run(&self) -> io::Result<()> {
        info!("Starting stdio transport");
        let mut lines = BufReader::new(tokio::io::stdin()).lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            if let Err(error) = self.process_line(&line).await {
                error!(%error, "Failed to process stdin message");
            }
        }

        Ok(())
    }

    async fn process_line(&self, line: &str) -> io::Result<()> {
        debug!("Processing line: {}", line);
        let response = match serde_json::from_str::<Value>(line) {
            Ok(request) => self.handler.handle_request(request).await,
            Err(error) => {
                error!(%error, "Failed to parse JSON-RPC request");
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": error.to_string()
                    },
                    "id": null
                })
            }
        };

        let mut stdout = tokio::io::stdout();
        stdout
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await
    }
}
