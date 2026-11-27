//! One HTTP request at a time per host, paced (source-network design 3.4 and 3.9): 1 s apart with
//! jitter, longer when the host answers slowly, with back-off on 429 and 5xx. A get carries the
//! last fetch's validators, so an unchanged page answers 304 and costs nothing further.

use crate::error::ConnectorError;
use crate::http::HttpClient;
use reqwest::header::{HeaderMap, HeaderValue, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::StatusCode;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The gap between two requests to one host.
pub(super) const HOST_GAP: Duration = Duration::from_secs(1);
/// Retries of a 429 or 5xx answer before it is returned as it is.
const RETRIES: u32 = 3;
/// The longest wait a `Retry-After` header may ask for.
const MAX_WAIT: Duration = Duration::from_secs(30);
/// A slow answer stretches the gap by its own duration, up to this.
const MAX_SLOW: Duration = Duration::from_secs(10);

/// When the next request to a host may start. Holding its lock is holding the host.
type HostSlot = Arc<tokio::sync::Mutex<Option<Instant>>>;

fn host_slot(host: &str) -> HostSlot {
    static HOSTS: OnceLock<Mutex<HashMap<String, HostSlot>>> = OnceLock::new();
    let hosts = HOSTS.get_or_init(Default::default);
    let mut hosts = hosts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    hosts.entry(host.to_string()).or_default().clone()
}

/// Up to half of `base`, from the clock's nanoseconds; enough to break a fixed rhythm.
fn jitter(base: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    base.mul_f64(f64::from(nanos % 1000) / 2000.0)
}

/// The wait before the next request, given how long the last one took.
pub(super) fn next_gap(base: Duration, took: Duration) -> Duration {
    base + took.min(MAX_SLOW) / 2 + jitter(base)
}

/// The back-off before retry `attempt` (1-based), or the server's `Retry-After` in seconds.
fn backoff(base: Duration, attempt: u32, retry_after: Option<&HeaderValue>) -> Duration {
    retry_after
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| base * 2u32.pow(attempt))
        .min(MAX_WAIT)
}

/// The last fetch's validators, sent as `If-None-Match` and `If-Modified-Since`.
#[derive(Debug, Default, Clone)]
pub(super) struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validators {
    pub fn from_args(args: &serde_json::Map<String, serde_json::Value>) -> Self {
        let text = |name: &str| {
            args.get(name)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Self {
            etag: text("if_none_match"),
            last_modified: text("if_modified_since"),
        }
    }

    fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

/// One answered request.
#[derive(Debug)]
pub(super) struct Fetched {
    pub status: StatusCode,
    /// The URL after redirects.
    pub final_url: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub validators: Validators,
}

fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// GET `url` through `client` (so through its proxy, when it has one), paced per host. A 429 or
/// 5xx is retried with back-off; the last answer is returned whatever its status.
pub(super) async fn get(
    client: &HttpClient,
    url: &str,
    headers: &HeaderMap,
    validators: &Validators,
    base: Duration,
) -> Result<Fetched, ConnectorError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ConnectorError::InvalidParams(format!("invalid URL {url}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ConnectorError::InvalidParams(format!("URL {url} has no host")))?
        .to_ascii_lowercase();
    let slot = host_slot(&host);
    let mut next = slot.lock().await;
    let mut attempt = 0;
    loop {
        if let Some(at) = *next {
            tokio::time::sleep_until(at.into()).await;
        }
        let started = Instant::now();
        let mut request = client.get(url).headers(headers.clone());
        if !validators.is_empty() {
            if let Some(etag) = &validators.etag {
                request = request.header(IF_NONE_MATCH, etag);
            }
            if let Some(since) = &validators.last_modified {
                request = request.header(IF_MODIFIED_SINCE, since);
            }
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                *next = Some(Instant::now() + next_gap(base, started.elapsed()));
                return Err(ConnectorError::Other(format!("fetch {url}: {error}")));
            }
        };
        let status = response.status();
        if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()) && attempt < RETRIES
        {
            attempt += 1;
            let wait = backoff(base, attempt, response.headers().get("retry-after"));
            *next = Some(Instant::now() + wait);
            continue;
        }
        let final_url = response.url().to_string();
        let response_headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(|e| ConnectorError::Other(format!("read {url}: {e}")))?
            .to_vec();
        *next = Some(Instant::now() + next_gap(base, started.elapsed()));
        return Ok(Fetched {
            status,
            final_url,
            body,
            content_type: header_text(&response_headers, reqwest::header::CONTENT_TYPE),
            validators: Validators {
                etag: header_text(&response_headers, ETAG),
                last_modified: header_text(&response_headers, LAST_MODIFIED),
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gap_is_at_least_the_base_and_grows_with_a_slow_host() {
        let base = Duration::from_secs(1);
        let quick = next_gap(base, Duration::from_millis(50));
        assert!(quick >= base && quick < base * 2);
        let slow = next_gap(base, Duration::from_secs(4));
        assert!(slow >= base + Duration::from_secs(2));
    }

    #[test]
    fn back_off_doubles_and_honours_retry_after_up_to_a_cap() {
        let base = Duration::from_secs(1);
        assert_eq!(backoff(base, 1, None), Duration::from_secs(2));
        assert_eq!(backoff(base, 2, None), Duration::from_secs(4));
        let later = HeaderValue::from_static("7");
        assert_eq!(backoff(base, 1, Some(&later)), Duration::from_secs(7));
        let far = HeaderValue::from_static("600");
        assert_eq!(backoff(base, 1, Some(&far)), MAX_WAIT);
    }
}
