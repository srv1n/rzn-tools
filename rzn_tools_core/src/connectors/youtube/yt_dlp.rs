//! The yt-dlp subprocess behind the caption fallback.

use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;

use crate::error::ConnectorError;

/// Runs `yt-dlp -J` for one video and returns its info JSON. With a proxy, yt-dlp gets
/// `--proxy`, and the proxy URL is scrubbed from any error text.
pub(super) async fn video_info(
    proxy_url: Option<&str>,
    video_id: &str,
) -> Result<Value, ConnectorError> {
    let mut cmd = Command::new("yt-dlp");
    cmd.arg("-J")
        .arg("--ignore-config")
        .arg("--no-playlist")
        .arg("--skip-download")
        .arg("--ignore-no-formats-error")
        .arg("--js-runtimes")
        .arg("node")
        .arg("--socket-timeout")
        .arg("10")
        .arg("--retries")
        .arg("1")
        .arg("--extractor-retries")
        .arg("1")
        .arg("--no-warnings");
    if let Some(proxy_url) = proxy_url {
        cmd.arg("--proxy").arg(proxy_url);
    }
    cmd.arg(format!("https://www.youtube.com/watch?v={video_id}"))
        .kill_on_drop(true);

    let output = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| ConnectorError::Other("yt-dlp timed out while fetching captions".to_string()))?
        .map_err(|e| ConnectorError::Other(format!("Failed to execute yt-dlp: {e}")))?;

    if !output.status.success() {
        let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if let Some(proxy_url) = proxy_url {
            stderr = stderr.replace(proxy_url, "<proxy_url>");
        }
        let reason = if stderr.is_empty() {
            format!("yt-dlp exited with status {}", output.status)
        } else {
            format!("yt-dlp failed: {stderr}")
        };
        return Err(ConnectorError::Other(reason));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| ConnectorError::Other(e.to_string()))
}
