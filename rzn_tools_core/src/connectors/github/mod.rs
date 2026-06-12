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
use crate::utils::structured_result_with_text;
use crate::Connector;
use crate::{URLParamExtraction, URLPatternSpec};

#[derive(Clone)]
pub struct GitHubConnector {
    auth: AuthDetails,
}

impl GitHubConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
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

    fn octo(&self) -> Result<octocrab::Octocrab, ConnectorError> {
        let token = self.resolve_token().ok_or_else(|| {
            ConnectorError::Authentication("GitHub token not configured".to_string())
        })?;
        octocrab::Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))
    }

    async fn send_with_backoff<F>(&self, build: F) -> Result<Value, ConnectorError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration};
        let client = reqwest::Client::builder()
            .user_agent("rzn-datasourcer/0.1 github-connector")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        const MAX_RETRIES: usize = 4;
        let mut delay_ms = 700u64;
        for attempt in 0..=MAX_RETRIES {
            let resp = build(&client)
                .try_clone()
                .unwrap_or_else(|| build(&client))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status.as_u16() == 429
                        || (status.as_u16() == 403 && r.headers().get("Retry-After").is_some())
                    {
                        let ra = r
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(delay_ms.div_ceil(1000));
                        if attempt == MAX_RETRIES {
                            return Err(ConnectorError::Other("GitHub rate limited".into()));
                        }
                        sleep(Duration::from_secs(ra)).await;
                        delay_ms = (delay_ms as f64 * 1.8) as u64;
                        continue;
                    }
                    if status.is_server_error() {
                        if attempt == MAX_RETRIES {
                            return Err(ConnectorError::Other(format!("HTTP {}", status)));
                        }
                        sleep(Duration::from_millis(delay_ms)).await;
                        delay_ms = (delay_ms as f64 * 1.6) as u64;
                        continue;
                    }
                    let v = r
                        .json::<Value>()
                        .await
                        .map_err(ConnectorError::HttpRequest)?;
                    return Ok(v);
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        return Err(ConnectorError::HttpRequest(e));
                    }
                    sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms as f64 * 1.6) as u64;
                    continue;
                }
            }
        }
        Err(ConnectorError::Other("request failed after retries".into()))
    }
}

/// Response format for controlling output verbosity
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    #[default]
    Concise,
    Detailed,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListIssuesInput {
    owner: String,
    repo: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    labels: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    per_page: Option<u8>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    response_format: ResponseFormat,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetIssueInput {
    owner: String,
    repo: String,
    number: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListPullsInput {
    owner: String,
    repo: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    per_page: Option<u8>,
    #[serde(default)]
    page: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetPullInput {
    owner: String,
    repo: String,
    number: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CodeSearchInput {
    query: String,
    #[serde(default)]
    per_page: Option<u8>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    response_format: ResponseFormat,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoSearchInput {
    query: String,
    #[serde(default)]
    per_page: Option<u8>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    response_format: ResponseFormat,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetRepositoryInput {
    owner: String,
    repo: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetFileInput {
    owner: String,
    repo: String,
    path: String,
    #[serde(default)]
    r#ref: Option<String>,
}

#[async_trait]
impl Connector for GitHubConnector {
    fn name(&self) -> &'static str {
        "github"
    }

    fn description(&self) -> &'static str {
        "GitHub issues/PRs/discussions, code search, and file fetch (read-only)."
    }

    fn display_name(&self) -> &'static str {
        "GitHub"
    }

    fn icon(&self) -> &'static str {
        "github"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["code", "developer", "collaboration"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?github\.com/([^/]+)/([^/]+)".to_string(),
            default_tool: "get_repository".to_string(),
            description: "Fetch repository details".to_string(),
            param_extraction: vec![
                URLParamExtraction {
                    capture_group: 1,
                    param_name: "owner".to_string(),
                    use_full_url: false,
                },
                URLParamExtraction {
                    capture_group: 2,
                    param_name: "repo".to_string(),
                    use_full_url: false,
                },
            ],
        }]
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
            server_info: Implementation { name: self.name().to_string(), title: None, version: "0.1.0".to_string(), icons: None, website_url: None },
            instructions: Some(
                "Set a fine-grained PAT with repo read + metadata scopes: `rzn-tools config set github --value <token>`. Use `search_repositories`/`code_search` for discovery, then `get_repository`/`get_file` for details."
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
                description: Some(Cow::Borrowed("Validate token and return viewer login.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{}
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("auth_start"),
                title: None,
                description: Some(Cow::Borrowed("Start GitHub device flow; returns user_code and verification_uri.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "client_id":{"type":"string"},
                        "scope":{"type":"string","description":"space-separated scopes, e.g. repo read:org"}
                    },
                    "required":["client_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("auth_poll"),
                title: None,
                description: Some(Cow::Borrowed("Poll GitHub for access_token using device_code.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "client_id":{"type":"string"},
                        "device_code":{"type":"string"},
                        "client_secret":{"type":"string","description":"optional for OAuth App"}
                    },
                    "required":["client_id","device_code"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_issues"),
                title: None,
                description: Some(Cow::Borrowed("List issues by repo with filters (state, labels, assignee).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string","description":"Repository owner (user or organization)"},
                        "repo":{"type":"string","description":"Repository name"},
                        "state":{"type":"string","description":"Filter by state: 'open', 'closed', or 'all'"},
                        "labels":{"type":"string","description":"Comma-separated list of label names"},
                        "assignee":{"type":"string","description":"Filter by assignee username"},
                        "per_page":{"type":"integer","minimum":1,"maximum":100},
                        "page":{"type":"integer","minimum":1},
                        "response_format":{"type":"string","enum":["concise","detailed"],"description":"'concise' returns only number/title/state, 'detailed' includes full metadata","default":"concise"}
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_issue"),
                title: None,
                description: Some(Cow::Borrowed("Get a single issue with comments.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"},
                        "number":{"type":"integer"}
                    },
                    "required":["owner","repo","number"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_pull_requests"),
                title: None,
                description: Some(Cow::Borrowed("List pull requests by repo.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"},
                        "state":{"type":"string"},
                        "per_page":{"type":"integer","minimum":1,"maximum":100},
                        "page":{"type":"integer","minimum":1}
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_pull_request"),
                title: None,
                description: Some(Cow::Borrowed("Get a pull request with reviews and comments.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"},
                        "number":{"type":"integer"}
                    },
                    "required":["owner","repo","number"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_pull_diff"),
                title: None,
                description: Some(Cow::Borrowed("Fetch the unified diff for a pull request (size guarded).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"},
                        "number":{"type":"integer"},
                        "max_kb":{"type":"integer","description":"Max size to fetch in KB (default 256)"}
                    },
                    "required":["owner","repo","number"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("code_search"),
                title: None,
                description: Some(Cow::Borrowed("Search code via GitHub search API. Use qualifiers like 'repo:owner/name', 'language:rust', 'path:src/'.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "query":{"type":"string","description":"Search query with optional qualifiers (e.g., 'error repo:rust-lang/rust language:rust')"},
                        "per_page":{"type":"integer","minimum":1,"maximum":100},
                        "page":{"type":"integer","minimum":1},
                        "response_format":{"type":"string","enum":["concise","detailed"],"description":"'concise' returns only path/repo/url, 'detailed' includes full metadata","default":"concise"}
                    },
                    "required":["query"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_repositories"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search repositories via GitHub search API (read-only).",
                )),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "query":{"type":"string","description":"Search query with optional qualifiers (e.g., 'language:rust stars:>5000')"},
                        "per_page":{"type":"integer","minimum":1,"maximum":100},
                        "page":{"type":"integer","minimum":1},
                        "response_format":{"type":"string","enum":["concise","detailed"],"description":"'concise' returns only full_name/url/stars, 'detailed' includes full metadata","default":"concise"}
                    },
                    "required":["query"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_repository"),
                title: None,
                description: Some(Cow::Borrowed("Get repository metadata by owner/repo.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"}
                    },
                    "required":["owner","repo"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_file"),
                title: None,
                description: Some(Cow::Borrowed("Get file contents by path/ref (base64 decoding when text).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "owner":{"type":"string"},
                        "repo":{"type":"string"},
                        "path":{"type":"string"},
                        "ref":{"type":"string"}
                    },
                    "required":["owner","repo","path"]
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
        let octo = self.octo()?;

        match name {
            "test_auth" => {
                let me = octo
                    .current()
                    .user()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                structured_result_with_text(&json!({"login": me.login, "id": me.id}), None)
            }
            "list_issues" => {
                let input: ListIssuesInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let state = input.state.unwrap_or_else(|| "open".to_string());
                let state_enum = match state.as_str() {
                    "open" => octocrab::params::State::Open,
                    "closed" => octocrab::params::State::Closed,
                    _ => octocrab::params::State::All,
                };
                let issues_api = octo.issues(&input.owner, &input.repo);
                let mut builder = issues_api.list().state(state_enum);
                let labels_vec: Option<Vec<String>> = input.labels.as_ref().map(|s| {
                    s.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect()
                });
                if let Some(ref v) = labels_vec {
                    builder = builder.labels(v);
                }
                if let Some(ref a) = input.assignee {
                    builder = builder.assignee(a.as_str());
                }
                builder = builder
                    .per_page(input.per_page.unwrap_or(50))
                    .page(input.page.unwrap_or(1));
                let issues = builder
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                // Return concise or detailed based on response_format
                if input.response_format == ResponseFormat::Concise {
                    let concise_items: Vec<_> = issues
                        .items
                        .iter()
                        .map(|i| {
                            json!({
                                "number": i.number,
                                "title": i.title,
                                "state": i.state
                            })
                        })
                        .collect();
                    structured_result_with_text(&json!({"items": concise_items}), None)
                } else {
                    structured_result_with_text(
                        &json!({"items": issues.items, "total_count": issues.total_count}),
                        None,
                    )
                }
            }
            "get_issue" => {
                let input: GetIssueInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let issue = octo
                    .issues(&input.owner, &input.repo)
                    .get(input.number)
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                // Comments
                let comments_page = octo
                    .issues(&input.owner, &input.repo)
                    .list_comments(input.number)
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                structured_result_with_text(
                    &json!({"issue": issue, "comments": comments_page.items}),
                    None,
                )
            }
            "list_pull_requests" => {
                let input: ListPullsInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let state = input.state.unwrap_or_else(|| "open".to_string());
                let state_enum = match state.as_str() {
                    "open" => octocrab::params::State::Open,
                    "closed" => octocrab::params::State::Closed,
                    _ => octocrab::params::State::All,
                };
                let pulls = octo
                    .pulls(&input.owner, &input.repo)
                    .list()
                    .state(state_enum)
                    .per_page(input.per_page.unwrap_or(50))
                    .page(input.page.unwrap_or(1))
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                structured_result_with_text(&json!({"items": pulls.items}), None)
            }
            "get_pull_request" => {
                let input: GetPullInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let pr = octo
                    .pulls(&input.owner, &input.repo)
                    .get(input.number)
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                // Reviews
                let reviews_page = octo
                    .pulls(&input.owner, &input.repo)
                    .list_reviews(input.number)
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                // Review comments
                let comments_page = octo
                    .pulls(&input.owner, &input.repo)
                    .list_comments(Some(input.number))
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                structured_result_with_text(
                    &json!({"pull_request": pr, "reviews": reviews_page.items, "review_comments": comments_page.items}),
                    None,
                )
            }
            "get_pull_diff" => {
                let input: GetPullInput =
                    serde_json::from_value(Value::Object(args_map.clone()))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let max_kb = args_map
                    .get("max_kb")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(256);
                let token = self.resolve_token().ok_or_else(|| {
                    ConnectorError::Authentication("GitHub token not configured".to_string())
                })?;
                let url = format!(
                    "https://api.github.com/repos/{}/{}/pulls/{}",
                    input.owner, input.repo, input.number
                );
                // Use reqwest for custom Accept header
                let _resp = self
                    .send_with_backoff(|client| {
                        client
                            .get(&url)
                            .header(reqwest::header::ACCEPT, "application/vnd.github.v3.diff")
                            .bearer_auth(&token)
                    })
                    .await?;
                // send_with_backoff parsed JSON; but diff is text. Fallback to bytes fetch without JSON parsing using one more request
                let raw = reqwest::Client::new()
                    .get(url)
                    .header(reqwest::header::ACCEPT, "application/vnd.github.v3.diff")
                    .bearer_auth(token)
                    .send()
                    .await
                    .map_err(ConnectorError::HttpRequest)?
                    .bytes()
                    .await
                    .map_err(ConnectorError::HttpRequest)?;
                let bytes = raw;
                let kb = (bytes.len() as u64).div_ceil(1024);
                if kb > max_kb {
                    return structured_result_with_text(
                        &json!({"truncated": true, "kb": kb}),
                        None,
                    );
                }
                let diff = String::from_utf8_lossy(&bytes).to_string();
                structured_result_with_text(
                    &json!({"diff": diff, "kb": kb, "truncated": false}),
                    None,
                )
            }
            "code_search" => {
                let input: CodeSearchInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let result = octo
                    .search()
                    .code(&input.query)
                    .per_page(input.per_page.unwrap_or(50))
                    .page(input.page.unwrap_or(1))
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                // Return concise or detailed based on response_format
                if input.response_format == ResponseFormat::Concise {
                    let concise_items: Vec<_> = result
                        .items
                        .iter()
                        .map(|i| {
                            json!({
                                "path": i.path,
                                "repository": i.repository.full_name,
                                "html_url": i.html_url
                            })
                        })
                        .collect();
                    structured_result_with_text(
                        &json!({"items": concise_items, "total_count": result.total_count}),
                        None,
                    )
                } else {
                    structured_result_with_text(
                        &json!({"total_count": result.total_count, "incomplete_results": result.incomplete_results, "items": result.items}),
                        None,
                    )
                }
            }
            "search_repositories" => {
                let input: RepoSearchInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let result = octo
                    .search()
                    .repositories(&input.query)
                    .per_page(input.per_page.unwrap_or(50))
                    .page(input.page.unwrap_or(1))
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                if input.response_format == ResponseFormat::Concise {
                    let concise_items: Vec<_> = result
                        .items
                        .iter()
                        .map(|r| {
                            json!({
                                "full_name": r.full_name,
                                "html_url": r.html_url,
                                "description": r.description,
                                "stargazers_count": r.stargazers_count,
                                "language": r.language,
                            })
                        })
                        .collect();
                    structured_result_with_text(
                        &json!({"items": concise_items, "total_count": result.total_count}),
                        None,
                    )
                } else {
                    structured_result_with_text(
                        &json!({"total_count": result.total_count, "incomplete_results": result.incomplete_results, "items": result.items}),
                        None,
                    )
                }
            }
            "get_repository" => {
                let input: GetRepositoryInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let repo = octo
                    .repos(&input.owner, &input.repo)
                    .get()
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                let v = serde_json::to_value(&repo)
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                structured_result_with_text(&v, None)
            }
            "auth_start" => {
                let client = reqwest::Client::new();
                let m: serde_json::Map<String, Value> = args_map;
                let client_id = m
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("client_id required".into()))?;
                let scope = m.get("scope").and_then(|v| v.as_str()).unwrap_or("");
                let resp = client
                    .post("https://github.com/login/device/code")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&[("client_id", client_id), ("scope", scope)])
                    .send()
                    .await
                    .map_err(ConnectorError::HttpRequest)?
                    .json::<Value>()
                    .await
                    .map_err(ConnectorError::HttpRequest)?;
                structured_result_with_text(&resp, None)
            }
            "auth_poll" => {
                let client = reqwest::Client::new();
                let m: serde_json::Map<String, Value> = args_map;
                let client_id = m
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("client_id required".into()))?;
                let device_code = m
                    .get("device_code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("device_code required".into()))?;
                let client_secret = m.get("client_secret").and_then(|v| v.as_str());
                let mut form: Vec<(&'static str, String)> = vec![
                    ("client_id", client_id.to_string()),
                    ("device_code", device_code.to_string()),
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code".to_string(),
                    ),
                ];
                if let Some(cs) = client_secret {
                    form.push(("client_secret", cs.to_string()));
                }
                let resp = client
                    .post("https://github.com/login/oauth/access_token")
                    .header(reqwest::header::ACCEPT, "application/json")
                    .form(&form)
                    .send()
                    .await
                    .map_err(ConnectorError::HttpRequest)?
                    .json::<Value>()
                    .await
                    .map_err(ConnectorError::HttpRequest)?;
                // If we received an access_token, persist as token
                if let Some(token) = resp.get("access_token").and_then(|v| v.as_str()) {
                    let mut auth = self.auth.clone();
                    auth.insert("token".into(), token.to_string());
                    let store = FileAuthStore::new_default();
                    let _ = store.save(self.name(), &auth);
                }
                structured_result_with_text(&resp, None)
            }
            "get_file" => {
                let input: GetFileInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let token = self.resolve_token().ok_or_else(|| {
                    ConnectorError::Authentication("GitHub token not configured".to_string())
                })?;
                let mut url = format!(
                    "https://api.github.com/repos/{}/{}/contents/{}",
                    input.owner, input.repo, input.path
                );
                if let Some(reference) = input.r#ref {
                    url.push_str(&format!("?ref={}", reference));
                }
                let v = self
                    .send_with_backoff(|client| client.get(&url).bearer_auth(&token))
                    .await?;
                structured_result_with_text(&v, None)
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
        if !self.auth.is_empty() {
            let store = FileAuthStore::new_default();
            let _ = store.save(self.name(), &details);
        }
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self
            .octo()?
            .current()
            .user()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![
            Field { name: "token".into(), label: "GitHub Token (fine-grained PAT)".into(), field_type: FieldType::Secret, required: false, description: Some("Provide a PAT with repo read and metadata; for private code search add code read.".into()), options: None },
            Field { name: "client_id".into(), label: "OAuth Client ID".into(), field_type: FieldType::Text, required: false, description: Some("For device-code flow.".into()), options: None },
            Field { name: "client_secret".into(), label: "OAuth Client Secret".into(), field_type: FieldType::Secret, required: false, description: Some("Optional for device-code token exchange.".into()), options: None },
        ] }
    }
}
