use async_trait::async_trait;
use roux::subreddit::response::AccountsActive;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use chrono;
use reqwest;
use roux::{Reddit, Subreddit, User};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Relationship, Source, Truncation,
};
use crate::utils::{
    collect_paginated_with_cursor, html_to_text, structured_result, structured_result_with_text,
    Page,
};
use crate::Connector;
use crate::{URLParamExtraction, URLPatternSpec};
use rmcp::model::*;

pub struct RedditConnector {
    client: Option<Reddit>,
    http_client: reqwest::Client,
    api_base_url: String,
}

const REDDIT_USER_AGENT: &str = "rzn-tools/0.1.0";
const REDDIT_CANONICAL_BASE_URL: &str = "https://www.reddit.com";
const REDDIT_OLD_BASE_URL: &str = "https://old.reddit.com";
const DEFAULT_COMMENT_LIMIT: u32 = 25;
// Soft limit to prevent runaway fetches when callers pass extremely large values.
const MAX_COMMENT_LIMIT: u32 = 5_000;
const MAX_SEARCH_LIMIT: u32 = 5_000;
const SEARCH_PAGE_SIZE_MAX: usize = 100;
const MAX_SEARCH_REQUESTS: usize = 50;
const MAX_LIST_LIMIT: u32 = 5_000;
const LIST_PAGE_SIZE_MAX: usize = 100;
const MAX_LIST_REQUESTS: usize = 50;
const MORECHILDREN_BATCH_SIZE: usize = 100;
const MAX_MORECHILDREN_REQUESTS: usize = 100;
const MAX_TOTAL_COMMENTS: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedditSearchCursor {
    after: String,
    count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedditListingCursor {
    after: String,
    count: usize,
}

struct RedditPostTarget {
    post_id: String,
    subreddit: Option<String>,
}

impl RedditConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let http_client = Self::reqwest_client(None, REDDIT_USER_AGENT)?;
        let mut connector = RedditConnector {
            client: None,
            http_client,
            api_base_url: REDDIT_CANONICAL_BASE_URL.to_string(),
        };
        connector.set_auth_details(auth).await?;

        Ok(connector)
    }

    fn reqwest_client(
        proxy_url: Option<&str>,
        user_agent: &str,
    ) -> Result<reqwest::Client, ConnectorError> {
        let mut builder = reqwest::Client::builder().user_agent(user_agent);
        if let Some(url) = proxy_url {
            let proxy = reqwest::Proxy::all(url).map_err(|e| {
                ConnectorError::InvalidParams(format!("Invalid proxy_url '{}': {}", url, e))
            })?;
            builder = builder.proxy(proxy);
        }
        builder
            .build()
            .map_err(|e| ConnectorError::Other(format!("Failed to build HTTP client: {}", e)))
    }
}

#[async_trait]
impl Connector for RedditConnector {
    fn name(&self) -> &'static str {
        "reddit"
    }

    fn description(&self) -> &'static str {
        "A connector for interacting with Reddit using the roux crate."
    }

    fn display_name(&self) -> &'static str {
        "Reddit"
    }

    fn icon(&self) -> &'static str {
        "reddit"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["social", "forum", "community"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![
            URLPatternSpec {
                pattern: r"(?:https?://)?(?:www\.)?reddit\.com/r/[^/]+/comments/[^/]+/[^/]*"
                    .to_string(),
                default_tool: "get".to_string(),
                description: "Fetch a Reddit thread by URL".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "post_url".to_string(),
                    use_full_url: true,
                }],
            },
            URLPatternSpec {
                pattern: r"(?:https?://)?(?:www\.)?reddit\.com/user/([^/]+)/?$".to_string(),
                default_tool: "user".to_string(),
                description: "Fetch a Reddit user profile by URL".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "username".to_string(),
                    use_full_url: false,
                }],
            },
        ]
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

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let proxy_url = details.get("proxy_url").map(String::as_str);
        self.api_base_url = Self::resolve_api_base_url(&details)?;

        // Check if we have credentials
        if let (Some(username), Some(password), Some(client_id), Some(client_secret)) = (
            details.get("username"),
            details.get("password"),
            details.get("client_id"),
            details.get("client_secret"),
        ) {
            let ua = format!("{} (by /u/{})", REDDIT_USER_AGENT, username);
            self.http_client = Self::reqwest_client(proxy_url, &ua)?;

            // Authenticated client
            let client_builder = Reddit::new(&ua, client_id, client_secret)
                .username(username)
                .password(password);

            // We'll store the client builder, not the authenticated client
            self.client = Some(client_builder.clone());

            // Test the authentication
            let me = client_builder
                .login()
                .await
                .map_err(|e| ConnectorError::Other(format!("Failed to authenticate: {}", e)))?;

            // Just to verify it works, we don't need to store the result
            match me.me().await {
                Ok(user) => tracing::debug!(user = %user.id, "Reddit authentication succeeded"),
                Err(e) => tracing::warn!(error = %e, "Reddit authentication verification failed"),
            }
        } else {
            // Anonymous client - no login needed
            let ua = format!("{} (anonymous)", REDDIT_USER_AGENT);
            self.http_client = Self::reqwest_client(proxy_url, &ua)?;
            let client = Reddit::new(
                &ua,
                "CLIENT_ID_NOT_NEEDED_FOR_ANONYMOUS",
                "CLIENT_SECRET_NOT_NEEDED_FOR_ANONYMOUS",
            );

            self.client = Some(client);
        }

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test by fetching a known user
        let _about = self.fetch_user_about_json("spez").await?;

        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "username".to_string(),
                    field_type: FieldType::Text,
                    description: Some(
                        "Reddit username (optional for anonymous access)".to_string(),
                    ),
                    required: false,
                    label: "Username".to_string(),
                    options: None,
                },
                Field {
                    name: "password".to_string(),
                    field_type: FieldType::Secret,
                    description: Some(
                        "Reddit password (optional for anonymous access)".to_string(),
                    ),
                    required: false,
                    label: "Password".to_string(),
                    options: None,
                },
                Field {
                    name: "client_id".to_string(),
                    field_type: FieldType::Text,
                    description: Some(
                        "Reddit API client ID (optional for anonymous access)".to_string(),
                    ),
                    required: false,
                    label: "Client ID".to_string(),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    field_type: FieldType::Secret,
                    description: Some(
                        "Reddit API client secret (optional for anonymous access)".to_string(),
                    ),
                    required: false,
                    label: "Client Secret".to_string(),
                    options: None,
                },
                Field {
                    name: "proxy_url".to_string(),
                    field_type: FieldType::Secret,
                    description: Some(
                        "Optional proxy URL (http(s)://host:port, may include user:pass)"
                            .to_string(),
                    ),
                    required: false,
                    label: "Proxy URL".to_string(),
                    options: None,
                },
                Field {
                    name: "api_base_url".to_string(),
                    field_type: FieldType::Text,
                    description: Some(
                        "Optional Reddit JSON base URL, e.g. https://www.reddit.com or https://old.reddit.com"
                            .to_string(),
                    ),
                    required: false,
                    label: "API Base URL".to_string(),
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
            instructions: Some(
                "Reddit connector for accessing posts, users, and subreddit data".to_string(),
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

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        // Keep the surface small to reduce ambiguity and context bloat for agents.
        // Back-compat: legacy tools are still accepted in call_tool(), but not listed here.
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List posts from a subreddit feed (hot/new/top). Use this for browsing a subreddit, not keyword search. Example: subreddit=\"rust\" sort=\"top\" time=\"week\" limit=10.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "subreddit": {
                            "type": "string",
                            "description": "Subreddit name, with or without r/ prefix (e.g., \"rust\" or \"r/rust\")."
                        },
                        "sort": {
                            "type": "string",
                            "enum": ["hot", "new", "top"],
                            "description": "Feed type. Use 'top' with a time window; 'hot' for trending; 'new' for latest.",
                            "default": "hot"
                        },
                        "time": {
                            "type": "string",
                            "enum": ["hour", "day", "week", "month", "year", "all"],
                            "description": "Only applies when sort='top'. Default: day.",
                            "default": "day"
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 5000,
                            "description": "Max posts to return (default: 10). Values above 100 are fetched across pages.",
                            "default": 10
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous response (normalized output)."
                        },
                        "include_nsfw": {
                            "type": "boolean",
                            "default": false,
                            "description": "Include NSFW listings by sending Reddit's include_over_18 parameter and over18 cookie."
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        }
                    },
                    "required": ["subreddit"],
                    "examples": [
                        {
                            "description": "Top posts this week in r/rust",
                            "input": { "subreddit": "rust", "sort": "top", "time": "week", "limit": 10 }
                        },
                        {
                            "description": "Latest posts in r/machinelearning",
                            "input": { "subreddit": "machinelearning", "sort": "new", "limit": 5 }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["social", "forum", "community"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                })
                .as_object()
                .expect("Schema object")
                .clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search posts by keywords. Tip: use subreddit=\"rust\" to scope results rather than embedding it in the query string. Example: query=\"async await\" subreddit=\"rust\" limit=10.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query text (keywords)." },
                        "sort": { "type": "string", "enum": ["relevance", "hot", "new", "top", "comments"], "default": "relevance", "description": "Search sort order." },
                        "time": { "type": "string", "enum": ["hour", "day", "week", "month", "year", "all"], "default": "all", "description": "Time window filter (maps to Reddit search 't=')." },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5000, "default": 10 },
                        "subreddit": { "type": "string", "description": "Optional subreddit filter (e.g., \"rust\" or \"r/rust\")." },
                        "author": { "type": "string", "description": "Optional author filter (e.g., \"spez\")." },
                        "include_nsfw": { "type": "boolean", "default": false },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous response (normalized output)."
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        }
                    },
                    "required": ["query"],
                    "examples": [
                        {
                            "description": "Search within a subreddit",
                            "input": { "query": "async await", "subreddit": "rust", "limit": 10 }
                        },
                        {
                            "description": "Search recent discussions",
                            "input": { "query": "open source license", "time": "month", "limit": 5 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["social", "forum", "community"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                })
                .as_object()
                .expect("Schema object")
                .clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a post with comments. Provide a full Reddit URL. Tip: set comment_sort=\"best\"|\"top\"|\"new\" and keep comment_limit small for token efficiency. The connector will paginate internally to fetch more than the first page when needed.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": { "type": "string", "description": "Normalized item_ref (e.g., reddit:post:abc123)." },
                        "url": { "type": "string", "description": "Canonical Reddit post URL." },
                        "post_url": { "type": "string", "description": "Full Reddit post URL." },
                        "comment_limit": { "type": "integer", "minimum": 0, "maximum": 5000, "default": 25 },
                        "comment_sort": { "type": "string", "enum": ["best", "top", "new", "controversial", "old", "qa"], "default": "best" },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        }
                    },
                    "examples": [
                        {
                            "description": "Get a thread by URL",
                            "input": {
                                "post_url": "https://www.reddit.com/r/rust/comments/8v1i5t/why_is_rust_so_hard_to_learn/",
                                "comment_limit": 20
                            }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["social", "forum", "community"],
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
            },
            Tool {
                name: Cow::Borrowed("media"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Resolve media URLs for a Reddit post, including galleries, hosted videos, direct images, crossposts, and external links.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Post id, normalized item_ref, or Reddit post URL."
                        },
                        "item_ref": {
                            "type": "string",
                            "description": "Normalized item_ref (e.g., reddit:post:abc123)."
                        },
                        "url": {
                            "type": "string",
                            "description": "Canonical Reddit post URL."
                        },
                        "post_url": {
                            "type": "string",
                            "description": "Full Reddit post URL."
                        },
                        "include_nsfw": {
                            "type": "boolean",
                            "default": false,
                            "description": "Send Reddit's over18 cookie when resolving gated posts."
                        }
                    },
                    "examples": [
                        {
                            "description": "Resolve post media by normalized ref",
                            "input": { "item_ref": "reddit:post:abc123" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["social", "forum", "community", "media"],
                        "auth_required": false,
                        "supports_output_format": false,
                        "supports_cursor": false
                    }
                })
                .as_object()
                .expect("Schema object")
                .clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("user"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a Reddit user's profile metadata (about.json): karma and account creation time. Works without auth for public profiles. Example: username=\"spez\".",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "username": {
                            "type": "string",
                            "description": "Reddit username (with or without u/ prefix). Example: \"spez\" or \"u/spez\"."
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        }
                    },
                    "required": ["username"],
                    "examples": [
                        {
                            "description": "Fetch user profile metadata",
                            "input": { "username": "spez" }
                        },
                        {
                            "description": "Fetch user profile metadata (normalized output)",
                            "input": { "username": "u/spez", "output_format": "normalized_v1" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["social", "forum", "community"],
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
            // === Canonical, low-ambiguity tools ===
            "list" | "list_posts" => {
                let subreddit_name = args.get("subreddit").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'subreddit' parameter".to_string()),
                )?;
                let subreddit_name = Self::normalize_subreddit_name(subreddit_name)?;
                let output_format = Self::parse_output_format(&args)?;
                let cursor = Self::parse_cursor::<RedditListingCursor>(args.get("cursor"))?;
                let desired_limit =
                    args.get("limit")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(10)
                        .clamp(1, i64::from(MAX_LIST_LIMIT)) as usize;
                let include_nsfw = Self::include_nsfw_from_args(&args);
                let sort = args
                    .get("sort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hot")
                    .to_lowercase();
                let time = if sort == "top" {
                    let time = args
                        .get("time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("day")
                        .to_lowercase();
                    Some(Self::validate_time_param(&time)?.to_string())
                } else {
                    None
                };
                if !matches!(sort.as_str(), "hot" | "new" | "top") {
                    return Err(ConnectorError::InvalidParams(
                        "sort must be one of: hot, new, top".to_string(),
                    ));
                }

                let collected = collect_paginated_with_cursor(
                    desired_limit,
                    MAX_LIST_REQUESTS,
                    cursor,
                    |cursor, remaining| {
                        let subreddit_name = subreddit_name.clone();
                        let sort = sort.clone();
                        let time = time.clone();
                        async move {
                            self.fetch_listing_page(
                                &subreddit_name,
                                &sort,
                                time.as_deref(),
                                include_nsfw,
                                cursor,
                                remaining,
                            )
                            .await
                        }
                    },
                    |post: &Value| post["id"].as_str().map(str::to_string),
                )
                .await?;
                let posts = collected.items;

                if output_format == OutputFormat::NormalizedV1 {
                    let items: Vec<ContentItem> = posts
                        .iter()
                        .map(Self::content_item_from_post_data)
                        .collect();

                    let next_cursor = collected
                        .next_cursor
                        .map(|c| ingest::encode_cursor(&c))
                        .transpose()?;
                    let has_more = next_cursor.is_some();
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(desired_limit as u64))),
                        Source::new("reddit", "list"),
                    );
                    return structured_result(&page);
                }

                let results: Vec<_> = posts.iter().map(Self::listing_raw_post).collect();

                let text = serde_json::to_string(&results)?;
                Ok(structured_result_with_text(&results, Some(text))?)
            }
            "media" | "resolve_media" => {
                let target = self.resolve_post_target(&args)?;
                let include_nsfw = Self::include_nsfw_from_args(&args);
                let post = self
                    .fetch_post_data(&target, 0, "best", include_nsfw)
                    .await?;
                let media = Self::resolved_media_for_post(&post);
                let count = media.len();
                let permalink = post["permalink"].as_str().unwrap_or("");
                let canonical_url = if permalink.is_empty() {
                    Value::Null
                } else {
                    json!(format!("{}{}", REDDIT_CANONICAL_BASE_URL, permalink))
                };
                let result = json!({
                    "post_id": post["id"].as_str().unwrap_or(&target.post_id),
                    "title": post["title"].as_str().unwrap_or(""),
                    "permalink": canonical_url,
                    "media": media,
                    "count": count,
                });

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            "user" | "user_about" => {
                let username = args.get("username").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' parameter".to_string()),
                )?;
                let username = Self::normalize_username(username);
                let output_format = Self::parse_output_format(&args)?;

                let about = self.fetch_user_about_json(&username).await?;
                let profile = Self::user_profile_from_about_json(&username, &about)?;

                if output_format == OutputFormat::NormalizedV1 {
                    let created_at = profile
                        .get("created_utc")
                        .and_then(|v| v.as_f64())
                        .and_then(Self::rfc3339_from_utc);

                    let canonical_url = Some(format!("https://www.reddit.com/user/{}/", username));
                    let item_ref = format!("reddit:user:{}", username);
                    let authors = vec![Author {
                        name: username.clone(),
                        id: None,
                    }];

                    let total_karma = profile
                        .get("total_karma")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let account_age_days = profile.get("account_age_days").and_then(|v| v.as_i64());

                    let mut summary = format!("u/{} — {} karma", username, total_karma);
                    if let Some(days) = account_age_days {
                        summary.push_str(&format!(" — {} days old", days));
                    }

                    let blocks = vec![ContentBlock {
                        block_ref: format!("reddit:user:{}:summary", username),
                        block_kind: "summary".to_string(),
                        text: summary,
                        author: Some(Author {
                            name: username.clone(),
                            id: None,
                        }),
                        created_at,
                        reply_to: None,
                        position: None,
                        score: None,
                        attachments: Vec::new(),
                        metadata: Some(profile.clone()),
                    }];

                    let item = ContentItem {
                        item_ref,
                        kind: "user_profile".to_string(),
                        canonical_url,
                        title: Some(format!("u/{}", username)),
                        created_at: profile
                            .get("created_iso")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        source_updated_at: None,
                        authors,
                        tags: Vec::new(),
                        metadata: Some(profile),
                        blocks,
                        relationships: Vec::new(),
                        truncation: None,
                    };

                    let normalized = NormalizedItemV1::new(
                        item,
                        Partial::complete(None),
                        Source::new("reddit", "user"),
                    );
                    return structured_result(&normalized);
                }

                let text = if output_format == OutputFormat::DisplayV1 {
                    let created_iso = profile
                        .get("created_iso")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let link_karma = profile
                        .get("link_karma")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let comment_karma = profile
                        .get("comment_karma")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let total_karma = profile
                        .get("total_karma")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let age_days = profile
                        .get("account_age_days")
                        .and_then(|v| v.as_i64())
                        .map(|d| format!(" ({} days old)", d))
                        .unwrap_or_default();
                    Some(format!(
                        "u/{} — {} karma (link {}, comment {}) — created {}{}",
                        username, total_karma, link_karma, comment_karma, created_iso, age_days
                    ))
                } else {
                    Some(serde_json::to_string(&profile)?)
                };

                Ok(structured_result_with_text(&profile, text)?)
            }
            "search" | "search_posts" => {
                let request = CallToolRequestParam {
                    name: "search_reddit".into(),
                    arguments: Some(args),
                };
                self.call_tool(request).await
            }
            "get" | "get_post" => {
                let request = CallToolRequestParam {
                    name: "get_post_details".into(),
                    arguments: Some(args),
                };
                self.call_tool(request).await
            }

            // === Legacy tool names (kept for compatibility) ===
            "get_user_info" => {
                let username = args.get("username").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' parameter".to_string()),
                )?;
                // Strip "u/", "/u/", or leading "/" from username
                let username = username
                    .strip_prefix("/u/")
                    .or_else(|| username.strip_prefix("u/"))
                    .unwrap_or(username);

                let user = User::new(username);
                let about = user
                    .about(None)
                    .await
                    .map_err(|e| ConnectorError::Other(format!("Failed to fetch user: {}", e)))?;

                let data = &about.data;
                let result = json!({
                    "name": data.name,
                    "id": data.id,
                    "link_karma": data.link_karma,
                    "comment_karma": data.comment_karma,
                    "created_utc": data.created_utc,
                    "is_gold": data.is_gold,
                    "is_mod": data.is_mod,
                    "verified": data.verified,
                });

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            "get_subreddit_top_posts" => {
                let mut forwarded_args = args.clone();
                forwarded_args.insert("sort".to_string(), json!("top"));
                let request = CallToolRequestParam {
                    name: "list".into(),
                    arguments: Some(forwarded_args),
                };
                self.call_tool(request).await
            }
            "get_subreddit_hot_posts" => {
                let mut forwarded_args = args.clone();
                forwarded_args.insert("sort".to_string(), json!("hot"));
                let request = CallToolRequestParam {
                    name: "list".into(),
                    arguments: Some(forwarded_args),
                };
                self.call_tool(request).await
            }
            "get_subreddit_new_posts" => {
                let mut forwarded_args = args.clone();
                forwarded_args.insert("sort".to_string(), json!("new"));
                let request = CallToolRequestParam {
                    name: "list".into(),
                    arguments: Some(forwarded_args),
                };
                self.call_tool(request).await
            }
            "get_subreddit_info" => {
                let subreddit_name = args.get("subreddit").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'subreddit' parameter".to_string()),
                )?;
                // Strip "r/" prefix if present
                let subreddit_name = subreddit_name.strip_prefix("r/").unwrap_or(subreddit_name);

                let subreddit = Subreddit::new(subreddit_name);
                let about = subreddit.about().await.map_err(|e| {
                    ConnectorError::Other(format!("Failed to fetch subreddit info: {}", e))
                })?;

                let data = &about;
                let result = json!({
                    "display_name": data.display_name,
                    "title": data.title,
                    "description": data.public_description,
                    "subscribers": data.subscribers,
                    "active_users": format!("{:#?}", data.active_user_count.as_ref().unwrap_or(&AccountsActive::Number(0))),
                    "url": data.url.as_ref().map_or("".to_string(), |url| format!("https://www.reddit.com{}", url)),
                    "created_utc": data.created_utc,
                    "over18": data.over18,
                });

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            "search_reddit" => {
                let output_format = Self::parse_output_format(&args)?;
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' parameter".to_string()),
                )?;
                let sort = args
                    .get("sort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("relevance")
                    .to_lowercase();
                let time = args
                    .get("time")
                    .and_then(|v| v.as_str())
                    .unwrap_or("all")
                    .to_lowercase();

                // Build advanced search query with optional filters
                let mut search_query = query.to_string();

                // Add author filter if provided
                if let Some(author) = args.get("author").and_then(|v| v.as_str()) {
                    if !author.is_empty() {
                        // Strip "u/", "/u/" prefix if present
                        let author = author
                            .strip_prefix("/u/")
                            .or_else(|| author.strip_prefix("u/"))
                            .unwrap_or(author);
                        search_query = format!("{} author:{}", search_query, author);
                    }
                }

                // Add subreddit filter if provided
                if let Some(subreddit) = args.get("subreddit").and_then(|v| v.as_str()) {
                    if !subreddit.is_empty() {
                        // Strip "r/" prefix if present
                        let subreddit_name = subreddit.strip_prefix("r/").unwrap_or(subreddit);
                        search_query = format!("{} subreddit:{}", search_query, subreddit_name);
                    }
                }

                // Add flair filter if provided
                if let Some(flair) = args.get("flair").and_then(|v| v.as_str()) {
                    if !flair.is_empty() {
                        // If flair contains spaces, wrap it in quotes
                        let formatted_flair = if flair.contains(' ') {
                            format!("\"{}\"", flair)
                        } else {
                            flair.to_string()
                        };
                        search_query = format!("{} flair:{}", search_query, formatted_flair);
                    }
                }

                // Add title filter if provided
                if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
                    if !title.is_empty() {
                        // If title contains spaces, wrap it in quotes
                        let formatted_title = if title.contains(' ') {
                            format!("\"{}\"", title)
                        } else {
                            title.to_string()
                        };
                        search_query = format!("{} title:{}", search_query, formatted_title);
                    }
                }

                // Add selftext filter if provided
                if let Some(selftext) = args.get("selftext").and_then(|v| v.as_str()) {
                    if !selftext.is_empty() {
                        // If selftext contains spaces, wrap it in quotes
                        let formatted_selftext = if selftext.contains(' ') {
                            format!("\"{}\"", selftext)
                        } else {
                            selftext.to_string()
                        };
                        search_query = format!("{} selftext:{}", search_query, formatted_selftext);
                    }
                }

                // Add site filter if provided
                if let Some(site) = args.get("site").and_then(|v| v.as_str()) {
                    if !site.is_empty() {
                        search_query = format!("{} site:{}", search_query, site);
                    }
                }

                // Add URL filter if provided
                if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
                    if !url.is_empty() {
                        search_query = format!("{} url:{}", search_query, url);
                    }
                }

                // Add self post filter if provided
                if let Some(self_post) = args.get("self").and_then(|v| v.as_bool()) {
                    search_query = format!("{} self:{}", search_query, self_post);
                }

                // Include NSFW content if specified
                let include_nsfw = Self::include_nsfw_from_args(&args);
                let cursor = Self::parse_cursor::<RedditSearchCursor>(args.get("cursor"))?;

                let desired_limit =
                    args.get("limit")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(10)
                        .clamp(1, i64::from(MAX_SEARCH_LIMIT)) as usize;
                let sort_param = match sort.as_str() {
                    "relevance" | "hot" | "new" | "top" | "comments" => sort.as_str(),
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "sort must be one of: relevance, hot, new, top, comments".to_string(),
                        ));
                    }
                }
                .to_string();
                let time_param = match time.as_str() {
                    "hour" | "day" | "week" | "month" | "year" | "all" => time.as_str(),
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "time must be one of: hour, day, week, month, year, all".to_string(),
                        ));
                    }
                }
                .to_string();

                let collected = collect_paginated_with_cursor(
                    desired_limit,
                    MAX_SEARCH_REQUESTS,
                    cursor,
                    |cursor, remaining| {
                        let search_query = search_query.clone();
                        let sort_param = sort_param.clone();
                        let time_param = time_param.clone();
                        async move {
                            let page_limit = remaining.min(SEARCH_PAGE_SIZE_MAX);

                            let mut params: Vec<(String, String)> = vec![
                                ("q".to_string(), search_query.clone()),
                                ("limit".to_string(), page_limit.to_string()),
                                ("include_over_18".to_string(), include_nsfw.to_string()),
                                ("sort".to_string(), sort_param.to_string()),
                                ("t".to_string(), time_param.to_string()),
                                ("raw_json".to_string(), "1".to_string()),
                            ];

                            let mut count = 0usize;
                            if let Some(c) = cursor {
                                count = c.count;
                                params.push(("after".to_string(), c.after));
                                params.push(("count".to_string(), count.to_string()));
                            }

                            let search_results = self
                                .fetch_reddit_json("/search.json", &params, include_nsfw)
                                .await?;

                            let data = search_results.get("data").ok_or_else(|| {
                                ConnectorError::Other("Invalid response format".to_string())
                            })?;

                            let children = data.get("children").and_then(|c| c.as_array()).ok_or(
                                ConnectorError::Other("Invalid response format".to_string()),
                            )?;

                            let after = data.get("after").and_then(|v| v.as_str()).unwrap_or("");
                            let next_cursor = if after.is_empty() {
                                None
                            } else {
                                Some(RedditSearchCursor {
                                    after: after.to_string(),
                                    count: count.saturating_add(children.len()),
                                })
                            };

                            Ok::<_, ConnectorError>(Page {
                                items: children.clone(),
                                next_cursor,
                            })
                        }
                    },
                    |post: &Value| post["data"]["id"].as_str().map(str::to_string),
                )
                .await?;

                let posts = collected.items;

                let mut img_results = Vec::new();
                let mut text_results = Vec::new();

                // Process results similar to Python code
                for post in posts.iter().take(desired_limit) {
                    let data = &post["data"];

                    let title = data["title"].as_str().unwrap_or("").to_string();
                    let permalink = data["permalink"].as_str().unwrap_or("").to_string();
                    let full_url = format!("{}{}", REDDIT_CANONICAL_BASE_URL, permalink);

                    // Check if thumbnail is a valid URL
                    let thumbnail = data["thumbnail"].as_str().unwrap_or("").to_string();
                    if thumbnail.starts_with("http") {
                        let img_src = data["url"].as_str().unwrap_or("").to_string();

                        img_results.push(json!({
                            "url": full_url,
                            "title": title,
                            "img_src": img_src,
                            "thumbnail_src": thumbnail,
                            "template": "images.html"
                        }));
                    } else {
                        // Text result
                        let mut content = data["selftext"].as_str().unwrap_or("").to_string();
                        if content.len() > 500 {
                            content = format!("{}...", &content[0..500]);
                        }

                        // Convert Unix timestamp to datetime
                        let created_utc = data["created_utc"].as_f64().unwrap_or(0.0) as i64;
                        let created = chrono::DateTime::from_timestamp(created_utc, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| "Unknown date".to_string());

                        text_results.push(json!({
                            "url": full_url,
                            "title": title,
                            "content": content,
                            "publishedDate": created
                        }));
                    }
                }

                // Combine results with images first, then text
                let mut combined_results = Vec::new();
                combined_results.extend(img_results);
                combined_results.extend(text_results);

                if output_format == OutputFormat::NormalizedV1 {
                    let items: Vec<ContentItem> = posts
                        .iter()
                        .map(|post| {
                            let data = &post["data"];
                            Self::content_item_from_post_data(data)
                        })
                        .collect();

                    let next_cursor = collected
                        .next_cursor
                        .map(|c| ingest::encode_cursor(&c))
                        .transpose()?;
                    let has_more = next_cursor.is_some();
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(desired_limit as u64))),
                        Source::new("reddit", "search"),
                    );
                    return structured_result(&page);
                }

                let text = serde_json::to_string(&combined_results)?;
                Ok(structured_result_with_text(&combined_results, Some(text))?)
            }
            "get_post_details" => {
                let target = self.resolve_post_target(&args)?;
                let comment_limit =
                    args.get("comment_limit")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(i64::from(DEFAULT_COMMENT_LIMIT))
                        .clamp(0, i64::from(MAX_COMMENT_LIMIT)) as u32;
                let comment_sort = args
                    .get("comment_sort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("best");
                let output_format = Self::parse_output_format(&args)?;
                let include_nsfw = Self::include_nsfw_from_args(&args);
                let post_data = self
                    .fetch_post_listing(&target, comment_limit, comment_sort, include_nsfw)
                    .await?;

                if post_data.len() < 2 {
                    return Err(ConnectorError::Other("Invalid response format".to_string()));
                }

                // Extract post details from the first element
                let post = &post_data[0]["data"]["children"][0]["data"];
                let post_id = post["id"].as_str().unwrap_or("");
                if post_id.is_empty() {
                    return Err(ConnectorError::Other(
                        "Invalid response: missing post id".to_string(),
                    ));
                }

                // Extract comments from the second element
                let link_fullname = format!("t3_{}", post_id);
                let comments = self
                    .fetch_comment_tree_with_more(
                        &post_data[1]["data"]["children"],
                        &link_fullname,
                        comment_limit,
                        comment_sort,
                    )
                    .await?;

                if output_format == OutputFormat::NormalizedV1 {
                    let item_ref = format!("reddit:post:{}", post_id);
                    let permalink = post["permalink"].as_str().unwrap_or("");
                    let canonical_url = if permalink.is_empty() {
                        None
                    } else {
                        Some(format!("https://www.reddit.com{}", permalink))
                    };
                    let author_name = post["author"].as_str().unwrap_or("");
                    let authors = if author_name.is_empty() {
                        Vec::new()
                    } else {
                        vec![Author {
                            name: author_name.to_string(),
                            id: None,
                        }]
                    };
                    let subreddit = post["subreddit"].as_str().unwrap_or("");
                    let tags = if subreddit.is_empty() {
                        Vec::new()
                    } else {
                        vec![subreddit.to_string()]
                    };

                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    let mut relationships: Vec<Relationship> = Vec::new();

                    let selftext = post["selftext"].as_str().unwrap_or("");
                    let selftext_html = post["selftext_html"].as_str().unwrap_or("");
                    let post_body_text = if !selftext.is_empty() {
                        selftext.to_string()
                    } else if !selftext_html.is_empty() {
                        html_to_text(selftext_html)
                    } else {
                        String::new()
                    };

                    if !post_body_text.is_empty() || post["is_self"].as_bool().unwrap_or(false) {
                        let post_body_ref = format!("reddit:post_body:{}", post_id);
                        blocks.push(ContentBlock {
                            block_ref: post_body_ref.clone(),
                            block_kind: "post_body".to_string(),
                            text: post_body_text,
                            author: authors.first().cloned(),
                            created_at: post["created_utc"]
                                .as_f64()
                                .and_then(Self::rfc3339_from_utc),
                            reply_to: None,
                            position: None,
                            score: None,
                            attachments: Vec::new(),
                            metadata: None,
                        });
                        relationships.push(Relationship {
                            rel: "has_block".to_string(),
                            from: item_ref.clone(),
                            to: post_body_ref,
                        });
                    }

                    Self::append_comment_blocks(
                        &comments,
                        None,
                        0,
                        &item_ref,
                        &mut blocks,
                        &mut relationships,
                    );

                    let num_comments = post["num_comments"].as_i64().unwrap_or(0).max(0) as u64;
                    let returned_blocks = blocks.len() as u64;
                    let is_truncated = num_comments > comment_limit as u64;
                    let truncation = if is_truncated {
                        Some(Truncation {
                            is_truncated: true,
                            reason: "comment_limit".to_string(),
                            total_blocks_hint: Some(num_comments),
                            returned_blocks,
                            policy: Some("top_level_limit".to_string()),
                        })
                    } else {
                        None
                    };

                    let item = ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "thread".to_string(),
                        canonical_url,
                        title: Some(post["title"].as_str().unwrap_or("").to_string()),
                        created_at: post["created_utc"]
                            .as_f64()
                            .and_then(Self::rfc3339_from_utc),
                        source_updated_at: None,
                        authors: authors.clone(),
                        tags,
                        metadata: Some(Self::listing_raw_post(post)),
                        blocks,
                        relationships,
                        truncation,
                    };

                    let partial = if is_truncated {
                        Partial::truncated(
                            "comment_limit",
                            Some(ingest::limits_max_blocks(
                                comment_limit.saturating_add(1) as u64
                            )),
                        )
                    } else {
                        Partial::complete(Some(ingest::limits_max_blocks(
                            comment_limit.saturating_add(1) as u64,
                        )))
                    };

                    let normalized =
                        NormalizedItemV1::new(item, partial, Source::new("reddit", "get"));
                    return structured_result(&normalized);
                }

                // Build the result
                let result = json!({
                    "post": {
                        "id": post["id"].as_str().unwrap_or(""),
                        "title": post["title"].as_str().unwrap_or(""),
                        "author": post["author"].as_str().unwrap_or(""),
                        "subreddit": post["subreddit"].as_str().unwrap_or(""),
                        "selftext": post["selftext"].as_str().unwrap_or(""),
                        "selftext_html": post["selftext_html"].as_str().unwrap_or(""),
                        "score": post["score"].as_i64().unwrap_or(0),
                        "upvote_ratio": post["upvote_ratio"].as_f64().unwrap_or(0.0),
                        "num_comments": post["num_comments"].as_i64().unwrap_or(0),
                        "created_utc": post["created_utc"].as_f64().unwrap_or(0.0),
                        "permalink": post["permalink"].as_str().unwrap_or(""),
                        "url": post["url"].as_str().unwrap_or(""),
                        "url_overridden_by_dest": post["url_overridden_by_dest"].as_str().unwrap_or(""),
                        "domain": post["domain"].as_str().unwrap_or(""),
                        "is_video": post["is_video"].as_bool().unwrap_or(false),
                        "is_self": post["is_self"].as_bool().unwrap_or(false),
                        "is_gallery": post["is_gallery"].as_bool().unwrap_or(false),
                        "over_18": post["over_18"].as_bool().unwrap_or(false),
                        "spoiler": post["spoiler"].as_bool().unwrap_or(false),
                        "post_hint": post["post_hint"].as_str().unwrap_or(""),
                        "preview": post["preview"].clone(),
                        "crosspost_parent_list": post["crosspost_parent_list"].clone(),
                        "media": post["media"].clone(),
                        "secure_media": post["secure_media"].clone(),
                        "media_metadata": post["media_metadata"].clone(),
                        "gallery_data": post["gallery_data"].clone(),
                        "resolved_media": Self::resolved_media_for_post(post),
                    },
                    "comments": comments
                });

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
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

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompts not supported".to_string(),
        ))
    }
}

// Helper struct to store post information extracted from URL
struct PostInfo {
    subreddit: Option<String>,
    post_id: String,
}

impl RedditConnector {
    // Helper method to extract post ID and subreddit from a Reddit post URL
    fn extract_post_info_from_url(&self, url: &str) -> Option<PostInfo> {
        // Handle different Reddit URL formats
        let url = url.trim();

        // Regular Reddit URL pattern: reddit.com/r/subreddit/comments/post_id/...
        let reddit_patterns = [
            r"(?:https?://)?(?:www\.)?reddit\.com/r/([^/]+)/comments/([^/]+)",
            r"(?:https?://)?(?:old\.)?reddit\.com/r/([^/]+)/comments/([^/]+)",
            r"(?:https?://)?(?:new\.)?reddit\.com/r/([^/]+)/comments/([^/]+)",
            r"(?:https?://)?(?:np\.)?reddit\.com/r/([^/]+)/comments/([^/]+)",
        ];

        for pattern in reddit_patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                if let Some(captures) = regex.captures(url) {
                    if captures.len() >= 3 {
                        return Some(PostInfo {
                            subreddit: Some(captures[1].to_string()),
                            post_id: captures[2].to_string(),
                        });
                    }
                }
            }
        }

        let generic_patterns = [
            r"(?:https?://)?(?:www\.)?reddit\.com/comments/([^/]+)",
            r"(?:https?://)?(?:old\.)?reddit\.com/comments/([^/]+)",
            r"(?:https?://)?(?:new\.)?reddit\.com/comments/([^/]+)",
            r"(?:https?://)?(?:np\.)?reddit\.com/comments/([^/]+)",
        ];

        for pattern in generic_patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                if let Some(captures) = regex.captures(url) {
                    if captures.len() >= 2 {
                        return Some(PostInfo {
                            subreddit: None,
                            post_id: captures[1].to_string(),
                        });
                    }
                }
            }
        }

        None
    }

    fn parse_output_format(
        args: &serde_json::Map<String, Value>,
    ) -> Result<OutputFormat, ConnectorError> {
        ingest::output_format_from_args(args)
    }

    fn normalize_username(username: &str) -> String {
        let username = username.trim();
        let username = username
            .strip_prefix("/u/")
            .or_else(|| username.strip_prefix("u/"))
            .unwrap_or(username);
        username.trim_start_matches('@').to_string()
    }

    fn normalize_subreddit_name(subreddit: &str) -> Result<String, ConnectorError> {
        let subreddit = subreddit.trim().trim_end_matches('/');
        let subreddit = subreddit
            .strip_prefix("/r/")
            .or_else(|| subreddit.strip_prefix("r/"))
            .unwrap_or(subreddit);

        if subreddit.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "Subreddit must be non-empty".to_string(),
            ));
        }
        if !subreddit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(ConnectorError::InvalidParams(
                "Subreddit contains invalid characters".to_string(),
            ));
        }

        Ok(subreddit.to_string())
    }

    fn include_nsfw_from_args(args: &serde_json::Map<String, Value>) -> bool {
        args.get("include_nsfw")
            .or_else(|| args.get("include_over_18"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    fn validate_time_param(time: &str) -> Result<&'static str, ConnectorError> {
        match time {
            "hour" | "now" => Ok("hour"),
            "day" | "today" => Ok("day"),
            "week" => Ok("week"),
            "month" => Ok("month"),
            "year" => Ok("year"),
            "all" | "alltime" => Ok("all"),
            _ => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'time' value: '{}'. Expected one of: hour, day, week, month, year, all.",
                time
            ))),
        }
    }

    fn resolve_api_base_url(details: &AuthDetails) -> Result<String, ConnectorError> {
        let configured = details
            .get("api_base_url")
            .or_else(|| details.get("base_url"))
            .cloned()
            .or_else(|| env::var("RZN_REDDIT_API_BASE_URL").ok())
            .unwrap_or_else(|| REDDIT_CANONICAL_BASE_URL.to_string());

        Self::normalize_api_base_url(&configured)
    }

    fn normalize_api_base_url(base_url: &str) -> Result<String, ConnectorError> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Ok(REDDIT_CANONICAL_BASE_URL.to_string());
        }
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            return Err(ConnectorError::InvalidParams(
                "api_base_url must start with http:// or https://".to_string(),
            ));
        }

        Ok(base_url.to_string())
    }

    fn reddit_base_url_candidates(&self) -> Vec<&str> {
        if self.api_base_url == REDDIT_CANONICAL_BASE_URL {
            vec![self.api_base_url.as_str(), REDDIT_OLD_BASE_URL]
        } else {
            vec![self.api_base_url.as_str()]
        }
    }

    fn reddit_url(base_url: &str, path: &str) -> String {
        format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn fetch_reddit_json(
        &self,
        path: &str,
        params: &[(String, String)],
        include_over_18: bool,
    ) -> Result<Value, ConnectorError> {
        let mut last_error = None;

        for base_url in self.reddit_base_url_candidates() {
            let url = Self::reddit_url(base_url, path);
            let mut request = self.http_client.get(&url).query(params);
            if include_over_18 {
                request = request.header(reqwest::header::COOKIE, "over18=1");
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(e) => {
                    last_error = Some(format!("Failed to send Reddit request to {}: {}", url, e));
                    continue;
                }
            };
            let status = response.status();
            let body = response.text().await.map_err(|e| {
                ConnectorError::Other(format!("Failed to read Reddit response body: {}", e))
            })?;

            if status.is_success() {
                return serde_json::from_str(&body).map_err(|e| {
                    ConnectorError::Other(format!(
                        "Failed to parse Reddit JSON from {}: {}",
                        url, e
                    ))
                });
            }

            let excerpt: String = body.trim().chars().take(240).collect();
            let hint = if status == reqwest::StatusCode::FORBIDDEN {
                " Anonymous Reddit JSON access may be IP-blocked; try api_base_url=https://old.reddit.com or configure reddit proxy_url."
            } else {
                ""
            };
            last_error = Some(if excerpt.is_empty() {
                format!("Reddit returned HTTP {} for {}.{}", status, url, hint)
            } else {
                format!(
                    "Reddit returned HTTP {} for {}: {}{}",
                    status, url, excerpt, hint
                )
            });
        }

        Err(ConnectorError::Other(last_error.unwrap_or_else(|| {
            "Reddit request failed before a response was received".to_string()
        })))
    }

    async fn fetch_listing_page(
        &self,
        subreddit: &str,
        sort: &str,
        time: Option<&str>,
        include_nsfw: bool,
        cursor: Option<RedditListingCursor>,
        remaining: usize,
    ) -> Result<Page<Value, RedditListingCursor>, ConnectorError> {
        let page_limit = remaining.min(LIST_PAGE_SIZE_MAX);
        let path = format!("/r/{}/{}.json", subreddit, sort);
        let mut params: Vec<(String, String)> = vec![
            ("limit".to_string(), page_limit.to_string()),
            ("raw_json".to_string(), "1".to_string()),
            ("include_over_18".to_string(), include_nsfw.to_string()),
        ];

        if let Some(time) = time {
            params.push(("t".to_string(), time.to_string()));
        }

        let mut count = 0usize;
        if let Some(cursor) = cursor {
            count = cursor.count;
            params.push(("after".to_string(), cursor.after));
            params.push(("count".to_string(), count.to_string()));
        }

        let listing = self.fetch_reddit_json(&path, &params, include_nsfw).await?;
        let data = listing
            .get("data")
            .ok_or_else(|| ConnectorError::Other("Invalid Reddit listing response".to_string()))?;
        let children = data
            .get("children")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ConnectorError::Other("Invalid Reddit listing response".to_string()))?;
        let items = children
            .iter()
            .filter_map(|child| child.get("data").cloned())
            .collect::<Vec<_>>();
        let after = data.get("after").and_then(|v| v.as_str()).unwrap_or("");
        let next_cursor = if after.is_empty() {
            None
        } else {
            Some(RedditListingCursor {
                after: after.to_string(),
                count: count.saturating_add(children.len()),
            })
        };

        Ok(Page { items, next_cursor })
    }

    async fn fetch_post_listing(
        &self,
        target: &RedditPostTarget,
        comment_limit: u32,
        comment_sort: &str,
        include_nsfw: bool,
    ) -> Result<Vec<Value>, ConnectorError> {
        let path = if let Some(subreddit) = target.subreddit.as_deref() {
            format!("/r/{}/comments/{}.json", subreddit, target.post_id)
        } else {
            format!("/comments/{}.json", target.post_id)
        };
        let params = vec![
            ("limit".to_string(), comment_limit.to_string()),
            ("sort".to_string(), comment_sort.to_string()),
            ("raw_json".to_string(), "1".to_string()),
        ];

        let value = self.fetch_reddit_json(&path, &params, include_nsfw).await?;
        value
            .as_array()
            .cloned()
            .ok_or_else(|| ConnectorError::Other("Invalid Reddit post response".to_string()))
    }

    async fn fetch_post_data(
        &self,
        target: &RedditPostTarget,
        comment_limit: u32,
        comment_sort: &str,
        include_nsfw: bool,
    ) -> Result<Value, ConnectorError> {
        let listing = self
            .fetch_post_listing(target, comment_limit, comment_sort, include_nsfw)
            .await?;
        let post = listing
            .first()
            .and_then(|v| v["data"]["children"].as_array())
            .and_then(|children| children.first())
            .and_then(|child| child.get("data"))
            .cloned()
            .ok_or_else(|| ConnectorError::Other("Invalid Reddit post response".to_string()))?;

        if post["id"].as_str().unwrap_or("").is_empty() {
            return Err(ConnectorError::Other(
                "Invalid Reddit post response: missing post id".to_string(),
            ));
        }

        Ok(post)
    }

    fn content_item_from_post_data(data: &Value) -> ContentItem {
        let post_id = data["id"].as_str().unwrap_or("");
        let permalink = data["permalink"].as_str().unwrap_or("");
        let canonical_url = if permalink.is_empty() {
            None
        } else if permalink.starts_with("http://") || permalink.starts_with("https://") {
            Some(permalink.to_string())
        } else {
            Some(format!("{}{}", REDDIT_CANONICAL_BASE_URL, permalink))
        };
        let author = data["author"].as_str().unwrap_or("");
        let authors = if author.is_empty() {
            Vec::new()
        } else {
            vec![Author {
                name: author.to_string(),
                id: None,
            }]
        };
        let subreddit = data["subreddit"].as_str().unwrap_or("");
        let tags = if subreddit.is_empty() {
            Vec::new()
        } else {
            vec![subreddit.to_string()]
        };

        ContentItem {
            item_ref: format!("reddit:post:{}", post_id),
            kind: "thread".to_string(),
            canonical_url,
            title: Some(data["title"].as_str().unwrap_or("").to_string()),
            created_at: data["created_utc"]
                .as_f64()
                .and_then(Self::rfc3339_from_utc),
            source_updated_at: None,
            authors,
            tags,
            metadata: Some(Self::listing_raw_post(data)),
            blocks: Vec::new(),
            relationships: Vec::new(),
            truncation: None,
        }
    }

    fn listing_raw_post(data: &Value) -> Value {
        let permalink = data["permalink"].as_str().unwrap_or("");
        let permalink = if permalink.is_empty() {
            String::new()
        } else if permalink.starts_with("http://") || permalink.starts_with("https://") {
            permalink.to_string()
        } else {
            format!("{}{}", REDDIT_CANONICAL_BASE_URL, permalink)
        };

        json!({
            "id": data["id"].as_str().unwrap_or(""),
            "name": data["name"].as_str().unwrap_or(""),
            "title": data["title"].as_str().unwrap_or(""),
            "subreddit": data["subreddit"].as_str().unwrap_or(""),
            "author": data["author"].as_str().unwrap_or(""),
            "score": data["score"].as_i64().unwrap_or(0),
            "upvote_ratio": data["upvote_ratio"].as_f64().unwrap_or(0.0),
            "num_comments": data["num_comments"].as_i64().unwrap_or(0),
            "permalink": permalink,
            "created_utc": data["created_utc"].as_f64().unwrap_or(0.0),
            "url": data["url"].as_str().unwrap_or(""),
            "url_overridden_by_dest": data["url_overridden_by_dest"].as_str().unwrap_or(""),
            "domain": data["domain"].as_str().unwrap_or(""),
            "thumbnail": data["thumbnail"].as_str().unwrap_or(""),
            "post_hint": data["post_hint"].as_str().unwrap_or(""),
            "is_video": data["is_video"].as_bool().unwrap_or(false),
            "is_self": data["is_self"].as_bool().unwrap_or(false),
            "is_gallery": data["is_gallery"].as_bool().unwrap_or(false),
            "over_18": data["over_18"].as_bool().unwrap_or(false),
            "spoiler": data["spoiler"].as_bool().unwrap_or(false),
            "stickied": data["stickied"].as_bool().unwrap_or(false),
            "gallery_data": data["gallery_data"].clone(),
            "media_metadata": data["media_metadata"].clone(),
            "media": data["media"].clone(),
            "secure_media": data["secure_media"].clone(),
            "preview": data["preview"].clone(),
            "crosspost_parent_list": data["crosspost_parent_list"].clone(),
            "resolved_media": Self::resolved_media_for_post(data),
        })
    }

    fn resolved_media_for_post(post: &Value) -> Vec<Value> {
        let mut media = Vec::new();
        let mut seen = HashSet::new();

        Self::append_post_media_without_crossposts(post, "post", &mut media, &mut seen);

        if media.is_empty() {
            if let Some(parents) = post["crosspost_parent_list"].as_array() {
                for parent in parents {
                    Self::append_post_media_without_crossposts(
                        parent,
                        "crosspost_parent",
                        &mut media,
                        &mut seen,
                    );
                    if !media.is_empty() {
                        break;
                    }
                }
            }
        }

        if media.is_empty() {
            Self::append_external_media(post, "external", &mut media, &mut seen);
        }

        media
    }

    fn append_post_media_without_crossposts(
        post: &Value,
        source: &str,
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
    ) {
        Self::append_gallery_media(post, source, media, seen);
        Self::append_reddit_video_media(post, source, media, seen);
        Self::append_direct_media(post, source, media, seen);
    }

    fn append_gallery_media(
        post: &Value,
        source: &str,
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
    ) {
        let Some(items) = post["gallery_data"]["items"].as_array() else {
            return;
        };

        for item in items {
            let Some(media_id) = item["media_id"].as_str() else {
                continue;
            };
            let metadata = &post["media_metadata"][media_id];
            let Some(url) = Self::media_metadata_url(media_id, metadata) else {
                continue;
            };
            let media_type = Self::media_type_from_metadata(metadata);
            let mime = metadata["m"].as_str();
            let width = metadata["s"]["x"].as_i64();
            let height = metadata["s"]["y"].as_i64();
            Self::push_media(
                media,
                seen,
                media_type,
                &url,
                source,
                Some(media_id),
                mime,
                width,
                height,
            );
        }
    }

    fn append_reddit_video_media(
        post: &Value,
        source: &str,
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
    ) {
        let candidates = [
            &post["media"]["reddit_video"],
            &post["secure_media"]["reddit_video"],
            &post["preview"]["reddit_video_preview"],
        ];

        for video in candidates {
            if !video.is_object() {
                continue;
            }
            let selected_url = video["fallback_url"]
                .as_str()
                .or_else(|| video["hls_url"].as_str())
                .or_else(|| video["dash_url"].as_str());
            let Some(url) = selected_url else {
                continue;
            };
            let clean_url = Self::clean_reddit_url(url);
            if clean_url.is_empty() || !seen.insert(clean_url.clone()) {
                continue;
            }

            let mut item = json!({
                "type": "video",
                "url": clean_url,
                "source": source,
                "width": video["width"].as_i64(),
                "height": video["height"].as_i64(),
                "duration": video["duration"].as_i64(),
                "is_gif": video["is_gif"].as_bool().unwrap_or(false),
            });
            if let Some(hls_url) = video["hls_url"].as_str() {
                item["hls_url"] = json!(Self::clean_reddit_url(hls_url));
            }
            if let Some(dash_url) = video["dash_url"].as_str() {
                item["dash_url"] = json!(Self::clean_reddit_url(dash_url));
            }
            media.push(item);
        }
    }

    fn append_direct_media(
        post: &Value,
        source: &str,
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
    ) {
        let before = media.len();
        let url = post["url_overridden_by_dest"]
            .as_str()
            .or_else(|| post["url"].as_str())
            .unwrap_or("");
        if Self::looks_like_media_url(url) {
            Self::push_media(
                media,
                seen,
                Self::media_type_from_url(url),
                url,
                source,
                None,
                None,
                None,
                None,
            );
        }
        if media.len() > before {
            return;
        }

        if let Some(preview_url) = post["preview"]["images"]
            .as_array()
            .and_then(|images| images.first())
            .and_then(|image| image["source"]["url"].as_str())
        {
            Self::push_media(
                media,
                seen,
                "image",
                preview_url,
                "preview",
                None,
                None,
                post["preview"]["images"][0]["source"]["width"].as_i64(),
                post["preview"]["images"][0]["source"]["height"].as_i64(),
            );
        }
    }

    fn append_external_media(
        post: &Value,
        source: &str,
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
    ) {
        let url = post["url_overridden_by_dest"]
            .as_str()
            .or_else(|| post["url"].as_str())
            .unwrap_or("");
        if url.is_empty()
            || post["is_self"].as_bool().unwrap_or(false)
            || url.contains("reddit.com/r/")
        {
            return;
        }

        Self::push_media(media, seen, "external", url, source, None, None, None, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_media(
        media: &mut Vec<Value>,
        seen: &mut HashSet<String>,
        media_type: &str,
        url: &str,
        source: &str,
        id: Option<&str>,
        mime: Option<&str>,
        width: Option<i64>,
        height: Option<i64>,
    ) {
        let url = Self::clean_reddit_url(url);
        if url.is_empty() || !seen.insert(url.clone()) {
            return;
        }

        let mut item = json!({
            "type": media_type,
            "url": url,
            "source": source,
        });
        if let Some(id) = id {
            item["id"] = json!(id);
        }
        if let Some(mime) = mime {
            item["mime"] = json!(mime);
        }
        if let Some(width) = width {
            item["width"] = json!(width);
        }
        if let Some(height) = height {
            item["height"] = json!(height);
        }

        media.push(item);
    }

    fn media_metadata_url(media_id: &str, metadata: &Value) -> Option<String> {
        let animated = metadata["e"].as_str().unwrap_or("") == "AnimatedImage";
        let selected = if animated {
            metadata["s"]["gif"]
                .as_str()
                .or_else(|| metadata["s"]["mp4"].as_str())
                .or_else(|| metadata["s"]["u"].as_str())
        } else {
            metadata["s"]["u"]
                .as_str()
                .or_else(|| metadata["s"]["gif"].as_str())
                .or_else(|| metadata["s"]["mp4"].as_str())
        };

        selected
            .map(Self::clean_reddit_url)
            .filter(|url| !url.is_empty())
            .or_else(|| {
                let mime = metadata["m"].as_str()?;
                let ext = Self::extension_from_mime(mime)?;
                Some(format!("https://i.redd.it/{}.{}", media_id, ext))
            })
    }

    fn media_type_from_metadata(metadata: &Value) -> &'static str {
        let e = metadata["e"].as_str().unwrap_or("");
        let mime = metadata["m"].as_str().unwrap_or("");
        if e == "AnimatedImage" || mime == "image/gif" {
            "animated"
        } else {
            "image"
        }
    }

    fn extension_from_mime(mime: &str) -> Option<&'static str> {
        match mime {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            _ => None,
        }
    }

    fn clean_reddit_url(url: &str) -> String {
        let url = url.trim().replace("&amp;", "&");
        if url.starts_with("//") {
            format!("https:{}", url)
        } else {
            url
        }
    }

    fn looks_like_media_url(url: &str) -> bool {
        let url = url
            .split('?')
            .next()
            .unwrap_or(url)
            .trim()
            .to_ascii_lowercase();
        matches!(
            url.rsplit('.').next(),
            Some(
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "gifv" | "mp4" | "webm" | "mov" | "m3u8"
            )
        )
    }

    fn media_type_from_url(url: &str) -> &'static str {
        let url = url
            .split('?')
            .next()
            .unwrap_or(url)
            .trim()
            .to_ascii_lowercase();
        match url.rsplit('.').next() {
            Some("gif" | "gifv") => "animated",
            Some("mp4" | "webm" | "mov" | "m3u8") => "video",
            _ => "image",
        }
    }

    async fn fetch_user_about_json(&self, username: &str) -> Result<Value, ConnectorError> {
        if username.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "Username must be non-empty".to_string(),
            ));
        }
        if !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ConnectorError::InvalidParams(
                "Username contains invalid characters".to_string(),
            ));
        }

        self.fetch_reddit_json(&format!("/user/{}/about.json", username), &[], false)
            .await
    }

    fn user_profile_from_about_json(
        username: &str,
        about: &Value,
    ) -> Result<Value, ConnectorError> {
        let data = about
            .get("data")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ConnectorError::Other(format!(
                    "Unexpected Reddit about.json response for user '{}'",
                    username
                ))
            })?;

        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(username)
            .to_string();
        let created_utc = data
            .get("created_utc")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let created_iso = Self::rfc3339_from_utc(created_utc);

        let link_karma = data.get("link_karma").and_then(|v| v.as_i64()).unwrap_or(0);
        let comment_karma = data
            .get("comment_karma")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let total_karma = link_karma.saturating_add(comment_karma);

        let account_age_days = Self::account_age_days(created_utc);

        Ok(json!({
            "name": name,
            "created_utc": created_utc,
            "created_iso": created_iso,
            "link_karma": link_karma,
            "comment_karma": comment_karma,
            "total_karma": total_karma,
            "account_age_days": account_age_days,
        }))
    }

    fn account_age_days(created_utc: f64) -> Option<i64> {
        if created_utc <= 0.0 {
            return None;
        }
        let now = chrono::Utc::now().timestamp() as f64;
        let delta = now - created_utc;
        if delta.is_finite() && delta >= 0.0 {
            Some((delta / 86_400.0).floor() as i64)
        } else {
            None
        }
    }

    fn parse_cursor<T: DeserializeOwned>(
        value: Option<&Value>,
    ) -> Result<Option<T>, ConnectorError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => {
                Ok(Some(ingest::decode_cursor::<T>(s).ok_or_else(|| {
                    ConnectorError::InvalidParams("Invalid cursor".to_string())
                })?))
            }
            _ => Err(ConnectorError::InvalidParams(
                "Cursor must be a string or null".to_string(),
            )),
        }
    }

    fn rfc3339_from_utc(utc: f64) -> Option<String> {
        if utc <= 0.0 {
            return None;
        }
        let secs = utc.trunc() as i64;
        let nanos = ((utc.fract()) * 1_000_000_000.0) as u32;
        chrono::DateTime::from_timestamp(secs, nanos).map(|dt| dt.to_rfc3339())
    }

    fn resolve_post_target(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<RedditPostTarget, ConnectorError> {
        let url = args
            .get("post_url")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("url").and_then(|v| v.as_str()))
            .or_else(|| {
                args.get("id")
                    .and_then(|v| v.as_str())
                    .filter(|id| id.starts_with("http://") || id.starts_with("https://"))
            });

        if let Some(url) = url {
            if let Some(info) = self.extract_post_info_from_url(url) {
                let subreddit = info
                    .subreddit
                    .as_deref()
                    .map(Self::normalize_subreddit_name)
                    .transpose()?;
                return Ok(RedditPostTarget {
                    post_id: Self::normalize_post_id(&info.post_id)?,
                    subreddit,
                });
            }
        }

        let item_ref = args.get("item_ref").and_then(|v| v.as_str()).or_else(|| {
            args.get("id")
                .and_then(|v| v.as_str())
                .filter(|id| id.starts_with("reddit:post:"))
        });

        if let Some(item_ref) = item_ref {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "reddit") {
                if kind == "post" {
                    return Ok(RedditPostTarget {
                        post_id: Self::normalize_post_id(&id)?,
                        subreddit: None,
                    });
                }
            }
        }

        if let Some(id) = args.get("id").and_then(|v| v.as_str()) {
            return Ok(RedditPostTarget {
                post_id: Self::normalize_post_id(id)?,
                subreddit: None,
            });
        }

        Err(ConnectorError::InvalidParams(
            "Missing post identifier. Provide id, post_url, url, or item_ref.".to_string(),
        ))
    }

    fn normalize_post_id(id: &str) -> Result<String, ConnectorError> {
        let id = id.trim().trim_start_matches("t3_");
        if id.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "Post id must be non-empty".to_string(),
            ));
        }
        if id.contains('/') || id.contains('?') || id.contains('#') {
            return Err(ConnectorError::InvalidParams(
                "Post id must not contain URL path or query characters".to_string(),
            ));
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ConnectorError::InvalidParams(
                "Post id contains invalid characters".to_string(),
            ));
        }

        Ok(id.to_string())
    }

    fn append_comment_blocks(
        comments: &[Value],
        parent_ref: Option<&str>,
        depth: i64,
        item_ref: &str,
        blocks: &mut Vec<ContentBlock>,
        relationships: &mut Vec<Relationship>,
    ) {
        for comment in comments {
            let id = comment["id"].as_str().unwrap_or("");
            if id.is_empty() {
                continue;
            }

            let block_ref = format!("reddit:comment:{}", id);
            let author_name = comment["author"].as_str().unwrap_or("");
            let author = if author_name.is_empty() {
                None
            } else {
                Some(Author {
                    name: author_name.to_string(),
                    id: None,
                })
            };
            let created_at = comment["created_utc"]
                .as_f64()
                .and_then(Self::rfc3339_from_utc);
            let position = Some(json!({ "kind": "thread_depth", "depth": depth }));
            let score = comment["score"].as_i64().map(|v| v as f64);
            let metadata = Some(json!({
                "permalink": comment["permalink"].as_str().unwrap_or(""),
                "is_submitter": comment["is_submitter"].as_bool().unwrap_or(false),
                "distinguished": comment["distinguished"].as_str().unwrap_or(""),
                "stickied": comment["stickied"].as_bool().unwrap_or(false),
            }));

            blocks.push(ContentBlock {
                block_ref: block_ref.clone(),
                block_kind: "comment".to_string(),
                text: comment["body"].as_str().unwrap_or("").to_string(),
                author,
                created_at,
                reply_to: parent_ref.map(str::to_string),
                position,
                score,
                attachments: Vec::new(),
                metadata,
            });

            relationships.push(Relationship {
                rel: "has_block".to_string(),
                from: item_ref.to_string(),
                to: block_ref.clone(),
            });
            if let Some(parent_ref) = parent_ref {
                relationships.push(Relationship {
                    rel: "replies_to".to_string(),
                    from: block_ref.clone(),
                    to: parent_ref.to_string(),
                });
            }

            if let Some(replies) = comment["replies"].as_array() {
                Self::append_comment_blocks(
                    replies,
                    Some(block_ref.as_str()),
                    depth.saturating_add(1),
                    item_ref,
                    blocks,
                    relationships,
                );
            }
        }
    }

    fn sort_for_morechildren(comment_sort: &str) -> &str {
        match comment_sort {
            // Reddit's /api/morechildren uses "confidence" instead of "best".
            "best" => "confidence",
            "top" => "top",
            "new" => "new",
            "controversial" => "controversial",
            "old" => "old",
            "qa" => "qa",
            // Keep same default behavior as the main comments endpoint.
            _ => "confidence",
        }
    }

    async fn fetch_comment_tree_with_more(
        &self,
        initial_children: &Value,
        link_fullname: &str,
        top_level_limit: u32,
        comment_sort: &str,
    ) -> Result<Vec<Value>, ConnectorError> {
        if top_level_limit == 0 {
            return Ok(Vec::new());
        }

        let mut order: u64 = 0;
        let mut comments_by_id: HashMap<String, CollectedComment> = HashMap::new();
        let mut more_queue: VecDeque<MorePlaceholder> = VecDeque::new();
        let mut seen: HashSet<String> = HashSet::new();

        Self::collect_from_listing(
            initial_children,
            link_fullname,
            &mut order,
            &mut comments_by_id,
            &mut more_queue,
            &mut seen,
        );

        let mut more_requests: usize = 0;

        while Self::top_level_count(&comments_by_id, link_fullname) < top_level_limit as usize
            && !more_queue.is_empty()
            && more_requests < MAX_MORECHILDREN_REQUESTS
            && comments_by_id.len() < MAX_TOTAL_COMMENTS
        {
            // Prefer placeholders that expand top-level comments (parent == link fullname).
            let preferred_idx = more_queue
                .iter()
                .position(|m| m.parent_fullname == link_fullname);
            let more = preferred_idx
                .and_then(|idx| more_queue.remove(idx))
                .unwrap_or_else(|| {
                    more_queue.pop_front().unwrap_or_else(|| MorePlaceholder {
                        parent_fullname: link_fullname.to_string(),
                        children: Vec::new(),
                        depth: 0,
                    })
                });

            if more.children.is_empty() {
                continue;
            }

            for chunk in more.children.chunks(MORECHILDREN_BATCH_SIZE) {
                if Self::top_level_count(&comments_by_id, link_fullname) >= top_level_limit as usize
                    || comments_by_id.len() >= MAX_TOTAL_COMMENTS
                    || more_requests >= MAX_MORECHILDREN_REQUESTS
                {
                    break;
                }

                let unfetched: Vec<String> = chunk
                    .iter()
                    .filter(|id| !seen.contains(*id))
                    .cloned()
                    .collect();
                if unfetched.is_empty() {
                    continue;
                }

                let things = self
                    .fetch_morechildren_things(link_fullname, &unfetched, comment_sort, more.depth)
                    .await?;
                more_requests += 1;

                Self::collect_from_things(
                    &things,
                    link_fullname,
                    &mut order,
                    &mut comments_by_id,
                    &mut more_queue,
                    &mut seen,
                );
            }
        }

        Ok(Self::build_comment_tree(
            &comments_by_id,
            link_fullname,
            top_level_limit as usize,
        ))
    }

    fn top_level_count(
        comments_by_id: &HashMap<String, CollectedComment>,
        link_fullname: &str,
    ) -> usize {
        comments_by_id
            .values()
            .filter(|c| c.parent_fullname == link_fullname)
            .count()
    }

    fn collect_from_listing(
        children: &Value,
        link_fullname: &str,
        order: &mut u64,
        comments_by_id: &mut HashMap<String, CollectedComment>,
        more_queue: &mut VecDeque<MorePlaceholder>,
        seen: &mut HashSet<String>,
    ) {
        let empty_vec = Vec::new();
        let items = children.as_array().unwrap_or(&empty_vec);
        Self::collect_from_things(
            items,
            link_fullname,
            order,
            comments_by_id,
            more_queue,
            seen,
        );
    }

    fn collect_from_things(
        things: &[Value],
        link_fullname: &str,
        order: &mut u64,
        comments_by_id: &mut HashMap<String, CollectedComment>,
        more_queue: &mut VecDeque<MorePlaceholder>,
        seen: &mut HashSet<String>,
    ) {
        for thing in things {
            let kind = thing["kind"].as_str().unwrap_or("");
            match kind {
                "t1" => {
                    let data = &thing["data"];
                    let id = data["id"].as_str().unwrap_or("").to_string();
                    if id.is_empty() || seen.contains(&id) {
                        continue;
                    }

                    let parent_fullname = data["parent_id"]
                        .as_str()
                        .unwrap_or(link_fullname)
                        .to_string();
                    let comment = CollectedComment {
                        id: id.clone(),
                        parent_fullname,
                        author: data["author"].as_str().unwrap_or("").to_string(),
                        body: data["body"].as_str().unwrap_or("").to_string(),
                        body_html: data["body_html"].as_str().unwrap_or("").to_string(),
                        score: data["score"].as_i64().unwrap_or(0),
                        created_utc: data["created_utc"].as_f64().unwrap_or(0.0),
                        permalink: data["permalink"].as_str().unwrap_or("").to_string(),
                        is_submitter: data["is_submitter"].as_bool().unwrap_or(false),
                        distinguished: data["distinguished"].as_str().unwrap_or("").to_string(),
                        stickied: data["stickied"].as_bool().unwrap_or(false),
                        order: *order,
                    };
                    *order = order.saturating_add(1);
                    seen.insert(id.clone());
                    comments_by_id.insert(id.clone(), comment);

                    if data["replies"].is_object() {
                        let replies = &data["replies"]["data"]["children"];
                        let empty_vec = Vec::new();
                        let reply_items = replies.as_array().unwrap_or(&empty_vec);
                        Self::collect_from_things(
                            reply_items,
                            link_fullname,
                            order,
                            comments_by_id,
                            more_queue,
                            seen,
                        );
                    }
                }
                "more" => {
                    let data = &thing["data"];
                    let parent_fullname = data["parent_id"]
                        .as_str()
                        .unwrap_or(link_fullname)
                        .to_string();
                    let depth = data["depth"].as_i64().unwrap_or(0);
                    let children = data["children"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    if !children.is_empty() {
                        more_queue.push_back(MorePlaceholder {
                            parent_fullname,
                            children,
                            depth,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    async fn fetch_morechildren_things(
        &self,
        link_fullname: &str,
        children: &[String],
        comment_sort: &str,
        depth: i64,
    ) -> Result<Vec<Value>, ConnectorError> {
        let params = vec![
            ("api_type".to_string(), "json".to_string()),
            ("link_id".to_string(), link_fullname.to_string()),
            ("children".to_string(), children.join(",")),
            ("limit_children".to_string(), "true".to_string()),
            (
                "sort".to_string(),
                Self::sort_for_morechildren(comment_sort).to_string(),
            ),
            ("raw_json".to_string(), "1".to_string()),
            ("depth".to_string(), depth.to_string()),
        ];

        let data = self
            .fetch_reddit_json("/api/morechildren.json", &params, false)
            .await?;
        let things = data["json"]["data"]["things"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(things)
    }

    fn build_comment_tree(
        comments_by_id: &HashMap<String, CollectedComment>,
        link_fullname: &str,
        top_level_limit: usize,
    ) -> Vec<Value> {
        let mut children_by_parent: HashMap<String, Vec<String>> = HashMap::new();
        for (id, comment) in comments_by_id {
            children_by_parent
                .entry(comment.parent_fullname.clone())
                .or_default()
                .push(id.clone());
        }

        for children in children_by_parent.values_mut() {
            children.sort_by_key(|id| comments_by_id.get(id).map(|c| c.order).unwrap_or(u64::MAX));
        }

        let top_level_ids = children_by_parent
            .get(link_fullname)
            .cloned()
            .unwrap_or_default();

        top_level_ids
            .into_iter()
            .take(top_level_limit)
            .filter_map(|id| Self::render_comment(&id, comments_by_id, &children_by_parent, 0))
            .collect()
    }

    fn render_comment(
        id: &str,
        comments_by_id: &HashMap<String, CollectedComment>,
        children_by_parent: &HashMap<String, Vec<String>>,
        depth: i64,
    ) -> Option<Value> {
        let comment = comments_by_id.get(id)?;

        let fullname = format!("t1_{}", comment.id);
        let reply_ids = children_by_parent
            .get(&fullname)
            .cloned()
            .unwrap_or_default();
        let replies: Vec<Value> = reply_ids
            .into_iter()
            .filter_map(|rid| {
                Self::render_comment(&rid, comments_by_id, children_by_parent, depth + 1)
            })
            .collect();

        Some(json!({
            "id": comment.id,
            "author": comment.author,
            "body": comment.body,
            "body_html": comment.body_html,
            "score": comment.score,
            "created_utc": comment.created_utc,
            "permalink": comment.permalink,
            "depth": depth,
            "is_submitter": comment.is_submitter,
            "distinguished": comment.distinguished,
            "stickied": comment.stickied,
            "replies": replies
        }))
    }
}

#[derive(Debug, Clone)]
struct MorePlaceholder {
    parent_fullname: String,
    children: Vec<String>,
    depth: i64,
}

#[derive(Debug, Clone)]
struct CollectedComment {
    id: String,
    parent_fullname: String,
    author: String,
    body: String,
    body_html: String,
    score: i64,
    created_utc: f64,
    permalink: String,
    is_submitter: bool,
    distinguished: String,
    stickied: bool,
    order: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as JsonValue;

    #[tokio::test]
    async fn rejects_invalid_proxy_url() {
        let mut connector = RedditConnector::new(AuthDetails::new()).await.unwrap();
        let mut details = AuthDetails::new();
        details.insert("proxy_url".to_string(), "not a url".to_string());

        let err = connector.set_auth_details(details).await.unwrap_err();
        match err {
            ConnectorError::InvalidParams(_) => {}
            other => panic!("expected InvalidParams, got: {other:?}"),
        }
    }

    #[test]
    fn builds_tree_from_morechildren_things() {
        let link_fullname = "t3_post";

        let initial_children = json!([
            { "kind": "t1", "data": { "id": "c1", "parent_id": "t3_post", "author": "a", "body": "b", "body_html": "h", "score": 1, "created_utc": 1.0, "permalink": "/r/x/comments/post/_/c1", "depth": 0, "is_submitter": false, "distinguished": "", "stickied": false, "replies": "" } },
            { "kind": "more", "data": { "parent_id": "t3_post", "children": ["c2", "c3"], "depth": 0 } }
        ]);

        let mut order = 0u64;
        let mut comments_by_id: HashMap<String, CollectedComment> = HashMap::new();
        let mut more_queue: VecDeque<MorePlaceholder> = VecDeque::new();
        let mut seen: HashSet<String> = HashSet::new();
        RedditConnector::collect_from_listing(
            &initial_children,
            link_fullname,
            &mut order,
            &mut comments_by_id,
            &mut more_queue,
            &mut seen,
        );

        assert_eq!(comments_by_id.len(), 1);
        assert_eq!(more_queue.len(), 1);

        let more_things = vec![
            json!({ "kind": "t1", "data": { "id": "c2", "parent_id": "t3_post", "author": "a2", "body": "b2", "body_html": "h2", "score": 2, "created_utc": 2.0, "permalink": "/r/x/comments/post/_/c2", "is_submitter": false, "distinguished": "", "stickied": false } }),
            json!({ "kind": "t1", "data": { "id": "c3", "parent_id": "t3_post", "author": "a3", "body": "b3", "body_html": "h3", "score": 3, "created_utc": 3.0, "permalink": "/r/x/comments/post/_/c3", "is_submitter": false, "distinguished": "", "stickied": false } }),
            json!({ "kind": "t1", "data": { "id": "r1", "parent_id": "t1_c2", "author": "ar", "body": "br", "body_html": "hr", "score": 1, "created_utc": 4.0, "permalink": "/r/x/comments/post/_/r1", "is_submitter": false, "distinguished": "", "stickied": false } }),
        ];

        RedditConnector::collect_from_things(
            &more_things,
            link_fullname,
            &mut order,
            &mut comments_by_id,
            &mut more_queue,
            &mut seen,
        );

        let tree = RedditConnector::build_comment_tree(&comments_by_id, link_fullname, 10);
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0]["id"], "c1");
        assert_eq!(tree[1]["id"], "c2");
        assert_eq!(tree[2]["id"], "c3");

        assert_eq!(tree[1]["depth"], 0);
        assert_eq!(tree[1]["replies"][0]["id"], "r1");
        assert_eq!(tree[1]["replies"][0]["depth"], 1);
    }

    #[test]
    fn parses_user_about_fixture_offline() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/reddit_user_about.json"
        ));
        let about: JsonValue = serde_json::from_str(raw).expect("fixture json parses");
        let profile =
            RedditConnector::user_profile_from_about_json("spez", &about).expect("profile parses");

        assert_eq!(profile["name"], "spez");
        assert!(profile["created_utc"].as_f64().unwrap_or(0.0) > 0.0);
        assert!(profile["created_iso"].as_str().unwrap_or("").contains('T'));
        assert_eq!(profile["link_karma"].as_i64().unwrap_or(0), 123);
        assert_eq!(profile["comment_karma"].as_i64().unwrap_or(0), 456);
        assert_eq!(profile["total_karma"].as_i64().unwrap_or(0), 579);
    }

    #[test]
    fn resolves_gallery_media_in_reddit_order() {
        let post = json!({
            "gallery_data": {
                "items": [
                    { "media_id": "second" },
                    { "media_id": "first" }
                ]
            },
            "media_metadata": {
                "first": {
                    "e": "Image",
                    "m": "image/png",
                    "s": { "u": "https://preview.redd.it/first.png?width=800&amp;format=png", "x": 800, "y": 600 }
                },
                "second": {
                    "e": "AnimatedImage",
                    "m": "image/gif",
                    "s": { "gif": "https://preview.redd.it/second.gif?format=mp4&amp;s=abc", "x": 320, "y": 240 }
                }
            }
        });

        let media = RedditConnector::resolved_media_for_post(&post);
        assert_eq!(media.len(), 2);
        assert_eq!(media[0]["id"], "second");
        assert_eq!(media[0]["type"], "animated");
        assert_eq!(
            media[0]["url"],
            "https://preview.redd.it/second.gif?format=mp4&s=abc"
        );
        assert_eq!(media[1]["id"], "first");
        assert_eq!(media[1]["type"], "image");
    }

    #[test]
    fn resolves_reddit_video_media() {
        let post = json!({
            "is_video": true,
            "media": {
                "reddit_video": {
                    "fallback_url": "https://v.redd.it/abc/DASH_720.mp4?source=fallback&amp;foo=1",
                    "hls_url": "https://v.redd.it/abc/HLSPlaylist.m3u8?a=1&amp;b=2",
                    "dash_url": "https://v.redd.it/abc/DASHPlaylist.mpd",
                    "width": 1280,
                    "height": 720,
                    "duration": 12,
                    "is_gif": false
                }
            }
        });

        let media = RedditConnector::resolved_media_for_post(&post);
        assert_eq!(media.len(), 1);
        assert_eq!(media[0]["type"], "video");
        assert_eq!(
            media[0]["url"],
            "https://v.redd.it/abc/DASH_720.mp4?source=fallback&foo=1"
        );
        assert_eq!(
            media[0]["hls_url"],
            "https://v.redd.it/abc/HLSPlaylist.m3u8?a=1&b=2"
        );
    }

    #[test]
    fn listing_raw_post_includes_media_archive_fields() {
        let post = json!({
            "id": "abc123",
            "name": "t3_abc123",
            "title": "Gallery post",
            "subreddit": "rust",
            "author": "Ferris",
            "score": 42,
            "upvote_ratio": 0.95,
            "num_comments": 7,
            "permalink": "/r/rust/comments/abc123/gallery_post/",
            "created_utc": 1_700_000_000.0,
            "url": "https://www.reddit.com/gallery/abc123",
            "url_overridden_by_dest": "https://www.reddit.com/gallery/abc123",
            "domain": "reddit.com",
            "thumbnail": "default",
            "post_hint": "image",
            "is_video": false,
            "is_self": false,
            "is_gallery": true,
            "over_18": true,
            "spoiler": false,
            "stickied": false,
            "gallery_data": { "items": [{ "media_id": "img1" }] },
            "media_metadata": {
                "img1": {
                    "e": "Image",
                    "m": "image/jpeg",
                    "s": { "u": "https://preview.redd.it/img1.jpg", "x": 100, "y": 80 }
                }
            },
            "preview": { "images": [] },
            "crosspost_parent_list": []
        });

        let raw = RedditConnector::listing_raw_post(&post);
        assert_eq!(raw["id"], "abc123");
        assert_eq!(raw["over_18"], true);
        assert_eq!(raw["is_gallery"], true);
        assert!(raw["gallery_data"].is_object());
        assert!(raw["media_metadata"].is_object());
        assert_eq!(raw["resolved_media"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn resolves_bare_item_ref_and_generic_comments_url() {
        let connector = RedditConnector::new(AuthDetails::new()).await.unwrap();
        let args = json!({ "id": "reddit:post:abc123" })
            .as_object()
            .unwrap()
            .clone();
        let target = connector.resolve_post_target(&args).unwrap();
        assert_eq!(target.post_id, "abc123");
        assert_eq!(target.subreddit, None);

        let args = json!({ "url": "https://www.reddit.com/comments/def456/example/" })
            .as_object()
            .unwrap()
            .clone();
        let target = connector.resolve_post_target(&args).unwrap();
        assert_eq!(target.post_id, "def456");
        assert_eq!(target.subreddit, None);
    }
}
