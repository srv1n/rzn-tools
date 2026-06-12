use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Source,
};
use crate::utils::{build_reqwest_client, structured_result, structured_result_with_text};
use crate::{auth::AuthDetails, Connector, URLParamExtraction, URLPatternSpec};
use async_trait::async_trait;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use reqwest::Client;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

// Define the structs for arXiv papers
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ArxivPaper {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub published: String,
    pub updated: String,
    pub categories: Vec<String>,
    pub links: Vec<ArxivLink>,
    pub doi: Option<String>,
    pub journal_ref: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ArxivLink {
    pub href: String,
    pub rel: String,
    pub title: Option<String>,
    pub link_type: Option<String>,
}

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
struct SearchPapersArgs {
    query: String,
    #[serde(default = "default_limit", alias = "max_results")]
    limit: i32,
    #[serde(default = "default_start")]
    start: i32,
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
    #[serde(default)]
    response_format: ResponseFormat,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetPaperDetailsArgs {
    paper_id: Option<String>,
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    response_format: ResponseFormat,
    #[serde(default)]
    output_format: OutputFormat,
}

fn default_max_results() -> i32 {
    10
}

fn default_limit() -> i32 {
    default_max_results()
}

fn default_start() -> i32 {
    0
}

fn default_sort_by() -> String {
    "relevance".to_string()
}

fn default_sort_order() -> String {
    "descending".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ArxivCursor {
    start: i32,
    limit: i32,
    query: String,
    sort_by: String,
    sort_order: String,
}

pub struct ArxivConnector {
    client: Client,
}

impl ArxivConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self {
            client: build_reqwest_client(Client::builder)?,
        })
    }

    // Helper method to search for papers
    async fn search_papers(
        &self,
        args: &SearchPapersArgs,
    ) -> Result<Vec<ArxivPaper>, ConnectorError> {
        let mut url = Url::parse("http://export.arxiv.org/api/query")
            .map_err(|e| ConnectorError::InvalidInput(format!("Failed to parse URL: {}", e)))?;

        url.query_pairs_mut()
            .append_pair("search_query", &args.query)
            .append_pair("start", &args.start.to_string())
            .append_pair("max_results", &args.limit.to_string())
            .append_pair("sortBy", &args.sort_by)
            .append_pair("sortOrder", &args.sort_order);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "arXiv API returned error status: {}",
                response.status()
            )));
        }

        let content = response.text().await.map_err(ConnectorError::HttpRequest)?;
        self.parse_arxiv_response(&content)
    }

    // Helper method to get paper details by ID
    async fn get_paper_details(&self, paper_id: &str) -> Result<ArxivPaper, ConnectorError> {
        let mut url = Url::parse("http://export.arxiv.org/api/query")
            .map_err(|e| ConnectorError::InvalidInput(format!("Failed to parse URL: {}", e)))?;

        url.query_pairs_mut()
            .append_pair("id_list", paper_id)
            .append_pair("max_results", "1");

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "arXiv API returned error status: {}",
                response.status()
            )));
        }

        let content = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let papers = self.parse_arxiv_response(&content)?;

        if papers.is_empty() {
            return Err(ConnectorError::ResourceNotFound);
        }

        Ok(papers[0].clone())
    }

    fn pdf_url(paper_id: &str) -> String {
        format!("https://arxiv.org/pdf/{}.pdf", paper_id)
    }

    fn extract_arxiv_id_from_url(url: &str) -> Option<String> {
        if let Some(pos) = url.find("arxiv.org/") {
            let rest = &url[pos + "arxiv.org/".len()..];
            let rest = rest.trim_start_matches("abs/").trim_start_matches("pdf/");
            let rest = rest.trim_end_matches(".pdf");
            let id = rest.split('/').next()?.trim();
            if id.is_empty() {
                None
            } else {
                Some(id.to_string())
            }
        } else {
            None
        }
    }

    fn resolve_arxiv_paper_id(args: &GetPaperDetailsArgs) -> Result<String, ConnectorError> {
        if let Some(id) = args.paper_id.as_ref().filter(|v| !v.trim().is_empty()) {
            return Ok(id.to_string());
        }
        if let Some(item_ref) = args.item_ref.as_deref() {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "arxiv") {
                if kind == "paper" {
                    return Ok(id);
                }
            }
        }
        if let Some(url) = args.url.as_deref() {
            if let Some(id) = Self::extract_arxiv_id_from_url(url) {
                return Ok(id);
            }
        }
        Err(ConnectorError::InvalidParams(
            "Missing paper identifier. Provide paper_id, item_ref, or url.".to_string(),
        ))
    }

    // Helper method to parse arXiv API response
    fn parse_arxiv_response(&self, xml_content: &str) -> Result<Vec<ArxivPaper>, ConnectorError> {
        let mut reader = Reader::from_str(xml_content);
        reader.trim_text(true);

        let mut papers = Vec::new();
        let mut current_paper: Option<ArxivPaper> = None;
        let mut current_authors: Vec<String> = Vec::new();
        let mut current_categories: Vec<String> = Vec::new();
        let mut current_links: Vec<ArxivLink> = Vec::new();

        let mut in_entry = false;
        let mut current_tag: Option<String> = None;
        let mut buffer = Vec::new();

        loop {
            match reader.read_event_into(&mut buffer) {
                Ok(Event::Start(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    match tag_name.as_str() {
                        "entry" => {
                            in_entry = true;
                            current_paper = Some(ArxivPaper {
                                id: String::new(),
                                title: String::new(),
                                summary: String::new(),
                                authors: Vec::new(),
                                published: String::new(),
                                updated: String::new(),
                                categories: Vec::new(),
                                links: Vec::new(),
                                doi: None,
                                journal_ref: None,
                                comment: None,
                            });
                            current_authors = Vec::new();
                            current_categories = Vec::new();
                            current_links = Vec::new();
                        }
                        "id" | "title" | "summary" | "published" | "updated" | "name"
                        | "arxiv:comment" | "arxiv:journal_ref" | "arxiv:doi"
                            if in_entry =>
                        {
                            current_tag = Some(tag_name);
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if let Some(ref tag) = current_tag {
                        if let Some(paper) = current_paper.as_mut() {
                            let text = e
                                .unescape()
                                .map_err(|_| ConnectorError::ParseError)?
                                .to_string();

                            match tag.as_str() {
                                "id" => paper.id = text.replace("http://arxiv.org/abs/", ""),
                                "title" => paper.title = text,
                                "summary" => paper.summary = text,
                                "published" => paper.published = text,
                                "updated" => paper.updated = text,
                                "name" => current_authors.push(text),
                                "arxiv:comment" => paper.comment = Some(text),
                                "arxiv:journal_ref" => paper.journal_ref = Some(text),
                                "arxiv:doi" => paper.doi = Some(text),
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    if tag_name == "link" && in_entry {
                        let mut href = String::new();
                        let mut rel = String::new();
                        let mut title = None;
                        let mut link_type = None;

                        for attr in e.attributes().filter_map(Result::ok) {
                            let attr_name = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let attr_value = String::from_utf8_lossy(&attr.value).to_string();

                            match attr_name.as_str() {
                                "href" => href = attr_value,
                                "rel" => rel = attr_value,
                                "title" => title = Some(attr_value),
                                "type" => link_type = Some(attr_value),
                                _ => {}
                            }
                        }

                        current_links.push(ArxivLink {
                            href,
                            rel,
                            title,
                            link_type,
                        });
                    } else if tag_name == "category" && in_entry {
                        for attr in e.attributes().filter_map(Result::ok) {
                            let attr_name = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let attr_value = String::from_utf8_lossy(&attr.value).to_string();

                            if attr_name == "term" {
                                current_categories.push(attr_value);
                            }
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    if tag_name == "entry" {
                        in_entry = false;
                        if let Some(mut paper) = current_paper.take() {
                            paper.authors = current_authors.clone();
                            paper.categories = current_categories.clone();
                            paper.links = current_links.clone();
                            papers.push(paper);
                        }
                    } else if tag_name == current_tag.as_deref().unwrap_or("") {
                        current_tag = None;
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => return Err(ConnectorError::ParseError),
                _ => {}
            }

            buffer.clear();
        }

        Ok(papers)
    }

    // Helper method to format paper for JSON response
    fn format_paper(&self, paper: &ArxivPaper) -> HashMap<String, Value> {
        let mut result = HashMap::new();

        result.insert("id".to_string(), json!(paper.id));
        result.insert("title".to_string(), json!(paper.title));
        result.insert("summary".to_string(), json!(paper.summary));
        result.insert("authors".to_string(), json!(paper.authors));
        result.insert("published".to_string(), json!(paper.published));
        result.insert("updated".to_string(), json!(paper.updated));
        result.insert("categories".to_string(), json!(paper.categories));

        // Extract PDF link
        let pdf_link = paper
            .links
            .iter()
            .find(|link| link.title.as_deref() == Some("pdf") || link.href.contains("/pdf/"))
            .map(|link| link.href.clone())
            .unwrap_or_else(|| format!("https://arxiv.org/pdf/{}.pdf", paper.id));

        result.insert("pdf_url".to_string(), json!(pdf_link));

        // Extract abstract page link
        let abstract_link = paper
            .links
            .iter()
            .find(|link| link.rel == "alternate" && link.link_type.as_deref() == Some("text/html"))
            .map(|link| link.href.clone())
            .unwrap_or_else(|| format!("https://arxiv.org/abs/{}", paper.id));

        result.insert("abstract_url".to_string(), json!(abstract_link));

        if let Some(ref doi) = paper.doi {
            result.insert("doi".to_string(), json!(doi));
        }

        if let Some(ref journal_ref) = paper.journal_ref {
            result.insert("journal_ref".to_string(), json!(journal_ref));
        }

        if let Some(ref comment) = paper.comment {
            result.insert("comment".to_string(), json!(comment));
        }

        result
    }

    // Helper method to format paper in concise format (fewer tokens)
    fn format_paper_concise(&self, paper: &ArxivPaper) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        result.insert("id".to_string(), json!(paper.id));
        result.insert("title".to_string(), json!(paper.title));
        result.insert("summary".to_string(), json!(paper.summary));
        result.insert(
            "abstract_url".to_string(),
            json!(format!("https://arxiv.org/abs/{}", paper.id)),
        );
        result.insert(
            "pdf_url".to_string(),
            json!(format!("https://arxiv.org/pdf/{}.pdf", paper.id)),
        );
        result
    }

    fn build_content_item(&self, paper: &ArxivPaper, include_blocks: bool) -> ContentItem {
        let mut blocks = Vec::new();
        if include_blocks && !paper.summary.trim().is_empty() {
            blocks.push(ContentBlock {
                block_ref: format!("arxiv:abstract:{}", paper.id),
                block_kind: "abstract".to_string(),
                text: paper.summary.trim().to_string(),
                author: None,
                created_at: None,
                reply_to: None,
                position: None,
                score: None,
                attachments: Vec::new(),
                metadata: None,
            });
        }

        let mut metadata = serde_json::Map::new();
        metadata.insert("pdf_url".to_string(), json!(Self::pdf_url(&paper.id)));
        metadata.insert(
            "abstract_url".to_string(),
            json!(format!("https://arxiv.org/abs/{}", paper.id)),
        );
        if let Some(doi) = &paper.doi {
            metadata.insert("doi".to_string(), json!(doi));
        }
        if let Some(journal_ref) = &paper.journal_ref {
            metadata.insert("journal_ref".to_string(), json!(journal_ref));
        }
        if let Some(comment) = &paper.comment {
            metadata.insert("comment".to_string(), json!(comment));
        }

        ContentItem {
            item_ref: format!("arxiv:paper:{}", paper.id),
            kind: "paper".to_string(),
            canonical_url: Some(format!("https://arxiv.org/abs/{}", paper.id)),
            title: if paper.title.trim().is_empty() {
                None
            } else {
                Some(paper.title.clone())
            },
            created_at: if paper.published.trim().is_empty() {
                None
            } else {
                Some(paper.published.clone())
            },
            source_updated_at: if paper.updated.trim().is_empty() {
                None
            } else {
                Some(paper.updated.clone())
            },
            authors: paper
                .authors
                .iter()
                .map(|name| Author {
                    name: name.clone(),
                    id: None,
                })
                .collect(),
            tags: paper.categories.clone(),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(Value::Object(metadata))
            },
            blocks,
            relationships: Vec::new(),
            truncation: None,
        }
    }
}

#[async_trait]
impl Connector for ArxivConnector {
    fn name(&self) -> &'static str {
        "arxiv"
    }

    fn description(&self) -> &'static str {
        "A connector for searching and retrieving papers from arXiv.org"
    }

    fn display_name(&self) -> &'static str {
        "arXiv"
    }

    fn icon(&self) -> &'static str {
        "arxiv"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["academic", "research", "science"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?arxiv\.org/(?:abs|pdf)/([0-9]+\.[0-9]+)".to_string(),
            default_tool: "get".to_string(),
            description: "Get paper by arXiv ID".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "paper_id".to_string(),
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
        // arXiv API doesn't require authentication
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        // arXiv API doesn't require authentication
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test the API by making a simple search request
        let args = SearchPapersArgs {
            query: "cat:cs.AI".to_string(),
            limit: 1,
            start: 0,
            sort_by: "relevance".to_string(),
            sort_order: "descending".to_string(),
            response_format: ResponseFormat::default(),
            output_format: OutputFormat::Raw,
            cursor: None,
        };

        let papers = self.search_papers(&args).await?;
        if papers.is_empty() {
            return Err(ConnectorError::Other(
                "Failed to retrieve papers from arXiv".to_string(),
            ));
        }

        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        // arXiv API doesn't require configuration
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
            instructions: Some(
                "arXiv connector for searching and retrieving academic papers".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let resources = vec![Resource {
            raw: RawResource {
                uri: "arxiv://paper/{paper_id}".to_string(),
                name: "arXiv Paper".to_string(),
                title: None,
                description: Some("An academic paper from arXiv.org".to_string()),
                mime_type: Some("application/json".to_string()),
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
        let uri = request.uri.as_str();

        if uri.starts_with("arxiv://paper/") {
            let parts: Vec<&str> = uri.split('/').collect();
            if parts.len() < 3 {
                return Err(ConnectorError::InvalidInput(format!(
                    "Invalid resource URI: {}",
                    uri
                )));
            }

            let paper_id = parts[2];
            let paper = self.get_paper_details(paper_id).await?;
            let _paper_json = serde_json::to_string(&self.format_paper(&paper))
                .map_err(ConnectorError::SerdeJson)?;

            let content_text = serde_json::to_string(&paper)?;
            Ok(vec![ResourceContents::text(content_text, uri)])
        } else {
            Err(ConnectorError::ResourceNotFound)
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search arXiv papers. Tip: use fielded queries like \"ti:transformer AND au:hinton\"; then pass a result id into get.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query. Can include field-specific searches like 'ti:neural AND au:hinton'."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 10). Keep this small for concise output."
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Deprecated alias for limit."
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque cursor from a previous response."
                        },
                        "start": {
                            "type": "integer",
                            "description": "Starting index for results (default: 0)"
                        },
                        "sort_by": {
                            "type": "string",
                            "description": "Sort results by: 'relevance', 'lastUpdatedDate', or 'submittedDate' (default: 'relevance')"
                        },
                        "sort_order": {
                            "type": "string",
                            "description": "Sort order: 'ascending' or 'descending' (default: 'descending')"
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["concise", "detailed"],
                            "description": "Response verbosity: 'concise' (default) returns only id/title/summary, 'detailed' includes all metadata (authors, dates, links, etc.)",
                            "default": "concise"
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
                            "description": "Simple keyword search",
                            "input": { "query": "transformer attention mechanism", "limit": 5 }
                        },
                        {
                            "description": "Fielded search by title and author",
                            "input": { "query": "ti:BERT AND au:Devlin", "limit": 5 }
                        },
                        {
                            "description": "Category filter",
                            "input": { "query": "cat:cs.AI", "limit": 10 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["academic", "research", "papers"],
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
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get paper metadata by arXiv ID (e.g., 2301.07041). Tip: use response_format='concise' for id/title/summary + urls.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": {
                            "type": "string",
                            "description": "Normalized item_ref (e.g., arxiv:paper:2301.07041)."
                        },
                        "url": {
                            "type": "string",
                            "description": "Canonical arXiv URL (e.g., https://arxiv.org/abs/2301.07041)."
                        },
                        "paper_id": {
                            "type": "string",
                            "description": "The arXiv ID of the paper (e.g., '2101.12345' or 'hep-th/9901001')"
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["concise", "detailed"],
                            "description": "Response verbosity: 'concise' (default) returns only id/title/summary, 'detailed' includes all metadata",
                            "default": "concise"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        }
                    },
                    "examples": [
                        {
                            "description": "Get paper by arXiv ID",
                            "input": { "paper_id": "2301.07041" }
                        },
                        {
                            "description": "Get paper by URL",
                            "input": { "url": "https://arxiv.org/abs/2301.07041" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["academic", "research", "papers"],
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
        match request.name.as_ref() {
            "search" | "search_papers" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let mut args: SearchPapersArgs = serde_json::from_value(args_value.clone())
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;

                if let Some(cursor) = args.cursor.as_deref() {
                    let decoded =
                        ingest::decode_cursor::<ArxivCursor>(cursor).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?;
                    if decoded.query != args.query
                        || decoded.sort_by != args.sort_by
                        || decoded.sort_order != args.sort_order
                    {
                        return Err(ConnectorError::InvalidParams(
                            "Cursor does not match query parameters".to_string(),
                        ));
                    }
                    if decoded.limit != args.limit {
                        return Err(ConnectorError::InvalidParams(
                            "Cursor does not match limit".to_string(),
                        ));
                    }
                    args.start = decoded.start;
                }

                if args.start < 0 || args.limit <= 0 {
                    return Err(ConnectorError::InvalidParams(
                        "start and limit must be positive".to_string(),
                    ));
                }

                let papers = self.search_papers(&args).await?;
                if args.output_format == OutputFormat::NormalizedV1 {
                    let items: Vec<ContentItem> = papers
                        .iter()
                        .map(|paper| self.build_content_item(paper, false))
                        .collect();
                    let has_more = papers.len() as i32 >= args.limit;
                    let next_cursor = if has_more {
                        ingest::encode_cursor(&ArxivCursor {
                            start: args.start + args.limit,
                            limit: args.limit,
                            query: args.query.clone(),
                            sort_by: args.sort_by.clone(),
                            sort_order: args.sort_order.clone(),
                        })
                        .ok()
                    } else {
                        None
                    };

                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(args.limit as u64))),
                        Source::new("arxiv", "search"),
                    );
                    return structured_result(&page);
                }

                let results: Vec<HashMap<String, Value>> = papers
                    .iter()
                    .map(|paper| {
                        if args.response_format == ResponseFormat::Concise {
                            self.format_paper_concise(paper)
                        } else {
                            self.format_paper(paper)
                        }
                    })
                    .collect();

                let data = if args.response_format == ResponseFormat::Concise {
                    json!({ "results": results })
                } else {
                    json!({
                        "query": args.query,
                        "start": args.start,
                        "limit": args.limit,
                        "sort_by": args.sort_by,
                        "sort_order": args.sort_order,
                        "results": results,
                    })
                };
                let text = serde_json::to_string(&data).map_err(ConnectorError::SerdeJson)?;
                Ok(structured_result_with_text(&data, Some(text))?)
            }
            "get" | "get_paper_details" => {
                let args: GetPaperDetailsArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(format!("Invalid arguments: {}", e)))?;

                let paper_id = Self::resolve_arxiv_paper_id(&args)?;

                match self.get_paper_details(&paper_id).await {
                    Ok(paper) => {
                        if args.output_format == OutputFormat::NormalizedV1 {
                            let item = self.build_content_item(&paper, true);
                            let normalized =
                                NormalizedItemV1::complete(item, Source::new("arxiv", "get"));
                            return structured_result(&normalized);
                        }

                        let result = if args.response_format == ResponseFormat::Concise {
                            self.format_paper_concise(&paper)
                        } else {
                            self.format_paper(&paper)
                        };
                        let text =
                            serde_json::to_string(&result).map_err(ConnectorError::SerdeJson)?;
                        Ok(structured_result_with_text(&result, Some(text))?)
                    }
                    Err(ConnectorError::ResourceNotFound) => {
                        let data = json!({
                            "requested_id": paper_id,
                            "papers": [],
                        });
                        let text =
                            serde_json::to_string(&data).map_err(ConnectorError::SerdeJson)?;
                        Ok(structured_result_with_text(&data, Some(text))?)
                    }
                    Err(err) => Err(err),
                }
            }
            "get_pdf_url" | "get_paper_pdf" => {
                let args: GetPaperDetailsArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(format!("Invalid arguments: {}", e)))?;

                let paper_id = Self::resolve_arxiv_paper_id(&args)?;

                let data = json!({
                    "paper_id": paper_id,
                    "pdf_url": Self::pdf_url(&paper_id),
                });
                let text = serde_json::to_string(&data).map_err(ConnectorError::SerdeJson)?;
                Ok(structured_result_with_text(&data, Some(text))?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let prompts = vec![
            Prompt {
                name: "summarize_paper".to_string(),
                title: None,
                description: Some("Generate a concise summary of an arXiv paper".to_string()),
                arguments: Some(vec![PromptArgument {
                    name: "paper_id".to_string(),
                    title: None,
                    description: Some("The arXiv ID of the paper to summarize".to_string()),
                    required: Some(true),
                }]),
                icons: None,
            },
            Prompt {
                name: "extract_key_findings".to_string(),
                title: None,
                description: Some(
                    "Extract the key findings and contributions from an arXiv paper".to_string(),
                ),
                arguments: Some(vec![PromptArgument {
                    name: "paper_id".to_string(),
                    title: None,
                    description: Some("The arXiv ID of the paper to analyze".to_string()),
                    required: Some(true),
                }]),
                icons: None,
            },
        ];

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        match name {
            "summarize_paper" => Ok(Prompt {
                name: "summarize_paper".to_string(),
                title: None,
                description: Some("Generate a concise summary of an arXiv paper".to_string()),
                arguments: Some(vec![PromptArgument {
                    name: "paper_id".to_string(),
                    title: None,
                    description: Some("The arXiv ID of the paper to summarize".to_string()),
                    required: Some(true),
                }]),
                icons: None,
            }),
            "extract_key_findings" => Ok(Prompt {
                name: "extract_key_findings".to_string(),
                title: None,
                description: Some(
                    "Extract the key findings and contributions from an arXiv paper".to_string(),
                ),
                arguments: Some(vec![PromptArgument {
                    name: "paper_id".to_string(),
                    title: None,
                    description: Some("The arXiv ID of the paper to analyze".to_string()),
                    required: Some(true),
                }]),
                icons: None,
            }),
            _ => Err(ConnectorError::InvalidParams(format!(
                "Prompt '{}' not found",
                name
            ))),
        }
    }
}
