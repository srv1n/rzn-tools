//! Search profile management.
//!
//! Profiles define groups of connectors with default parameters
//! for common search scenarios. Everything has sensible defaults.

use super::MergeMode;
use crate::auth_store::config_dir;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Default Values
// ============================================================================

/// Default results per source
pub const DEFAULT_LIMIT: u32 = 10;

/// Default per-source timeout in milliseconds
pub const DEFAULT_TIMEOUT_MS: u64 = 10000;

/// Default global timeout in milliseconds
pub const DEFAULT_GLOBAL_TIMEOUT_MS: u64 = 30000;

/// Default weight for sources
pub const DEFAULT_WEIGHT: f32 = 1.0;

// ============================================================================
// SearchDefaults
// ============================================================================

/// Default parameters applied to all connectors in a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDefaults {
    /// Maximum results per connector (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Response format: concise or detailed (default: concise)
    #[serde(default = "default_response_format")]
    pub response_format: String,

    /// Merge mode for results (default: grouped)
    #[serde(default)]
    pub merge_mode: MergeMode,
}

fn default_limit() -> u32 {
    DEFAULT_LIMIT
}

fn default_response_format() -> String {
    "concise".to_string()
}

impl Default for SearchDefaults {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            response_format: "concise".to_string(),
            merge_mode: MergeMode::Grouped,
        }
    }
}

// ============================================================================
// Deduplication Config
// ============================================================================

/// Strategy for detecting duplicates across sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeduplicationStrategy {
    /// Match by URL
    #[default]
    Url,
    /// Match by DOI (for academic papers)
    Doi,
    /// Fuzzy title matching
    TitleFuzzy,
}

/// Configuration for result deduplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeduplicationConfig {
    /// Whether deduplication is enabled (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// Strategy for detecting duplicates
    #[serde(default)]
    pub strategy: DeduplicationStrategy,

    /// Preferred sources when duplicate found (first = highest priority)
    #[serde(default)]
    pub prefer: Vec<String>,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: DeduplicationStrategy::Url,
            prefer: Vec::new(),
        }
    }
}

// ============================================================================
// SearchProfile
// ============================================================================

/// A named search profile configuration.
///
/// Profiles can be:
/// - Built-in (shipped with rzn-tools)
/// - User-defined (in `~/.config/rzn-tools/profiles.yaml`)
/// - Extended from other profiles using `extends`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchProfile {
    /// Profile name
    pub name: String,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Base profile to extend (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,

    /// Connectors to search (if not extending)
    #[serde(default)]
    pub connectors: Vec<String>,

    /// Connectors to add (when extending)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<String>,

    /// Connectors to exclude (when extending)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// Default parameters for all connectors
    #[serde(default)]
    pub defaults: SearchDefaults,

    /// Per-source weighting (default: 1.0)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub weights: HashMap<String, f32>,

    /// Per-connector parameter overrides
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub overrides: HashMap<String, Value>,

    /// Per-source timeout in milliseconds (default: 5000)
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Global timeout in milliseconds (default: 15000)
    #[serde(default = "default_global_timeout_ms")]
    pub global_timeout_ms: u64,

    /// Deduplication configuration
    #[serde(default)]
    pub deduplication: DeduplicationConfig,
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_global_timeout_ms() -> u64 {
    DEFAULT_GLOBAL_TIMEOUT_MS
}

impl SearchProfile {
    /// Create a new profile with the given name and connectors.
    pub fn new(name: impl Into<String>, connectors: Vec<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            extends: None,
            connectors,
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults::default(),
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        }
    }

    /// Get a built-in profile by name.
    pub fn get_builtin(name: &str) -> Option<Self> {
        BUILTIN_PROFILES.iter().find(|p| p.name == name).cloned()
    }

    /// List all built-in profiles.
    pub fn list_builtin() -> &'static [SearchProfile] {
        &BUILTIN_PROFILES
    }

    /// Get the effective connectors for this profile.
    ///
    /// If `extends` is set, this resolves the inheritance chain.
    pub fn effective_connectors(&self, store: Option<&ProfileStore>) -> Vec<String> {
        let base_connectors = if let Some(ref extends) = self.extends {
            // Resolve from store or built-in
            let base = store
                .and_then(|s| s.load(extends))
                .or_else(|| Self::get_builtin(extends));

            base.map(|p| p.effective_connectors(store))
                .unwrap_or_default()
        } else {
            self.connectors.clone()
        };

        // Apply add/exclude
        let mut result: Vec<String> = base_connectors
            .into_iter()
            .filter(|c| !self.exclude.contains(c))
            .collect();

        for connector in &self.add {
            if !result.contains(connector) {
                result.push(connector.clone());
            }
        }

        result
    }

    /// Get the limit for a specific connector.
    pub fn limit_for(&self, connector: &str) -> u32 {
        // Check connector-specific override first
        if let Some(overrides) = self.overrides.get(connector) {
            if let Some(limit) = overrides.get("limit").and_then(|v| v.as_u64()) {
                return limit as u32;
            }
        }
        // Fall back to defaults
        self.defaults.limit
    }

    /// Get the response format for a specific connector.
    pub fn response_format_for(&self, connector: &str) -> &str {
        // Check connector-specific override first
        if let Some(overrides) = self.overrides.get(connector) {
            if let Some(format) = overrides.get("response_format").and_then(|v| v.as_str()) {
                return format;
            }
        }
        // Fall back to defaults
        &self.defaults.response_format
    }

    /// Get the weight for a specific connector.
    pub fn weight_for(&self, connector: &str) -> f32 {
        self.weights
            .get(connector)
            .copied()
            .unwrap_or(DEFAULT_WEIGHT)
    }

    /// Get all overrides for a specific connector.
    pub fn overrides_for(&self, connector: &str) -> Option<&Value> {
        self.overrides.get(connector)
    }

    /// Check if a connector is in this profile's effective list.
    pub fn has_connector(&self, connector: &str, store: Option<&ProfileStore>) -> bool {
        self.effective_connectors(store)
            .iter()
            .any(|c| c == connector)
    }
}

// ============================================================================
// Built-in Profiles
// ============================================================================

/// Built-in profiles shipped with rzn-tools.
static BUILTIN_PROFILES: Lazy<Vec<SearchProfile>> = Lazy::new(|| {
    vec![
        SearchProfile {
            name: "research".to_string(),
            description: Some("Academic research across multiple databases".to_string()),
            extends: None,
            connectors: vec![
                "pubmed".to_string(),
                "arxiv".to_string(),
                "semantic-scholar".to_string(),
                "google-scholar".to_string(),
            ],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults::default(),
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
        SearchProfile {
            name: "enterprise".to_string(),
            description: Some("Enterprise document and communication search".to_string()),
            extends: None,
            connectors: vec![
                "slack".to_string(),
                "atlassian".to_string(),
                "github".to_string(),
            ],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults {
                limit: 20,
                ..SearchDefaults::default()
            },
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
        SearchProfile {
            name: "social".to_string(),
            description: Some("Social media and forum discussions".to_string()),
            extends: None,
            connectors: vec!["reddit".to_string(), "hackernews".to_string()],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults {
                limit: 15,
                ..SearchDefaults::default()
            },
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
        SearchProfile {
            name: "code".to_string(),
            description: Some("Code search across repositories".to_string()),
            extends: None,
            connectors: vec!["github".to_string()],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults {
                limit: 25,
                ..SearchDefaults::default()
            },
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
        SearchProfile {
            name: "web".to_string(),
            description: Some("Web search using AI-powered search providers".to_string()),
            extends: None,
            connectors: vec![
                "perplexity-search".to_string(),
                "exa-search".to_string(),
                "tavily-search".to_string(),
                "parallel-search".to_string(),
            ],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults::default(),
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
        SearchProfile {
            name: "media".to_string(),
            description: Some("Video and reference content search".to_string()),
            extends: None,
            connectors: vec!["youtube".to_string(), "wikipedia".to_string()],
            add: Vec::new(),
            exclude: Vec::new(),
            defaults: SearchDefaults::default(),
            weights: HashMap::new(),
            overrides: HashMap::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            global_timeout_ms: DEFAULT_GLOBAL_TIMEOUT_MS,
            deduplication: DeduplicationConfig::default(),
        },
    ]
});

// ============================================================================
// ProfileStore
// ============================================================================

/// Storage for user-defined profiles.
///
/// Profiles are stored in YAML format at `~/.config/rzn-tools/profiles.yaml`.
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    /// Create a new profile store at the default location.
    pub fn new_default() -> Self {
        let path = config_dir().join("profiles.yaml");
        Self { path }
    }

    /// Create a profile store at a custom path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Get the path to the profiles file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Load all user-defined profiles.
    pub fn load_all(&self) -> HashMap<String, SearchProfile> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => serde_yaml::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    /// Load a specific profile by name.
    ///
    /// Resolution order:
    /// 1. User profiles (from file)
    /// 2. Built-in profiles
    pub fn load(&self, name: &str) -> Option<SearchProfile> {
        // Check user profiles first
        if let Some(profile) = self.load_all().get(name).cloned() {
            return Some(profile);
        }
        // Fall back to built-in
        SearchProfile::get_builtin(name)
    }

    /// Save a profile.
    pub fn save(&self, profile: &SearchProfile) -> Result<(), ProfileStoreError> {
        let mut profiles = self.load_all();
        profiles.insert(profile.name.clone(), profile.clone());
        self.write_all(&profiles)
    }

    /// Delete a user profile.
    ///
    /// Returns `Ok(true)` if deleted, `Ok(false)` if not found.
    /// Cannot delete built-in profiles.
    pub fn delete(&self, name: &str) -> Result<bool, ProfileStoreError> {
        let mut profiles = self.load_all();
        let existed = profiles.remove(name).is_some();
        if existed {
            self.write_all(&profiles)?;
        }
        Ok(existed)
    }

    /// List all available profiles (user + built-in).
    pub fn list_all(&self) -> Vec<SearchProfile> {
        let mut profiles: Vec<SearchProfile> = self.load_all().into_values().collect();

        // Add built-in profiles that aren't overridden
        for builtin in SearchProfile::list_builtin() {
            if !profiles.iter().any(|p| p.name == builtin.name) {
                profiles.push(builtin.clone());
            }
        }

        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        profiles
    }

    /// List only built-in profile names.
    pub fn list_builtin_names() -> Vec<&'static str> {
        BUILTIN_PROFILES.iter().map(|p| p.name.as_str()).collect()
    }

    fn write_all(
        &self,
        profiles: &HashMap<String, SearchProfile>,
    ) -> Result<(), ProfileStoreError> {
        // Ensure directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ProfileStoreError::Io(e.to_string()))?;
        }

        let content = serde_yaml::to_string(profiles)
            .map_err(|e| ProfileStoreError::Serialize(e.to_string()))?;

        std::fs::write(&self.path, content).map_err(|e| ProfileStoreError::Io(e.to_string()))?;

        Ok(())
    }
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self::new_default()
    }
}

/// Errors from profile storage operations.
#[derive(Debug, thiserror::Error)]
pub enum ProfileStoreError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialize(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles() {
        let research = SearchProfile::get_builtin("research").unwrap();
        assert!(research.connectors.contains(&"pubmed".to_string()));
        assert!(research.connectors.contains(&"arxiv".to_string()));
        assert_eq!(research.defaults.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn test_all_builtin_profiles_exist() {
        for name in &["research", "enterprise", "social", "code", "web", "media"] {
            assert!(
                SearchProfile::get_builtin(name).is_some(),
                "Built-in profile '{}' should exist",
                name
            );
        }
    }

    #[test]
    fn test_profile_extension() {
        let mut child = SearchProfile::new("my-research", Vec::new());
        child.extends = Some("research".to_string());
        child.add = vec!["wikipedia".to_string()];
        child.exclude = vec!["semantic-scholar".to_string()];

        let effective = child.effective_connectors(None);
        assert!(effective.contains(&"pubmed".to_string()));
        assert!(effective.contains(&"arxiv".to_string()));
        assert!(effective.contains(&"wikipedia".to_string()));
        assert!(!effective.contains(&"semantic-scholar".to_string()));
    }

    #[test]
    fn test_connector_overrides() {
        let mut profile = SearchProfile::new("test", vec!["pubmed".to_string()]);
        profile.overrides.insert(
            "pubmed".to_string(),
            serde_json::json!({"limit": 5, "start_year": 2020}),
        );

        assert_eq!(profile.limit_for("pubmed"), 5);
        assert_eq!(profile.limit_for("arxiv"), DEFAULT_LIMIT); // Falls back to default
    }

    #[test]
    fn test_weight_for() {
        let mut profile =
            SearchProfile::new("test", vec!["pubmed".to_string(), "arxiv".to_string()]);
        profile.weights.insert("pubmed".to_string(), 1.5);

        assert_eq!(profile.weight_for("pubmed"), 1.5);
        assert_eq!(profile.weight_for("arxiv"), DEFAULT_WEIGHT);
    }

    #[test]
    fn test_yaml_serialization() {
        let profile = SearchProfile::new("test", vec!["pubmed".to_string()]);
        let yaml = serde_yaml::to_string(&profile).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("pubmed"));

        let parsed: SearchProfile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "test");
    }

    #[test]
    fn test_defaults() {
        let defaults = SearchDefaults::default();
        assert_eq!(defaults.limit, DEFAULT_LIMIT);
        assert_eq!(defaults.response_format, "concise");
        assert_eq!(defaults.merge_mode, MergeMode::Grouped);
    }
}
