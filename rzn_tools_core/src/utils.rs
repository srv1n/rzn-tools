use crate::error::ConnectorError;
use chrono::{Datelike, Duration, Utc};
#[cfg(feature = "browser-cookies")]
use publicsuffix::{List, Psl};
use rmcp::model::{CallToolResult, Content};
#[cfg(all(feature = "browser-cookies", target_os = "macos"))]
use rookie::safari;
#[cfg(feature = "browser-cookies")]
use rookie::{brave, chrome, common::enums::CookieToString, edge, firefox};
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashSet;
use std::future::Future;
use thiserror::Error;
use url::Url;

pub fn build_reqwest_client(
    make_builder: impl Fn() -> reqwest::ClientBuilder,
) -> Result<reqwest::Client, ConnectorError> {
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| make_builder().build()));

    match built {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(e)) => Err(ConnectorError::HttpRequest(e)),
        Err(_) => Ok(make_builder().no_proxy().build()?),
    }
}

#[cfg(feature = "browser-cookies")]
#[derive(Debug, Clone)]
pub enum Browser {
    Firefox,
    Chrome,
    Edge,
    Safari,
    Brave,
}

#[cfg(not(feature = "browser-cookies"))]
#[derive(Debug, Clone)]
pub enum Browser {
    Firefox,
    Chrome,
    Edge,
    Safari,
    Brave,
}

#[derive(Debug, Error)]
pub enum ScraperError {
    CookieError(String),
}

impl std::fmt::Display for ScraperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScraperError::CookieError(msg) => write!(f, "Cookie error: {}", msg),
        }
    }
}

#[cfg(feature = "browser-cookies")]
pub async fn get_cookies(browser: Browser, domain: String) -> Result<String, ScraperError> {
    // Check if domain has a scheme, if not add https://
    let domain_with_scheme = if !domain.starts_with("http://") && !domain.starts_with("https://") {
        format!("https://{}", domain)
    } else {
        domain.to_string()
    };

    let url = Url::parse(&domain_with_scheme)
        .map_err(|e| ScraperError::CookieError(format!("Invalid URL: {}", e)))?;
    let list = List::from_bytes(&include_bytes!("../public_suffix_list.dat")[..]).map_err(|e| {
        ScraperError::CookieError(format!("Failed to parse public suffix list: {}", e))
    })?;
    let domain_str = url
        .host_str()
        .ok_or_else(|| ScraperError::CookieError("URL has no host".to_string()))?;
    let domain = list.domain(domain_str.as_bytes()).ok_or_else(|| {
        ScraperError::CookieError(format!("Could not extract domain from: {}", domain_str))
    })?;

    // Convert suffix bytes to string

    let domain_str = String::from_utf8_lossy(domain.as_bytes()).to_string();
    //    println!("Domain: {}", domain_str);

    let cookies = match browser {
        Browser::Firefox => firefox(Some(vec![domain_str.to_string()])),
        Browser::Chrome => chrome(Some(vec![domain_str.to_string()])),
        Browser::Edge => edge(Some(vec![domain_str.to_string()])),
        #[cfg(target_os = "macos")]
        Browser::Safari => safari(Some(vec![domain_str.to_string()])),
        #[cfg(not(target_os = "macos"))]
        Browser::Safari => {
            return Err(ScraperError::CookieError(
                "Safari cookies are only available on macOS".to_string(),
            ))
        }
        Browser::Brave => brave(Some(vec![domain_str.to_string()])),
    }
    .map_err(|e| ScraperError::CookieError(e.to_string()))?;
    //   println!("Cookies: {:?}", cookies);
    Ok(cookies.to_string())
}

#[cfg(not(feature = "browser-cookies"))]
pub async fn get_cookies(_browser: Browser, _domain: String) -> Result<String, ScraperError> {
    Err(ScraperError::CookieError(
        "browser-cookies feature not enabled".to_string(),
    ))
}

#[cfg(feature = "browser-cookies")]
pub async fn match_browser(browser: String) -> Result<Browser, ConnectorError> {
    match browser.as_str() {
        "firefox" => Ok(Browser::Firefox),
        "chrome" => Ok(Browser::Chrome),
        "edge" => Ok(Browser::Edge),
        "safari" => Ok(Browser::Safari),
        "brave" => Ok(Browser::Brave),
        _ => Err(ConnectorError::Other(format!(
            "Invalid browser: {}",
            browser
        ))),
    }
}

#[cfg(not(feature = "browser-cookies"))]
pub async fn match_browser(browser: String) -> Result<Browser, ConnectorError> {
    match browser.as_str() {
        "firefox" => Ok(Browser::Firefox),
        "chrome" => Ok(Browser::Chrome),
        "edge" => Ok(Browser::Edge),
        "safari" => Ok(Browser::Safari),
        "brave" => Ok(Browser::Brave),
        _ => Err(ConnectorError::Other(format!(
            "Invalid browser: {}",
            browser
        ))),
    }
}

#[cfg(feature = "browser-cookies")]
pub fn get_domain(url: &str) -> Result<String, ConnectorError> {
    let url_with_scheme = if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    let url = Url::parse(&url_with_scheme)
        .map_err(|e| ConnectorError::Other(format!("Invalid URL: {}", e)))?;

    let host_str = url
        .host_str()
        .ok_or_else(|| ConnectorError::Other("URL has no host".to_string()))?;

    let list = List::from_bytes(&include_bytes!("../public_suffix_list.dat")[..])
        .map_err(|e| ConnectorError::Other(format!("Failed to parse public suffix list: {}", e)))?;

    let domain = list.domain(host_str.as_bytes()).ok_or_else(|| {
        ConnectorError::Other(format!("Could not extract domain from: {}", host_str))
    })?;

    // Convert suffix bytes to string
    let domain_str = String::from_utf8_lossy(domain.as_bytes()).to_string();
    Ok(domain_str)
}

#[cfg(not(feature = "browser-cookies"))]
pub fn get_domain(url: &str) -> Result<String, ConnectorError> {
    let url_with_scheme = if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    let url = Url::parse(&url_with_scheme)
        .map_err(|e| ConnectorError::Other(format!("Invalid URL: {}", e)))?;

    let domain = url
        .host_str()
        .ok_or_else(|| ConnectorError::Other(format!("URL has no host: {}", url_with_scheme)))?;

    Ok(domain.to_string())
}

#[cfg(feature = "browser-cookies")]
pub fn get_user_agent(browser: Browser) -> String {
    match browser {
        Browser::Firefox => "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:136.0) Gecko/20100101 Firefox/136.0".to_string(),
        Browser::Chrome => "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36".to_string(),
        Browser::Edge => "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36 Edg/133.0.0.0".to_string(),
        Browser::Safari => "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.1 Safari/605.1.15".to_string(),
        Browser::Brave => "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
    }
}

#[cfg(not(feature = "browser-cookies"))]
pub fn get_user_agent(_browser: Browser) -> String {
    // Return a generic UA; useful for minimal builds.
    "Mozilla/5.0".to_string()
}

pub fn strip_multiple_newlines(text: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;
    let mut in_quote_block = false;
    let mut consecutive_newlines = 0;

    for line in text.lines() {
        // Check for code block markers
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
            consecutive_newlines = 0;
            continue;
        }

        // Check for quote block
        if line.trim().starts_with('>') {
            in_quote_block = true;
        } else if in_quote_block && !line.trim().is_empty() {
            in_quote_block = false;
        }

        // Handle line based on context
        if line.trim().is_empty() {
            if !in_code_block && !in_quote_block {
                consecutive_newlines += 1;
                if consecutive_newlines <= 1 {
                    result.push('\n');
                }
            } else {
                // Preserve empty lines in code blocks and quotes
                result.push('\n');
            }
        } else {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
            consecutive_newlines = 0;
        }
    }

    result
}

pub fn clean_html_entities(text: &str) -> String {
    let mut cleaned = text.to_string();
    // Try decoding multiple times in case of double-encoding
    for _ in 0..2 {
        let decoded = html_escape::decode_html_entities(&cleaned).into_owned();
        if decoded == cleaned {
            break;
        }
        cleaned = decoded;
    }

    // Handle any remaining common entities manually
    cleaned
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub struct Page<T, C> {
    pub items: Vec<T>,
    pub next_cursor: Option<C>,
}

pub struct Collected<T, C> {
    pub items: Vec<T>,
    pub next_cursor: Option<C>,
}

pub async fn collect_paginated_with_cursor<T, C, Fetch, Fut, KeyFn>(
    desired: usize,
    max_requests: usize,
    mut cursor: Option<C>,
    mut fetch: Fetch,
    mut key_fn: KeyFn,
) -> Result<Collected<T, C>, ConnectorError>
where
    Fetch: FnMut(Option<C>, usize) -> Fut,
    Fut: Future<Output = Result<Page<T, C>, ConnectorError>>,
    KeyFn: FnMut(&T) -> Option<String>,
{
    if desired == 0 {
        return Ok(Collected {
            items: Vec::new(),
            next_cursor: cursor,
        });
    }

    let mut out: Vec<T> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut requests: usize = 0;

    while out.len() < desired && requests < max_requests {
        let remaining = desired.saturating_sub(out.len());
        if remaining == 0 {
            break;
        }

        let page = fetch(cursor, remaining).await?;
        cursor = page.next_cursor;

        if page.items.is_empty() {
            break;
        }

        for item in page.items {
            if out.len() >= desired {
                break;
            }

            if let Some(key) = key_fn(&item) {
                if !seen.insert(key) {
                    continue;
                }
            }

            out.push(item);
        }

        requests = requests.saturating_add(1);
        if cursor.is_none() {
            break;
        }
    }

    Ok(Collected {
        items: out,
        next_cursor: cursor,
    })
}

pub async fn collect_paginated<T, C, Fetch, Fut, KeyFn>(
    desired: usize,
    max_requests: usize,
    cursor: Option<C>,
    fetch: Fetch,
    key_fn: KeyFn,
) -> Result<Vec<T>, ConnectorError>
where
    Fetch: FnMut(Option<C>, usize) -> Fut,
    Fut: Future<Output = Result<Page<T, C>, ConnectorError>>,
    KeyFn: FnMut(&T) -> Option<String>,
{
    Ok(
        collect_paginated_with_cursor(desired, max_requests, cursor, fetch, key_fn)
            .await?
            .items,
    )
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn collects_with_dedupe_and_cursor() {
        #[derive(Debug)]
        struct Item {
            id: &'static str,
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let items = collect_paginated(
            3,
            10,
            None::<usize>,
            {
                let calls = Arc::clone(&calls);
                move |cursor, _remaining| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async move {
                        let page = match cursor {
                            None => Page {
                                items: vec![Item { id: "a" }, Item { id: "b" }],
                                next_cursor: Some(1),
                            },
                            Some(1) => Page {
                                items: vec![Item { id: "b" }, Item { id: "c" }],
                                next_cursor: None,
                            },
                            _ => Page {
                                items: vec![],
                                next_cursor: None,
                            },
                        };
                        Ok::<_, ConnectorError>(page)
                    }
                }
            },
            |i: &Item| Some(i.id.to_string()),
        )
        .await
        .unwrap();

        assert_eq!(items.len(), 3);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}

/// Clean a URL by removing tracking parameters and truncating if too long.
fn clean_url(url: &str) -> String {
    // Try to parse and clean the URL
    if let Ok(mut parsed) = url::Url::parse(url) {
        // Remove common tracking/token parameters
        let dominated_params: std::collections::HashSet<&str> = [
            "utm_source",
            "utm_medium",
            "utm_campaign",
            "utm_term",
            "utm_content",
            "access_token",
            "token",
            "auth_token",
            "api_key",
            "key",
            "fbclid",
            "gclid",
            "mc_eid",
            "mc_cid",
            "ref",
            "source",
            "unsub",
            "redirect_uri",
            "callback",
        ]
        .into_iter()
        .collect();

        let clean_pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| !dominated_params.contains(k.as_ref()))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        if clean_pairs.is_empty() {
            parsed.set_query(None);
        } else {
            parsed.query_pairs_mut().clear();
            for (k, v) in clean_pairs {
                parsed.query_pairs_mut().append_pair(&k, &v);
            }
        }

        let cleaned = parsed.to_string();
        // If still too long, just show domain + path (truncated)
        if cleaned.len() > 80 {
            let domain = parsed.host_str().unwrap_or("");
            let path = parsed.path();
            let short_path = if path.len() > 30 {
                format!("{}...", &path[..27])
            } else {
                path.to_string()
            };
            format!("https://{}{}", domain, short_path)
        } else {
            cleaned
        }
    } else {
        // Can't parse, just truncate if too long
        if url.len() > 80 {
            format!("{}...", &url[..77])
        } else {
            url.to_string()
        }
    }
}

/// Convert HTML to plain text by stripping tags and extracting readable content.
/// Useful for email bodies and web content where LLM-friendly text is needed.
/// Links are converted to markdown format [text](url) with cleaned URLs.
pub fn html_to_text(html: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::{Captures, Regex};

    // Compile regexes once
    static RE_SCRIPT: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    static RE_STYLE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    // Match <a href="url">text</a> - capture href and inner text
    static RE_LINK: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?is)<a\s+[^>]*href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap()
    });
    static RE_BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static RE_BLOCK_CLOSE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)</(p|div|tr|li|h[1-6])>").unwrap());
    static RE_BLOCK_OPEN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)<(p|div|tr|li|h[1-6])[^>]*>").unwrap());
    static RE_TAGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    static RE_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+").unwrap());
    static RE_NL_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n[ \t]+").unwrap());
    static RE_SPACE_NL: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+\n").unwrap());
    static RE_MULTI_NL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

    let mut text = html.to_string();

    // Remove script and style blocks entirely (including content)
    text = RE_SCRIPT.replace_all(&text, "").to_string();
    text = RE_STYLE.replace_all(&text, "").to_string();

    // Convert links to markdown format [text](cleaned_url) before stripping other tags
    text = RE_LINK
        .replace_all(&text, |caps: &Captures| {
            let url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let link_text = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            // Strip any nested tags from link text
            let clean_text = RE_TAGS.replace_all(link_text, "").trim().to_string();
            let clean_url = clean_url(url);

            if clean_text.is_empty() || clean_text == url || clean_text == clean_url {
                // Just show cleaned URL if no meaningful text
                clean_url
            } else {
                format!("[{}]({})", clean_text, clean_url)
            }
        })
        .to_string();

    // Convert common block elements to newlines
    text = RE_BR.replace_all(&text, "\n").to_string();
    text = RE_BLOCK_CLOSE.replace_all(&text, "\n").to_string();
    text = RE_BLOCK_OPEN.replace_all(&text, "\n").to_string();

    // Remove all remaining HTML tags
    text = RE_TAGS.replace_all(&text, "").to_string();

    // Decode HTML entities
    text = clean_html_entities(&text);

    // Normalize whitespace
    text = RE_SPACES.replace_all(&text, " ").to_string();
    text = RE_NL_SPACE.replace_all(&text, "\n").to_string();
    text = RE_SPACE_NL.replace_all(&text, "\n").to_string();
    text = RE_MULTI_NL.replace_all(&text, "\n\n").to_string();

    text.trim().to_string()
}

//     html_escape::decode_html_entities(text).into_owned().replace("\n", " ").replace("&#39;", "'")
// }

/// Build a CallToolResult that carries only structured JSON (no text fallback).
/// This prioritizes first-class machine-readable results for modern MCP clients.
const RESULT_LIST_KEYS: &[&str] = &[
    "results",
    "articles",
    "items",
    "entries",
    "documents",
    "records",
    "posts",
    "stories",
    "videos",
    "papers",
    "messages",
    "mailboxes",
    "conversations",
    "threads",
    "hits",
    "search_results",
    "content",
    "data",
];

const COUNT_KEYS: &[&str] = &[
    "total_results",
    "total_count",
    "count",
    "results_count",
    "result_count",
    "nbHits",
    "nb_hits",
    "match_count",
    "hits",
];

const QUERY_FIELD_KEYS: &[&str] = &[
    "query",
    "search_query",
    "term",
    "search_term",
    "keywords",
    "keyword",
    "q",
];

fn build_no_results_message(key: &str, query_hint: Option<String>) -> String {
    let label = match key {
        "data" | "results" | "total_results" | "total_count" | "count" | "nbHits" | "nb_hits"
        | "hits" | "result_count" | "results_count" => "results".to_string(),
        other => other.replace('_', " "),
    };

    match query_hint {
        Some(query) => format!("No {} found for \"{}\".", label, query),
        None => format!("No {} found for the requested input.", label),
    }
}

fn maybe_attach_no_results_message(map: &mut JsonMap<String, JsonValue>) -> Option<String> {
    // Any non-empty result list means we have data and should not set a no-results message.
    for key in RESULT_LIST_KEYS {
        if let Some(JsonValue::Array(items)) = map.get(*key) {
            if !items.is_empty() {
                return None;
            }
        }
    }

    // Capture a query hint if the payload includes one.
    let query_hint = map
        .iter()
        .find_map(|(key, value)| {
            if QUERY_FIELD_KEYS.iter().any(|candidate| candidate == key) {
                value.as_str().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty());

    let mut message: Option<String> = None;

    for key in RESULT_LIST_KEYS {
        if let Some(value) = map.get(*key) {
            match value {
                JsonValue::Array(items) if items.is_empty() => {
                    message = Some(build_no_results_message(key, query_hint.clone()));
                    break;
                }
                JsonValue::Null => {
                    message = Some(build_no_results_message(key, query_hint.clone()));
                    break;
                }
                JsonValue::String(s) if s.trim().is_empty() => {
                    message = Some(build_no_results_message(key, query_hint.clone()));
                    break;
                }
                JsonValue::Object(obj) if obj.is_empty() => {
                    message = Some(build_no_results_message(key, query_hint.clone()));
                    break;
                }
                JsonValue::Number(num) if num.as_u64() == Some(0) => {
                    message = Some(build_no_results_message(key, query_hint.clone()));
                    break;
                }
                _ => {}
            }
        }
    }

    if message.is_none() {
        if let Some(JsonValue::Array(items)) = map.get("data") {
            if items.is_empty() {
                message = Some(build_no_results_message("results", query_hint.clone()));
            }
        } else if let Some(JsonValue::Object(obj)) = map.get("data") {
            if obj.is_empty() {
                message = Some(build_no_results_message("results", query_hint.clone()));
            }
        }
    }

    if message.is_none() {
        for key in COUNT_KEYS {
            if let Some(value) = map.get(*key) {
                if value.as_u64() == Some(0) {
                    message = Some(build_no_results_message("results", query_hint.clone()));
                    break;
                }
                if let Some(as_str) = value.as_str() {
                    if as_str.trim() == "0" {
                        message = Some(build_no_results_message("results", query_hint.clone()));
                        break;
                    }
                }
            }
        }
    }

    if message.is_none() && map.is_empty() {
        message = Some(build_no_results_message("results", query_hint.clone()));
    }

    if let Some(message_text) = message.clone() {
        map.entry("message".to_string())
            .or_insert(JsonValue::String(message_text.clone()));
        map.entry("no_results".to_string())
            .or_insert(JsonValue::Bool(true));
    }

    message
}

pub fn structured_result_with_text<T: Serialize>(
    data: &T,
    text_fallback: Option<String>,
) -> Result<CallToolResult, ConnectorError> {
    let value = serde_json::to_value(data).map_err(|e| ConnectorError::Other(e.to_string()))?;

    // Convert to an object map; if it's not an object, wrap under a `data` key.
    let mut map: JsonMap<String, JsonValue> = match value {
        JsonValue::Object(m) => m,
        other => {
            let mut m = JsonMap::new();
            m.insert("data".to_string(), other);
            m
        }
    };

    maybe_attach_no_results_message(&mut map);
    let content_text = tool_result_text_fallback(&JsonValue::Object(map.clone()), text_fallback);

    Ok(CallToolResult {
        content: vec![Content::text(content_text)],
        structured_content: Some(JsonValue::Object(map)),
        is_error: Some(false),
        meta: None,
    })
}

/// Build a CallToolResult with structured JSON without injecting helper fields.
/// Prefer this for strict schema outputs (e.g., normalized ingest payloads).
pub fn structured_result<T: Serialize>(data: &T) -> Result<CallToolResult, ConnectorError> {
    let value = serde_json::to_value(data).map_err(|e| ConnectorError::Other(e.to_string()))?;
    let content_text = tool_result_text_fallback(&value, None);

    Ok(CallToolResult {
        content: vec![Content::text(content_text)],
        structured_content: Some(value),
        is_error: Some(false),
        meta: None,
    })
}

const TOOL_RESULT_TEXT_FALLBACK_MAX_CHARS: usize = 12_000;

fn tool_result_text_fallback(value: &JsonValue, explicit_text: Option<String>) -> String {
    let candidate = explicit_text
        .filter(|text| !text.trim().is_empty())
        .or_else(|| summarize_structured_tool_value(value))
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()));

    truncate_tool_result_text(candidate, TOOL_RESULT_TEXT_FALLBACK_MAX_CHARS)
}

fn truncate_tool_result_text(text: String, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text;
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!(
        "{}\n\n[truncated {} chars from tool text fallback]",
        truncated,
        char_count - max_chars
    )
}

fn summarize_structured_tool_value(value: &JsonValue) -> Option<String> {
    match value.get("type").and_then(JsonValue::as_str) {
        Some(crate::ingest::NORMALIZED_ITEM_V1_TYPE) => summarize_normalized_item(value),
        Some(crate::ingest::NORMALIZED_PAGE_V1_TYPE) => summarize_normalized_page(value),
        Some(crate::display::v1::DISPLAY_ITEM_V1_TYPE) => summarize_display_item(value),
        Some(crate::display::v1::DISPLAY_PAGE_V1_TYPE) => summarize_display_page(value),
        _ => None,
    }
}

fn summarize_normalized_item(value: &JsonValue) -> Option<String> {
    let item = value.get("item")?;
    let title = item
        .get("title")
        .and_then(JsonValue::as_str)
        .unwrap_or("Untitled item");
    let kind = item
        .get("kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("item");
    let url = item.get("canonical_url").and_then(JsonValue::as_str);
    let authors = join_strings(
        item.get("authors")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(|author| author.get("name").and_then(JsonValue::as_str)),
        4,
    );
    let blocks = item
        .get("blocks")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let has_transcript = blocks.iter().any(|block| {
        matches!(
            block.get("block_kind").and_then(JsonValue::as_str),
            Some("transcript" | "transcript_segment")
        )
    });
    let description = blocks.iter().find_map(|block| {
        (block.get("block_kind").and_then(JsonValue::as_str) == Some("description"))
            .then(|| block.get("text").and_then(JsonValue::as_str))
            .flatten()
    });
    let transcript = blocks.iter().find_map(|block| {
        matches!(
            block.get("block_kind").and_then(JsonValue::as_str),
            Some("transcript" | "transcript_segment")
        )
        .then(|| block.get("text").and_then(JsonValue::as_str))
        .flatten()
    });

    let mut lines = vec![format!("{}: {}", kind.to_ascii_uppercase(), title)];
    if let Some(url) = url {
        lines.push(format!("URL: {}", url));
    }
    if let Some(authors) = authors {
        lines.push(format!("Authors: {}", authors));
    }
    lines.push(format!(
        "Transcript available: {}",
        if has_transcript { "yes" } else { "no" }
    ));
    if let Some(description) = description {
        lines.push(String::new());
        lines.push("Description excerpt:".to_string());
        lines.push(excerpt(description, 700));
    }
    if let Some(transcript) = transcript {
        lines.push(String::new());
        lines.push("Transcript excerpt:".to_string());
        lines.push(excerpt(transcript, 4000));
    }

    Some(lines.join("\n"))
}

fn summarize_normalized_page(value: &JsonValue) -> Option<String> {
    let items = value.get("items")?.as_array()?;
    let mut lines = vec![format!("Fetched {} items.", items.len())];

    for (idx, item) in items.iter().take(5).enumerate() {
        let title = item
            .get("title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Untitled item");
        let url = item
            .get("canonical_url")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if url.is_empty() {
            lines.push(format!("{}. {}", idx + 1, title));
        } else {
            lines.push(format!("{}. {} — {}", idx + 1, title, url));
        }
    }
    if items.len() > 5 {
        lines.push(format!("...and {} more items.", items.len() - 5));
    }

    Some(lines.join("\n"))
}

fn summarize_display_item(value: &JsonValue) -> Option<String> {
    let item = value.get("item")?;
    let title = item
        .get("title")
        .and_then(JsonValue::as_str)
        .unwrap_or("Untitled item");
    let url = item.get("url").and_then(JsonValue::as_str);
    let snippet = item.get("snippet").and_then(JsonValue::as_str);
    let markdown = value
        .get("blocks")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .find_map(|block| {
            (block.get("type").and_then(JsonValue::as_str) == Some("markdown"))
                .then(|| block.get("markdown").and_then(JsonValue::as_str))
                .flatten()
        });

    let mut lines = vec![
        format!("Fetched item successfully."),
        format!("Title: {}", title),
    ];
    if let Some(url) = url {
        lines.push(format!("URL: {}", url));
    }
    if let Some(snippet) = snippet {
        lines.push(String::new());
        lines.push("Snippet:".to_string());
        lines.push(excerpt(snippet, 700));
    }
    if let Some(markdown) = markdown {
        lines.push(String::new());
        lines.push("Content excerpt:".to_string());
        lines.push(excerpt(markdown, 4000));
    }

    Some(lines.join("\n"))
}

fn summarize_display_page(value: &JsonValue) -> Option<String> {
    let items = value.get("items")?.as_array()?;
    let mut lines = vec![format!("Fetched {} items.", items.len())];

    for (idx, item) in items.iter().take(5).enumerate() {
        let title = item
            .get("title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Untitled item");
        let url = item.get("url").and_then(JsonValue::as_str).unwrap_or("");
        if url.is_empty() {
            lines.push(format!("{}. {}", idx + 1, title));
        } else {
            lines.push(format!("{}. {} — {}", idx + 1, title, url));
        }
    }
    if items.len() > 5 {
        lines.push(format!("...and {} more items.", items.len() - 5));
    }

    Some(lines.join("\n"))
}

fn join_strings<'a>(values: impl IntoIterator<Item = &'a str>, max_items: usize) -> Option<String> {
    let collected = values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .take(max_items)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    (!collected.is_empty()).then(|| collected.join(", "))
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = normalized.chars().count();
    if char_count <= max_chars {
        return normalized;
    }

    let truncated: String = normalized.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::{structured_result, structured_result_with_text};
    use rmcp::model::RawContent;
    use serde_json::json;

    #[test]
    fn structured_result_with_text_populates_content_text() {
        let result =
            structured_result_with_text(&json!({"ok": true}), Some("hello from tool".to_string()))
                .expect("tool result");

        let first = result.content.first().expect("content item");
        match &**first {
            RawContent::Text(text) => assert_eq!(text.text, "hello from tool"),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn structured_result_adds_non_empty_text_fallback() {
        let result =
            structured_result(&json!({"ok": true, "items": [1, 2, 3]})).expect("tool result");

        let first = result.content.first().expect("content item");
        match &**first {
            RawContent::Text(text) => {
                assert!(!text.text.is_empty(), "content text should not be empty");
                assert!(text.text.contains("\"ok\":true"));
            }
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn structured_result_summarizes_normalized_item_for_models() {
        let result = structured_result(&json!({
            "type": crate::ingest::NORMALIZED_ITEM_V1_TYPE,
            "item": {
                "kind": "video",
                "title": "Paperclip demo",
                "canonical_url": "https://example.com/video",
                "authors": [{"name": "AI Engineer"}],
                "blocks": [
                    {"block_kind": "description", "text": "Short description"},
                    {"block_kind": "transcript", "text": "Transcript body goes here"}
                ]
            }
        }))
        .expect("tool result");

        let first = result.content.first().expect("content item");
        match &**first {
            RawContent::Text(text) => {
                assert!(text.text.contains("VIDEO: Paperclip demo"));
                assert!(text.text.contains("Transcript available: yes"));
                assert!(text.text.contains("Transcript excerpt:"));
                assert!(!text
                    .text
                    .contains("\"type\":\"rzn-tools.normalized_item.v1\""));
            }
            other => panic!("expected text content, got {other:?}"),
        }
    }
}

// --- Uniform search filter helpers for connectors ---

#[derive(Debug, Clone)]
pub struct SearchFilters {
    pub language: Option<String>,
    pub region: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
}

fn ymd_string(days_from_now: i64) -> String {
    let d = Utc::now().date_naive() + Duration::days(days_from_now);
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

fn month_start_string() -> String {
    let d = Utc::now().date_naive();
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), 1)
}

fn parse_date_preset(preset: &str) -> Option<(String, String)> {
    let p = preset.to_lowercase();
    match p.as_str() {
        "last_24_hours" | "past_day" => Some((ymd_string(-1), ymd_string(0))),
        "last_7_days" | "past_week" => Some((ymd_string(-7), ymd_string(0))),
        "last_30_days" | "past_month" => Some((ymd_string(-30), ymd_string(0))),
        "this_month" => Some((month_start_string(), ymd_string(0))),
        "last_365_days" | "past_year" => Some((ymd_string(-365), ymd_string(0))),
        _ => None,
    }
}

fn parse_locale(locale: &str) -> (Option<String>, Option<String>) {
    let loc = locale.replace('_', "-");
    let parts: Vec<&str> = loc.split('-').collect();
    match parts.len() {
        1 => (Some(parts[0].to_lowercase()), None),
        2 => (Some(parts[0].to_lowercase()), Some(parts[1].to_uppercase())),
        _ => (None, None),
    }
}

pub fn resolve_search_filters(args: &JsonMap<String, JsonValue>) -> SearchFilters {
    let mut language = args
        .get("language")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut region = args
        .get("region")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut since = args
        .get("since")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut until = args
        .get("until")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if language.is_none() || region.is_none() {
        if let Some(loc) = args.get("locale").and_then(|v| v.as_str()) {
            let (lang, reg) = parse_locale(loc);
            if language.is_none() {
                language = lang;
            }
            if region.is_none() {
                region = reg;
            }
        }
    }

    if since.is_none() && until.is_none() {
        if let Some(preset) = args.get("date_preset").and_then(|v| v.as_str()) {
            if let Some((s, u)) = parse_date_preset(preset) {
                since = Some(s);
                until = Some(u);
            }
        }
    }

    let include_domains = args
        .get("include_domains")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let exclude_domains = args
        .get("exclude_domains")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect()
        })
        .unwrap_or_default();

    SearchFilters {
        language,
        region,
        since,
        until,
        include_domains,
        exclude_domains,
    }
}

pub fn build_filters_clause(filters: &SearchFilters) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(v) = &filters.language {
        parts.push(format!("language={}", v));
    }
    if let Some(v) = &filters.region {
        parts.push(format!("region={}", v));
    }
    if let Some(v) = &filters.since {
        parts.push(format!("since={}", v));
    }
    if let Some(v) = &filters.until {
        parts.push(format!("until={}", v));
    }
    if !filters.include_domains.is_empty() {
        parts.push(format!("include_domains={:?}", filters.include_domains));
    }
    if !filters.exclude_domains.is_empty() {
        parts.push(format!("exclude_domains={:?}", filters.exclude_domains));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("\nFilters: {}", parts.join("; "))
    }
}
