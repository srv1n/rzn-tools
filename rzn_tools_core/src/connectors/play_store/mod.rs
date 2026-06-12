use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_LANGUAGE, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    Author, ContentBlock, ContentItem, NormalizedItemV1, OutputFormat, Partial, Source, Truncation,
};
use crate::utils::{html_to_text, structured_result, structured_result_with_text};
use crate::{Connector, URLParamExtraction, URLPatternSpec};
use rmcp::model::*;

const DEFAULT_HL: &str = "en";
const DEFAULT_GL: &str = "US";

// A browser-ish UA reduces 403/robot challenges. Keep it simple to avoid churn.
const PLAY_STORE_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayStoreApp {
    id: String,
    url: String,
    retrieved_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    developer_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    developer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    rating_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating_count: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    downloads: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_on_iso: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_rating: Option<String>,
}

pub struct PlayStoreConnector {
    http: Client,
}

impl PlayStoreConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(PLAY_STORE_USER_AGENT));
        let http = Client::builder().default_headers(headers).build()?;
        Ok(Self { http })
    }

    fn build_app_url(id: &str, hl: &str, gl: &str) -> String {
        format!(
            "https://play.google.com/store/apps/details?id={}&hl={}&gl={}",
            id, hl, gl
        )
    }

    fn normalize_package_id(raw: &str) -> Result<String, ConnectorError> {
        let id = raw.trim();
        if id.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "Missing 'id' parameter".to_string(),
            ));
        }
        // Keep validation conservative (best-effort). Reject obvious junk.
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid Play Store package id: '{}'",
                id
            )));
        }
        Ok(id.to_string())
    }

    async fn fetch_html(&self, url: &str, hl: &str) -> Result<String, ConnectorError> {
        let resp = self
            .http
            .get(url)
            .header(ACCEPT_LANGUAGE, hl)
            .send()
            .await?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(ConnectorError::ResourceNotFound);
        }
        if status.as_u16() == 429 {
            return Err(ConnectorError::PageIsCaptchaOrAuthChallenge);
        }
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Play Store returned HTTP {}",
                status
            )));
        }

        Ok(resp.text().await?)
    }

    fn parse_json_ld_software_application(html: &str) -> Option<Value> {
        // Play pages often contain one or more JSON-LD scripts. We scan them and pick the first
        // object with @type=SoftwareApplication.
        for marker in [
            r#"type="application/ld+json""#,
            r#"type='application/ld+json'"#,
        ] {
            let mut start_at = 0usize;
            while let Some(idx) = html[start_at..].find(marker) {
                let idx = start_at + idx;
                let after = &html[idx..];
                let json_start = after.find('>')? + 1;
                let after = &after[json_start..];
                let json_end = after.find("</script>")?;
                let json_str = after[..json_end].trim();

                if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                    let found = match parsed {
                        Value::Object(map) => {
                            if map.get("@type").and_then(|v| v.as_str())
                                == Some("SoftwareApplication")
                            {
                                Some(Value::Object(map))
                            } else {
                                None
                            }
                        }
                        Value::Array(items) => items.into_iter().find(|v| {
                            v.get("@type").and_then(|t| t.as_str()) == Some("SoftwareApplication")
                        }),
                        _ => None,
                    };
                    if found.is_some() {
                        return found;
                    }
                }

                start_at = idx + marker.len();
            }
        }
        None
    }

    fn extract_downloads_en(html: &str) -> Option<String> {
        // Best-effort extraction for the "Downloads" tile in the metadata row.
        // This is locale-sensitive (label changes with hl).
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE
            .get_or_init(|| {
                Regex::new(
                    r#"(?s)<div[^>]*>\s*(?P<val>[^<]{1,30})\s*</div>\s*<div[^>]*>\s*Downloads\s*</div>"#,
                )
                .expect("valid regex")
            });
        re.captures(html)
            .and_then(|caps| caps.name("val").map(|m| html_to_text(m.as_str().trim())))
    }

    fn extract_updated_on_en(html: &str) -> Option<String> {
        // Best-effort extraction for "Updated on" -> value.
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r#"(?s)Updated on</div>\s*<div[^>]*>\s*(?P<val>[^<]{1,60})\s*</div>"#)
                .expect("valid regex")
        });
        re.captures(html)
            .and_then(|caps| caps.name("val").map(|m| html_to_text(m.as_str().trim())))
    }

    fn parse_updated_on_iso(updated_on: &str) -> Option<String> {
        // English-only, best-effort. Example: "Jan 10, 2026".
        // If parsing fails, keep the raw string only.
        let cleaned = updated_on.trim();
        let date = NaiveDate::parse_from_str(cleaned, "%b %d, %Y")
            .or_else(|_| NaiveDate::parse_from_str(cleaned, "%B %d, %Y"))
            .ok()?;
        Some(date.format("%Y-%m-%d").to_string())
    }

    fn app_from_html(id: &str, url: &str, html: &str) -> PlayStoreApp {
        let retrieved_at = Utc::now().to_rfc3339();
        let mut app = PlayStoreApp {
            id: id.to_string(),
            url: url.to_string(),
            retrieved_at,
            name: None,
            description: None,
            developer_name: None,
            developer_url: None,
            category: None,
            rating_value: None,
            rating_count: None,
            downloads: None,
            updated_on: None,
            updated_on_iso: None,
            image_url: None,
            content_rating: None,
        };

        if let Some(ld) = Self::parse_json_ld_software_application(html) {
            app.name = ld
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            app.description = ld
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            app.category = ld
                .get("applicationCategory")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            app.content_rating = ld
                .get("contentRating")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            app.image_url = ld
                .get("image")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(author) = ld.get("author") {
                app.developer_name = author
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                app.developer_url = author
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            if let Some(rating) = ld.get("aggregateRating") {
                app.rating_value = rating
                    .get("ratingValue")
                    .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok()));
                app.rating_count = rating.get("ratingCount").and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
                        .or_else(|| v.as_str()?.replace(',', "").parse::<u64>().ok())
                });
            }
        }

        // Best-effort HTML label extraction (English UI with hl=en works best).
        if let Some(downloads) = Self::extract_downloads_en(html) {
            app.downloads = Some(downloads);
        }
        if let Some(updated_on) = Self::extract_updated_on_en(html) {
            app.updated_on_iso = Self::parse_updated_on_iso(&updated_on);
            app.updated_on = Some(updated_on);
        }

        app
    }
}

#[async_trait]
impl Connector for PlayStoreConnector {
    fn name(&self) -> &'static str {
        "play-store"
    }

    fn description(&self) -> &'static str {
        "Best-effort Google Play Store app metadata via public HTML parsing."
    }

    fn display_name(&self) -> &'static str {
        "Play Store"
    }

    fn icon(&self) -> &'static str {
        "google_play"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["app_store", "metadata", "mobile"]
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?play\.google\.com/store/apps/details\?id=([^&\s#]+).*"
                .to_string(),
            default_tool: "app".to_string(),
            description: "Fetch Play Store app details by URL".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "id".to_string(),
                use_full_url: false,
            }],
        }]
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Play Store connector for public app listing metadata (best-effort).".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt '{}' not found",
            name
        )))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![Tool {
            name: Cow::Borrowed("app"),
            title: None,
            description: Some(Cow::Borrowed(
                "Fetch Play Store app metadata by package id. Best-effort scraping of public HTML.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Android package name (e.g., com.whatsapp)."
                    },
                    "hl": {
                        "type": "string",
                        "description": "UI language hint (best-effort; parsing is most reliable with 'en').",
                        "default": "en"
                    },
                    "gl": {
                        "type": "string",
                        "description": "Region hint (2-letter country code).",
                        "default": "US"
                    },
                    "output_format": {
                        "type": "string",
                        "enum": ["raw", "normalized_v1", "display_v1"],
                        "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                        "default": "raw"
                    }
                },
                "required": ["id"],
                "examples": [
                    { "description": "Fetch app metadata", "input": { "id": "com.whatsapp" } }
                ],
                "_meta": {
                    "category": "read",
                    "tags": ["mobile", "app_store", "metadata"],
                    "auth_required": false,
                    "supports_output_format": true,
                    "supports_cursor": false
                }
            })
            .as_object()
            .expect("Schema object")
            .clone()),
            output_schema: None,
            annotations: None,
            icons: None,
        }];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        match name {
            "app" | "details" => {
                let id = args.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
                    ConnectorError::InvalidParams("Missing 'id' parameter".into())
                })?;
                let id = Self::normalize_package_id(id)?;
                let hl = args
                    .get("hl")
                    .and_then(|v| v.as_str())
                    .unwrap_or(DEFAULT_HL);
                let gl = args
                    .get("gl")
                    .and_then(|v| v.as_str())
                    .unwrap_or(DEFAULT_GL);
                let output_format = crate::ingest::output_format_from_args(&args)?;

                let url = Self::build_app_url(&id, hl, gl);
                let html = self.fetch_html(&url, hl).await?;
                let app = Self::app_from_html(&id, &url, &html);

                if output_format == OutputFormat::NormalizedV1 {
                    let item_ref = format!("play-store:app:{}", id);
                    let title = app.name.clone().or_else(|| Some(id.clone()));

                    let mut summary = String::new();
                    if let Some(name) = &app.name {
                        summary.push_str(name);
                    } else {
                        summary.push_str(&id);
                    }
                    if let Some(rating) = app.rating_value {
                        summary.push_str(&format!(" — {:.1}★", rating));
                    }
                    if let Some(downloads) = &app.downloads {
                        summary.push_str(&format!(" — {} downloads", downloads));
                    }
                    if let Some(updated) = app.updated_on_iso.clone().or(app.updated_on.clone()) {
                        summary.push_str(&format!(" — updated {}", updated));
                    }
                    if let Some(dev) = &app.developer_name {
                        summary.push_str(&format!(" — {}", dev));
                    }

                    let authors = app
                        .developer_name
                        .as_ref()
                        .map(|name| {
                            vec![Author {
                                name: name.clone(),
                                id: None,
                            }]
                        })
                        .unwrap_or_default();

                    let blocks = vec![ContentBlock {
                        block_ref: format!("play-store:app:{}:summary", id),
                        block_kind: "summary".to_string(),
                        text: summary,
                        author: authors.first().cloned(),
                        created_at: None,
                        reply_to: None,
                        position: None,
                        score: None,
                        attachments: Vec::new(),
                        metadata: Some(serde_json::to_value(&app)?),
                    }];

                    let item = ContentItem {
                        item_ref,
                        kind: "app".to_string(),
                        canonical_url: Some(url),
                        title,
                        created_at: None,
                        source_updated_at: app.updated_on_iso.clone(),
                        authors,
                        tags: Vec::new(),
                        metadata: Some(serde_json::to_value(&app)?),
                        blocks,
                        relationships: Vec::new(),
                        truncation: Some(Truncation {
                            is_truncated: true,
                            reason: "Best-effort Play Store HTML parsing; fields may be missing."
                                .to_string(),
                            total_blocks_hint: None,
                            returned_blocks: 1,
                            policy: Some("best_effort_html".to_string()),
                        }),
                    };

                    let normalized = NormalizedItemV1::new(
                        item,
                        Partial::complete(None),
                        Source::new("play-store", "app"),
                    );
                    return structured_result(&normalized);
                }

                let text = if output_format == OutputFormat::DisplayV1 {
                    let mut summary = String::new();
                    if let Some(name) = &app.name {
                        summary.push_str(name);
                    } else {
                        summary.push_str(&app.id);
                    }
                    if let Some(dev) = &app.developer_name {
                        summary.push_str(&format!(" — {}", dev));
                    }
                    if let Some(rating) = app.rating_value {
                        summary.push_str(&format!(" — {:.1}★", rating));
                    }
                    if let Some(count) = app.rating_count {
                        summary.push_str(&format!(" ({} ratings)", count));
                    }
                    if let Some(downloads) = &app.downloads {
                        summary.push_str(&format!(" — {} downloads", downloads));
                    }
                    if let Some(updated) = app.updated_on_iso.clone().or(app.updated_on.clone()) {
                        summary.push_str(&format!(" — updated {}", updated));
                    }
                    Some(summary)
                } else {
                    Some(serde_json::to_string(&app)?)
                };
                Ok(structured_result_with_text(&app, text)?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_ld_and_labels() {
        let html = r#"
<!doctype html>
<html>
  <head>
    <script type="application/ld+json">
      {
        "@context": "https://schema.org",
        "@type": "SoftwareApplication",
        "name": "Example App",
        "applicationCategory": "TOOLS",
        "description": "An example app.",
        "image": "https://example.com/icon.png",
        "contentRating": "Everyone",
        "author": { "@type": "Person", "name": "Example Dev", "url": "https://dev.example" },
        "aggregateRating": { "@type": "AggregateRating", "ratingValue": "4.6", "ratingCount": "1234" }
      }
    </script>
  </head>
  <body>
    <div><div class="wVqUob"><div class="ClM7O">10K+</div><div class="g1rdde">Downloads</div></div></div>
    <div class=\"lXlx5\">Updated on</div><div class=\"xg1aie\">Jan 10, 2026</div>
  </body>
</html>
"#;

        let app = PlayStoreConnector::app_from_html(
            "com.example.app",
            "https://play.google.com/store/apps/details?id=com.example.app&hl=en&gl=US",
            html,
        );

        assert_eq!(app.name.as_deref(), Some("Example App"));
        assert_eq!(app.developer_name.as_deref(), Some("Example Dev"));
        assert_eq!(app.category.as_deref(), Some("TOOLS"));
        assert_eq!(app.downloads.as_deref(), Some("10K+"));
        assert_eq!(app.updated_on_iso.as_deref(), Some("2026-01-10"));
        assert_eq!(app.rating_value, Some(4.6));
        assert_eq!(app.rating_count, Some(1234));
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let html = r#"
<html><head>
<script type="application/ld+json">
{ "@context": "https://schema.org", "@type": "SoftwareApplication", "name": "X" }
</script>
</head><body></body></html>
"#;
        let app = PlayStoreConnector::app_from_html("x", "u", html);
        assert_eq!(app.name.as_deref(), Some("X"));
        assert!(app.developer_name.is_none());
        assert!(app.rating_value.is_none());
        assert!(app.downloads.is_none());
    }
}
