//! Federated search execution engine.
//!
//! Coordinates parallel searches across multiple connectors and consolidates results.

use super::{
    FederatedSearchResult, MergeMode, SearchProfile, SourceResults, UnifiedSearchResult,
    DEFAULT_TIMEOUT_MS,
};
use crate::{CallToolRequestParam, Connector, PaginatedRequestParam, ProviderRegistry};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Engine for executing federated searches across multiple connectors.
pub struct FederatedSearch<'a> {
    registry: &'a ProviderRegistry,
}

impl<'a> FederatedSearch<'a> {
    /// Create a new federated search engine.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self { registry }
    }

    /// Execute a federated search using a profile.
    pub async fn search_with_profile(
        &self,
        query: &str,
        profile: &SearchProfile,
        merge_mode: Option<MergeMode>,
    ) -> FederatedSearchResult {
        let start = Instant::now();

        // Get effective connectors (resolving inheritance)
        let connector_names = profile.effective_connectors(None);

        let connectors: Vec<_> = connector_names
            .iter()
            .filter_map(|name| {
                self.registry
                    .get_provider(name)
                    .map(|p| (name.clone(), Arc::clone(p)))
            })
            .collect();

        let mut result = self.execute_search(query, &connectors, Some(profile)).await;

        result.profile = Some(profile.name.clone());

        // Apply merge mode
        let mode = merge_mode.unwrap_or(profile.defaults.merge_mode);
        if mode == MergeMode::Interleaved {
            result.finalize_interleaved();
        }

        result.duration_ms = Some(start.elapsed().as_millis() as u64);
        result
    }

    /// Execute a federated search with an ad-hoc list of connectors.
    pub async fn search_adhoc(
        &self,
        query: &str,
        connector_names: &[String],
        merge_mode: MergeMode,
    ) -> FederatedSearchResult {
        let start = Instant::now();

        let connectors: Vec<_> = connector_names
            .iter()
            .filter_map(|name| {
                self.registry
                    .get_provider(name)
                    .map(|p| (name.clone(), Arc::clone(p)))
            })
            .collect();

        let mut result = self.execute_search(query, &connectors, None).await;

        if merge_mode == MergeMode::Interleaved {
            result.finalize_interleaved();
        }

        result.duration_ms = Some(start.elapsed().as_millis() as u64);
        result
    }

    #[allow(clippy::type_complexity)]
    async fn execute_search(
        &self,
        query: &str,
        connectors: &[(String, Arc<Mutex<Box<dyn Connector>>>)],
        profile: Option<&SearchProfile>,
    ) -> FederatedSearchResult {
        let mut result = FederatedSearchResult::new_grouped(query);

        // Get timeout from profile or use default
        let timeout_ms = profile.map(|p| p.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS);

        // Execute searches in parallel with timeout
        let futures: Vec<_> = connectors
            .iter()
            .map(|(name, connector)| {
                let name = name.clone();
                let connector = Arc::clone(connector);
                let query = query.to_string();
                let limit = profile.map(|p| p.limit_for(&name));
                let response_format = profile.map(|p| p.response_format_for(&name).to_string());
                let weight = profile.map(|p| p.weight_for(&name)).unwrap_or(1.0);
                let overrides = profile.and_then(|p| p.overrides_for(&name).cloned());

                async move {
                    let start = Instant::now();

                    // Wrap the search in a timeout
                    let search_future = search_single_connector(
                        name.clone(),
                        connector,
                        query,
                        limit,
                        response_format,
                        weight,
                        overrides,
                    );

                    match timeout(Duration::from_millis(timeout_ms), search_future).await {
                        Ok(search_result) => match search_result {
                            Ok(mut source_results) => {
                                source_results.duration_ms =
                                    Some(start.elapsed().as_millis() as u64);
                                Ok(source_results)
                            }
                            Err((source, error)) => Err((source, error, false)),
                        },
                        Err(_) => Err((name, format!("timeout after {}ms", timeout_ms), true)),
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        for search_result in results {
            match search_result {
                Ok(source_results) => result.add_source(source_results),
                Err((source, error, is_timeout)) => result.add_error(source, error, is_timeout),
            }
        }

        result
    }
}

/// Search a single connector and normalize results.
async fn search_single_connector(
    name: String,
    connector: Arc<Mutex<Box<dyn Connector>>>,
    query: String,
    limit: Option<u32>,
    response_format: Option<String>,
    weight: f32,
    overrides: Option<Value>,
) -> Result<SourceResults, (String, String)> {
    let connector = connector.lock().await;

    // Find the search tool
    let tools = connector
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await
        .map_err(|e| (name.clone(), e.to_string()))?;

    let search_tool = tools
        .tools
        .iter()
        .find(|t| t.name.contains("search") || t.name.contains("query"))
        .ok_or_else(|| (name.clone(), "No search tool found".to_string()))?;

    // Build arguments
    let mut args = json!({
        "query": query,
    });

    if let Some(l) = limit {
        args["limit"] = json!(l);
    }

    if let Some(format) = response_format {
        args["response_format"] = json!(format);
    }

    // Merge any connector-specific overrides
    if let Some(overrides) = overrides {
        if let (Some(args_obj), Some(overrides_obj)) = (args.as_object_mut(), overrides.as_object())
        {
            for (k, v) in overrides_obj {
                args_obj.insert(k.clone(), v.clone());
            }
        }
    }

    // Execute search
    let request = CallToolRequestParam {
        name: search_tool.name.clone(),
        arguments: Some(args.as_object().unwrap().clone()),
    };

    let response = connector
        .call_tool(request)
        .await
        .map_err(|e| (name.clone(), e.to_string()))?;

    // Normalize results
    let raw_results = response.structured_content.unwrap_or(json!({}));
    let normalized = normalize_results(&name, &raw_results, weight);

    Ok(SourceResults {
        source: name,
        count: normalized.len(),
        total_available: extract_total_count(&raw_results),
        results: normalized,
        duration_ms: None, // Will be set by caller
    })
}

/// Extract total count from various result formats.
fn extract_total_count(raw: &Value) -> Option<usize> {
    raw.get("total_results")
        .or_else(|| raw.get("total_count"))
        .or_else(|| raw.get("totalCount"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
}

/// Normalize raw results to unified format.
///
/// This handles the various result formats from different connectors.
fn normalize_results(source: &str, raw: &Value, weight: f32) -> Vec<UnifiedSearchResult> {
    // Try to find the results array
    let items = find_results_array(raw);

    items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| normalize_single_result(source, item, idx + 1, weight))
        .collect()
}

/// Find the results array in various response formats.
fn find_results_array(raw: &Value) -> Vec<&Value> {
    // Try common array field names
    for field in &[
        "results", "articles", "papers", "items", "stories", "posts", "videos",
    ] {
        if let Some(arr) = raw.get(*field).and_then(|v| v.as_array()) {
            return arr.iter().collect();
        }
    }

    // If raw is already an array
    if let Some(arr) = raw.as_array() {
        return arr.iter().collect();
    }

    Vec::new()
}

/// Normalize a single result item to unified format.
fn normalize_single_result(
    source: &str,
    item: &Value,
    rank: usize,
    weight: f32,
) -> Option<UnifiedSearchResult> {
    let id = extract_id(source, item)?;
    let title = extract_title(item)?;

    let mut result = UnifiedSearchResult::new(source, id, title, rank).with_weight(weight);

    // Extract optional fields
    if let Some(snippet) = extract_snippet(item) {
        result = result.with_snippet(snippet);
    }

    if let Some(url) = extract_url(item) {
        result = result.with_url(url);
    }

    // Preserve source-specific metadata
    result = result.with_metadata(extract_metadata(source, item));

    Some(result)
}

/// Extract ID from various formats.
fn extract_id(source: &str, item: &Value) -> Option<String> {
    // Source-specific ID formats
    match source {
        "pubmed" => item
            .get("pmid")
            .and_then(|v| v.as_str())
            .map(|s| format!("PMID:{}", s)),
        "arxiv" => item
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| format!("arXiv:{}", s)),
        "biorxiv" => item
            .get("doi")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "hackernews" => item
            .get("id")
            .and_then(|v| v.as_u64())
            .map(|n| format!("hn:{}", n)),
        "github" => {
            // Could be issue number or file path
            if let Some(num) = item.get("number").and_then(|v| v.as_u64()) {
                Some(format!("#{}", num))
            } else {
                item.get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
        }
        "reddit" => item
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| format!("reddit:{}", s)),
        "wikipedia" => item
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| format!("wiki:{}", s.replace(' ', "_"))),
        "google-scholar" => {
            // Google Scholar doesn't have IDs, use the link as identifier
            item.get("link")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }
        "semantic-scholar" | "semantic_scholar" => {
            // Semantic Scholar uses paperId
            item.get("paperId")
                .or_else(|| item.get("paper_id"))
                .or_else(|| item.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| format!("S2:{}", s))
        }
        _ => {
            // Try common ID fields, falling back to link/url for web sources
            item.get("id")
                .or_else(|| item.get("pmid"))
                .or_else(|| item.get("doi"))
                .or_else(|| item.get("link"))
                .or_else(|| item.get("url"))
                .and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                })
        }
    }
}

/// Extract title from various formats.
fn extract_title(item: &Value) -> Option<String> {
    item.get("title")
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extract snippet/preview from various formats.
fn extract_snippet(item: &Value) -> Option<String> {
    for field in &[
        "snippet",
        "abstract",
        "abstract_text",
        "summary",
        "description",
        "text",
        "body",
        "selftext",
    ] {
        if let Some(s) = item.get(*field).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                // Truncate long snippets
                let truncated = if s.len() > 300 {
                    format!("{}...", &s[..300])
                } else {
                    s.to_string()
                };
                return Some(truncated);
            }
        }
    }
    None
}

/// Extract URL from various formats.
fn extract_url(item: &Value) -> Option<String> {
    for field in &["url", "html_url", "link", "pdf_url", "web_url", "permalink"] {
        if let Some(s) = item.get(*field).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

/// Extract source-specific metadata.
fn extract_metadata(source: &str, item: &Value) -> Value {
    match source {
        "pubmed" => json!({
            "authors": item.get("authors"),
            "journal": item.get("journal"),
            "citation": item.get("citation"),
        }),
        "arxiv" => json!({
            "authors": item.get("authors"),
            "categories": item.get("categories"),
            "published": item.get("published"),
        }),
        "biorxiv" => json!({
            "authors": item.get("authors"),
            "category": item.get("category"),
            "date": item.get("date"),
        }),
        "hackernews" => json!({
            "score": item.get("score"),
            "by": item.get("by"),
            "descendants": item.get("descendants"),
        }),
        "github" => json!({
            "repository": item.get("repository"),
            "state": item.get("state"),
            "labels": item.get("labels"),
        }),
        "reddit" => json!({
            "subreddit": item.get("subreddit"),
            "score": item.get("score"),
            "author": item.get("author"),
        }),
        "wikipedia" => json!({
            "pageid": item.get("pageid"),
        }),
        "google-scholar" => json!({
            "authors_venue_year": item.get("authors_venue_year"),
            "year": item.get("year"),
        }),
        "semantic-scholar" | "semantic_scholar" => json!({
            "authors": item.get("authors"),
            "year": item.get("year"),
            "citationCount": item.get("citationCount"),
            "venue": item.get("venue"),
        }),
        _ => {
            // Return a minimal subset for unknown sources
            json!({})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_pubmed_result() {
        let raw = json!({
            "pmid": "12345678",
            "title": "Test Article Title",
            "authors": "Smith J, Jones A",
            "abstract_text": "This is the abstract."
        });

        let result = normalize_single_result("pubmed", &raw, 1, 1.0).unwrap();
        assert_eq!(result.source, "pubmed");
        assert_eq!(result.id, "PMID:12345678");
        assert_eq!(result.title, "Test Article Title");
        assert!(result.snippet.is_some());
        assert_eq!(result.federation.source_rank, 1);
        assert_eq!(result.federation.weight, 1.0);
    }

    #[test]
    fn test_normalize_hackernews_result() {
        let raw = json!({
            "id": 38500000,
            "title": "Show HN: Something Cool",
            "score": 150,
            "by": "username",
            "url": "https://example.com"
        });

        let result = normalize_single_result("hackernews", &raw, 2, 1.5).unwrap();
        assert_eq!(result.source, "hackernews");
        assert_eq!(result.id, "hn:38500000");
        assert_eq!(result.url, Some("https://example.com".to_string()));
        assert_eq!(result.federation.source_rank, 2);
        assert_eq!(result.federation.weight, 1.5);
    }

    #[test]
    fn test_find_results_array() {
        let with_articles = json!({"articles": [{"id": 1}, {"id": 2}]});
        assert_eq!(find_results_array(&with_articles).len(), 2);

        let with_results = json!({"results": [{"id": 1}]});
        assert_eq!(find_results_array(&with_results).len(), 1);

        let direct_array = json!([{"id": 1}, {"id": 2}, {"id": 3}]);
        assert_eq!(find_results_array(&direct_array).len(), 3);
    }

    #[test]
    fn test_normalize_results_with_weights() {
        let raw = json!({
            "articles": [
                {"pmid": "1", "title": "First"},
                {"pmid": "2", "title": "Second"},
            ]
        });

        let results = normalize_results("pubmed", &raw, 1.5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].federation.source_rank, 1);
        assert_eq!(results[0].federation.weight, 1.5);
        assert_eq!(results[1].federation.source_rank, 2);
        assert_eq!(results[1].federation.weight, 1.5);
    }
}
