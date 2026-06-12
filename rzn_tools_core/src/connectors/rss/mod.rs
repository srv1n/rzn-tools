use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedPageV1, OutputFormat, Partial, Source,
};
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use feed_rs::parser;
use reqwest::Client;
use rmcp::model::*;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct GetFeedArgs {
    url: String,
    limit: Option<usize>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct ListEntriesArgs {
    url: String,
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct SearchFeedArgs {
    url: String,
    query: String,
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct DiscoverFeedsArgs {
    url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RssCursor {
    url: String,
    query: Option<String>,
    offset: usize,
}

pub struct RssConnector {
    client: Client,
}

impl RssConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self {
            client: Client::builder()
                .user_agent("rzn-tools-rss-connector/0.1.0")
                .build()
                .map_err(ConnectorError::HttpRequest)?,
        })
    }

    async fn fetch_and_parse(&self, url: &str) -> Result<feed_rs::model::Feed, ConnectorError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Failed to fetch feed: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let cursor = Cursor::new(bytes);

        parser::parse(cursor)
            .map_err(|e| ConnectorError::Other(format!("Failed to parse feed: {}", e)))
    }
}

fn rss_item_ref(id: &str) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(id.as_bytes());
    format!("rss:entry:{}", encoded)
}

fn rss_item_from_entry(entry: &feed_rs::model::Entry, feed_url: &str) -> ContentItem {
    let base_id = if !entry.id.is_empty() {
        entry.id.clone()
    } else {
        entry
            .links
            .first()
            .map(|l| l.href.clone())
            .unwrap_or_else(|| feed_url.to_string())
    };
    let item_ref = rss_item_ref(&base_id);
    let canonical_url = entry.links.first().map(|l| l.href.clone());
    let title = entry.title.as_ref().map(|t| t.content.clone());
    let authors = entry
        .authors
        .iter()
        .map(|a| Author {
            name: a.name.clone(),
            id: None,
        })
        .collect::<Vec<_>>();
    let summary = entry.summary.as_ref().map(|s| s.content.clone());
    let content = entry
        .content
        .as_ref()
        .and_then(|c| c.body.clone())
        .unwrap_or_default();

    let mut blocks = Vec::new();
    if let Some(summary_text) = summary.clone() {
        blocks.push(ContentBlock {
            block_ref: format!("{}:summary", item_ref),
            block_kind: "summary".to_string(),
            text: summary_text,
            author: None,
            created_at: entry.published.map(|d| d.to_rfc3339()),
            reply_to: None,
            position: None,
            score: None,
            attachments: Vec::new(),
            metadata: None,
        });
    } else if !content.trim().is_empty() {
        blocks.push(ContentBlock {
            block_ref: format!("{}:content", item_ref),
            block_kind: "content".to_string(),
            text: content,
            author: None,
            created_at: entry.published.map(|d| d.to_rfc3339()),
            reply_to: None,
            position: None,
            score: None,
            attachments: Vec::new(),
            metadata: None,
        });
    }

    ContentItem {
        item_ref,
        kind: "entry".to_string(),
        canonical_url,
        title,
        created_at: entry.published.map(|d| d.to_rfc3339()),
        source_updated_at: entry.updated.map(|d| d.to_rfc3339()),
        authors,
        tags: Vec::new(),
        metadata: Some(json!({
            "feed_url": feed_url,
            "summary": summary
        })),
        blocks,
        relationships: Vec::new(),
        truncation: None,
    }
}

#[async_trait]
impl Connector for RssConnector {
    fn name(&self) -> &'static str {
        "rss"
    }

    fn description(&self) -> &'static str {
        "Fetch and parse RSS/Atom feeds"
    }

    fn display_name(&self) -> &'static str {
        "RSS"
    }

    fn icon(&self) -> &'static str {
        "rss"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["feeds", "news", "web"]
    }

    fn requires_auth(&self) -> bool {
        false
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
        ConnectorConfigSchema { fields: Vec::new() }
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
            instructions: Some("Fetch and read RSS/Atom/JSON feeds.".to_string()),
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("get_feed"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a feed (metadata + recent entries). Use when you have a feed URL. \
Example: url=\"https://www.nasa.gov/rss/dyn/breaking_news.rss\" limit=5.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "URL of the RSS/Atom feed"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Number of entries to return (default: 5)"
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            }
                        },
                        "required": ["url"],
                        "examples": [
                            { "description": "Fetch feed metadata", "input": { "url": "https://www.nasa.gov/rss/dyn/breaking_news.rss", "limit": 5 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["feeds", "rss"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": false
                        }
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_entries"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List recent entries from a feed. Use when you don't need full metadata. \
Example: url=\"https://example.com/feed.xml\" limit=10.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "URL of the RSS/Atom feed"
                            },
                            "cursor": {
                                "type": ["string", "null"],
                                "description": "Opaque cursor from a previous normalized response."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Number of entries to return (default: 10)"
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            }
                        },
                        "required": ["url"],
                        "examples": [
                            { "description": "List entries", "input": { "url": "https://example.com/feed.xml", "limit": 10 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["feeds", "rss"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_feed"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search a feed's entries by keyword. Use when you have a feed URL and \
want matching items. Example: url=\"https://example.com/feed.xml\" query=\"rust\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "URL of the RSS/Atom feed"
                            },
                            "query": {
                                "type": "string",
                                "description": "Keyword to search for in entry titles or summaries"
                            },
                            "cursor": {
                                "type": ["string", "null"],
                                "description": "Opaque cursor from a previous normalized response."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Number of matching entries to return (default: 10)"
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            }
                        },
                        "required": ["url", "query"],
                        "examples": [
                            { "description": "Search entries", "input": { "url": "https://example.com/feed.xml", "query": "rust", "limit": 5 } }
                        ],
                        "_meta": {
                            "category": "search",
                            "tags": ["feeds", "rss"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("discover_feeds"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Discover RSS/Atom feeds linked from a webpage. Use when you have a site \
URL, not a feed URL. Example: url=\"https://blog.rust-lang.org\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "URL of the webpage to inspect"
                            }
                        },
                        "required": ["url"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
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
        match request.name.as_ref() {
            "get_feed" => {
                let args: GetFeedArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let feed = self.fetch_and_parse(&args.url).await?;
                let limit = args.limit.unwrap_or(5);

                if args.output_format == OutputFormat::NormalizedV1 {
                    let items = feed
                        .entries
                        .iter()
                        .take(limit)
                        .map(|e| rss_item_from_entry(e, &args.url))
                        .collect::<Vec<_>>();
                    let page = NormalizedPageV1::new(
                        items,
                        None,
                        false,
                        Partial::complete(Some(ingest::limits_max_items(limit as u64))),
                        Source::new("rss", "get_feed"),
                    );
                    return crate::utils::structured_result(&page);
                }

                // Convert feed-rs model to JSON
                // We'll construct a simplified version to avoid huge blobs
                let entries: Vec<Value> = feed
                    .entries
                    .iter()
                    .take(limit)
                    .map(|e| {
                        json!({
                            "id": e.id,
                            "title": e.title.as_ref().map(|t| t.content.clone()),
                            "link": e.links.first().map(|l| l.href.clone()),
                            "published": e.published.map(|d| d.to_rfc3339()),
                            "summary": e.summary.as_ref().map(|s| s.content.clone()),
                        })
                    })
                    .collect();

                let data = json!({
                    "title": feed.title.as_ref().map(|t| t.content.clone()),
                    "description": feed.description.as_ref().map(|d| d.content.clone()),
                    "link": feed.links.first().map(|l| l.href.clone()),
                    "entries_count": feed.entries.len(),
                    "entries": entries // First 5
                });

                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "list_entries" => {
                let args: ListEntriesArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let feed = self.fetch_and_parse(&args.url).await?;
                let limit = args.limit.unwrap_or(10);

                if args.output_format == OutputFormat::NormalizedV1 {
                    let cursor_str = args.cursor.as_deref();
                    let cursor = cursor_str.and_then(ingest::decode_cursor::<RssCursor>);
                    if cursor_str.is_some() && cursor.is_none() {
                        return Err(ConnectorError::InvalidParams("Invalid cursor".to_string()));
                    }
                    if let Some(ref c) = cursor {
                        if c.url != args.url || c.query.is_some() {
                            return Err(ConnectorError::InvalidParams(
                                "Cursor does not match feed URL".to_string(),
                            ));
                        }
                    }
                    let offset = cursor.map(|c| c.offset).unwrap_or(0);
                    let items = feed
                        .entries
                        .iter()
                        .skip(offset)
                        .take(limit)
                        .map(|e| rss_item_from_entry(e, &args.url))
                        .collect::<Vec<_>>();
                    let next_offset = offset.saturating_add(limit);
                    let next_cursor = if next_offset < feed.entries.len() {
                        Some(ingest::encode_cursor(&RssCursor {
                            url: args.url.clone(),
                            query: None,
                            offset: next_offset,
                        })?)
                    } else {
                        None
                    };
                    let has_more = next_cursor.is_some();
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(limit as u64))),
                        Source::new("rss", "list_entries"),
                    );
                    return crate::utils::structured_result(&page);
                }

                let entries: Vec<Value> = feed.entries.iter().take(limit).map(|e| {
                    json!({
                        "id": e.id,
                        "title": e.title.as_ref().map(|t| t.content.clone()),
                        "link": e.links.first().map(|l| l.href.clone()),
                        "published": e.published.map(|d| d.to_rfc3339()),
                        "updated": e.updated.map(|d| d.to_rfc3339()),
                        "summary": e.summary.as_ref().map(|s| s.content.clone()),
                        "content": e.content.as_ref().map(|c| c.body.clone().unwrap_or_default()),
                        "authors": e.authors.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
                    })
                }).collect();

                let data = json!({
                    "url": args.url,
                    "count": entries.len(),
                    "entries": entries
                });

                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "search_feed" => {
                let args: SearchFeedArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let feed = self.fetch_and_parse(&args.url).await?;
                let limit = args.limit.unwrap_or(10);
                let query_lower = args.query.to_lowercase();

                let matching_entries: Vec<_> = feed
                    .entries
                    .iter()
                    .filter(|e| {
                        let title_match = e
                            .title
                            .as_ref()
                            .is_some_and(|t| t.content.to_lowercase().contains(&query_lower));
                        let summary_match = e
                            .summary
                            .as_ref()
                            .is_some_and(|s| s.content.to_lowercase().contains(&query_lower));
                        title_match || summary_match
                    })
                    .collect();

                if args.output_format == OutputFormat::NormalizedV1 {
                    let cursor_str = args.cursor.as_deref();
                    let cursor = cursor_str.and_then(ingest::decode_cursor::<RssCursor>);
                    if cursor_str.is_some() && cursor.is_none() {
                        return Err(ConnectorError::InvalidParams("Invalid cursor".to_string()));
                    }
                    if let Some(ref c) = cursor {
                        if c.url != args.url || c.query.as_deref() != Some(args.query.as_str()) {
                            return Err(ConnectorError::InvalidParams(
                                "Cursor does not match feed URL/query".to_string(),
                            ));
                        }
                    }
                    let offset = cursor.map(|c| c.offset).unwrap_or(0);
                    let items = matching_entries
                        .iter()
                        .skip(offset)
                        .take(limit)
                        .map(|e| rss_item_from_entry(e, &args.url))
                        .collect::<Vec<_>>();
                    let next_offset = offset.saturating_add(limit);
                    let next_cursor = if next_offset < matching_entries.len() {
                        Some(ingest::encode_cursor(&RssCursor {
                            url: args.url.clone(),
                            query: Some(args.query.clone()),
                            offset: next_offset,
                        })?)
                    } else {
                        None
                    };
                    let has_more = next_cursor.is_some();
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(limit as u64))),
                        Source::new("rss", "search_feed"),
                    );
                    return crate::utils::structured_result(&page);
                }

                let entries: Vec<Value> = matching_entries
                    .iter()
                    .take(limit)
                    .map(|e| {
                        json!({
                            "id": e.id,
                            "title": e.title.as_ref().map(|t| t.content.clone()),
                            "link": e.links.first().map(|l| l.href.clone()),
                            "published": e.published.map(|d| d.to_rfc3339()),
                            "updated": e.updated.map(|d| d.to_rfc3339()),
                            "summary": e.summary.as_ref().map(|s| s.content.clone()),
                            "content": e.content.as_ref().map(|c| c.body.clone().unwrap_or_default()),
                            "authors": e.authors.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
                        })
                    })
                    .collect();

                let data = json!({
                    "url": args.url,
                    "query": args.query,
                    "count": entries.len(),
                    "results": entries
                });

                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "discover_feeds" => {
                let args: DiscoverFeedsArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let response = self
                    .client
                    .get(&args.url)
                    .send()
                    .await
                    .map_err(ConnectorError::HttpRequest)?;

                if !response.status().is_success() {
                    return Err(ConnectorError::Other(format!(
                        "Failed to fetch webpage: {}",
                        response.status()
                    )));
                }

                let html_content = response.text().await.map_err(ConnectorError::HttpRequest)?;
                let document = Html::parse_document(&html_content);

                let selector = Selector::parse("link[rel='alternate'][type*='rss'], link[rel='alternate'][type*='atom'], link[rel='alternate'][type*='json']").unwrap();

                let mut feeds = Vec::new();
                for element in document.select(&selector) {
                    if let Some(href) = element.value().attr("href") {
                        feeds.push(json!({
                            "url": href,
                            "title": element.value().attr("title"),
                            "type": element.value().attr("type"),
                        }));
                    }
                }

                let data = json!({
                    "searched_url": args.url,
                    "found_feeds": feeds,
                });

                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
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
}
