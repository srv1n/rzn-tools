use std::io;

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::mcp_server::JsonRpcHandler;

pub const DEFAULT_MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Line-delimited JSON-RPC transport over stdin/stdout.
pub struct StdioTransport {
    handler: JsonRpcHandler,
    max_frame_bytes: usize,
}

impl StdioTransport {
    pub fn new(handler: JsonRpcHandler) -> Self {
        Self::with_max_frame_bytes(handler, DEFAULT_MAX_FRAME_BYTES)
    }

    pub fn with_max_frame_bytes(handler: JsonRpcHandler, max_frame_bytes: usize) -> Self {
        Self {
            handler,
            max_frame_bytes: max_frame_bytes.max(1),
        }
    }

    pub async fn run(&self) -> io::Result<()> {
        info!("Starting stdio transport");
        let mut reader = BufReader::new(tokio::io::stdin());

        while let Some(frame) = read_frame(&mut reader, self.max_frame_bytes).await? {
            match frame {
                Frame::Oversize => {
                    self.write_response(serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32600,
                            "message": "Request frame exceeds maximum size"
                        },
                        "id": null
                    }))
                    .await?;
                }
                Frame::Data(bytes) => {
                    if bytes.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    if let Err(error) = self.process_line(&bytes).await {
                        error!(%error, "Failed to process stdin message");
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_line(&self, line: &[u8]) -> io::Result<()> {
        debug!(frame_bytes = line.len(), "Processing stdio frame");
        let response = match serde_json::from_slice::<Value>(line) {
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

        self.write_response(response).await
    }

    async fn write_response(&self, response: Value) -> io::Result<()> {
        let mut payload = serde_json::to_vec(&response)?;
        if payload.len() > self.max_frame_bytes {
            payload = serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32603,
                    "message": "Response frame exceeds maximum size"
                },
                "id": null
            }))?;
        }

        let mut stdout = tokio::io::stdout();
        stdout.write_all(&payload).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await
    }
}

enum Frame {
    Data(Vec<u8>),
    Oversize,
}

async fn read_frame<R>(reader: &mut R, max_frame_bytes: usize) -> io::Result<Option<Frame>>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let mut oversize = false;

    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            if frame.is_empty() && !oversize {
                return Ok(None);
            }
            return Ok(Some(if oversize {
                Frame::Oversize
            } else {
                Frame::Data(frame)
            }));
        }

        let end = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(buffer.len());
        let has_newline = end < buffer.len();
        if !oversize {
            if frame.len().saturating_add(end) > max_frame_bytes {
                oversize = true;
                frame.clear();
            } else {
                frame.extend_from_slice(&buffer[..end]);
            }
        }
        reader.consume(end);

        if has_newline {
            reader.consume(1);
            return Ok(Some(if oversize {
                Frame::Oversize
            } else {
                Frame::Data(frame)
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn refuses_oversize_frames_without_unbounded_buffering() {
        let input = vec![b'x'; 9];
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        assert!(matches!(
            read_frame(&mut reader, 8).await.expect("frame"),
            Some(Frame::Oversize)
        ));
    }

    #[tokio::test]
    async fn processes_final_frame_at_eof() {
        let mut reader = BufReader::new(std::io::Cursor::new(br#"{"jsonrpc":"2.0"}"#));
        let frame = read_frame(&mut reader, 64).await.expect("frame");
        assert!(matches!(frame, Some(Frame::Data(bytes)) if bytes == br#"{"jsonrpc":"2.0"}"#));
        assert!(read_frame(&mut reader, 64).await.expect("eof").is_none());
    }

    #[tokio::test]
    async fn consumes_newline_delimited_frames() {
        let mut reader = BufReader::new(std::io::Cursor::new(b"one\ntwo\n"));
        let first = read_frame(&mut reader, 8).await.expect("first");
        let second = read_frame(&mut reader, 8).await.expect("second");
        assert!(matches!(first, Some(Frame::Data(bytes)) if bytes == b"one"));
        assert!(matches!(second, Some(Frame::Data(bytes)) if bytes == b"two"));
        let mut sink = String::new();
        reader.read_to_string(&mut sink).await.expect("drain");
        assert!(sink.is_empty());
    }
}
