//! Smart input resolver that detects URLs, IDs, and queries and routes them to the appropriate connector.
//!
//! This module provides a pattern-matching layer on top of connectors. Given an arbitrary input string,
//! it determines which connector and tool to use, and extracts the relevant parameters.
//!
//! # Example
//!
//! ```rust,ignore
//! use rzn_tools_core::resolver::{SmartResolver, ResolvedAction};
//!
//! let resolver = SmartResolver::new();
//!
//! // YouTube URL -> get
//! let action = resolver.resolve("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
//! assert_eq!(action.connector, "youtube");
//! assert_eq!(action.tool, "get");
//!
//! // PubMed ID -> get
//! let action = resolver.resolve("PMID:12345678");
//! assert_eq!(action.connector, "pubmed");
//!
//! // ArXiv ID -> get
//! let action = resolver.resolve("arXiv:2301.07041");
//! assert_eq!(action.connector, "arxiv");
//! ```
//!
//! Note: the resolver only routes to tools that are implemented and exposed by each connector's
//! `list_tools()` surface (kept intentionally small for agent use).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A resolved action ready to be executed against a connector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAction {
    /// The connector to use (e.g., "youtube", "pubmed")
    pub connector: String,
    /// The tool to call on the connector (e.g., "get", "search")
    pub tool: String,
    /// Arguments to pass to the tool
    pub arguments: HashMap<String, serde_json::Value>,
    /// Confidence score (0.0 - 1.0) for this match
    pub confidence: f32,
    /// Priority of the pattern (higher = more specific)
    pub priority: u32,
    /// Human-readable description of what was detected
    pub description: String,
}

/// Pattern definition for matching inputs
#[derive(Debug, Clone)]
pub struct InputPattern {
    /// Unique identifier for this pattern
    pub id: &'static str,
    /// The connector this pattern routes to
    pub connector: &'static str,
    /// The tool to call when matched
    pub tool: &'static str,
    /// Regex pattern to match against input
    pub pattern: Regex,
    /// Names of capture groups to extract as arguments
    pub captures: &'static [&'static str],
    /// How to map captures to tool arguments (capture_name -> arg_name)
    pub arg_mapping: &'static [(&'static str, &'static str)],
    /// Priority (higher = checked first)
    pub priority: u32,
    /// Human-readable description
    pub description: &'static str,
}

/// Smart resolver that matches inputs to connector actions
pub struct SmartResolver {
    patterns: Vec<InputPattern>,
}

impl Default for SmartResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartResolver {
    /// Create a new resolver with all default patterns
    pub fn new() -> Self {
        Self {
            patterns: build_default_patterns(),
        }
    }

    /// Resolve an input string to an action
    ///
    /// Returns `None` if no pattern matches the input.
    pub fn resolve(&self, input: &str) -> Option<ResolvedAction> {
        let input = input.trim();

        for pattern in &self.patterns {
            if let Some(captures) = pattern.pattern.captures(input) {
                let mut arguments = HashMap::new();

                // Extract captures and map to arguments
                for (capture_name, arg_name) in pattern.arg_mapping {
                    if let Some(m) = captures.name(capture_name) {
                        arguments.insert(
                            arg_name.to_string(),
                            serde_json::Value::String(m.as_str().to_string()),
                        );
                    }
                }

                // Special handling for biorxiv/medrxiv server argument
                if pattern.id == "biorxiv_url" {
                    if let Some(server_match) = captures.name("server") {
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String(server_match.as_str().to_string()),
                        );
                    }
                } else if pattern.id == "biorxiv_doi" {
                    if let Some(prefix_match) = captures.name("prefix") {
                        let server = match prefix_match.as_str() {
                            "biorxiv" => "biorxiv",
                            "medrxiv" => "medrxiv",
                            _ => "biorxiv", // Default if prefix is not biorxiv/medrxiv (should not happen with regex)
                        };
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String(server.to_string()),
                        );
                    } else {
                        // Default to biorxiv if no prefix is matched (e.g. bare DOI)
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String("biorxiv".to_string()),
                        );
                    }
                }

                return Some(ResolvedAction {
                    connector: pattern.connector.to_string(),
                    tool: pattern.tool.to_string(),
                    arguments,
                    confidence: 1.0,
                    priority: pattern.priority,
                    description: pattern.description.to_string(),
                });
            }
        }

        None
    }

    /// Resolve input, returning all possible matches sorted by confidence
    pub fn resolve_all(&self, input: &str) -> Vec<ResolvedAction> {
        let input = input.trim();
        let mut results = Vec::new();

        for pattern in &self.patterns {
            if let Some(captures) = pattern.pattern.captures(input) {
                let mut arguments = HashMap::new();

                for (capture_name, arg_name) in pattern.arg_mapping {
                    if let Some(m) = captures.name(capture_name) {
                        arguments.insert(
                            arg_name.to_string(),
                            serde_json::Value::String(m.as_str().to_string()),
                        );
                    }
                }

                // Special handling for biorxiv/medrxiv server argument
                if pattern.id == "biorxiv_url" {
                    if let Some(server_match) = captures.name("server") {
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String(server_match.as_str().to_string()),
                        );
                    }
                } else if pattern.id == "biorxiv_doi" {
                    if let Some(prefix_match) = captures.name("prefix") {
                        let server = match prefix_match.as_str() {
                            "biorxiv" => "biorxiv",
                            "medrxiv" => "medrxiv",
                            _ => "biorxiv", // Default if prefix is not biorxiv/medrxiv (should not happen with regex)
                        };
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String(server.to_string()),
                        );
                    } else {
                        // Default to biorxiv if no prefix is matched (e.g. bare DOI)
                        arguments.insert(
                            "server".to_string(),
                            serde_json::Value::String("biorxiv".to_string()),
                        );
                    }
                }

                results.push(ResolvedAction {
                    connector: pattern.connector.to_string(),
                    tool: pattern.tool.to_string(),
                    arguments,
                    confidence: 1.0,
                    priority: pattern.priority,
                    description: pattern.description.to_string(),
                });
            }
        }

        results
    }

    /// Check if an input matches any pattern
    pub fn can_resolve(&self, input: &str) -> bool {
        let input = input.trim();
        self.patterns.iter().any(|p| p.pattern.is_match(input))
    }

    /// Get list of all supported patterns (for documentation/help)
    pub fn list_patterns(&self) -> Vec<PatternInfo> {
        self.patterns
            .iter()
            .map(|p| PatternInfo {
                id: p.id.to_string(),
                connector: p.connector.to_string(),
                tool: p.tool.to_string(),
                description: p.description.to_string(),
                example: get_pattern_example(p.id),
            })
            .collect()
    }
}

/// Information about a pattern for documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternInfo {
    pub id: String,
    pub connector: String,
    pub tool: String,
    pub description: String,
    pub example: String,
}

/// Build the default set of patterns
fn build_default_patterns() -> Vec<InputPattern> {
    let mut patterns = vec![
        // === YouTube ===
        InputPattern {
            id: "youtube_playlist_url",
            connector: "youtube",
            tool: "list",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/playlist\?(?:[^#\s]*?&)?list=(?P<playlist>[A-Za-z0-9_-]+)").unwrap(),
            captures: &["playlist"],
            arg_mapping: &[("playlist", "playlist")],
            priority: 110,
            description: "YouTube playlist URL (enumerates videos)",
        },
        InputPattern {
            id: "youtube_playlist_id",
            connector: "youtube",
            tool: "list",
            pattern: Regex::new(r"^(?P<playlist>(?:PL|UU|OLAK5)[A-Za-z0-9_-]{10,})$").unwrap(),
            captures: &["playlist"],
            arg_mapping: &[("playlist", "playlist")],
            priority: 60,
            description: "YouTube playlist ID (enumerates videos)",
        },
        InputPattern {
            id: "youtube_channel_handle_url",
            connector: "youtube",
            tool: "list",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/(?P<channel>@[A-Za-z0-9_.-]+)(?:/(?:videos|streams|shorts|featured))?/?(?:[?#][^\s]*)?$").unwrap(),
            captures: &["channel"],
            arg_mapping: &[("channel", "channel")],
            priority: 110,
            description: "YouTube channel handle URL (enumerates uploads)",
        },
        InputPattern {
            id: "youtube_channel_id_url",
            connector: "youtube",
            tool: "list",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/channel/(?P<channel>UC[A-Za-z0-9_-]{10,})(?:/(?:videos|streams|shorts|featured))?/?(?:[?#][^\s]*)?$").unwrap(),
            captures: &["channel"],
            arg_mapping: &[("channel", "channel")],
            priority: 110,
            description: "YouTube channel URL (enumerates uploads)",
        },
        InputPattern {
            id: "youtube_url_watch",
            connector: "youtube",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/watch\?v=(?P<video_id>[a-zA-Z0-9_-]{11})").unwrap(),
            captures: &["video_id"],
            arg_mapping: &[("video_id", "video_id")],
            priority: 100,
            description: "YouTube video URL (youtube.com/watch?v=...)",
        },
        InputPattern {
            id: "youtube_url_short",
            connector: "youtube",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?youtu\.be/(?P<video_id>[a-zA-Z0-9_-]{11})").unwrap(),
            captures: &["video_id"],
            arg_mapping: &[("video_id", "video_id")],
            priority: 100,
            description: "YouTube short URL (youtu.be/...)",
        },
        InputPattern {
            id: "youtube_url_embed",
            connector: "youtube",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?youtube\.com/embed/(?P<video_id>[a-zA-Z0-9_-]{11})").unwrap(),
            captures: &["video_id"],
            arg_mapping: &[("video_id", "video_id")],
            priority: 100,
            description: "YouTube embed URL",
        },
        InputPattern {
            id: "youtube_video_id",
            connector: "youtube",
            tool: "get",
            pattern: Regex::new(r"^(?P<video_id>[a-zA-Z0-9_-]{11})$").unwrap(),
            captures: &["video_id"],
            arg_mapping: &[("video_id", "video_id")],
            priority: 10, // Low priority - only match bare 11-char strings
            description: "YouTube video ID (11 characters)",
        },

        // === Hacker News ===
        InputPattern {
            id: "hackernews_url",
            connector: "hackernews",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?news\.ycombinator\.com/item\?id=(?P<item_id>\d+)").unwrap(),
            captures: &["item_id"],
            arg_mapping: &[("item_id", "id")],
            priority: 100,
            description: "Hacker News item URL",
        },
        InputPattern {
            id: "hackernews_id",
            connector: "hackernews",
            tool: "get",
            pattern: Regex::new(r"^(?:hn:|HN:)?(?P<item_id>\d{7,9})$").unwrap(),
            captures: &["item_id"],
            arg_mapping: &[("item_id", "id")],
            priority: 50,
            description: "Hacker News item ID (7-9 digits, optionally prefixed with hn:)",
        },

        // === ArXiv ===
        InputPattern {
            id: "arxiv_url",
            connector: "arxiv",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?arxiv\.org/(?:abs|pdf)/(?P<arxiv_id>\d{4}\.\d{4,5}(?:v\d+)?)").unwrap(),
            captures: &["arxiv_id"],
            arg_mapping: &[("arxiv_id", "paper_id")],
            priority: 100,
            description: "ArXiv paper URL",
        },
        InputPattern {
            id: "arxiv_id",
            connector: "arxiv",
            tool: "get",
            pattern: Regex::new(r"^(?:arXiv:|arxiv:)?(?P<arxiv_id>\d{4}\.\d{4,5}(?:v\d+)?)$").unwrap(),
            captures: &["arxiv_id"],
            arg_mapping: &[("arxiv_id", "paper_id")],
            priority: 90,
            description: "ArXiv paper ID (e.g., 2301.07041 or arXiv:2301.07041)",
        },
        InputPattern {
            id: "arxiv_old_id",
            connector: "arxiv",
            tool: "get",
            pattern: Regex::new(r"^(?:arXiv:|arxiv:)?(?P<arxiv_id>[a-z-]+/\d{7})$").unwrap(),
            captures: &["arxiv_id"],
            arg_mapping: &[("arxiv_id", "paper_id")],
            priority: 90,
            description: "ArXiv old-style ID (e.g., hep-th/9901001)",
        },

        // === PubMed ===
        InputPattern {
            id: "pubmed_url",
            connector: "pubmed",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?(?:ncbi\.nlm\.nih\.gov/pubmed/|pubmed\.ncbi\.nlm\.nih\.gov/)(?P<pmid>\d+)").unwrap(),
            captures: &["pmid"],
            arg_mapping: &[("pmid", "pmid")],
            priority: 100,
            description: "PubMed article URL",
        },
        InputPattern {
            id: "pubmed_id",
            connector: "pubmed",
            tool: "get",
            pattern: Regex::new(r"^(?:PMID:|pmid:|PubMed:)?(?P<pmid>\d{7,8})$").unwrap(),
            captures: &["pmid"],
            arg_mapping: &[("pmid", "pmid")],
            priority: 80,
            description: "PubMed ID (7-8 digits, optionally prefixed with PMID:)",
        },

        // === DOI ===
        InputPattern {
            id: "doi_url",
            connector: "semantic-scholar",
            tool: "get_paper_details",
            pattern: Regex::new(r"(?:https?://)?(?:dx\.)?doi\.org/(?P<doi>10\.\d{4,}/[^\s]+)").unwrap(),
            captures: &["doi"],
            arg_mapping: &[("doi", "paper_id")],
            priority: 100,
            description: "DOI URL (doi.org/...)",
        },
        InputPattern {
            id: "doi_bare",
            connector: "semantic-scholar",
            tool: "get_paper_details",
            pattern: Regex::new(r"^(?:doi:|DOI:)?(?P<doi>10\.\d{4,}/[^\s]+)$").unwrap(),
            captures: &["doi"],
            arg_mapping: &[("doi", "paper_id")],
            priority: 90,
            description: "DOI (e.g., 10.1234/example)",
        },

        // === DOI → SciHub (open-access lookup) ===
        InputPattern {
            id: "doi_url_scihub",
            connector: "scihub",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?(?:dx\.)?doi\.org/(?P<doi>10\.\d{4,}/[^\s]+)").unwrap(),
            captures: &["doi"],
            arg_mapping: &[("doi", "doi")],
            priority: 99,
            description: "DOI URL → Open-access PDF lookup (doi.org/...)",
        },
        InputPattern {
            id: "doi_bare_scihub",
            connector: "scihub",
            tool: "get",
            pattern: Regex::new(r"^(?:doi:|DOI:)?(?P<doi>10\.\d{4,}/[^\s]+)$").unwrap(),
            captures: &["doi"],
            arg_mapping: &[("doi", "doi")],
            priority: 89,
            description: "DOI → Open-access PDF lookup (e.g., 10.1234/example)",
        },

        // === Semantic Scholar ===
        InputPattern {
            id: "semantic_scholar_url",
            connector: "semantic-scholar",
            tool: "get_paper_details",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?semanticscholar\.org/paper/[^/]+/(?P<paper_id>[a-f0-9]{40})").unwrap(),
            captures: &["paper_id"],
            arg_mapping: &[("paper_id", "paper_id")],
            priority: 100,
            description: "Semantic Scholar paper URL",
        },

        // === Wikipedia ===
        InputPattern {
            id: "wikipedia_url",
            connector: "wikipedia",
            tool: "get",
            pattern: Regex::new(r"(?:https?://)?(?P<lang>[a-z]{2})\.wikipedia\.org/wiki/(?P<title>[^\s?#]+)").unwrap(),
            captures: &["lang", "title"],
            arg_mapping: &[("title", "title")],
            priority: 100,
            description: "Wikipedia article URL",
        },

        // === GitHub ===
        InputPattern {
            id: "github_repo_url",
            connector: "github",
            tool: "get_repository",
            pattern: Regex::new(r"(?:https?://)?github\.com/(?P<owner>[a-zA-Z0-9_-]+)/(?P<repo>[a-zA-Z0-9_.-]+)/?$").unwrap(),
            captures: &["owner", "repo"],
            arg_mapping: &[("owner", "owner"), ("repo", "repo")],
            priority: 100,
            description: "GitHub repository URL",
        },
        InputPattern {
            id: "github_issue_url",
            connector: "github",
            tool: "get_issue",
            pattern: Regex::new(r"(?:https?://)?github\.com/(?P<owner>[a-zA-Z0-9_-]+)/(?P<repo>[a-zA-Z0-9_.-]+)/issues/(?P<number>\d+)").unwrap(),
            captures: &["owner", "repo", "number"],
            arg_mapping: &[("owner", "owner"), ("repo", "repo"), ("number", "number")],
            priority: 100,
            description: "GitHub issue URL",
        },
        InputPattern {
            id: "github_pr_url",
            connector: "github",
            tool: "get_pull_request",
            pattern: Regex::new(r"(?:https?://)?github\.com/(?P<owner>[a-zA-Z0-9_-]+)/(?P<repo>[a-zA-Z0-9_.-]+)/pull/(?P<number>\d+)").unwrap(),
            captures: &["owner", "repo", "number"],
            arg_mapping: &[("owner", "owner"), ("repo", "repo"), ("number", "number")],
            priority: 100,
            description: "GitHub pull request URL",
        },
        InputPattern {
            id: "github_repo_shorthand",
            connector: "github",
            tool: "get_repository",
            pattern: Regex::new(r"^(?P<owner>[a-zA-Z0-9_-]+)/(?P<repo>[a-zA-Z0-9_.-]+)$").unwrap(),
            captures: &["owner", "repo"],
            arg_mapping: &[("owner", "owner"), ("repo", "repo")],
            priority: 50,
            description: "GitHub repository shorthand (owner/repo)",
        },

        // === Reddit ===
        InputPattern {
            id: "reddit_post_url",
            connector: "reddit",
            tool: "get",
            pattern: Regex::new(r"(?P<post_url>(?:https?://)?(?:www\.)?reddit\.com/r/[a-zA-Z0-9_]+/comments/[a-z0-9]+(?:/[^\s?#]+)?)").unwrap(),
            captures: &["post_url"],
            arg_mapping: &[("post_url", "post_url")],
            priority: 100,
            description: "Reddit post URL",
        },
        InputPattern {
            id: "reddit_user_url",
            connector: "reddit",
            tool: "user",
            pattern: Regex::new(
                r"(?:https?://)?(?:www\.)?reddit\.com/(?:user|u)/(?P<username>[a-zA-Z0-9_-]+)/?$",
            )
            .unwrap(),
            captures: &["username"],
            arg_mapping: &[("username", "username")],
            priority: 100,
            description: "Reddit user URL (profile metadata)",
        },
        InputPattern {
            id: "reddit_subreddit_url",
            connector: "reddit",
            tool: "list",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?reddit\.com/r/(?P<subreddit>[a-zA-Z0-9_]+)/?$").unwrap(),
            captures: &["subreddit"],
            arg_mapping: &[("subreddit", "subreddit")],
            priority: 100,
            description: "Reddit subreddit URL (lists posts)",
        },
        InputPattern {
            id: "reddit_subreddit_shorthand",
            connector: "reddit",
            tool: "list",
            pattern: Regex::new(r"^r/(?P<subreddit>[a-zA-Z0-9_]+)$").unwrap(),
            captures: &["subreddit"],
            arg_mapping: &[("subreddit", "subreddit")],
            priority: 80,
            description: "Reddit subreddit shorthand (lists posts)",
        },

        // === Polymarket ===
        #[cfg(feature = "polymarket")]
        InputPattern {
            id: "polymarket_event_url",
            connector: "polymarket",
            tool: "get",
            pattern: Regex::new(
                r"(?:https?://)?(?:www\.)?polymarket\.com/event/(?P<slug>[a-zA-Z0-9][a-zA-Z0-9_-]*)",
            )
            .unwrap(),
            captures: &["slug"],
            arg_mapping: &[("slug", "slug")],
            priority: 100,
            description: "Polymarket event URL",
        },

        // === Kalshi ===
        #[cfg(feature = "kalshi")]
        InputPattern {
            id: "kalshi_event_url",
            connector: "kalshi",
            tool: "get",
            pattern: Regex::new(
                r"(?P<url>(?:https?://)?(?:www\.)?kalshi\.com/markets(?:/[a-zA-Z0-9_-]+)+/?(?:[?#].*)?)$",
            )
            .unwrap(),
            captures: &["url"],
            arg_mapping: &[("url", "url")],
            priority: 100,
            description: "Kalshi event page URL",
        },

        // === Play Store (Google Play) ===
        #[cfg(feature = "play-store")]
        InputPattern {
            id: "play_store_app_url",
            connector: "play-store",
            tool: "app",
            pattern: Regex::new(
                r"(?:https?://)?play\.google\.com/store/apps/details\?(?:[^#\s]*?&)?id=(?P<id>[a-zA-Z0-9._-]+)(?:[&#][^\s]*)?$",
            )
            .unwrap(),
            captures: &["id"],
            arg_mapping: &[("id", "id")],
            priority: 100,
            description: "Google Play Store app details URL",
        },

        // === X (Twitter) ===
        InputPattern {
            id: "twitter_tweet_url",
            connector: "x",
            tool: "get_tweet",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?(?:twitter\.com|x\.com)/(?P<username>[a-zA-Z0-9_]+)/status/(?P<tweet_id>\d+)").unwrap(),
            captures: &["username", "tweet_id"],
            arg_mapping: &[("tweet_id", "tweet_id")],
            priority: 100,
            description: "X/Twitter tweet URL",
        },
        InputPattern {
            id: "twitter_profile_url",
            connector: "x",
            tool: "get_profile",
            pattern: Regex::new(r"(?:https?://)?(?:www\.)?(?:twitter\.com|x\.com)/(?P<username>[a-zA-Z0-9_]+)/?$").unwrap(),
            captures: &["username"],
            arg_mapping: &[("username", "username")],
            priority: 90,
            description: "X/Twitter profile URL",
        },
        InputPattern {
            id: "twitter_handle",
            connector: "x",
            tool: "get_profile",
            pattern: Regex::new(r"^@(?P<username>[a-zA-Z0-9_]+)$").unwrap(),
            captures: &["username"],
            arg_mapping: &[("username", "username")],
            priority: 80,
            description: "X/Twitter handle (@username)",
        },

        // === bioRxiv / medRxiv ===
        InputPattern {
            id: "biorxiv_url",
            connector: "biorxiv",
            tool: "get",
            pattern: Regex::new(r"https?://(?:www\.)?(?P<server>biorxiv|medrxiv)\.org/content/(?P<doi>10\.1101/[^\s]+)").unwrap(),
            captures: &["server", "doi"],
            arg_mapping: &[("doi", "doi")], // Server will be handled in resolve_all
            priority: 100,
            description: "bioRxiv/medRxiv paper URL (e.g., https://biorxiv.org/content/10.1101/2024.01.01.000000)",
        },
        InputPattern {
            id: "biorxiv_doi",
            connector: "biorxiv",
            tool: "get",
            pattern: Regex::new(r"^(?P<prefix>biorxiv|medrxiv):(?P<doi>10\.1101/[^\s]+)$").unwrap(),
            captures: &["prefix", "doi"],
            arg_mapping: &[("doi", "doi")], // Server will be handled in resolve_all
            priority: 95,
            description: "bioRxiv/medRxiv DOI (e.g., biorxiv:10.1101/2024.01.01.000000)",
        },

        // === RSS ===
        InputPattern {
            id: "rss_feed_url",
            connector: "rss",
            tool: "get_feed",
            pattern: Regex::new(r"^(?P<url>https?://[^\s]+\.(?:rss|xml|atom|json|feed)(?:[^\s]*))$").unwrap(),
            captures: &["url"],
            arg_mapping: &[("url", "url")],
            priority: 90,
            description: "Direct RSS/Atom/JSON feed URL",
        },

        // === Discord ===
        InputPattern {
            id: "discord_channel_url",
            connector: "discord",
            tool: "read_messages",
            pattern: Regex::new(r"https?://(?:www\.)?discord\.com/channels/(?:@me|(?P<guild_id>\d+))/(?P<channel_id>\d+)").unwrap(),
            captures: &["channel_id"],
            arg_mapping: &[("channel_id", "channel_id")],
            priority: 80,
            description: "Discord channel URL (e.g., https://discord.com/channels/12345/67890)",
        },

        // === macOS Spotlight (file search) ===
        #[cfg(target_os = "macos")]
        InputPattern {
            id: "spotlight_file_path",
            connector: "spotlight",
            tool: "get_metadata",
            pattern: Regex::new(r"^file://(?P<path>/[^\s]+)$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 95,
            description: "Local file path URL (file:///path/to/file)",
        },
        #[cfg(target_os = "macos")]
        InputPattern {
            id: "spotlight_absolute_path",
            connector: "spotlight",
            tool: "get_metadata",
            pattern: Regex::new(r"^(?P<path>/(?:Users|Volumes|Applications|System|Library|tmp|var|etc|opt|usr)[^\s]*)$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 90,
            description: "Absolute file path (macOS)",
        },
        #[cfg(target_os = "macos")]
        InputPattern {
            id: "spotlight_home_path",
            connector: "spotlight",
            tool: "get_metadata",
            pattern: Regex::new(r"^(?P<path>~/[^\s]*)$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 90,
            description: "Home-relative file path (~/...)",
        },
        #[cfg(target_os = "macos")]
        InputPattern {
            id: "spotlight_query_prefix",
            connector: "spotlight",
            tool: "search",
            pattern: Regex::new(r"^(?:spotlight:|mdfind:)(?P<query>.+)$").unwrap(),
            captures: &["query"],
            arg_mapping: &[("query", "query")],
            priority: 85,
            description: "Spotlight search query (spotlight:query or mdfind:query)",
        },

        // === Local Filesystem ===
        #[cfg(feature = "localfs")]
        InputPattern {
            id: "localfs_document_file",
            connector: "localfs",
            tool: "extract_text",
            pattern: Regex::new(r"^(?P<path>(?:/|~/)[^\s]*\.(?:pdf|epub|docx?|md|markdown|html?|txt|tex))$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 95,
            description: "Local document file (PDF, EPUB, DOCX, MD, HTML, TXT)",
        },
        #[cfg(feature = "localfs")]
        InputPattern {
            id: "localfs_code_file",
            connector: "localfs",
            tool: "extract_text",
            pattern: Regex::new(r"^(?P<path>(?:/|~/)[^\s]*\.(?:rs|py|js|ts|jsx|tsx|go|java|cpp|c|h|hpp|rb|swift|kt|scala|sh|yaml|yml|json|toml|xml|css|sql|vue|svelte))$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 90,
            description: "Local code file",
        },
        #[cfg(feature = "localfs")]
        InputPattern {
            id: "localfs_absolute_dir",
            connector: "localfs",
            tool: "list_files",
            pattern: Regex::new(r"^(?P<path>/(?:Users|home|Volumes|mnt|opt|var|tmp|etc)[^\s]*/)$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 85,
            description: "Local directory path (ending with /)",
        },
        #[cfg(feature = "localfs")]
        InputPattern {
            id: "localfs_home_dir",
            connector: "localfs",
            tool: "list_files",
            pattern: Regex::new(r"^(?P<path>~(?:/[^\s]*)?)/$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 85,
            description: "Home directory path (~/.../ ending with /)",
        },
        #[cfg(feature = "localfs")]
        InputPattern {
            id: "localfs_file_prefix",
            connector: "localfs",
            tool: "extract_text",
            pattern: Regex::new(r"^(?:file:|localfs:)(?P<path>[^\s]+)$").unwrap(),
            captures: &["path"],
            arg_mapping: &[("path", "path")],
            priority: 95,
            description: "Local file with prefix (file:/path or localfs:/path)",
        },

        // === Generic Web URLs ===
        InputPattern {
            id: "web_url",
            connector: "web",
            tool: "scrape_url",
            pattern: Regex::new(r"^(?P<url>https?://[^\s]+)$").unwrap(),
            captures: &["url"],
            arg_mapping: &[("url", "url")],
            priority: 1, // Lowest priority - catch-all for URLs
            description: "Generic web URL",
        },
    ];

    // Sort by priority (highest first)
    patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.priority));
    patterns
}

/// Get an example input for a pattern
fn get_pattern_example(pattern_id: &str) -> String {
    match pattern_id {
        "youtube_playlist_url" => "https://www.youtube.com/playlist?list=PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n",
        "youtube_playlist_id" => "PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n",
        "youtube_channel_handle_url" => "https://www.youtube.com/@hubermanlab/videos",
        "youtube_channel_id_url" => "https://www.youtube.com/channel/UC2D2CMWXMOVWx7giW1n3LIg/videos",
        "youtube_url_watch" => "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "youtube_url_short" => "https://youtu.be/dQw4w9WgXcQ",
        "youtube_url_embed" => "https://www.youtube.com/embed/dQw4w9WgXcQ",
        "youtube_video_id" => "dQw4w9WgXcQ",
        "hackernews_url" => "https://news.ycombinator.com/item?id=38500000",
        "hackernews_id" => "38500000",
        "arxiv_url" => "https://arxiv.org/abs/2301.07041",
        "arxiv_id" => "arXiv:2301.07041",
        "arxiv_old_id" => "hep-th/9901001",
        "pubmed_url" => "https://pubmed.ncbi.nlm.nih.gov/12345678",
        "pubmed_id" => "PMID:12345678",
        "doi_url" => "https://doi.org/10.1038/nature12373",
        "doi_bare" => "10.1038/nature12373",
        "doi_url_scihub" => "https://doi.org/10.1038/nature12373",
        "doi_bare_scihub" => "10.1038/nature12373",
        "semantic_scholar_url" => {
            "https://www.semanticscholar.org/paper/Attention-Is-All-You-Need/abc123..."
        }
        "wikipedia_url" => "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        "github_repo_url" => "https://github.com/rust-lang/rust",
        "github_issue_url" => "https://github.com/rust-lang/rust/issues/12345",
        "github_pr_url" => "https://github.com/rust-lang/rust/pull/12345",
        "github_repo_shorthand" => "rust-lang/rust",
        "reddit_post_url" => "https://www.reddit.com/r/rust/comments/abc123",
        "reddit_user_url" => "https://www.reddit.com/user/spez/",
        "reddit_subreddit_url" => "https://www.reddit.com/r/rust",
        "reddit_subreddit_shorthand" => "r/rust",
        "polymarket_event_url" => "https://polymarket.com/event/will-bitcoin-hit-150k-in-2026",
        "kalshi_event_url" => {
            "https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99"
        }
        "play_store_app_url" => {
            "https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US"
        }
        "twitter_tweet_url" => "https://x.com/elonmusk/status/1234567890",
        "twitter_profile_url" => "https://x.com/elonmusk",
        "twitter_handle" => "@elonmusk",
        "biorxiv_url" => "https://biorxiv.org/content/10.1101/2024.01.01.000000",
        "biorxiv_doi" => "biorxiv:10.1101/2024.01.01.000000",
        "rss_feed_url" => "https://www.nasa.gov/rss/dyn/breaking_news.rss",
        "discord_channel_url" => {
            "https://discord.com/channels/123456789012345678/987654321098765432"
        }
        "spotlight_file_path" => "file:///Users/me/Documents/report.pdf",
        "spotlight_absolute_path" => "/Users/me/Documents/report.pdf",
        "spotlight_home_path" => "~/Documents/report.pdf",
        "spotlight_query_prefix" => "spotlight:CRISPR research",
        "localfs_document_file" => "/path/to/document.pdf",
        "localfs_code_file" => "/path/to/script.py",
        "localfs_absolute_dir" => "/Users/me/Documents/",
        "localfs_home_dir" => "~/Downloads/",
        "localfs_file_prefix" => "file:/path/to/file.epub",
        "web_url" => "https://example.com/page",
        _ => "",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_youtube_urls() {
        let resolver = SmartResolver::new();

        // Standard watch URL
        let action = resolver
            .resolve("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            .unwrap();
        assert_eq!(action.connector, "youtube");
        assert_eq!(action.tool, "get");
        assert_eq!(action.arguments.get("video_id").unwrap(), "dQw4w9WgXcQ");

        // Short URL
        let action = resolver.resolve("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(action.connector, "youtube");
        assert_eq!(action.arguments.get("video_id").unwrap(), "dQw4w9WgXcQ");

        // Bare video ID
        let action = resolver.resolve("dQw4w9WgXcQ").unwrap();
        assert_eq!(action.connector, "youtube");

        // Playlist URL should enumerate, not scrape the HTML page.
        let action = resolver
            .resolve("https://www.youtube.com/playlist?list=PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n")
            .unwrap();
        assert_eq!(action.connector, "youtube");
        assert_eq!(action.tool, "list");
        assert_eq!(
            action.arguments.get("playlist").unwrap(),
            "PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n"
        );

        // Channel URLs should enumerate uploads.
        let action = resolver
            .resolve("https://www.youtube.com/@hubermanlab/videos")
            .unwrap();
        assert_eq!(action.connector, "youtube");
        assert_eq!(action.tool, "list");
        assert_eq!(action.arguments.get("channel").unwrap(), "@hubermanlab");
    }

    #[test]
    fn test_arxiv() {
        let resolver = SmartResolver::new();

        // ArXiv URL
        let action = resolver
            .resolve("https://arxiv.org/abs/2301.07041")
            .unwrap();
        assert_eq!(action.connector, "arxiv");
        assert_eq!(action.arguments.get("paper_id").unwrap(), "2301.07041");

        // ArXiv ID with prefix
        let action = resolver.resolve("arXiv:2301.07041").unwrap();
        assert_eq!(action.connector, "arxiv");
        assert_eq!(action.arguments.get("paper_id").unwrap(), "2301.07041");
    }

    #[test]
    fn test_pubmed() {
        let resolver = SmartResolver::new();

        // PubMed URL
        let action = resolver
            .resolve("https://pubmed.ncbi.nlm.nih.gov/12345678")
            .unwrap();
        assert_eq!(action.connector, "pubmed");
        assert_eq!(action.arguments.get("pmid").unwrap(), "12345678");

        // PMID prefix
        let action = resolver.resolve("PMID:12345678").unwrap();
        assert_eq!(action.connector, "pubmed");
    }

    #[test]
    fn test_github() {
        let resolver = SmartResolver::new();

        // Repo URL
        let action = resolver
            .resolve("https://github.com/rust-lang/rust")
            .unwrap();
        assert_eq!(action.connector, "github");
        assert_eq!(action.tool, "get_repository");

        // Shorthand
        let action = resolver.resolve("rust-lang/rust").unwrap();
        assert_eq!(action.connector, "github");
    }

    #[test]
    fn test_hackernews() {
        let resolver = SmartResolver::new();

        let action = resolver
            .resolve("https://news.ycombinator.com/item?id=38500000")
            .unwrap();
        assert_eq!(action.connector, "hackernews");
        assert_eq!(action.arguments.get("id").unwrap(), "38500000");
    }

    #[test]
    #[cfg(feature = "polymarket")]
    fn test_polymarket_event_url() {
        let resolver = SmartResolver::new();

        let action = resolver
            .resolve("https://polymarket.com/event/will-bitcoin-hit-150k-in-2026")
            .unwrap();
        assert_eq!(action.connector, "polymarket");
        assert_eq!(action.tool, "get");
        assert_eq!(
            action.arguments.get("slug").unwrap(),
            "will-bitcoin-hit-150k-in-2026"
        );
    }

    #[test]
    #[cfg(feature = "kalshi")]
    fn test_kalshi_event_url() {
        let resolver = SmartResolver::new();

        let action = resolver
            .resolve("https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99")
            .unwrap();
        assert_eq!(action.connector, "kalshi");
        assert_eq!(action.tool, "get");
        assert_eq!(
            action.arguments.get("url").unwrap(),
            "https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99"
        );
    }

    #[test]
    #[cfg(feature = "play-store")]
    fn test_play_store() {
        let resolver = SmartResolver::new();

        let action = resolver
            .resolve("https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US")
            .unwrap();
        assert_eq!(action.connector, "play-store");
        assert_eq!(action.tool, "app");
        assert_eq!(action.arguments.get("id").unwrap(), "com.whatsapp");
    }

    #[test]
    fn test_priority() {
        let resolver = SmartResolver::new();

        // GitHub URL should match github, not generic web
        let action = resolver
            .resolve("https://github.com/rust-lang/rust")
            .unwrap();
        assert_eq!(action.connector, "github");

        // Random URL should fall back to web
        let action = resolver.resolve("https://example.com/page").unwrap();
        assert_eq!(action.connector, "web");
    }

    #[test]
    #[cfg(feature = "all-connectors")]
    fn resolver_patterns_reference_real_tools() {
        use crate::build_registry_enabled_only;
        use tokio::runtime::Runtime;

        let rt = Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let registry = build_registry_enabled_only().await;
            let resolver = SmartResolver::new();

            for pattern in &resolver.patterns {
                let Some(provider) = registry.get_provider(pattern.connector) else {
                    panic!(
                        "Resolver references missing connector: {}",
                        pattern.connector
                    );
                };

                let c = provider.lock().await;
                let tools_response = c.list_tools(None).await.expect("list_tools");
                let exists = tools_response
                    .tools
                    .iter()
                    .any(|t| t.name.as_ref() == pattern.tool);

                assert!(
                    exists,
                    "Resolver pattern '{}' references missing tool '{}' on connector '{}'",
                    pattern.id, pattern.tool, pattern.connector
                );
            }
        });
    }
}
