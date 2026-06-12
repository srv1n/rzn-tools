//! Federated search across multiple connectors.
//!
//! This module provides:
//! - `UnifiedSearchResult`: A normalized search result format
//! - `SearchProfile`: Named configurations for connector groups
//! - `FederatedSearch`: Engine for parallel multi-connector search
//!
//! # Example
//!
//! ```ignore
//! use rzn_tools_core::federated::{FederatedSearch, SearchProfile};
//!
//! let profile = SearchProfile::get_builtin("research").unwrap();
//! let engine = FederatedSearch::new(&registry);
//! let results = engine.search_with_profile("choline supplementation", &profile).await;
//! ```

mod engine;
mod profiles;
mod types;

pub use engine::FederatedSearch;
pub use profiles::{
    DeduplicationConfig, DeduplicationStrategy, ProfileStore, ProfileStoreError, SearchDefaults,
    SearchProfile, DEFAULT_GLOBAL_TIMEOUT_MS, DEFAULT_LIMIT, DEFAULT_TIMEOUT_MS, DEFAULT_WEIGHT,
};
pub use types::{
    FederatedResults, FederatedSearchResult, FederationMeta, MergeMode, SourceError, SourceResults,
    UnifiedSearchResult,
};
