//! Core types for federated search results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How to merge results from multiple sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMode {
    /// Results grouped by source (default)
    #[default]
    Grouped,
    /// Results interleaved into single ranked list
    Interleaved,
}

/// Federation metadata attached to each result for transparency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationMeta {
    /// Original rank within source (1-indexed, before merge)
    pub source_rank: usize,

    /// Weight applied to this source (default: 1.0)
    #[serde(default = "default_weight")]
    pub weight: f32,

    /// Computed score for interleaved merge (higher = better)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

fn default_weight() -> f32 {
    1.0
}

impl Default for FederationMeta {
    fn default() -> Self {
        Self {
            source_rank: 1,
            weight: 1.0,
            score: None,
        }
    }
}

impl FederationMeta {
    /// Create federation metadata for a result at the given rank.
    pub fn new(source_rank: usize) -> Self {
        Self {
            source_rank,
            weight: 1.0,
            score: None,
        }
    }

    /// Set the weight for this result's source.
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    /// Compute and set the score for interleaved ranking.
    ///
    /// Formula: score = (1 / source_rank) * weight
    pub fn compute_score(&mut self) {
        self.score = Some((1.0 / self.source_rank as f32) * self.weight);
    }
}

/// A normalized search result from any connector.
///
/// This provides a common structure that all connector results map to,
/// enabling unified display and processing across heterogeneous sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedSearchResult {
    /// Source connector name (e.g., "pubmed", "arxiv")
    pub source: String,

    /// Unique identifier within the source.
    ///
    /// Format varies by source:
    /// - PubMed: "PMID:12345678"
    /// - ArXiv: "arXiv:2301.07041"
    /// - GitHub: "rust-lang/rust#12345"
    /// - HackerNews: "hn:38500000"
    pub id: String,

    /// Result title
    pub title: String,

    /// Preview/snippet/abstract (truncated if needed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,

    /// URL to the full content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// When the content was created/published
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,

    /// Source-specific metadata (authors, tags, categories, etc.)
    ///
    /// This preserves connector-specific fields that don't map to
    /// the common schema but may be useful for display or filtering.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,

    /// Federation tracking metadata
    #[serde(rename = "_federation")]
    pub federation: FederationMeta,
}

impl UnifiedSearchResult {
    /// Create a new unified result with required fields.
    pub fn new(
        source: impl Into<String>,
        id: impl Into<String>,
        title: impl Into<String>,
        source_rank: usize,
    ) -> Self {
        Self {
            source: source.into(),
            id: id.into(),
            title: title.into(),
            snippet: None,
            url: None,
            timestamp: None,
            metadata: Value::Null,
            federation: FederationMeta::new(source_rank),
        }
    }

    /// Builder method to add a snippet.
    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }

    /// Builder method to add a URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Builder method to add a timestamp.
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Builder method to add metadata.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Builder method to set the source weight.
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.federation.weight = weight;
        self
    }

    /// Compute the score for interleaved ranking.
    pub fn compute_score(&mut self) {
        self.federation.compute_score();
    }
}

/// Results from a single source in a federated search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResults {
    /// Connector name
    pub source: String,

    /// Normalized results from this source
    pub results: Vec<UnifiedSearchResult>,

    /// Number of results returned
    pub count: usize,

    /// Total results available (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_available: Option<usize>,

    /// Time taken to fetch results (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Error from a source that failed during federated search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceError {
    /// Connector name that failed
    pub source: String,

    /// Error message
    pub error: String,

    /// Whether this was a timeout
    #[serde(default)]
    pub is_timeout: bool,
}

/// Results container - either grouped or interleaved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FederatedResults {
    /// Results grouped by source
    Grouped { sources: Vec<SourceResults> },

    /// Results interleaved into single ranked list
    Interleaved { results: Vec<UnifiedSearchResult> },
}

/// Complete results from a federated search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedSearchResult {
    /// The search query
    pub query: String,

    /// Profile used (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// Merge mode used
    pub merge_mode: MergeMode,

    /// Results (grouped or interleaved)
    pub results: FederatedResults,

    /// Total results across all sources
    pub total_count: usize,

    /// Sources that completed successfully
    pub completed: Vec<String>,

    /// Sources that failed (partial results)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<SourceError>,

    /// Whether results are partial (some sources failed/timed out)
    #[serde(default)]
    pub partial: bool,

    /// Total time taken (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl FederatedSearchResult {
    /// Create a new federated result for grouped mode.
    pub fn new_grouped(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            profile: None,
            merge_mode: MergeMode::Grouped,
            results: FederatedResults::Grouped {
                sources: Vec::new(),
            },
            total_count: 0,
            completed: Vec::new(),
            errors: Vec::new(),
            partial: false,
            duration_ms: None,
        }
    }

    /// Create a new federated result for interleaved mode.
    pub fn new_interleaved(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            profile: None,
            merge_mode: MergeMode::Interleaved,
            results: FederatedResults::Interleaved {
                results: Vec::new(),
            },
            total_count: 0,
            completed: Vec::new(),
            errors: Vec::new(),
            partial: false,
            duration_ms: None,
        }
    }

    /// Add results from a source (for grouped mode).
    pub fn add_source(&mut self, source: SourceResults) {
        self.total_count += source.count;
        self.completed.push(source.source.clone());

        if let FederatedResults::Grouped { sources } = &mut self.results {
            sources.push(source);
        }
    }

    /// Add an error from a failed source.
    pub fn add_error(
        &mut self,
        source: impl Into<String>,
        error: impl Into<String>,
        is_timeout: bool,
    ) {
        self.errors.push(SourceError {
            source: source.into(),
            error: error.into(),
            is_timeout,
        });
        self.partial = true;
    }

    /// Finalize interleaved results from grouped sources.
    ///
    /// This takes grouped results, computes scores, sorts by score,
    /// and converts to interleaved format.
    pub fn finalize_interleaved(&mut self) {
        if let FederatedResults::Grouped { sources } = &self.results {
            let mut all_results: Vec<UnifiedSearchResult> =
                sources.iter().flat_map(|s| s.results.clone()).collect();

            // Compute scores and sort by descending score
            for result in &mut all_results {
                result.compute_score();
            }
            all_results.sort_by(|a, b| {
                let score_a = a.federation.score.unwrap_or(0.0);
                let score_b = b.federation.score.unwrap_or(0.0);
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            self.results = FederatedResults::Interleaved {
                results: all_results,
            };
            self.merge_mode = MergeMode::Interleaved;
        }
    }

    /// Check if any sources failed.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Check if all sources failed (no results).
    pub fn all_failed(&self) -> bool {
        self.completed.is_empty() && !self.errors.is_empty()
    }

    /// Get all results flattened (regardless of mode).
    pub fn all_results(&self) -> Vec<&UnifiedSearchResult> {
        match &self.results {
            FederatedResults::Grouped { sources } => {
                sources.iter().flat_map(|s| s.results.iter()).collect()
            }
            FederatedResults::Interleaved { results } => results.iter().collect(),
        }
    }

    /// Get grouped sources (returns None if interleaved).
    pub fn grouped_sources(&self) -> Option<&Vec<SourceResults>> {
        match &self.results {
            FederatedResults::Grouped { sources } => Some(sources),
            FederatedResults::Interleaved { .. } => None,
        }
    }

    /// Get interleaved results (returns None if grouped).
    pub fn interleaved_results(&self) -> Option<&Vec<UnifiedSearchResult>> {
        match &self.results {
            FederatedResults::Grouped { .. } => None,
            FederatedResults::Interleaved { results } => Some(results),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_unified_result_builder() {
        let result = UnifiedSearchResult::new("pubmed", "PMID:12345", "Test Article", 1)
            .with_snippet("This is a test abstract...")
            .with_url("https://pubmed.ncbi.nlm.nih.gov/12345")
            .with_metadata(json!({"authors": ["Smith J", "Jones A"]}))
            .with_weight(1.5);

        assert_eq!(result.source, "pubmed");
        assert_eq!(result.id, "PMID:12345");
        assert_eq!(result.title, "Test Article");
        assert!(result.snippet.is_some());
        assert!(result.url.is_some());
        assert_eq!(result.federation.weight, 1.5);
        assert_eq!(result.federation.source_rank, 1);
    }

    #[test]
    fn test_federation_meta_score() {
        let mut meta = FederationMeta::new(1).with_weight(1.5);
        meta.compute_score();
        assert_eq!(meta.score, Some(1.5)); // (1/1) * 1.5

        let mut meta2 = FederationMeta::new(2).with_weight(1.0);
        meta2.compute_score();
        assert_eq!(meta2.score, Some(0.5)); // (1/2) * 1.0

        let mut meta3 = FederationMeta::new(3).with_weight(2.0);
        meta3.compute_score();
        assert!((meta3.score.unwrap() - 0.666).abs() < 0.01); // (1/3) * 2.0
    }

    #[test]
    fn test_federated_result_grouped() {
        let mut federated = FederatedSearchResult::new_grouped("test query");

        federated.add_source(SourceResults {
            source: "pubmed".to_string(),
            results: vec![
                UnifiedSearchResult::new("pubmed", "PMID:1", "Article 1", 1),
                UnifiedSearchResult::new("pubmed", "PMID:2", "Article 2", 2),
            ],
            count: 2,
            total_available: Some(100),
            duration_ms: Some(150),
        });

        federated.add_source(SourceResults {
            source: "arxiv".to_string(),
            results: vec![UnifiedSearchResult::new(
                "arxiv",
                "arXiv:2301.00001",
                "Paper 1",
                1,
            )],
            count: 1,
            total_available: None,
            duration_ms: Some(200),
        });

        federated.add_error("biorxiv", "Connection timeout", true);

        assert_eq!(federated.total_count, 3);
        assert_eq!(federated.completed.len(), 2);
        assert!(federated.has_errors());
        assert!(federated.partial);
        assert!(!federated.all_failed());
        assert_eq!(federated.all_results().len(), 3);
    }

    #[test]
    fn test_finalize_interleaved() {
        let mut federated = FederatedSearchResult::new_grouped("test");

        // Add pubmed results with weight 1.5
        federated.add_source(SourceResults {
            source: "pubmed".to_string(),
            results: vec![
                UnifiedSearchResult::new("pubmed", "PMID:1", "PubMed #1", 1).with_weight(1.5),
                UnifiedSearchResult::new("pubmed", "PMID:2", "PubMed #2", 2).with_weight(1.5),
            ],
            count: 2,
            total_available: None,
            duration_ms: None,
        });

        // Add arxiv results with weight 1.0
        federated.add_source(SourceResults {
            source: "arxiv".to_string(),
            results: vec![
                UnifiedSearchResult::new("arxiv", "arXiv:1", "ArXiv #1", 1).with_weight(1.0)
            ],
            count: 1,
            total_available: None,
            duration_ms: None,
        });

        // Convert to interleaved
        federated.finalize_interleaved();

        assert_eq!(federated.merge_mode, MergeMode::Interleaved);

        if let Some(results) = federated.interleaved_results() {
            assert_eq!(results.len(), 3);
            // PubMed #1 should be first (score 1.5), then ArXiv #1 (1.0), then PubMed #2 (0.75)
            assert_eq!(results[0].source, "pubmed");
            assert_eq!(results[0].id, "PMID:1");
            assert_eq!(results[1].source, "arxiv");
            assert_eq!(results[2].source, "pubmed");
            assert_eq!(results[2].id, "PMID:2");
        } else {
            panic!("Expected interleaved results");
        }
    }

    #[test]
    fn test_serialization() {
        let result = UnifiedSearchResult::new("pubmed", "PMID:12345", "Test", 1);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"_federation\""));
        assert!(json.contains("\"source_rank\":1"));
    }
}
