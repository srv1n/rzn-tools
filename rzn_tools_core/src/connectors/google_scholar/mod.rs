use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::Client;
use rmcp::model::*;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::time::{self, Duration};

#[derive(Debug, Deserialize)]
struct SearchPapersArgs {
    query: String,
    limit: Option<usize>,
}

pub struct GoogleScholarConnector {
    client: Client,
}

impl GoogleScholarConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self {
            client: Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
                .cookie_store(true)
                .build()
                .map_err(ConnectorError::HttpRequest)?,
        })
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Value>, ConnectorError> {
        time::sleep(Duration::from_secs(3)).await; // Rate limit Google Scholar
        let url = format!(
            "https://scholar.google.com/scholar?q={}&hl=en",
            urlencoding::encode(query)
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Scholar returned status: {}",
                response.status()
            )));
        }

        let html_content = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let document = Html::parse_document(&html_content);

        // Selectors
        let result_sel = Selector::parse(".gs_r.gs_or.gs_scl").unwrap();
        let title_sel = Selector::parse(".gs_rt").unwrap();
        let link_sel = Selector::parse(".gs_rt a").unwrap();
        let meta_sel = Selector::parse(".gs_a").unwrap();
        let snippet_sel = Selector::parse(".gs_rs").unwrap();

        let mut papers = Vec::new();

        for element in document.select(&result_sel).take(limit) {
            let title = element
                .select(&title_sel)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
            let link = element
                .select(&link_sel)
                .next()
                .and_then(|e| e.value().attr("href"))
                .map(|s| s.to_string());
            let meta = element
                .select(&meta_sel)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
            let snippet = element
                .select(&snippet_sel)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();

            // Extract year roughly from meta (e.g., "Author - Venue, 2023 - source")
            let year = meta.split(" - ").find_map(|part| {
                part.split(',').find_map(|s| {
                    let trimmed = s.trim();
                    if trimmed.len() == 4 && trimmed.chars().all(char::is_numeric) {
                        Some(trimmed.to_string())
                    } else {
                        None
                    }
                })
            });

            papers.push(json!({
                "title": title,
                "link": link,
                "authors_venue_year": meta,
                "year": year,
                "snippet": snippet
            }));
        }

        Ok(papers)
    }
}

#[async_trait]
impl Connector for GoogleScholarConnector {
    fn name(&self) -> &'static str {
        "google-scholar"
    }

    fn description(&self) -> &'static str {
        "Search academic papers on Google Scholar"
    }

    fn display_name(&self) -> &'static str {
        "Google Scholar"
    }

    fn icon(&self) -> &'static str {
        "google_scholar"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["academic", "research"]
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
                website_url: Some("https://scholar.google.com".to_string()),
            },
            instructions: Some(
                "Search Google Scholar by scraping. This method is unofficial, subject to Google's Terms of Service, and may be unreliable due to CAPTCHAs or HTML changes. Use with caution.".to_string(),
            ),
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools =
            vec![
            Tool {
                name: Cow::Borrowed("search_papers"),
                title: None,
                description: Some(Cow::Borrowed("Search Google Scholar papers.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "limit": { "type": "integer", "description": "Max results (default 10)" }
                    },
                    "required": ["query"]
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
            "search_papers" => {
                let args: SearchPapersArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let papers = self.search(&args.query, args.limit.unwrap_or(10)).await?;

                let data = json!({
                    "query": args.query,
                    "count": papers.len(),
                    "results": papers
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
