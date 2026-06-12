use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct ExaSearchConnector {
    client: Client,
    api_key: Option<String>,
}

impl ExaSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.2.4")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("EXA_API_KEY").ok());
        Ok(Self { client, api_key })
    }

    fn get_headers(&self) -> Result<HeaderMap, ConnectorError> {
        let key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::InvalidInput(
                "Missing credentials: set EXA_API_KEY or run 'rzn-tools setup exa'".into(),
            )
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(key).map_err(|e| ConnectorError::Other(e.to_string()))?,
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    async fn search_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;

        let mut body = json!({
            "query": query,
            "numResults": args
                .get("limit")
                .or_else(|| args.get("num_results"))
                .and_then(|v| v.as_u64())
                .unwrap_or(10),
        });

        // Search type (neural, auto, fast, deep)
        if let Some(search_type) = args.get("type").and_then(|v| v.as_str()) {
            body["type"] = json!(search_type);
        }

        // Use autoprompt
        if let Some(use_autoprompt) = args.get("use_autoprompt").and_then(|v| v.as_bool()) {
            body["useAutoprompt"] = json!(use_autoprompt);
        }

        // Category filter
        if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
            body["category"] = json!(category);
        }

        // Geographic location bias - use when searching for a different market
        // e.g., user_location="US" to find US-based results while in India
        if let Some(location) = args.get("user_location").and_then(|v| v.as_str()) {
            body["userLocation"] = json!(location);
        }

        // Date filters
        if let Some(start_crawl) = args.get("start_crawl_date").and_then(|v| v.as_str()) {
            body["startCrawlDate"] = json!(start_crawl);
        }
        if let Some(end_crawl) = args.get("end_crawl_date").and_then(|v| v.as_str()) {
            body["endCrawlDate"] = json!(end_crawl);
        }
        if let Some(start_pub) = args.get("start_published_date").and_then(|v| v.as_str()) {
            body["startPublishedDate"] = json!(start_pub);
        }
        if let Some(end_pub) = args.get("end_published_date").and_then(|v| v.as_str()) {
            body["endPublishedDate"] = json!(end_pub);
        }

        // Domain filters
        if let Some(include_domains) = args.get("include_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = include_domains
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !domains.is_empty() {
                body["includeDomains"] = json!(domains);
            }
        }
        if let Some(exclude_domains) = args.get("exclude_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = exclude_domains
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !domains.is_empty() {
                body["excludeDomains"] = json!(domains);
            }
        }

        // Text filters
        if let Some(include_text) = args.get("include_text").and_then(|v| v.as_array()) {
            let texts: Vec<String> = include_text
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !texts.is_empty() {
                body["includeText"] = json!(texts);
            }
        }
        if let Some(exclude_text) = args.get("exclude_text").and_then(|v| v.as_array()) {
            let texts: Vec<String> = exclude_text
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !texts.is_empty() {
                body["excludeText"] = json!(texts);
            }
        }

        // Contents options - request highlights by default for useful snippets
        if let Some(contents) = args.get("contents") {
            body["contents"] = contents.clone();
        } else {
            // Build contents object with defaults for useful display
            let mut contents_obj = json!({
                // Request highlights by default (max 1 snippet, 200 chars each)
                "highlights": {
                    "numSentences": 2,
                    "highlightsPerUrl": 1
                }
            });

            // Override with explicit parameters if provided
            if let Some(text) = args.get("text").and_then(|v| v.as_bool()) {
                contents_obj["text"] = json!(text);
            }
            if let Some(highlights) = args.get("highlights") {
                contents_obj["highlights"] = highlights.clone();
            }
            if let Some(summary) = args.get("summary") {
                contents_obj["summary"] = summary.clone();
            }

            body["contents"] = contents_obj;
        }

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post("https://api.exa.ai/search")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Exa API error: {} - {}",
                status, value
            )));
        }

        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);

        let mut data = json!({
            "provider": "exa",
            "query": query,
            "results": value.get("results").cloned().unwrap_or_else(|| json!([]))
        });

        if detailed {
            data["raw"] = value.clone();
        }

        structured_result_with_text(&data, None)
    }

    async fn get_contents_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let ids = args
            .get("ids")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'ids' array".into()))?;

        let id_strings: Vec<String> = ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        if id_strings.is_empty() {
            return Err(ConnectorError::InvalidParams("ids array is empty".into()));
        }

        let mut body = json!({
            "ids": id_strings,
        });

        // Contents options
        if let Some(text) = args.get("text") {
            body["text"] = text.clone();
        }
        if let Some(highlights) = args.get("highlights") {
            body["highlights"] = highlights.clone();
        }
        if let Some(summary) = args.get("summary") {
            body["summary"] = summary.clone();
        }
        if let Some(livecrawl) = args.get("livecrawl").and_then(|v| v.as_str()) {
            body["livecrawl"] = json!(livecrawl);
        }
        if let Some(subpages) = args.get("subpages").and_then(|v| v.as_u64()) {
            body["subpages"] = json!(subpages);
        }
        if let Some(subpage_target) = args.get("subpageTarget") {
            body["subpageTarget"] = subpage_target.clone();
        }

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post("https://api.exa.ai/contents")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Exa API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "exa",
            "operation": "get_contents",
            "results": value.get("results").cloned().unwrap_or_else(|| json!([]))
        });

        structured_result_with_text(&data, None)
    }

    async fn find_similar_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'url'".into()))?;

        let mut body = json!({
            "url": url,
            "numResults": args
                .get("limit")
                .or_else(|| args.get("num_results"))
                .and_then(|v| v.as_u64())
                .unwrap_or(10),
        });

        // Category filter
        if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
            body["category"] = json!(category);
        }

        // Date filters
        if let Some(start_crawl) = args.get("start_crawl_date").and_then(|v| v.as_str()) {
            body["startCrawlDate"] = json!(start_crawl);
        }
        if let Some(end_crawl) = args.get("end_crawl_date").and_then(|v| v.as_str()) {
            body["endCrawlDate"] = json!(end_crawl);
        }

        // Domain filters
        if let Some(include_domains) = args.get("include_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = include_domains
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !domains.is_empty() {
                body["includeDomains"] = json!(domains);
            }
        }
        if let Some(exclude_domains) = args.get("exclude_domains").and_then(|v| v.as_array()) {
            let domains: Vec<String> = exclude_domains
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !domains.is_empty() {
                body["excludeDomains"] = json!(domains);
            }
        }

        // Exclude source domain
        if let Some(exclude_source) = args.get("exclude_source_domain").and_then(|v| v.as_bool()) {
            body["excludeSourceDomain"] = json!(exclude_source);
        }

        // Contents options
        if let Some(contents) = args.get("contents") {
            body["contents"] = contents.clone();
        }

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post("https://api.exa.ai/findSimilar")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Exa API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "exa",
            "operation": "find_similar",
            "url": url,
            "results": value.get("results").cloned().unwrap_or_else(|| json!([]))
        });

        structured_result_with_text(&data, None)
    }

    async fn answer_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;

        let mut body = json!({
            "query": query,
        });

        // Answer mode
        if let Some(mode) = args.get("mode").and_then(|v| v.as_str()) {
            body["mode"] = json!(mode);
        }

        // Number of search results to use
        if let Some(num_results) = args
            .get("limit")
            .or_else(|| args.get("num_results"))
            .and_then(|v| v.as_u64())
        {
            body["numResults"] = json!(num_results);
        }

        // Category filter
        if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
            body["category"] = json!(category);
        }

        // Include citations
        if let Some(include_citations) = args.get("include_citations").and_then(|v| v.as_bool()) {
            body["includeCitations"] = json!(include_citations);
        }

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post("https://api.exa.ai/answer")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Exa API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "exa",
            "operation": "answer",
            "query": query,
            "answer": value.get("answer").cloned().unwrap_or(Value::Null),
            "citations": value.get("citations").cloned().unwrap_or_else(|| json!([])),
            "search_results": value.get("searchResults").cloned()
        });

        structured_result_with_text(&data, None)
    }

    async fn research_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let instructions = args
            .get("instructions")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'instructions'".into()))?;

        let mut body = json!({
            "instructions": instructions,
        });

        // Model selection
        if let Some(model) = args.get("model").and_then(|v| v.as_str()) {
            body["model"] = json!(model);
        }

        // Output schema for structured results
        if let Some(schema) = args.get("output_schema") {
            body["outputSchema"] = schema.clone();
        }

        let headers = self.get_headers()?;

        // Start the research task
        let resp = self
            .client
            .post("https://api.exa.ai/research/v1")
            .headers(headers.clone())
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let start_value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Exa API error: {} - {}",
                status, start_value
            )));
        }

        let research_id = start_value
            .get("researchId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::Other("No researchId in response".into()))?;

        // Poll for results (max 60 seconds with 2-second intervals)
        let max_attempts = 30;
        for attempt in 0..max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let poll_resp = self
                .client
                .get(format!("https://api.exa.ai/research/v1/{}", research_id))
                .headers(headers.clone())
                .send()
                .await
                .map_err(ConnectorError::HttpRequest)?;

            let poll_status = poll_resp.status();
            let poll_value: Value = poll_resp
                .json()
                .await
                .map_err(ConnectorError::HttpRequest)?;

            if !poll_status.is_success() {
                return Err(ConnectorError::Other(format!(
                    "Exa research poll error: {} - {}",
                    poll_status, poll_value
                )));
            }

            let status = poll_value
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            match status {
                "completed" => {
                    let data = json!({
                        "provider": "exa",
                        "operation": "research",
                        "research_id": research_id,
                        "status": "completed",
                        "output": poll_value.get("output").cloned(),
                        "cost": poll_value.get("cost").cloned(),
                        "events": poll_value.get("events").cloned()
                    });
                    return structured_result_with_text(&data, None);
                }
                "failed" => {
                    return Err(ConnectorError::Other(format!(
                        "Research task failed: {:?}",
                        poll_value.get("error")
                    )));
                }
                _ => {
                    // Still running, continue polling
                    if attempt == max_attempts - 1 {
                        let data = json!({
                            "provider": "exa",
                            "operation": "research",
                            "research_id": research_id,
                            "status": status,
                            "message": "Research still in progress. Use research_id to check status later.",
                            "partial_events": poll_value.get("events").cloned()
                        });
                        return structured_result_with_text(&data, None);
                    }
                }
            }
        }

        Err(ConnectorError::Other("Research polling timeout".into()))
    }
}

#[async_trait]
impl Connector for ExaSearchConnector {
    fn name(&self) -> &'static str {
        "exa"
    }
    fn description(&self) -> &'static str {
        "Entity-focused web search and content extraction via Exa."
    }

    fn display_name(&self) -> &'static str {
        "Exa Search"
    }

    fn icon(&self) -> &'static str {
        "exa"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "web"]
    }

    fn requires_auth(&self) -> bool {
        true
    }
    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _r: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().into(),
                version: "0.2.0".into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
r#"Exa is for entity-oriented discovery and source retrieval.

Use Exa when you need:
- specific entity types such as people, companies, papers, GitHub repos, tweets, or filings
- one good seed URL expanded into similar pages with find_similar
- a cited answer to a focused question
- clean page text, highlights, or summaries for known URLs or Exa ids

Preferred tool flow:
1. search -> discover candidate URLs/entities. Lean on category, include_domains, and published-date filters.
2. get_contents -> fetch readable text/highlights/summary for URLs or Exa result ids you already trust.
3. find_similar -> branch out from one strong seed URL.
4. answer -> produce a grounded answer with citations when the user wants an answer, not a result set.
5. research -> run one deeper synthesis job or emit structured JSON.

Use Parallel Search instead when the task is broad fan-out search, comparison across many subqueries, recurring monitoring, or a token-sensitive agent loop."#.into(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _r: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Ok(vec![])
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let search_tool = Tool {
            name: Cow::Borrowed("search"),
            title: Some("Exa Search".into()),
            description: Some(Cow::Borrowed(
                "Discover URLs or entities with Exa's semantic search. Best for people, companies, research papers, GitHub repos, tweets, filings, and filtered web discovery. Start here when you need candidates, not a final prose answer. Key args: query; optional category, type, limit, include_domains/exclude_domains, published-date filters, user_location, and contents/highlights/summary controls.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query. For people/company search, be descriptive: 'CTO at AI startups in NYC' or 'seed-stage fintech companies'"
                    },
                    "num_results": {
                        "type": "integer",
                        "default": 10,
                        "maximum": 100,
                        "description": "Number of results (max 100)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Alias for num_results (preferred name across search providers)."
                    },
                    "type": {
                        "type": "string",
                        "enum": ["neural", "auto", "fast", "deep"],
                        "default": "auto",
                        "description": "auto=smart hybrid, deep=comprehensive research, fast=<500ms, neural=pure semantic"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["people", "company", "research paper", "news", "pdf", "github", "tweet", "linkedin profile", "financial report", "personal site"],
                        "description": "IMPORTANT: Use this to search specific content types. 'people' for professionals, 'company' for businesses, 'research paper' for academic work"
                    },
                    "user_location": {
                        "type": "string",
                        "description": "Two-letter country code (ISO 3166-1) to bias results toward a region. Essential for cross-border research. Example: 'US' to find US companies while searching from India, 'DE' for German market research, 'GB' for UK."
                    },
                    "use_autoprompt": {
                        "type": "boolean",
                        "description": "Let Exa optimize your query. Good for natural language questions."
                    },
                    "start_published_date": {
                        "type": "string",
                        "description": "ISO 8601 datetime for content published after this date. Example: '2024-01-01T00:00:00Z'"
                    },
                    "end_published_date": {
                        "type": "string",
                        "description": "ISO 8601 datetime for content published before this date"
                    },
                    "start_crawl_date": {
                        "type": "string",
                        "description": "Filter by when Exa discovered the page"
                    },
                    "end_crawl_date": {
                        "type": "string",
                        "description": "Filter by when Exa discovered the page"
                    },
                    "include_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Only search these domains. Example: ['linkedin.com', 'twitter.com']"
                    },
                    "exclude_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Exclude these domains from results"
                    },
                    "include_text": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Results must contain these strings (max 1 string, 5 words)"
                    },
                    "exclude_text": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Exclude results containing these strings"
                    },
                    "text": {
                        "type": "boolean",
                        "description": "Include full page text in results"
                    },
                    "highlights": {
                        "type": "object",
                        "description": "Get relevant snippets. Example: {\"numSentences\": 3, \"highlightsPerUrl\": 2}"
                    },
                    "summary": {
                        "type": "object",
                        "description": "Get AI summaries. Example: {\"query\": \"summarize the main findings\"}"
                    },
                    "contents": {
                        "type": "object",
                        "description": "Advanced: combine text, highlights, summary options"
                    }
                },
                "required": ["query"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let get_contents_tool = Tool {
            name: Cow::Borrowed("get_contents"),
            title: Some("Fetch Exa Contents".into()),
            description: Some(Cow::Borrowed(
                "Fetch readable content for URLs or Exa result ids you already have. Use after search when you want page text, targeted highlights, summaries, or a fresh crawl. Key args: ids (required URL/result-id list); optional text, highlights, summary, livecrawl, subpages, subpageTarget.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "ids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "URLs or Exa result IDs to fetch content from"
                    },
                    "text": {
                        "type": ["boolean", "object"],
                        "description": "Get page text. Use object for options: {\"maxCharacters\": 5000, \"includeHtmlTags\": false}"
                    },
                    "highlights": {
                        "type": "object",
                        "description": "Extract relevant snippets. Example: {\"numSentences\": 3, \"query\": \"what is the pricing?\"}"
                    },
                    "summary": {
                        "type": "object",
                        "description": "AI summary. Can include schema for structured output: {\"query\": \"extract company info\", \"schema\": {...}}"
                    },
                    "livecrawl": {
                        "type": "string",
                        "enum": ["never", "fallback", "preferred", "always"],
                        "description": "never=cache only, fallback=cache then crawl, preferred=crawl then cache, always=fresh crawl"
                    },
                    "subpages": {
                        "type": "integer",
                        "description": "Number of linked pages to also crawl (0-10)"
                    },
                    "subpageTarget": {
                        "type": ["string", "array"],
                        "description": "Keywords to find in subpages. Example: 'pricing' or ['docs', 'api']"
                    }
                },
                "required": ["ids"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let find_similar_tool = Tool {
            name: Cow::Borrowed("find_similar"),
            title: Some("Find Similar Pages".into()),
            description: Some(Cow::Borrowed(
                "Expand from one strong seed URL to adjacent pages, companies, papers, repos, or blogs. Use this when you know one good example and want close neighbors, not a keyword search from scratch. Key args: url (required); optional category, limit, exclude_source_domain, include_domains/exclude_domains, and contents fetch options.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to find similar pages for"
                    },
                    "num_results": {
                        "type": "integer",
                        "default": 10,
                        "maximum": 100,
                        "description": "Number of similar results to return"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Alias for num_results (preferred name across search providers)."
                    },
                    "category": {
                        "type": "string",
                        "enum": ["people", "company", "research paper", "news", "pdf", "github", "tweet", "linkedin profile", "financial report", "personal site"],
                        "description": "Filter similar results to this category"
                    },
                    "exclude_source_domain": {
                        "type": "boolean",
                        "description": "Exclude results from the same domain as the input URL"
                    },
                    "start_crawl_date": {"type": "string", "description": "Results discovered after this date"},
                    "end_crawl_date": {"type": "string", "description": "Results discovered before this date"},
                    "include_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Only include results from these domains"
                    },
                    "exclude_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Exclude results from these domains"
                    },
                    "contents": {
                        "type": "object",
                        "description": "Also fetch page contents (text, highlights, summary)"
                    }
                },
                "required": ["url"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let answer_tool = Tool {
            name: Cow::Borrowed("answer"),
            title: Some("Grounded Answer".into()),
            description: Some(Cow::Borrowed(
                "Return a grounded answer with source citations when the task is a question, not bulk result collection. Prefer this over search when the user wants one concise answer backed by sources. Key args: query (required); optional mode, limit, category, include_citations.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Question to answer"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["precise", "detailed"],
                        "description": "precise=short factual answers, detailed=comprehensive summaries"
                    },
                    "num_results": {
                        "type": "integer",
                        "description": "Number of search results to use for generating the answer"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Alias for num_results (preferred name across search providers)."
                    },
                    "category": {
                        "type": "string",
                        "enum": ["people", "company", "research paper", "news", "pdf", "github", "tweet", "linkedin profile", "financial report", "personal site"],
                        "description": "Filter sources to this category"
                    },
                    "include_citations": {
                        "type": "boolean",
                        "default": true,
                        "description": "Include source URLs and citations in response"
                    }
                },
                "required": ["query"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let research_tool = Tool {
            name: Cow::Borrowed("research"),
            title: Some("Deep Research".into()),
            description: Some(Cow::Borrowed(
                "Run one deeper Exa research job that synthesizes sources and can emit structured JSON. Use for longer investigations or schema-shaped output, not simple lookups. Key args: instructions (required); optional model and output_schema.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "instructions": {
                        "type": "string",
                        "description": "Detailed research instructions (max 4096 chars). Be specific about what to find and how to structure output."
                    },
                    "model": {
                        "type": "string",
                        "enum": ["exa-research-fast", "exa-research", "exa-research-pro"],
                        "description": "fast=quick research, default=balanced, pro=most thorough"
                    },
                    "output_schema": {
                        "type": "object",
                        "description": "JSON Schema for structured output. If provided, results validate against this schema."
                    }
                },
                "required": ["instructions"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        Ok(ListToolsResult {
            tools: vec![
                search_tool,
                get_contents_tool,
                find_similar_tool,
                answer_tool,
                research_tool,
            ],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let args = request.arguments.unwrap_or_default();

        match request.name.as_ref() {
            "search" => self.search_impl(&args).await,
            "get_contents" => self.get_contents_impl(&args).await,
            "find_similar" => self.find_similar_impl(&args).await,
            "answer" => self.answer_impl(&args).await,
            "research" => self.research_impl(&args).await,
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ToolNotFound)
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut auth = AuthDetails::new();
        if let Some(v) = &self.api_key {
            auth.insert("api_key".into(), v.clone());
        }
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.api_key = details
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("EXA_API_KEY").ok());
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        if self
            .api_key
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        {
            Ok(())
        } else {
            Err(ConnectorError::InvalidInput("Missing api_key".into()))
        }
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "api_key".into(),
                label: "Exa API Key".into(),
                field_type: FieldType::Secret,
                required: true,
                description: Some("Get your API key from https://dashboard.exa.ai/api-keys".into()),
                options: None,
            }],
        }
    }
}
