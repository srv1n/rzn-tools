use async_trait::async_trait;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;

const SLACK_API_BASE: &str = "https://slack.com/api";
const SLACK_MAX_PER_REQUEST: u32 = 200;
const SLACK_MAX_TOTAL: u32 = 5_000;
const SLACK_MAX_REQUESTS: usize = 100;

#[derive(Clone)]
pub struct SlackConnector {
    client: reqwest::Client,
    auth: AuthDetails,
}

impl SlackConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .user_agent("rzn-datasourcer/0.1 slack-connector")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(Self { client, auth })
    }

    fn resolve_token(&self) -> Option<String> {
        if let Some(t) = self.auth.get("token") {
            return Some(t.clone());
        }
        let store = FileAuthStore::new_default();
        store
            .load(self.name())
            .and_then(|m| m.get("token").cloned())
    }

    async fn api_get(
        &self,
        method: &str,
        params: &[(&str, String)],
    ) -> Result<Value, ConnectorError> {
        let token = self.resolve_token().ok_or_else(|| {
            ConnectorError::Authentication("Slack token not configured".to_string())
        })?;
        let url = format!("{}/{}", SLACK_API_BASE, method);
        self.send_with_backoff(|client| client.get(&url).bearer_auth(&token).query(&params))
            .await
    }

    async fn send_with_backoff<F>(&self, build: F) -> Result<Value, ConnectorError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration};
        const MAX_RETRIES: usize = 4; // total attempts = 1 + retries
        let mut delay_ms = 800u64;
        let mut last_status: Option<u16> = None;

        for attempt in 0..=MAX_RETRIES {
            let resp = build(&self.client)
                .try_clone()
                .unwrap_or_else(|| build(&self.client))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status.as_u16() == 429 {
                        // Rate-limited: compute wait
                        let retry_after = r
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(Duration::from_secs)
                            .unwrap_or_else(|| Duration::from_millis(delay_ms));
                        if attempt == MAX_RETRIES {
                            return Err(ConnectorError::Other(format!(
                                "Slack rate limited (429) after {} attempts",
                                attempt + 1
                            )));
                        }
                        sleep(retry_after).await;
                        delay_ms = (delay_ms as f64 * 1.8) as u64; // exponential-ish
                        last_status = Some(429);
                        continue;
                    }
                    if status.is_server_error() {
                        if attempt == MAX_RETRIES {
                            let body = r.text().await.unwrap_or_default();
                            return Err(ConnectorError::Other(format!(
                                "Slack server error {}: {}",
                                status.as_u16(),
                                body
                            )));
                        }
                        sleep(Duration::from_millis(delay_ms)).await;
                        delay_ms = (delay_ms as f64 * 1.6) as u64;
                        last_status = Some(status.as_u16());
                        continue;
                    }
                    // Parse JSON and check ok
                    let v: Value = r.json().await.map_err(ConnectorError::HttpRequest)?;
                    if v.get("ok").and_then(|x| x.as_bool()) == Some(true) {
                        return Ok(v);
                    } else {
                        let err = v
                            .get("error")
                            .and_then(|e| e.as_str())
                            .unwrap_or("unknown_error");
                        return Err(ConnectorError::Other(format!("Slack API error: {}", err)));
                    }
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        return Err(ConnectorError::HttpRequest(e));
                    }
                    // network error: backoff then retry
                    sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms as f64 * 1.6) as u64;
                    last_status = None;
                    continue;
                }
            }
        }
        Err(ConnectorError::Other(format!(
            "Slack request failed after retries (last_status={:?})",
            last_status
        )))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ListChannelsInput {
    #[serde(default = "default_types")]
    types: String, // e.g., "public_channel,private_channel,im,mpim"
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>, // 1..=200
}

fn default_types() -> String {
    "public_channel,private_channel".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct ListMessagesInput {
    channel: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    oldest: Option<String>,
    #[serde(default)]
    latest: Option<String>,
    #[serde(default)]
    inclusive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetThreadInput {
    channel: String,
    thread_ts: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchMessagesInput {
    query: String,
    #[serde(default)]
    sort: Option<String>, // score|timestamp
    #[serde(default)]
    sort_dir: Option<String>, // asc|desc
    #[serde(default)]
    count: Option<u32>, // results per page
    #[serde(default)]
    page: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListFilesInput {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    ts_from: Option<String>,
    #[serde(default)]
    ts_to: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListUsersInput {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetThreadByPermalinkInput {
    permalink: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

fn ts_from_p_segment(p: &str) -> Option<String> {
    // p-segment format: p{16 digits}, e.g., p1716932719000123 → 1716932719.000123
    if p.len() == 17 && p.starts_with('p') {
        let digits = &p[1..];
        if digits.len() == 16 && digits.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}.{}", &digits[0..10], &digits[10..16]));
        }
    }
    None
}

fn normalize_ts(s: &str) -> Option<String> {
    // Accept dotted ts (e.g., 1716932719.000123) or 16-digit, or p-segment
    let s = s.trim();
    if let Some(ts) = ts_from_p_segment(s) {
        return Some(ts);
    }
    if s.contains('.') {
        // basic validation
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() == 2
            && parts[0].chars().all(|c| c.is_ascii_digit())
            && parts[1].chars().all(|c| c.is_ascii_digit())
        {
            return Some(s.to_string());
        }
    } else if s.len() == 16 && s.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("{}.{}", &s[0..10], &s[10..16]));
    }
    None
}

fn parse_permalink(permalink: &str) -> Option<(String, String, Option<String>)> {
    // Returns (channel_id, message_ts, thread_ts_opt)
    if let Ok(url) = url::Url::parse(permalink) {
        let path = url.path().to_string();
        // Try to extract channel id (Cxxxx) from path segments
        let channel_re = regex::Regex::new(r"/(C[0-9A-Z]+)/").ok()?;
        let channel_id = channel_re
            .captures(&(path.clone() + "/"))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())?;

        // Find first p-segment (p{16}) in path or selected query
        let p_re = regex::Regex::new(r"p\d{16}").ok()?;
        let mut message_ts: Option<String> = None;
        if let Some(m) = p_re.find(&path) {
            message_ts = ts_from_p_segment(m.as_str());
        }
        if message_ts.is_none() {
            if let Some(query) = url.query() {
                if let Some(m) = p_re.find(query) {
                    message_ts = ts_from_p_segment(m.as_str());
                }
            }
        }
        // thread_ts from query param if present
        let thread_ts = url
            .query_pairs()
            .find(|(k, _)| k == "thread_ts")
            .and_then(|(_, v)| normalize_ts(&v));

        if let Some(msg_ts) = message_ts {
            return Some((channel_id, msg_ts, thread_ts));
        }
    }
    None
}

fn slack_next_cursor(v: &Value) -> Option<String> {
    v.get("response_metadata")
        .and_then(|m| m.get("next_cursor"))
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[async_trait]
impl Connector for SlackConnector {
    fn name(&self) -> &'static str {
        "slack"
    }

    fn description(&self) -> &'static str {
        "Slack Web API: channels/DMs/threads/messages/files (read-only MVP)."
    }

    fn display_name(&self) -> &'static str {
        "Slack"
    }

    fn icon(&self) -> &'static str {
        "slack"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "communication"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(Default::default()),
            ..Default::default()
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
                "Provide a Slack token via `rzn-tools config set slack --value <xoxb-...>`."
                    .to_string(),
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
        let tools = vec![
            Tool {
                name: Cow::Borrowed("test_auth"),
                title: None,
                description: Some(Cow::Borrowed("Validate token and return team/user info.")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("list_channels"),
                title: None,
                description: Some(Cow::Borrowed("List channels/DMs the token can access.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "types": {"type":"string","description":"public_channel,private_channel,im,mpim"},
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000}
	                    }
	                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("list_messages"),
                title: None,
                description: Some(Cow::Borrowed("List recent messages in a channel.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "channel":{"type":"string"},
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000},
	                        "oldest": {"type":"string"},
	                        "latest": {"type":"string"},
	                        "inclusive": {"type":"boolean"}
                    },
                    "required":["channel"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("get_thread"),
                title: None,
                description: Some(Cow::Borrowed("Fetch a thread (root + replies) by channel and thread_ts.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "channel":{"type":"string"},
                        "thread_ts":{"type":"string"},
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000}
	                    },
	                    "required":["channel","thread_ts"]
	                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_messages"),
                title: None,
                description: Some(Cow::Borrowed("Search Slack messages across accessible conversations.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "query":{"type":"string"},
                        "sort": {"type":"string","enum":["score","timestamp"]},
                        "sort_dir": {"type":"string","enum":["asc","desc"]},
                        "count": {"type":"integer","minimum":1,"maximum":100},
                        "page": {"type":"integer","minimum":1}
                    },
                    "required":["query"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("list_files"),
                title: None,
                description: Some(Cow::Borrowed("List files by channel/user/time window.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "channel":{"type":"string"},
                        "user":{"type":"string"},
                        "ts_from":{"type":"string"},
                        "ts_to":{"type":"string"},
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000}
	                    }
	                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("get_thread_by_permalink"),
                title: None,
                description: Some(Cow::Borrowed("Resolve a Slack message permalink and fetch the thread (root + replies). If the link is a reply, uses thread_ts when present.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "permalink": {"type":"string"},
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000}
	                    },
	                    "required":["permalink"]
	                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
	            Tool {
	                name: Cow::Borrowed("list_users"),
                title: None,
                description: Some(Cow::Borrowed("List workspace users the token can see.")),
	                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "cursor": {"type":"string"},
	                        "limit": {"type":"integer","minimum":1,"maximum":5000}
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
        let args_map = serde_json::Map::from_iter(args);

        match name {
            "test_auth" => {
                let v = self.api_get("auth.test", &[]).await?;
                structured_result_with_text(&v, None)
            }
            "list_channels" => {
                let input: ListChannelsInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;
                let types = input.types.clone();

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| {
                        let types = types.clone();
                        async move {
                            let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                            let mut params =
                                vec![("types", types), ("limit", per_page.to_string())];
                            if let Some(c) = cursor {
                                params.push(("cursor", c));
                            }
                            let v = self.api_get("conversations.list", &params).await?;
                            let items = v
                                .get("channels")
                                .and_then(|x| x.as_array())
                                .cloned()
                                .unwrap_or_default();
                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: slack_next_cursor(&v),
                            })
                        }
                    },
                    |c: &Value| c.get("id").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "channels": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
            }
            "list_messages" => {
                let input: ListMessagesInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;
                let channel = input.channel.clone();
                let oldest = input.oldest.clone();
                let latest = input.latest.clone();
                let inclusive = input.inclusive;

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| {
                        let channel = channel.clone();
                        let oldest = oldest.clone();
                        let latest = latest.clone();
                        async move {
                            let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                            let mut params =
                                vec![("channel", channel), ("limit", per_page.to_string())];
                            if let Some(c) = cursor {
                                params.push(("cursor", c));
                            }
                            if let Some(o) = oldest {
                                params.push(("oldest", o));
                            }
                            if let Some(l) = latest {
                                params.push(("latest", l));
                            }
                            if let Some(inc) = inclusive {
                                params.push(("inclusive", (inc as u8).to_string()));
                            }

                            let v = self.api_get("conversations.history", &params).await?;
                            let items = v
                                .get("messages")
                                .and_then(|x| x.as_array())
                                .cloned()
                                .unwrap_or_default();
                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: slack_next_cursor(&v),
                            })
                        }
                    },
                    |m: &Value| m.get("ts").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "messages": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
            }
            "get_thread" => {
                let input: GetThreadInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;
                let channel = input.channel.clone();
                let ts = input.thread_ts.clone();

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| {
                        let channel = channel.clone();
                        let ts = ts.clone();
                        async move {
                            let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                            let mut params = vec![
                                ("channel", channel),
                                ("ts", ts),
                                ("limit", per_page.to_string()),
                            ];
                            if let Some(c) = cursor {
                                params.push(("cursor", c));
                            }
                            let v = self.api_get("conversations.replies", &params).await?;
                            let items = v
                                .get("messages")
                                .and_then(|x| x.as_array())
                                .cloned()
                                .unwrap_or_default();
                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: slack_next_cursor(&v),
                            })
                        }
                    },
                    |m: &Value| m.get("ts").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "messages": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
            }
            "search_messages" => {
                let input: SearchMessagesInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![("query", input.query)];
                if let Some(s) = input.sort {
                    params.push(("sort", s));
                }
                if let Some(sd) = input.sort_dir {
                    params.push(("sort_dir", sd));
                }
                if let Some(c) = input.count {
                    params.push(("count", c.to_string()));
                }
                if let Some(p) = input.page {
                    params.push(("page", p.to_string()));
                }
                let v = self.api_get("search.messages", &params).await?;
                // Structure: { messages: { matches: [...], pagination/ paging }, ... }
                let out = json!({
                    "messages": v.get("messages").cloned().unwrap_or(json!({})),
                });
                structured_result_with_text(&out, None)
            }
            "list_files" => {
                let input: ListFilesInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;
                let channel = input.channel.clone();
                let user = input.user.clone();
                let ts_from = input.ts_from.clone();
                let ts_to = input.ts_to.clone();

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| {
                        let channel = channel.clone();
                        let user = user.clone();
                        let ts_from = ts_from.clone();
                        let ts_to = ts_to.clone();
                        async move {
                            let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                            let mut params: Vec<(&str, String)> =
                                vec![("limit", per_page.to_string())];
                            if let Some(ch) = channel {
                                params.push(("channel", ch));
                            }
                            if let Some(u) = user {
                                params.push(("user", u));
                            }
                            if let Some(f) = ts_from {
                                params.push(("ts_from", f));
                            }
                            if let Some(t) = ts_to {
                                params.push(("ts_to", t));
                            }
                            if let Some(c) = cursor {
                                params.push(("cursor", c));
                            }

                            let v = self.api_get("files.list", &params).await?;
                            let items = v
                                .get("files")
                                .and_then(|x| x.as_array())
                                .cloned()
                                .unwrap_or_default();
                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: slack_next_cursor(&v),
                            })
                        }
                    },
                    |f: &Value| f.get("id").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "files": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
            }
            "get_thread_by_permalink" => {
                let input: GetThreadByPermalinkInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let (channel, msg_ts, thread_ts_opt) = parse_permalink(&input.permalink)
                    .ok_or_else(|| {
                        ConnectorError::InvalidInput("Could not parse Slack permalink".to_string())
                    })?;
                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;
                let parent_ts = thread_ts_opt.unwrap_or(msg_ts);
                let channel_id = channel.clone();
                let ts = parent_ts.clone();

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| {
                        let channel_id = channel_id.clone();
                        let ts = ts.clone();
                        async move {
                            let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                            let mut params = vec![
                                ("channel", channel_id),
                                ("ts", ts),
                                ("limit", per_page.to_string()),
                            ];
                            if let Some(c) = cursor {
                                params.push(("cursor", c));
                            }
                            let v = self.api_get("conversations.replies", &params).await?;
                            let items = v
                                .get("messages")
                                .and_then(|x| x.as_array())
                                .cloned()
                                .unwrap_or_default();
                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: slack_next_cursor(&v),
                            })
                        }
                    },
                    |m: &Value| m.get("ts").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "messages": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
            }
            "list_users" => {
                let input: ListUsersInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let desired = input
                    .limit
                    .unwrap_or(SLACK_MAX_PER_REQUEST)
                    .clamp(1, SLACK_MAX_TOTAL) as usize;

                let collected = collect_paginated_with_cursor(
                    desired,
                    SLACK_MAX_REQUESTS,
                    input.cursor,
                    |cursor, remaining| async move {
                        let per_page = (remaining as u32).clamp(1, SLACK_MAX_PER_REQUEST);
                        let mut params: Vec<(&str, String)> = vec![("limit", per_page.to_string())];
                        if let Some(c) = cursor {
                            params.push(("cursor", c));
                        }
                        let v = self.api_get("users.list", &params).await?;
                        let items = v
                            .get("members")
                            .and_then(|x| x.as_array())
                            .cloned()
                            .unwrap_or_default();
                        Ok::<_, ConnectorError>(Page {
                            items,
                            next_cursor: slack_next_cursor(&v),
                        })
                    },
                    |m: &Value| m.get("id").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let out = json!({
                    "members": collected.items,
                    "response_metadata": collected.next_cursor.map(|c| json!({"next_cursor": c})).unwrap_or(json!({}))
                });
                structured_result_with_text(&out, None)
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
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = details.clone();
        // Persist for CLI convenience
        if !self.auth.is_empty() {
            let store = FileAuthStore::new_default();
            let _ = store.save(self.name(), &details);
        }
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self.api_get("auth.test", &[]).await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "token".to_string(),
                label: "Slack Token (xoxb/xoxp)".to_string(),
                field_type: FieldType::Secret,
                required: false,
                description: Some("Provide a bot (xoxb) or user (xoxp) token with read scopes (conversations:read, channels:history, groups:history, im:history, mpim:history, users:read, files:read, search:read).".to_string()),
                options: None,
            }],
        }
    }
}
