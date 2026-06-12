use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;
use url::Url;
use urlencoding::encode;

#[derive(Debug, Serialize, Deserialize)]
pub struct SciHubResult {
    pub doi: String,
    pub pdf_url: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub journal: Option<String>,
    pub year: Option<String>,
    pub success: bool,
    pub message: String,
}

pub struct SciHubConnector {
    client: reqwest::Client,
    headers: HeaderMap,
    unpaywall_email: Option<String>,
    unpaywall_base_url: String,
    openalex_base_url: String,
}

#[derive(Debug, Deserialize)]
struct UnpaywallAuthor {
    family: Option<String>,
    given: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnpaywallLocation {
    url_for_pdf: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnpaywallResponse {
    is_oa: Option<bool>,
    title: Option<String>,
    journal_name: Option<String>,
    year: Option<i64>,
    z_authors: Option<Vec<UnpaywallAuthor>>,
    best_oa_location: Option<UnpaywallLocation>,
    oa_locations: Option<Vec<UnpaywallLocation>>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexVenue {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexOpenAccess {
    is_oa: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexLocation {
    pdf_url: Option<String>,
    landing_page_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    doi: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i64>,
    host_venue: Option<OpenAlexVenue>,
    open_access: Option<OpenAlexOpenAccess>,
    authorships: Option<Vec<OpenAlexAuthorship>>,
    best_oa_location: Option<OpenAlexLocation>,
    locations: Option<Vec<OpenAlexLocation>>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSearchResponse {
    results: Vec<OpenAlexWork>,
    #[allow(dead_code)]
    meta: Option<OpenAlexMeta>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAlexMeta {
    count: Option<i64>,
    per_page: Option<i64>,
    page: Option<i64>,
}

impl SciHubConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let unpaywall_email = std::env::var("UNPAYWALL_EMAIL")
            .ok()
            .filter(|v| !v.trim().is_empty());

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let mut connector = SciHubConnector {
            client: reqwest::Client::new(),
            headers,
            unpaywall_email,
            unpaywall_base_url: "https://api.unpaywall.org".to_string(),
            openalex_base_url: "https://api.openalex.org".to_string(),
        };

        connector.set_auth_details(auth).await?;
        Ok(connector)
    }

    async fn resolve_open_access(&self, doi: &str) -> Result<SciHubResult, ConnectorError> {
        if let Some(email) = self.unpaywall_email.as_deref() {
            match self.lookup_unpaywall(doi, email).await {
                Ok(result) => return Ok(result),
                Err(unpaywall_err) => {
                    let mut result = self.lookup_openalex(doi).await?;
                    if !result.success {
                        result.message = format!(
                            "Unpaywall lookup error: {}. Fallback: {}",
                            unpaywall_err, result.message
                        );
                    }
                    return Ok(result);
                }
            }
        }

        self.lookup_openalex(doi).await
    }

    fn result_from_unpaywall_payload(&self, doi: &str, payload: UnpaywallResponse) -> SciHubResult {
        let mut pdf_url = payload
            .best_oa_location
            .as_ref()
            .and_then(|l| l.url_for_pdf.clone().or_else(|| l.url.clone()));

        if pdf_url.is_none() {
            pdf_url = payload.oa_locations.as_ref().and_then(|locs| {
                locs.iter()
                    .find_map(|l| l.url_for_pdf.clone().or_else(|| l.url.clone()))
            });
        }

        let authors = payload.z_authors.as_ref().map(|authors| {
            authors
                .iter()
                .filter_map(|a| match (a.given.as_deref(), a.family.as_deref()) {
                    (Some(g), Some(f)) => Some(format!("{} {}", g, f)),
                    (None, Some(f)) => Some(f.to_string()),
                    (Some(g), None) => Some(g.to_string()),
                    (None, None) => None,
                })
                .collect::<Vec<_>>()
                .join(", ")
        });

        let is_oa = payload.is_oa.unwrap_or(false);
        let success = is_oa && pdf_url.is_some();
        let message = if success {
            "Found open-access PDF via Unpaywall".to_string()
        } else if is_oa {
            "Open-access work found, but no direct PDF URL available via Unpaywall".to_string()
        } else {
            "No open-access copy found via Unpaywall".to_string()
        };

        SciHubResult {
            doi: doi.to_string(),
            pdf_url,
            title: payload.title,
            authors: authors.filter(|s| !s.trim().is_empty()),
            journal: payload.journal_name,
            year: payload.year.map(|y| y.to_string()),
            success,
            message,
        }
    }

    async fn lookup_unpaywall(
        &self,
        doi: &str,
        email: &str,
    ) -> Result<SciHubResult, ConnectorError> {
        let encoded_doi = encode(doi);
        let mut url = Url::parse(&format!("{}/v2/{}", self.unpaywall_base_url, encoded_doi))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        url.query_pairs_mut().append_pair("email", email);

        let response = self
            .client
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "HTTP status {}",
                response.status()
            )));
        }

        let payload: UnpaywallResponse = response
            .json()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        Ok(self.result_from_unpaywall_payload(doi, payload))
    }

    fn result_from_openalex_payload(&self, doi: &str, payload: OpenAlexWork) -> SciHubResult {
        let mut pdf_url = payload
            .best_oa_location
            .as_ref()
            .and_then(|l| l.pdf_url.clone().or_else(|| l.landing_page_url.clone()));

        if pdf_url.is_none() {
            pdf_url = payload.locations.as_ref().and_then(|locs| {
                locs.iter()
                    .find_map(|l| l.pdf_url.clone().or_else(|| l.landing_page_url.clone()))
            });
        }

        let is_oa = payload.open_access.and_then(|oa| oa.is_oa).unwrap_or(false);
        let success = is_oa && pdf_url.is_some();
        let message = if success {
            "Found open-access PDF via OpenAlex".to_string()
        } else if is_oa {
            "Open-access work found, but no direct PDF URL available via OpenAlex".to_string()
        } else {
            "No open-access copy found via OpenAlex".to_string()
        };

        let authors = payload.authorships.as_ref().map(|authorships| {
            authorships
                .iter()
                .filter_map(|a| a.author.as_ref().and_then(|au| au.display_name.clone()))
                .collect::<Vec<_>>()
                .join(", ")
        });

        SciHubResult {
            doi: doi.to_string(),
            pdf_url,
            title: payload.display_name,
            authors: authors.filter(|s| !s.trim().is_empty()),
            journal: payload.host_venue.and_then(|v| v.display_name),
            year: payload.publication_year.map(|y| y.to_string()),
            success,
            message,
        }
    }

    async fn lookup_openalex(&self, doi: &str) -> Result<SciHubResult, ConnectorError> {
        let doi_url = format!("https://doi.org/{}", doi);
        let encoded_doi_url = encode(&doi_url);
        let mut url = Url::parse(&format!(
            "{}/works/{}",
            self.openalex_base_url, encoded_doi_url
        ))
        .map_err(|e| ConnectorError::Other(e.to_string()))?;

        if let Some(email) = self.unpaywall_email.as_deref() {
            url.query_pairs_mut().append_pair("mailto", email);
        }

        let response = self
            .client
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        if !response.status().is_success() {
            return Ok(SciHubResult {
                doi: doi.to_string(),
                pdf_url: None,
                title: None,
                authors: None,
                journal: None,
                year: None,
                success: false,
                message: format!("OpenAlex lookup failed: HTTP status {}", response.status()),
            });
        }

        let payload: OpenAlexWork = response
            .json()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        Ok(self.result_from_openalex_payload(doi, payload))
    }

    async fn search_openalex(
        &self,
        query: &str,
        limit: u32,
        page: u32,
        oa_only: bool,
    ) -> Result<Vec<SciHubResult>, ConnectorError> {
        let mut url = Url::parse(&format!("{}/works", self.openalex_base_url))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        url.query_pairs_mut()
            .append_pair("search", query)
            .append_pair("per_page", &limit.to_string())
            .append_pair("page", &page.to_string());

        if oa_only {
            url.query_pairs_mut().append_pair("filter", "is_oa:true");
        }

        if let Some(email) = self.unpaywall_email.as_deref() {
            url.query_pairs_mut().append_pair("mailto", email);
        }

        let response = self
            .client
            .get(url)
            .headers(self.headers.clone())
            .send()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "OpenAlex search failed: HTTP status {}",
                response.status()
            )));
        }

        let payload: OpenAlexSearchResponse = response
            .json()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let results = payload
            .results
            .into_iter()
            .map(|work| {
                let doi = work
                    .doi
                    .as_deref()
                    .and_then(|d| d.strip_prefix("https://doi.org/"))
                    .unwrap_or("unknown")
                    .to_string();
                let mut result = self.result_from_openalex_payload(&doi, work);
                result.doi = doi;
                result
            })
            .collect();

        Ok(results)
    }

    fn doi_from_paper_uri<'a>(&self, uri_str: &'a str) -> Result<&'a str, ConnectorError> {
        let doi = uri_str
            .strip_prefix("scihub://paper/")
            .ok_or(ConnectorError::ResourceNotFound)?;
        if doi.trim().is_empty() {
            return Err(ConnectorError::InvalidInput(format!(
                "Invalid resource URI: {}",
                uri_str
            )));
        }
        Ok(doi)
    }
}

#[async_trait]
impl Connector for SciHubConnector {
    fn name(&self) -> &'static str {
        "scihub"
    }

    fn description(&self) -> &'static str {
        "Best-effort open-access paper lookup by DOI (via Unpaywall/OpenAlex)"
    }

    fn display_name(&self) -> &'static str {
        "Open Access (DOI)"
    }

    fn icon(&self) -> &'static str {
        "scihub"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["academic", "research", "science"]
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

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        if let Some(email) = details
            .get("unpaywall_email")
            .or_else(|| details.get("email"))
        {
            let trimmed = email.trim();
            if trimmed.is_empty() {
                self.unpaywall_email = None;
            } else {
                self.unpaywall_email = Some(trimmed.to_string());
            }
        }

        if let Some(unpaywall_base_url) = details.get("unpaywall_base_url") {
            let trimmed = unpaywall_base_url.trim();
            if !trimmed.is_empty() {
                self.unpaywall_base_url = trimmed.to_string();
            }
        }

        if let Some(openalex_base_url) = details.get("openalex_base_url") {
            let trimmed = openalex_base_url.trim();
            if !trimmed.is_empty() {
                self.openalex_base_url = trimmed.to_string();
            }
        }

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "unpaywall_email".to_string(),
                label: "Unpaywall Email (recommended)".to_string(),
                field_type: FieldType::Text,
                required: false,
                description: Some(
                    "Contact email used for the Unpaywall API (or set UNPAYWALL_EMAIL). If omitted, the connector falls back to OpenAlex.".to_string(),
                ),
                options: None,
            }],
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
                "Best-effort open-access lookup by DOI. Supply the article DOI whenever you can—it is the most precise lookup key. This connector does not bypass paywalls; it only returns openly available locations when present. Optionally set UNPAYWALL_EMAIL (or provide unpaywall_email in config) for better open-access resolution.".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let resources = vec![Resource {
            raw: RawResource {
                uri: "scihub://paper/{doi}".to_string(),
                name: "Paper Lookup Result".to_string(),
                title: None,
                description: Some(
                    "Best-effort open-access location metadata for a DOI".to_string(),
                ),
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
        let uri_str = request.uri.as_str();

        if uri_str.starts_with("scihub://paper/") {
            let doi = self.doi_from_paper_uri(uri_str)?;

            let result = self.resolve_open_access(doi).await?;

            if !result.success {
                return Err(ConnectorError::ResourceNotFound);
            }

            let content_text = serde_json::to_string(&result)?;
            Ok(vec![ResourceContents::text(content_text, uri_str)])
        } else {
            Err(ConnectorError::ResourceNotFound)
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let input_schema = Arc::new(
            json!({
                "type": "object",
                "properties": {
                    "doi": {
                        "type": "string",
                        "description": "The DOI (Digital Object Identifier) of the paper"
                    }
                },
                "required": ["doi"]
            })
            .as_object()
            .expect("Schema object")
            .clone(),
        );

        let search_schema = Arc::new(
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (title, author, keywords)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (default 10, max 200)",
                        "default": 10
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number for pagination (default 1)",
                        "default": 1
                    },
                    "oa_only": {
                        "type": "boolean",
                        "description": "If true, only return open-access works",
                        "default": false
                    }
                },
                "required": ["query"]
            })
            .as_object()
            .expect("Schema object")
            .clone(),
        );

        let batch_schema = Arc::new(
            json!({
                "type": "object",
                "properties": {
                    "dois": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of DOIs to look up (max 50)"
                    }
                },
                "required": ["dois"]
            })
            .as_object()
            .expect("Schema object")
            .clone(),
        );

        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: Cow::Borrowed("get"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Best-effort open-access lookup by DOI. Example: doi=\"10.1371/journal.pone.0000308\".",
                    )),
                    input_schema: input_schema.clone(),
                    output_schema: None,
                    annotations: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("get_paper"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Alias of scihub/get for compatibility (open-access lookup only).",
                    )),
                    input_schema,
                    output_schema: None,
                    annotations: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("search"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Search for academic papers by title, author, or keywords via OpenAlex. Example: query=\"attention mechanism\".",
                    )),
                    input_schema: search_schema,
                    output_schema: None,
                    annotations: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("batch_get"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Look up multiple DOIs concurrently (max 50). Returns per-DOI results with error tolerance.",
                    )),
                    input_schema: batch_schema,
                    output_schema: None,
                    annotations: None,
                    icons: None,
                },
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
            "get" | "get_paper" => {
                let doi = args.get("doi").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'doi' parameter".to_string()),
                )?;

                let result = self.resolve_open_access(doi).await?;
                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            "search" => {
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' parameter".to_string()),
                )?;
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10)
                    .min(200) as u32;
                let page = args
                    .get("page")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1) as u32;
                let oa_only = args
                    .get("oa_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let results = self.search_openalex(query, limit, page, oa_only).await?;
                let text = serde_json::to_string(&results)?;
                Ok(structured_result_with_text(&results, Some(text))?)
            }
            "batch_get" => {
                let dois_val = args.get("dois").ok_or(ConnectorError::InvalidParams(
                    "Missing 'dois' parameter".to_string(),
                ))?;
                let dois: Vec<String> = dois_val
                    .as_array()
                    .ok_or(ConnectorError::InvalidParams(
                        "'dois' must be an array of strings".to_string(),
                    ))?
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if dois.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "'dois' array must not be empty".to_string(),
                    ));
                }
                if dois.len() > 50 {
                    return Err(ConnectorError::InvalidParams(
                        "'dois' array must not exceed 50 entries".to_string(),
                    ));
                }

                let futures: Vec<_> = dois
                    .iter()
                    .map(|doi| async move {
                        match self.resolve_open_access(doi).await {
                            Ok(result) => result,
                            Err(e) => SciHubResult {
                                doi: doi.clone(),
                                pdf_url: None,
                                title: None,
                                authors: None,
                                journal: None,
                                year: None,
                                success: false,
                                message: format!("Lookup failed: {}", e),
                            },
                        }
                    })
                    .collect();

                let results = futures::future::join_all(futures).await;
                let text = serde_json::to_string(&results)?;
                Ok(structured_result_with_text(&results, Some(text))?)
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

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt with name {} not found",
            name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_connector() -> SciHubConnector {
        SciHubConnector {
            client: reqwest::Client::new(),
            headers: HeaderMap::new(),
            unpaywall_email: None,
            unpaywall_base_url: "https://api.unpaywall.org".to_string(),
            openalex_base_url: "https://api.openalex.org".to_string(),
        }
    }

    #[test]
    fn parses_paper_uri_doi_with_slashes() {
        let connector = test_connector();
        let doi = connector
            .doi_from_paper_uri("scihub://paper/10.1038/nature12373")
            .expect("valid DOI extraction");
        assert_eq!(doi, "10.1038/nature12373");
    }

    #[test]
    fn unpaywall_payload_maps_pdf_url_and_authors() {
        let connector = test_connector();
        let payload: UnpaywallResponse = serde_json::from_str(
            r#"{
              "is_oa": true,
              "title": "Example Title",
              "journal_name": "Example Journal",
              "year": 2020,
              "z_authors": [{"given":"Jane","family":"Doe"},{"family":"Smith"}],
              "best_oa_location": {"url_for_pdf":"https://example.test/paper.pdf"}
            }"#,
        )
        .expect("valid payload");

        let result = connector.result_from_unpaywall_payload("10.0000/example", payload);
        assert!(result.success);
        assert_eq!(
            result.pdf_url.as_deref(),
            Some("https://example.test/paper.pdf")
        );
        assert_eq!(result.authors.as_deref(), Some("Jane Doe, Smith"));
        assert_eq!(result.title.as_deref(), Some("Example Title"));
        assert_eq!(result.journal.as_deref(), Some("Example Journal"));
        assert_eq!(result.year.as_deref(), Some("2020"));
    }

    #[test]
    fn openalex_payload_maps_best_location() {
        let connector = test_connector();
        let payload: OpenAlexWork = serde_json::from_str(
            r#"{
              "display_name":"Example Title",
              "publication_year":2019,
              "host_venue":{"display_name":"Venue"},
              "open_access":{"is_oa":true},
              "authorships":[{"author":{"display_name":"A. Author"}},{"author":{"display_name":"B. Author"}}],
              "best_oa_location":{"pdf_url":"https://example.test/paper.pdf"}
            }"#,
        )
        .expect("valid payload");

        let result = connector.result_from_openalex_payload("10.0000/example", payload);
        assert!(result.success);
        assert_eq!(
            result.pdf_url.as_deref(),
            Some("https://example.test/paper.pdf")
        );
        assert_eq!(result.authors.as_deref(), Some("A. Author, B. Author"));
        assert_eq!(result.journal.as_deref(), Some("Venue"));
        assert_eq!(result.year.as_deref(), Some("2019"));
    }
}
