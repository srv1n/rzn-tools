use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::ingest::{
    self, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat, Partial,
    Source,
};
use crate::utils::{collect_paginated, structured_result_with_text, Page};
use crate::{auth::AuthDetails, Connector, URLParamExtraction, URLPatternSpec};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Client;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

/// Response format for controlling output verbosity
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    #[default]
    Concise,
    Detailed,
}

// Define the structs for search arguments
#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    response_format: ResponseFormat,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WikiSearchCursor {
    offset: u32,
    query: String,
}

#[derive(Debug, Deserialize)]
struct GeoSearchArgs {
    latitude: f64,
    longitude: f64,
    #[serde(default = "default_radius")]
    radius: u16,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct GetArticleArgs {
    title: Option<String>,
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    response_format: ResponseFormat,
    #[serde(default)]
    output_format: OutputFormat,
}

fn default_limit() -> u32 {
    10
}

fn default_radius() -> u16 {
    1000
}

// Define the Wikipedia connector
pub struct WikipediaConnector {
    client: Client,
    language: String,
    search_limit: u32,
}

const MAX_SEARCH_LIMIT: u32 = 5_000;
const MAX_SEARCH_REQUESTS: usize = 100;
const MAX_SR_LIMIT_PER_REQUEST: u32 = 50;

impl WikipediaConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let language = auth.get("language").unwrap_or(&"en".to_string()).clone();
        let search_limit = auth
            .get("search_limit")
            .and_then(|l| l.parse::<u32>().ok())
            .unwrap_or(10);

        Ok(WikipediaConnector {
            client,
            language,
            search_limit,
        })
    }

    // Helper method to get the base API URL
    fn base_url(&self) -> String {
        format!("https://{}.wikipedia.org/w/api.php", self.language)
    }

    // Helper method to format article content
    fn format_article(
        &self,
        title: &str,
        content: &str,
        summary: Option<&str>,
    ) -> HashMap<String, Value> {
        let mut result = HashMap::new();

        result.insert("title".to_string(), json!(title));
        result.insert("content".to_string(), json!(content));

        if let Some(summary) = summary {
            result.insert("summary".to_string(), json!(summary));
        }

        result
    }

    // Search for articles
    async fn search_articles(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<String>, ConnectorError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let desired_limit = limit.min(MAX_SEARCH_LIMIT) as usize;

        collect_paginated(
            desired_limit,
            MAX_SEARCH_REQUESTS,
            None::<u32>,
            |cursor, remaining| async move {
                let remaining_u32 = u32::try_from(remaining).unwrap_or(MAX_SR_LIMIT_PER_REQUEST);
                let srlimit = remaining_u32.clamp(1, MAX_SR_LIMIT_PER_REQUEST);

                let mut params: Vec<(String, String)> = vec![
                    ("list".to_string(), "search".to_string()),
                    ("srprop".to_string(), "".to_string()),
                    ("srlimit".to_string(), srlimit.to_string()),
                    ("srsearch".to_string(), query.to_string()),
                    ("format".to_string(), "json".to_string()),
                    ("action".to_string(), "query".to_string()),
                ];

                if let Some(o) = cursor {
                    params.push(("sroffset".to_string(), o.to_string()));
                }

                let response = self
                    .client
                    .get(self.base_url())
                    .query(&params)
                    .send()
                    .await
                    .map_err(ConnectorError::HttpRequest)?;

                let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;
                let items = extract_search_titles(&data)?;
                let next_cursor = extract_search_continue_offset(&data);

                Ok::<_, ConnectorError>(Page { items, next_cursor })
            },
            |t: &String| Some(t.clone()),
        )
        .await
    }

    async fn search_articles_page(
        &self,
        query: &str,
        limit: u32,
        cursor: Option<u32>,
    ) -> Result<(Vec<String>, Option<u32>), ConnectorError> {
        if limit == 0 {
            return Ok((Vec::new(), None));
        }

        let desired_limit = limit.min(MAX_SEARCH_LIMIT);
        let mut items: Vec<String> = Vec::new();
        let mut offset = cursor.unwrap_or(0);
        let mut next_cursor: Option<u32> = None;
        let mut remaining = desired_limit as usize;
        let mut attempts = 0usize;

        while remaining > 0 && attempts < MAX_SEARCH_REQUESTS {
            attempts += 1;
            let srlimit = (remaining as u32).clamp(1, MAX_SR_LIMIT_PER_REQUEST);

            let mut params: Vec<(String, String)> = vec![
                ("list".to_string(), "search".to_string()),
                ("srprop".to_string(), "".to_string()),
                ("srlimit".to_string(), srlimit.to_string()),
                ("srsearch".to_string(), query.to_string()),
                ("format".to_string(), "json".to_string()),
                ("action".to_string(), "query".to_string()),
            ];

            if offset > 0 {
                params.push(("sroffset".to_string(), offset.to_string()));
            }

            let response = self
                .client
                .get(self.base_url())
                .query(&params)
                .send()
                .await
                .map_err(ConnectorError::HttpRequest)?;

            let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;
            let mut batch = extract_search_titles(&data)?;
            next_cursor = extract_search_continue_offset(&data);

            items.append(&mut batch);
            if items.len() >= desired_limit as usize {
                items.truncate(desired_limit as usize);
                break;
            }

            remaining = desired_limit as usize - items.len();
            if let Some(next) = next_cursor {
                offset = next;
            } else {
                break;
            }
        }

        let has_more = next_cursor.is_some() && items.len() as u32 >= desired_limit;
        let next_cursor = if has_more { next_cursor } else { None };

        Ok((items, next_cursor))
    }

    // Geo search for articles
    async fn geo_search(
        &self,
        latitude: f64,
        longitude: f64,
        radius: u16,
    ) -> Result<Vec<String>, ConnectorError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(ConnectorError::InvalidParams(
                "latitude must be between -90 and 90".to_string(),
            ));
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(ConnectorError::InvalidParams(
                "longitude must be between -180 and 180".to_string(),
            ));
        }
        if !(10..=10000).contains(&radius) {
            return Err(ConnectorError::InvalidParams(
                "radius must be between 10 and 10000".to_string(),
            ));
        }

        let params = [
            ("list", "geosearch"),
            ("gsradius", &radius.to_string()),
            ("gscoord", &format!("{}|{}", latitude, longitude)),
            ("gslimit", &self.search_limit.to_string()),
            ("format", "json"),
            ("action", "query"),
        ];

        let response = self
            .client
            .get(self.base_url())
            .query(&params)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;

        let results = data
            .get("query")
            .and_then(|q| q.get("geosearch"))
            .and_then(|s| s.as_array())
            .ok_or_else(|| ConnectorError::Other("Invalid response format".to_string()))?;

        let titles = results
            .iter()
            .filter_map(|item| {
                item.get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(titles)
    }

    // Get article content
    async fn get_article_content(&self, title: &str) -> Result<String, ConnectorError> {
        let params = [
            ("prop", "extracts"),
            ("explaintext", ""),
            ("redirects", ""),
            ("titles", title),
            ("format", "json"),
            ("action", "query"),
        ];

        let response = self
            .client
            .get(self.base_url())
            .query(&params)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;

        let pages = data
            .get("query")
            .and_then(|q| q.get("pages"))
            .and_then(|p| p.as_object())
            .ok_or_else(|| ConnectorError::Other("Invalid response format".to_string()))?;

        // Get the first page (there should only be one)
        let page = pages
            .values()
            .next()
            .ok_or_else(|| ConnectorError::ResourceNotFound)?;

        // Check if the page has a "missing" field, which indicates the article doesn't exist
        if page.get("missing").is_some() {
            return Err(ConnectorError::ResourceNotFound);
        }

        // Try to get the extract, or return a default message if not found
        let content = page
            .get("extract")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("No content found for article: {}", title));

        Ok(content)
    }

    // Get article summary
    async fn get_article_summary(&self, title: &str) -> Result<String, ConnectorError> {
        let params = [
            ("prop", "extracts"),
            ("explaintext", ""),
            ("exintro", ""),
            ("redirects", ""),
            ("titles", title),
            ("format", "json"),
            ("action", "query"),
        ];

        let response = self
            .client
            .get(self.base_url())
            .query(&params)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;

        let pages = data
            .get("query")
            .and_then(|q| q.get("pages"))
            .and_then(|p| p.as_object())
            .ok_or_else(|| ConnectorError::Other("Invalid response format".to_string()))?;

        // Get the first page (there should only be one)
        let page = pages
            .values()
            .next()
            .ok_or_else(|| ConnectorError::ResourceNotFound)?;

        let summary = page
            .get("extract")
            .and_then(|e| e.as_str())
            .ok_or_else(|| ConnectorError::Other("No summary found".to_string()))?
            .to_string();

        Ok(summary)
    }
}

fn extract_search_titles(data: &Value) -> Result<Vec<String>, ConnectorError> {
    let results = data
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(|s| s.as_array())
        .ok_or_else(|| ConnectorError::Other("Invalid response format".to_string()))?;

    Ok(results
        .iter()
        .filter_map(|item| {
            item.get("title")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .collect())
}

fn extract_search_continue_offset(data: &Value) -> Option<u32> {
    data.get("continue")
        .and_then(|c| c.get("sroffset"))
        .and_then(|o| o.as_u64())
        .and_then(|o| u32::try_from(o).ok())
}

fn wiki_item_ref(title: &str) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(title.as_bytes());
    format!("wikipedia:article:{}", encoded)
}

fn wiki_title_from_item_ref(item_ref: &str) -> Option<String> {
    let (kind, id) = ingest::parse_item_ref_for_connector(item_ref, "wikipedia")?;
    if kind != "article" {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(id).ok()?;
    String::from_utf8(bytes).ok()
}

fn wiki_canonical_url(title: &str, language: &str) -> String {
    let slug = title.trim().replace(' ', "_");
    format!("https://{}.wikipedia.org/wiki/{}", language, slug)
}

fn resolve_wikipedia_title(
    title: Option<String>,
    item_ref: Option<String>,
    url: Option<String>,
) -> Result<String, ConnectorError> {
    if let Some(title) = title {
        if !title.trim().is_empty() {
            return Ok(title);
        }
    }
    if let Some(item_ref) = item_ref {
        if let Some(title) = wiki_title_from_item_ref(&item_ref) {
            return Ok(title);
        }
    }
    if let Some(url) = url {
        if let Some(title) = extract_title_from_url(&url) {
            return Ok(title);
        }
    }
    Err(ConnectorError::InvalidParams(
        "Missing article identifier: provide title, item_ref, or url".to_string(),
    ))
}

fn extract_title_from_url(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split("/wiki/").collect();
    let slug = parts.get(1)?.trim();
    if slug.is_empty() {
        return None;
    }
    Some(slug.replace('_', " "))
}

fn wiki_item_from_title(title: &str, language: &str) -> ContentItem {
    ContentItem {
        item_ref: wiki_item_ref(title),
        kind: "article".to_string(),
        canonical_url: Some(wiki_canonical_url(title, language)),
        title: Some(title.to_string()),
        created_at: None,
        source_updated_at: None,
        authors: Vec::new(),
        tags: Vec::new(),
        metadata: None,
        blocks: Vec::new(),
        relationships: Vec::new(),
        truncation: None,
    }
}

fn wiki_item_with_content(
    title: &str,
    content: &str,
    summary: Option<&str>,
    language: &str,
) -> (ContentItem, Partial) {
    let item_ref = wiki_item_ref(title);
    let mut blocks = Vec::new();
    if let Some(summary_text) = summary {
        blocks.push(ContentBlock {
            block_ref: format!("{}:summary", item_ref),
            block_kind: "summary".to_string(),
            text: summary_text.to_string(),
            author: None,
            created_at: None,
            reply_to: None,
            position: None,
            score: None,
            attachments: Vec::new(),
            metadata: None,
        });
    }
    blocks.push(ContentBlock {
        block_ref: format!("{}:content", item_ref),
        block_kind: "content".to_string(),
        text: content.to_string(),
        author: None,
        created_at: None,
        reply_to: None,
        position: None,
        score: None,
        attachments: Vec::new(),
        metadata: None,
    });

    let item = ContentItem {
        item_ref,
        kind: "article".to_string(),
        canonical_url: Some(wiki_canonical_url(title, language)),
        title: Some(title.to_string()),
        created_at: None,
        source_updated_at: None,
        authors: Vec::new(),
        tags: Vec::new(),
        metadata: None,
        blocks,
        relationships: Vec::new(),
        truncation: None,
    };

    (item, Partial::complete(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_titles_and_continue_offset() {
        let data = json!({
            "continue": { "sroffset": 50, "continue": "-||" },
            "query": {
                "search": [
                    { "title": "A" },
                    { "title": "B" }
                ]
            }
        });

        let titles = extract_search_titles(&data).unwrap();
        assert_eq!(titles, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(extract_search_continue_offset(&data), Some(50));
    }
}

#[async_trait]
impl Connector for WikipediaConnector {
    fn name(&self) -> &'static str {
        "wikipedia"
    }

    fn description(&self) -> &'static str {
        "A connector for searching and retrieving content from Wikipedia."
    }

    fn display_name(&self) -> &'static str {
        "Wikipedia"
    }

    fn icon(&self) -> &'static str {
        "wikipedia"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["reference", "encyclopedia"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?[a-z]+\.wikipedia\.org/wiki/([^#?]+)".to_string(),
            default_tool: "get".to_string(),
            description: "Fetch article by title".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "title".to_string(),
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
        let mut auth = AuthDetails::new();
        auth.insert("language".to_string(), self.language.clone());
        auth.insert("search_limit".to_string(), self.search_limit.to_string());
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        if let Some(language) = details.get("language") {
            self.language = language.clone();
        }

        if let Some(limit) = details
            .get("search_limit")
            .and_then(|l| l.parse::<u32>().ok())
        {
            self.search_limit = limit;
        }

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Simple test to check if the API is accessible
        tracing::debug!("Testing Wikipedia connector auth");
        self.search_articles("test", 1).await?;
        tracing::debug!("Wikipedia auth test succeeded");
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "language".to_string(),
                    label: "Language".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Wikipedia language code (e.g., 'en' for English, 'es' for Spanish)"
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "search_limit".to_string(),
                    label: "Search Results Limit".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some("Maximum number of search results to return".to_string()),
                    options: None,
                },
            ],
        }
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
            instructions: Some("MCP connector for various data sources".to_string()),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let resources = vec![Resource {
            raw: RawResource {
                uri: "wikipedia://article/{title}".to_string(),
                name: "Wikipedia Article".to_string(),
                title: None,
                description: Some("Represents a Wikipedia article.".to_string()),
                mime_type: Some("application/vnd.wikipedia.article+json".to_string()),
                size: None,
                icons: None,
            },
            annotations: None,
        }];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        let uri_str = request.uri.as_str();

        if uri_str.starts_with("wikipedia://article/") {
            let parts: Vec<&str> = uri_str.split('/').collect();
            if parts.len() < 4 {
                return Err(ConnectorError::InvalidInput(format!(
                    "Invalid resource URI: {}",
                    uri_str
                )));
            }
            let title = parts[3];

            let content = self.get_article_content(title).await?;
            let article_data = self.format_article(title, &content, None);
            let _json_content = serde_json::to_string(&article_data)?;

            let content_text = serde_json::to_string(&article_data)?;
            Ok(vec![ResourceContents::text(content_text, uri_str)])
        } else {
            Err(ConnectorError::ResourceNotFound)
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools =
            vec![
            Tool {
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search article titles by keyword. Use when you need candidate titles to \
pass into get. Example: query=\"rust language\" limit=5.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query (e.g., 'quantum computing')"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 10)"
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["concise", "detailed"],
                            "description": "Response verbosity: 'concise' returns only article titles, 'detailed' includes query metadata",
                            "default": "concise"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "default": "raw",
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                        }
                    },
                    "required": ["query"],
                    "examples": [
                        {
                            "description": "Find candidate titles",
                            "input": { "query": "rust language", "limit": 5 }
                        },
                        {
                            "description": "Search a concept",
                            "input": { "query": "large language model", "limit": 5 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["reference", "encyclopedia"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("geosearch"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Find articles near a lat/lon. Use when location is the primary key. \
Example: latitude=37.77 longitude=-122.42 radius=1000.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "latitude": {
                            "type": "number",
                            "description": "Latitude coordinate."
                        },
                        "longitude": {
                            "type": "number",
                            "description": "Longitude coordinate."
                        },
                        "radius": {
                            "type": "integer",
                            "description": "Search radius in meters (default: 1000)."
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "default": "raw",
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                        }
                    },
                    "required": ["latitude", "longitude"],
                    "examples": [
                        {
                            "description": "Find articles near San Francisco",
                            "input": { "latitude": 37.7749, "longitude": -122.4194, "radius": 1000 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["reference", "geography"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get article content by exact title. Use response_format='concise' to keep \
tokens down. Example: title=\"Rust (programming language)\".",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": {
                            "type": "string",
                            "description": "Normalized item_ref (e.g., wikipedia:article:<encoded_title>)."
                        },
                        "url": {
                            "type": "string",
                            "description": "Canonical article URL (e.g., https://en.wikipedia.org/wiki/Rust_(programming_language))."
                        },
                        "title": {
                            "type": "string",
                            "description": "The title of the article (e.g., 'Rust (programming language)')"
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["concise", "detailed"],
                            "description": "Response verbosity: 'concise' returns only title and summary (first paragraph), 'detailed' includes full content",
                            "default": "concise"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "default": "raw",
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                        }
                    },
                    "examples": [
                        {
                            "description": "Get a specific article",
                            "input": { "title": "Rust (programming language)", "response_format": "concise" }
                        },
                        {
                            "description": "Fetch by canonical URL",
                            "input": { "url": "https://en.wikipedia.org/wiki/Rust_(programming_language)" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["reference", "encyclopedia"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

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
            "search" => {
                let args: SearchArgs = serde_json::from_value(json!(args)).map_err(|e| {
                    ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                })?;

                if args.output_format == OutputFormat::NormalizedV1 {
                    let cursor_str = args.cursor.as_deref();
                    let cursor = cursor_str.and_then(ingest::decode_cursor::<WikiSearchCursor>);
                    if cursor_str.is_some() && cursor.is_none() {
                        return Err(ConnectorError::InvalidParams("Invalid cursor".to_string()));
                    }
                    if let Some(ref c) = cursor {
                        if c.query != args.query {
                            return Err(ConnectorError::InvalidParams(
                                "Cursor does not match query".to_string(),
                            ));
                        }
                    }
                    let offset = cursor.map(|c| c.offset);
                    let (results, next_offset) = self
                        .search_articles_page(&args.query, args.limit, offset)
                        .await?;
                    let items = results
                        .into_iter()
                        .map(|title| wiki_item_from_title(&title, &self.language))
                        .collect::<Vec<_>>();
                    let next_cursor = next_offset
                        .map(|next| {
                            ingest::encode_cursor(&WikiSearchCursor {
                                offset: next,
                                query: args.query.clone(),
                            })
                        })
                        .transpose()?;
                    let has_more = next_cursor.is_some();
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(args.limit as u64))),
                        Source::new("wikipedia", "search"),
                    );
                    return crate::utils::structured_result(&page);
                }

                let results = self.search_articles(&args.query, args.limit).await?;

                // Return concise or detailed based on response_format
                let data = if args.response_format == ResponseFormat::Concise {
                    json!({ "results": results })
                } else {
                    json!({
                        "query": args.query,
                        "limit": args.limit,
                        "results": results,
                        "count": results.len()
                    })
                };
                let text = serde_json::to_string(&data)?;
                Ok(structured_result_with_text(&data, Some(text))?)
            }
            "geosearch" => {
                let args: GeoSearchArgs = serde_json::from_value(json!(args)).map_err(|e| {
                    ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                })?;

                let results = self
                    .geo_search(args.latitude, args.longitude, args.radius)
                    .await?;

                if args.output_format == OutputFormat::NormalizedV1 {
                    let items = results
                        .into_iter()
                        .map(|title| wiki_item_from_title(&title, &self.language))
                        .collect::<Vec<_>>();
                    let page = NormalizedPageV1::new(
                        items,
                        None,
                        false,
                        Partial::complete(None),
                        Source::new("wikipedia", "geosearch"),
                    );
                    return crate::utils::structured_result(&page);
                }

                let data = json!({
                    "latitude": args.latitude,
                    "longitude": args.longitude,
                    "radius": args.radius,
                    "results": results,
                    "count": results.len()
                });
                let text = serde_json::to_string(&data)?;
                Ok(structured_result_with_text(&data, Some(text))?)
            }
            "get" | "get_article" => {
                let args: GetArticleArgs = serde_json::from_value(json!(args)).map_err(|e| {
                    ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                })?;

                let title = resolve_wikipedia_title(args.title, args.item_ref, args.url)?;

                match self.get_article_content(&title).await {
                    Ok(content) => {
                        let summary = self.get_article_summary(&title).await.ok();

                        if args.output_format == OutputFormat::NormalizedV1 {
                            let (item, partial) = wiki_item_with_content(
                                &title,
                                &content,
                                summary.as_deref(),
                                &self.language,
                            );
                            let normalized = NormalizedItemV1::new(
                                item,
                                partial,
                                Source::new("wikipedia", "get"),
                            );
                            return crate::utils::structured_result(&normalized);
                        }

                        // Return concise or detailed based on response_format
                        let article_data = if args.response_format == ResponseFormat::Concise {
                            // Concise: just title and summary (first paragraph)
                            let mut result = HashMap::new();
                            result.insert("title".to_string(), json!(title));
                            if let Some(ref s) = summary {
                                result.insert("summary".to_string(), json!(s));
                            }
                            result
                        } else {
                            self.format_article(&title, &content, summary.as_deref())
                        };
                        let text = serde_json::to_string(&article_data)?;
                        Ok(structured_result_with_text(&article_data, Some(text))?)
                    }
                    Err(ConnectorError::ResourceNotFound) => {
                        let payload = json!({
                            "title": title,
                            "content": serde_json::Value::Null,
                            "summary": serde_json::Value::Null,
                        });
                        let text = serde_json::to_string(&payload)?;
                        Ok(structured_result_with_text(&payload, Some(text))?)
                    }
                    Err(err) => Err(err),
                }
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let prompts = vec![Prompt {
            name: "summarize_article".to_string(),
            title: None,
            description: Some("Summarizes a Wikipedia article.".to_string()),
            arguments: Some(vec![PromptArgument {
                name: "title".to_string(),
                title: None,
                description: Some("The title of the article to summarize.".to_string()),
                required: Some(true),
            }]),
            icons: None,
        }];

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        match name {
            "summarize_article" => Ok(Prompt {
                name: "summarize_article".to_string(),
                title: None,
                description: Some("Summarizes a Wikipedia article.".to_string()),
                arguments: Some(vec![PromptArgument {
                    name: "title".to_string(),
                    title: None,
                    description: Some("The title of the article to summarize.".to_string()),
                    required: Some(true),
                }]),
                icons: None,
            }),
            _ => Err(ConnectorError::InvalidParams(format!(
                "Prompt with name {} not found",
                name
            ))),
        }
    }
}
