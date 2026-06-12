# RZN Integrations Launcher Integration Specification

> **Purpose**: Detailed implementation spec for enhancements that enable dynamic launcher integration
> **Priority**: HIGH - Required for RZN Desktop launcher integration
> **Estimated Effort**: 2-3 days
> **Date**: December 2025

---

## Overview

The RZN Desktop launcher needs to **dynamically discover** all available connectors and tools from
**RZN Integrations** (`rzn-tools`) without hardcoding. While the current `tools/list` endpoint
provides most required data, several enhancements will significantly improve the launcher UX.

---

## Current State vs Required State

| Feature | Current State | Required State | Priority |
|---------|---------------|----------------|----------|
| Tool names | ✅ Available via `tools/list` | ✅ No change needed | - |
| Tool descriptions | ✅ Available via `tools/list` | ✅ No change needed | - |
| Tool input schemas | ✅ Available via `tools/list` | ✅ No change needed | - |
| Connector metadata | ⚠️ Only `name()` and `description()` | Need icon, display_name, url_patterns | 🔴 HIGH |
| Connectors list endpoint | ❌ Not available | Need `connectors/list` MCP method | 🔴 HIGH |
| Tool examples | ❌ Not in schema | Add `examples` to input_schema | 🟡 MEDIUM |
| Tool categories | ❌ Not available | Add via annotations | 🟢 LOW |
| URL patterns | ❌ Not exposed | Expose via connector metadata | 🟡 MEDIUM |

---

## Implementation Tasks

### Task 1: Extend Connector Trait with Metadata Methods

**File**: `rzn_tools_core/src/lib.rs`

**Current Connector trait** (lines 49-106):
```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    // ... other methods
}
```

**Required additions**:
```rust
#[async_trait]
pub trait Connector: Send + Sync {
    // EXISTING - Keep as-is
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn credential_provider(&self) -> &'static str { self.name() }

    // NEW - Add these methods with defaults

    /// Human-readable display name for UI (e.g., "Hacker News" instead of "hackernews")
    fn display_name(&self) -> &'static str {
        self.name()  // Default: use technical name
    }

    /// Emoji or icon identifier for the connector
    /// Can be an emoji (e.g., "📰") or an icon name (e.g., "hackernews")
    fn icon(&self) -> &'static str {
        "🔧"  // Default: generic tool icon
    }

    /// URL patterns this connector can handle
    /// Used by launchers to auto-detect URLs and suggest appropriate tools
    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![]  // Default: no URL handling
    }

    /// Categories this connector belongs to
    /// e.g., ["social", "news"], ["academic", "research"]
    fn categories(&self) -> Vec<&'static str> {
        vec![]  // Default: uncategorized
    }

    /// Whether this connector requires authentication to function
    fn requires_auth(&self) -> bool {
        false  // Default: no auth required
    }

    // ... rest of existing methods unchanged
}
```

**New struct to add** (in same file or new `src/url_patterns.rs`):
```rust
/// Specification for URL patterns a connector can handle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct URLPatternSpec {
    /// Regex pattern to match URLs (e.g., r"youtube\.com/watch\?v=([a-zA-Z0-9_-]+)")
    pub pattern: String,

    /// The tool to invoke when this URL is matched (e.g., "get_transcript")
    pub default_tool: String,

    /// Description of what happens when this URL is pasted
    pub description: String,

    /// How to extract parameters from the URL
    /// Maps capture group index to parameter name
    /// e.g., [(1, "video_id")] means capture group 1 becomes "video_id" param
    pub param_extraction: Vec<URLParamExtraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct URLParamExtraction {
    /// Capture group index (1-based) from the regex
    pub capture_group: usize,

    /// Parameter name to pass to the tool
    pub param_name: String,

    /// Whether to use the full URL instead of extracted param
    /// If true, ignore capture_group and pass full URL as this param
    pub use_full_url: bool,
}
```

---

### Task 2: Implement Connector Metadata for Each Connector

**Files**: Each connector's `mod.rs` file in `rzn_tools_core/src/connectors/*/`

**Example: YouTube Connector**

File: `rzn_tools_core/src/connectors/youtube/mod.rs`

```rust
impl Connector for YoutubeConnector {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn description(&self) -> &'static str {
        "Fetch YouTube video transcripts, search videos, and get channel information"
    }

    // NEW METHODS:

    fn display_name(&self) -> &'static str {
        "YouTube"
    }

    fn icon(&self) -> &'static str {
        "🎥"  // Or "youtube" for icon lookup
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["video", "social", "media"]
    }

    fn requires_auth(&self) -> bool {
        false  // YouTube transcripts work without auth
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![
            URLPatternSpec {
                pattern: r"(?:youtube\.com/watch\?v=|youtu\.be/)([a-zA-Z0-9_-]+)".to_string(),
                default_tool: "get_transcript".to_string(),
                description: "Get video transcript".to_string(),
                param_extraction: vec![
                    URLParamExtraction {
                        capture_group: 0,  // Full match
                        param_name: "url".to_string(),
                        use_full_url: true,
                    }
                ],
            },
            URLPatternSpec {
                pattern: r"youtube\.com/@([a-zA-Z0-9_-]+)".to_string(),
                default_tool: "get_channel".to_string(),
                description: "Get channel information".to_string(),
                param_extraction: vec![
                    URLParamExtraction {
                        capture_group: 1,
                        param_name: "channel_handle".to_string(),
                        use_full_url: false,
                    }
                ],
            },
        ]
    }

    // ... rest of existing implementation
}
```

**Connector Metadata Reference Table**:

| Connector | display_name | icon | categories | url_pattern |
|-----------|--------------|------|------------|-------------|
| `hackernews` | "Hacker News" | "📰" | ["news", "tech", "social"] | `news.ycombinator.com/item?id=(\d+)` |
| `arxiv` | "arXiv Papers" | "📚" | ["academic", "research", "science"] | `arxiv.org/(abs\|pdf)/(\d+\.\d+)` |
| `youtube` | "YouTube" | "🎥" | ["video", "media", "social"] | `youtube.com/watch?v=*`, `youtu.be/*` |
| `reddit` | "Reddit" | "🤖" | ["social", "forum", "community"] | `reddit.com/r/*/comments/*` |
| `wikipedia` | "Wikipedia" | "📖" | ["reference", "encyclopedia"] | `*.wikipedia.org/wiki/*` |
| `github` | "GitHub" | "🐙" | ["code", "developer", "social"] | `github.com/*/*` |
| `pubmed` | "PubMed" | "🔬" | ["academic", "medical", "research"] | `pubmed.ncbi.nlm.nih.gov/*` |
| `semantic_scholar` | "Semantic Scholar" | "🎓" | ["academic", "research"] | `semanticscholar.org/paper/*` |
| `slack` | "Slack" | "💬" | ["productivity", "communication"] | None |
| `google_gmail` | "Gmail" | "📧" | ["productivity", "email"] | None |
| `google_drive` | "Google Drive" | "📁" | ["productivity", "storage"] | `drive.google.com/*` |
| `notion` | "Notion" | "📝" | ["productivity", "notes"] | `notion.so/*` |
| `x` (Twitter API) | "X (Twitter)" | "🐦" | ["social", "news", "api"] | `twitter.com/*/status/*`, `x.com/*/status/*` |
| `x-browser` (Twitter Browser Cookies) | "X (Browser Cookies)" | "🐦" | ["social", "news"] | `twitter.com/*/status/*`, `x.com/*/status/*` |
| `exa_search` | "Exa Search" | "🔍" | ["search", "web"] | None |
| `perplexity_search` | "Perplexity" | "🧠" | ["search", "ai"] | None |
| `gemini_search` | "Gemini Search" | "✨" | ["search", "ai"] | None |

---

### Task 3: Add `connectors/list` MCP Endpoint

**File**: `rzn_tools_core/src/mcp_server.rs`

**Step 1**: Add new response struct (near other result structs):

```rust
/// Response for connectors/list endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct ListConnectorsResult {
    pub connectors: Vec<ConnectorMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorMetadata {
    /// Technical name (e.g., "hackernews")
    pub name: String,

    /// Human-readable name (e.g., "Hacker News")
    pub display_name: String,

    /// Connector description
    pub description: String,

    /// Icon (emoji or identifier)
    pub icon: String,

    /// Number of tools this connector provides
    pub tools_count: usize,

    /// Tool names for quick reference
    pub tools: Vec<String>,

    /// Categories this connector belongs to
    pub categories: Vec<String>,

    /// URL patterns this connector handles (for launcher auto-detection)
    pub url_patterns: Vec<URLPatternSpec>,

    /// Whether authentication is required
    pub auth_required: bool,

    /// Current authentication status
    pub auth_status: AuthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    /// Authentication not required for this connector
    NotRequired,
    /// Authentication required but not configured
    NeedsSetup,
    /// Authentication configured and valid
    Ready,
    /// Authentication configured but invalid/expired
    Invalid,
    /// Unable to determine auth status
    Unknown,
}
```

**Step 2**: Add handler method to `McpServer` impl:

```rust
impl McpServer {
    // ... existing methods ...

    /// Handle connectors/list request
    pub async fn handle_list_connectors(&self) -> Result<ListConnectorsResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut connectors = Vec::new();

        for (name, connector) in registry.providers.iter() {
            let c = connector.lock().await;

            // Get tools for this connector
            let tools_result = c.list_tools(None).await.unwrap_or(ListToolsResult {
                tools: vec![],
                next_cursor: None,
            });

            // Determine auth status
            let auth_status = if !c.requires_auth() {
                AuthStatus::NotRequired
            } else {
                match c.test_auth().await {
                    Ok(()) => AuthStatus::Ready,
                    Err(ConnectorError::AuthRequired(_)) => AuthStatus::NeedsSetup,
                    Err(ConnectorError::AuthFailed(_)) => AuthStatus::Invalid,
                    Err(_) => AuthStatus::Unknown,
                }
            };

            connectors.push(ConnectorMetadata {
                name: name.clone(),
                display_name: c.display_name().to_string(),
                description: c.description().to_string(),
                icon: c.icon().to_string(),
                tools_count: tools_result.tools.len(),
                tools: tools_result.tools.iter().map(|t| t.name.to_string()).collect(),
                categories: c.categories().iter().map(|s| s.to_string()).collect(),
                url_patterns: c.url_patterns(),
                auth_required: c.requires_auth(),
                auth_status,
            });
        }

        // Sort by display name for consistent ordering
        connectors.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        Ok(ListConnectorsResult { connectors })
    }
}
```

**Step 3**: Add JSON-RPC routing in `handle_request`:

In the `handle_request` method (around line 716), add the new case:

```rust
// In the match statement for method names:
"connectors/list" => {
    self.server
        .handle_list_connectors()
        .await
        .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
        .map_err(|e| e.to_jsonrpc_error())
}
```

---

### Task 4: Add Tool Examples to Input Schemas

**Files**: Each connector's tool definitions in `list_tools()` method

**Pattern**: Add `examples` array to the input_schema JSON object

**Example: arXiv Connector**

File: `rzn_tools_core/src/connectors/arxiv/mod.rs`

**Before**:
```rust
Tool {
    name: Cow::Borrowed("search"),
    description: Some(Cow::Borrowed("Search arXiv papers...")),
    input_schema: Arc::new(json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query. Supports fielded queries like ti:transformer"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum results (default: 10)",
                "default": 10
            }
        },
        "required": ["query"]
    }).as_object().unwrap().clone()),
    ...
}
```

**After**:
```rust
Tool {
    name: Cow::Borrowed("search"),
    description: Some(Cow::Borrowed("Search arXiv papers...")),
    input_schema: Arc::new(json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query. Supports fielded queries like ti:transformer"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum results (default: 10)",
                "default": 10
            }
        },
        "required": ["query"],
        // NEW: Add examples
        "examples": [
            {
                "description": "Simple keyword search",
                "input": { "query": "transformer attention mechanism" }
            },
            {
                "description": "Search by title and author",
                "input": { "query": "ti:BERT AND au:Devlin", "limit": 5 }
            },
            {
                "description": "Recent papers in category",
                "input": { "query": "cat:cs.AI", "limit": 20 }
            }
        ]
    }).as_object().unwrap().clone()),
    ...
}
```

**Example: YouTube Connector**

```rust
Tool {
    name: Cow::Borrowed("get_transcript"),
    input_schema: Arc::new(json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "YouTube video URL"
            }
        },
        "required": ["url"],
        "examples": [
            {
                "description": "Get transcript from standard URL",
                "input": { "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ" }
            },
            {
                "description": "Get transcript from short URL",
                "input": { "url": "https://youtu.be/dQw4w9WgXcQ" }
            }
        ]
    }).as_object().unwrap().clone()),
    ...
}
```

**Example: Hacker News Connector**

```rust
Tool {
    name: Cow::Borrowed("search"),
    input_schema: Arc::new(json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Search query" },
            "limit": { "type": "integer", "default": 10 },
            "sort": {
                "type": "string",
                "enum": ["relevance", "date"],
                "default": "relevance"
            }
        },
        "required": ["query"],
        "examples": [
            {
                "description": "Search for AI discussions",
                "input": { "query": "transformer architecture" }
            },
            {
                "description": "Recent posts about Rust",
                "input": { "query": "rust programming", "sort": "date", "limit": 20 }
            }
        ]
    }).as_object().unwrap().clone()),
    ...
}
```

---

### Task 5: Add Tool Categories via Annotations

**Files**: Each connector's tool definitions

**Pattern**: Use the MCP `annotations` field with custom extensions

```rust
use rmcp::model::{Annotated, Annotations};

Tool {
    name: Cow::Borrowed("search"),
    description: Some(Cow::Borrowed("Search papers...")),
    input_schema: /* ... */,
    annotations: Some(Annotations {
        audience: None,
        priority: None,
        // Use custom extension for categories
        // Note: Check rmcp crate for exact Annotations structure
    }),
    // Alternative: Add to input_schema as custom field
    // input_schema with "_meta" field:
    // "_meta": {
    //     "category": "search",
    //     "tags": ["discovery", "research"],
    //     "auth_required": false,
    //     "rate_limit_per_min": 60
    // }
}
```

**If annotations don't support custom fields**, add `_meta` to input_schema:

```rust
input_schema: Arc::new(json!({
    "type": "object",
    "properties": { /* ... */ },
    "required": ["query"],
    "examples": [ /* ... */ ],
    // Custom metadata (prefixed with _ to indicate non-standard)
    "_meta": {
        "category": "search",
        "tags": ["discovery", "research", "academic"],
        "auth_required": false,
        "estimated_latency_ms": 500,
        "rate_limit": {
            "requests_per_minute": 60
        }
    }
}).as_object().unwrap().clone()),
```

---

## Testing Requirements

### Unit Tests

Add tests to verify the new functionality:

```rust
// rzn_tools_core/src/tests/connector_metadata_tests.rs

#[tokio::test]
async fn test_connector_display_name() {
    let connector = YoutubeConnector::new(None).await.unwrap();
    assert_eq!(connector.display_name(), "YouTube");
    assert_ne!(connector.display_name(), connector.name()); // Should be different
}

#[tokio::test]
async fn test_connector_url_patterns() {
    let connector = YoutubeConnector::new(None).await.unwrap();
    let patterns = connector.url_patterns();

    assert!(!patterns.is_empty());

    // Test that pattern compiles
    for pattern in &patterns {
        let regex = regex::Regex::new(&pattern.pattern);
        assert!(regex.is_ok(), "Invalid regex pattern: {}", pattern.pattern);
    }

    // Test URL matching
    let youtube_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let pattern = &patterns[0];
    let regex = regex::Regex::new(&pattern.pattern).unwrap();
    assert!(regex.is_match(youtube_url));
}

#[tokio::test]
async fn test_connectors_list_endpoint() {
    let registry = build_registry_enabled_only().await;
    let server = McpServer::new(Arc::new(Mutex::new(registry)));

    let result = server.handle_list_connectors().await;
    assert!(result.is_ok());

    let connectors = result.unwrap().connectors;
    assert!(!connectors.is_empty());

    // Verify structure
    for connector in &connectors {
        assert!(!connector.name.is_empty());
        assert!(!connector.display_name.is_empty());
        assert!(!connector.icon.is_empty());
    }
}

#[tokio::test]
async fn test_tool_examples_in_schema() {
    let connector = ArxivConnector::new(None).await.unwrap();
    let tools = connector.list_tools(None).await.unwrap();

    let search_tool = tools.tools.iter().find(|t| t.name == "search");
    assert!(search_tool.is_some());

    let schema = &search_tool.unwrap().input_schema;
    assert!(schema.contains_key("examples"), "Tool should have examples");

    let examples = schema.get("examples").unwrap().as_array().unwrap();
    assert!(!examples.is_empty(), "Examples should not be empty");
}
```

### Integration Tests

```rust
// rzn_tools_core/tests/mcp_integration_tests.rs

#[tokio::test]
async fn test_connectors_list_via_mcp() {
    let registry = build_registry_enabled_only().await;
    let server = McpServer::new(Arc::new(Mutex::new(registry)));
    let handler = JsonRpcHandler::new(server);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "connectors/list",
        "params": {}
    });

    let response = handler.handle_request(request).await;

    assert!(response.get("error").is_none());
    assert!(response.get("result").is_some());

    let result = response.get("result").unwrap();
    let connectors = result.get("connectors").unwrap().as_array().unwrap();

    assert!(!connectors.is_empty());

    // Verify each connector has required fields
    for connector in connectors {
        assert!(connector.get("name").is_some());
        assert!(connector.get("display_name").is_some());
        assert!(connector.get("icon").is_some());
        assert!(connector.get("tools_count").is_some());
    }
}
```

---

## Migration Guide

### For Existing Connectors

Each existing connector needs to be updated with the new trait methods. Here's the checklist:

#### Connector Update Checklist

- [ ] **hackernews** - Add display_name, icon, categories, url_patterns
- [ ] **arxiv** - Add display_name, icon, categories, url_patterns
- [ ] **youtube** - Add display_name, icon, categories, url_patterns
- [ ] **reddit** - Add display_name, icon, categories, url_patterns
- [ ] **wikipedia** - Add display_name, icon, categories, url_patterns
- [ ] **github** - Add display_name, icon, categories, url_patterns
- [ ] **pubmed** - Add display_name, icon, categories, url_patterns
- [ ] **semantic_scholar** - Add display_name, icon, categories
- [ ] **x** (Twitter) - Add display_name, icon, categories, url_patterns
- [ ] **slack** - Add display_name, icon, categories, requires_auth
- [ ] **google_gmail** - Add display_name, icon, categories, requires_auth
- [ ] **google_drive** - Add display_name, icon, categories, requires_auth, url_patterns
- [ ] **google_calendar** - Add display_name, icon, categories, requires_auth
- [ ] **notion** - Add display_name, icon, categories, requires_auth, url_patterns
- [ ] **exa_search** - Add display_name, icon, categories, requires_auth
- [ ] **perplexity_search** - Add display_name, icon, categories, requires_auth
- [ ] **gemini_search** - Add display_name, icon, categories, requires_auth
- [ ] **openai_search** - Add display_name, icon, categories, requires_auth
- [ ] **anthropic_search** - Add display_name, icon, categories, requires_auth
- [ ] All other connectors...

#### Tool Update Checklist (Add Examples)

- [ ] **hackernews/search** - Add 2-3 examples
- [ ] **hackernews/get_top** - Add 1 example
- [ ] **arxiv/search** - Add 2-3 examples with fielded queries
- [ ] **arxiv/get** - Add 1 example with paper ID
- [ ] **youtube/get_transcript** - Add 2 examples (standard URL, short URL)
- [ ] **youtube/search** - Add 2 examples
- [ ] **reddit/search** - Add 2 examples
- [ ] **reddit/get_thread** - Add 1 example
- [ ] All other tools...

---

## Expected Output Format

### `connectors/list` Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "connectors": [
      {
        "name": "arxiv",
        "display_name": "arXiv Papers",
        "description": "Search and retrieve academic papers from arXiv",
        "icon": "📚",
        "tools_count": 3,
        "tools": ["search", "get", "recent"],
        "categories": ["academic", "research", "science"],
        "url_patterns": [
          {
            "pattern": "arxiv\\.org/(abs|pdf)/(\\d+\\.\\d+)",
            "default_tool": "get",
            "description": "Get paper by arXiv ID",
            "param_extraction": [
              { "capture_group": 2, "param_name": "paper_id", "use_full_url": false }
            ]
          }
        ],
        "auth_required": false,
        "auth_status": "not_required"
      },
      {
        "name": "youtube",
        "display_name": "YouTube",
        "description": "Fetch video transcripts, search videos, and get channel information",
        "icon": "🎥",
        "tools_count": 4,
        "tools": ["get_transcript", "search", "get_video", "get_channel"],
        "categories": ["video", "media", "social"],
        "url_patterns": [
          {
            "pattern": "(?:youtube\\.com/watch\\?v=|youtu\\.be/)([a-zA-Z0-9_-]+)",
            "default_tool": "get_transcript",
            "description": "Get video transcript",
            "param_extraction": [
              { "capture_group": 0, "param_name": "url", "use_full_url": true }
            ]
          }
        ],
        "auth_required": false,
        "auth_status": "not_required"
      },
      {
        "name": "slack",
        "display_name": "Slack",
        "description": "Search messages, channels, and users in Slack workspaces",
        "icon": "💬",
        "tools_count": 5,
        "tools": ["search", "list_channels", "get_channel", "list_users", "post_message"],
        "categories": ["productivity", "communication"],
        "url_patterns": [],
        "auth_required": true,
        "auth_status": "needs_setup"
      }
    ]
  }
}
```

### `tools/list` Response Example (with examples)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "arxiv/search",
        "description": "Search arXiv papers. Supports fielded queries like ti:transformer AND au:hinton",
        "inputSchema": {
          "type": "object",
          "properties": {
            "query": {
              "type": "string",
              "description": "Search query"
            },
            "limit": {
              "type": "integer",
              "description": "Maximum results",
              "default": 10
            }
          },
          "required": ["query"],
          "examples": [
            {
              "description": "Simple keyword search",
              "input": { "query": "transformer attention mechanism" }
            },
            {
              "description": "Search by title and author",
              "input": { "query": "ti:BERT AND au:Devlin", "limit": 5 }
            }
          ]
        }
      }
    ]
  }
}
```

---

## File Changes Summary

| File | Changes |
|------|---------|
| `rzn_tools_core/src/lib.rs` | Add new trait methods, URLPatternSpec struct |
| `rzn_tools_core/src/mcp_server.rs` | Add `connectors/list` handler, ConnectorMetadata struct |
| `rzn_tools_core/src/connectors/youtube/mod.rs` | Implement new trait methods |
| `rzn_tools_core/src/connectors/hackernews/mod.rs` | Implement new trait methods |
| `rzn_tools_core/src/connectors/arxiv/mod.rs` | Implement new trait methods |
| `rzn_tools_core/src/connectors/reddit/mod.rs` | Implement new trait methods |
| `rzn_tools_core/src/connectors/*/mod.rs` | Implement new trait methods for all connectors |
| `rzn_tools_core/src/tests/` | Add unit tests for new functionality |
| `rzn_tools_core/tests/` | Add integration tests |

---

## Priority Order

**Recommended implementation order:**

1. **Day 1**: Add trait methods with defaults (doesn't break existing code)
2. **Day 1**: Add `connectors/list` endpoint
3. **Day 2**: Update 5 core connectors (youtube, hackernews, arxiv, reddit, wikipedia)
4. **Day 2**: Add examples to those 5 connectors' tools
5. **Day 3**: Update remaining connectors
6. **Day 3**: Add tests

---

## Questions for Downstream Developer

If anything is unclear:

1. Should URL patterns use full regex or glob-style patterns?
2. Should `auth_status` be computed lazily or cached?
3. Should `connectors/list` support pagination?
4. Should we add a `connectors/get` endpoint for single connector details?

---

**End of Specification**

Contact: Create issues in the rzn-tools repo for questions or clarifications.
