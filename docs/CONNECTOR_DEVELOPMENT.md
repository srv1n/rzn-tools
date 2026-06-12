# Connector Development Guide

This guide walks you through creating a new connector for rzn-tools. By the end, your connector will be fully integrated with the CLI, MCP server, federated search, and smart resolver.

## Table of Contents

1. [Quick Start Checklist](#quick-start-checklist)
2. [Architecture Overview](#architecture-overview)
3. [Step-by-Step Implementation](#step-by-step-implementation)
4. [The Connector Trait](#the-connector-trait)
5. [Tool Design Guidelines](#tool-design-guidelines)
6. [Authentication](#authentication)
7. [Feature Flags and Registration](#feature-flags-and-registration)
8. [Smart Resolver Integration](#smart-resolver-integration)
9. [Federated Search Integration](#federated-search-integration)
10. [Testing](#testing)
11. [Documentation](#documentation)
12. [Complete Example](#complete-example)

---

## Quick Start Checklist

Use this checklist when adding a new connector:

### Phase 1: Core Implementation
- [ ] Create module at `rzn_tools_core/src/connectors/<name>/mod.rs`
- [ ] Implement the `Connector` trait
- [ ] Define tools with JSON schemas
- [ ] Handle authentication (if required)
- [ ] Add feature flag to `rzn_tools_core/Cargo.toml`
- [ ] Add module declaration to `rzn_tools_core/src/connectors/mod.rs`
- [ ] Register in `build_registry_enabled_only()` in `rzn_tools_core/src/lib.rs`

### Phase 2: CLI Integration
- [ ] Forward feature flag in `rzn_tools_cli/Cargo.toml`
- [ ] Forward feature flag in `rzn_tools_mcp/Cargo.toml`
- [ ] Add to "full" and "all-connectors" feature lists

### Phase 3: Smart Features (Optional but Recommended)
- [ ] Add URL/ID patterns to Smart Resolver
- [ ] Add to federated search profile (if has search tool)
- [ ] Support `response_format` parameter for search tools

### Phase 4: Documentation & Testing
- [ ] Add tests
- [ ] Update `README.md` connector table
- [ ] Create connector-specific docs (optional)

---

## Architecture Overview

```
rzn-tools/
├── rzn_tools_core/
│   └── src/
│       ├── connectors/
│       │   ├── mod.rs              # Module declarations
│       │   ├── your_connector/     # Your new connector
│       │   │   └── mod.rs
│       │   └── ...
│       ├── lib.rs                  # Registry registration
│       ├── resolver.rs             # Smart resolver patterns
│       └── federated/
│           └── profiles.rs         # Search profiles
├── rzn_tools_cli/
│   └── Cargo.toml                  # Feature forwarding
└── rzn_tools_mcp/
    └── Cargo.toml                  # Feature forwarding
```

---

## Step-by-Step Implementation

### Step 1: Create the Module

Create a new directory and file:

```bash
mkdir -p rzn_tools_core/src/connectors/myconnector
touch rzn_tools_core/src/connectors/myconnector/mod.rs
```

### Step 2: Basic Connector Structure

```rust
// rzn_tools_core/src/connectors/myconnector/mod.rs

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;
use async_trait::async_trait;
use reqwest::Client;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

/// Arguments for the search tool
#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    response_format: Option<String>,
}

fn default_limit() -> u32 {
    10
}

pub struct MyConnector {
    client: Client,
    api_key: Option<String>,
}

impl MyConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let api_key = auth.get("api_key").map(|s| s.to_string());

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(ConnectorError::HttpRequest)?,
            api_key,
        })
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Value>, ConnectorError> {
        // Your API implementation here
        let url = format!("https://api.example.com/search?q={}&limit={}",
            urlencoding::encode(query), limit);

        let mut request = self.client.get(&url);

        if let Some(ref key) = self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let response = request.send().await.map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "API returned status: {}", response.status()
            )));
        }

        let data: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;

        // Extract and return results
        Ok(data.get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }
}
```

### Step 3: Implement the Connector Trait

```rust
#[async_trait]
impl Connector for MyConnector {
    fn name(&self) -> &'static str {
        "myconnector"  // Used in CLI: rzn-tools myconnector <tool>
    }

    fn description(&self) -> &'static str {
        "A connector for searching and retrieving content from MyService"
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,  // Tools are listed via list_tools
            ..Default::default()
        }
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut details = AuthDetails::new();
        if let Some(ref key) = self.api_key {
            details.set("api_key", key.clone());
        }
        Ok(details)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.api_key = details.get("api_key").map(|s| s.to_string());
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Make a lightweight API call to verify credentials
        // Return Ok(()) if auth is valid or not required
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                // Define required auth fields for the setup wizard
                // See auth section below
            ],
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
                title: Some("My Connector".to_string()),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: Some("https://example.com".to_string()),
            },
            instructions: Some(
                "Search and retrieve content from MyService.".to_string()
            ),
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search".to_string()),
                description: Some(Cow::Borrowed(
                    "Search for content on MyService"
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of results (default: 10)",
                                "default": 10
                            },
                            "response_format": {
                                "type": "string",
                                "enum": ["concise", "detailed"],
                                "description": "Response verbosity (default: concise)",
                                "default": "concise"
                            }
                        },
                        "required": ["query"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_item"),
                title: Some("Get Item".to_string()),
                description: Some(Cow::Borrowed(
                    "Get details of a specific item by ID"
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "The item ID"
                            }
                        },
                        "required": ["id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
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
            "search" => {
                let args: SearchArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let results = self.search(&args.query, args.limit).await?;

                let data = json!({
                    "query": args.query,
                    "count": results.len(),
                    "results": results
                });

                structured_result_with_text(&data, Some(serde_json::to_string_pretty(&data)?))
            }
            "get_item" => {
                // Implement get_item
                todo!()
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    // Resources (optional - return empty if not used)
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

    // Prompts (optional - return empty if not used)
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
            "Prompt '{}' not found", name
        )))
    }
}
```

---

## The Connector Trait

Every connector must implement these methods:

| Method | Purpose |
|--------|---------|
| `name()` | Unique identifier (e.g., `"github"`, `"pubmed"`) |
| `description()` | Human-readable description |
| `capabilities()` | MCP capabilities (usually just default) |
| `initialize()` | Return server info and instructions |
| `list_tools()` | Return available tools with JSON schemas |
| `call_tool()` | Execute a tool and return results |
| `get_auth_details()` | Return current auth configuration |
| `set_auth_details()` | Update auth configuration |
| `test_auth()` | Verify credentials work |
| `config_schema()` | Define auth fields for setup wizard |
| `list_resources()` | MCP resources (optional, return empty) |
| `read_resource()` | MCP resource reading (optional) |
| `list_prompts()` | MCP prompts (optional, return empty) |
| `get_prompt()` | MCP prompt retrieval (optional) |

---

## Tool Design Guidelines

### Naming Conventions

Tools should use snake_case and follow these patterns:

| Pattern | Use Case | Examples |
|---------|----------|----------|
| `search_*` | Keyword search | `search_papers`, `search_videos`, `search_issues` |
| `get_*` | Fetch by ID | `get_paper`, `get_video`, `get_issue` |
| `list_*` | Enumerate items | `list_channels`, `list_repos` |
| `create_*` | Create new items | `create_issue`, `create_message` |

### Search Tool Requirements

For federated search compatibility, search tools MUST:

1. Have "search" or "query" in the name
2. Accept a `query` parameter (string)
3. Accept a `limit` parameter (integer, optional)
4. Return results in a consistent format:

```json
{
  "query": "search terms",
  "count": 10,
  "results": [
    {
      "id": "unique-id",
      "title": "Result Title",
      "url": "https://...",
      "snippet": "Preview text..."
    }
  ]
}
```

### Response Format Support

Support `response_format` parameter for token efficiency:

```rust
#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    limit: Option<u32>,
    #[serde(default = "default_response_format")]
    response_format: String,
}

fn default_response_format() -> String {
    "concise".to_string()
}

// In your tool implementation:
let is_concise = args.response_format != "detailed";

let result = if is_concise {
    json!({
        "id": item.id,
        "title": item.title,
        "url": item.url
    })
} else {
    json!({
        "id": item.id,
        "title": item.title,
        "url": item.url,
        "description": item.description,
        "created_at": item.created_at,
        "author": item.author,
        // ... all fields
    })
};
```

---

## Authentication

### Config Schema

Define authentication requirements for the setup wizard:

```rust
use crate::capabilities::{ConnectorConfigField, ConnectorConfigSchema, FieldType};

fn config_schema(&self) -> ConnectorConfigSchema {
    ConnectorConfigSchema {
        fields: vec![
            ConnectorConfigField {
                key: "api_key".to_string(),
                label: "API Key".to_string(),
                field_type: FieldType::Secret,
                required: true,
                description: Some("Your API key from https://example.com/settings".to_string()),
                default_value: None,
                env_var: Some("MYSERVICE_API_KEY".to_string()),
            },
        ],
    }
}
```

### Field Types

| Type | Use Case |
|------|----------|
| `FieldType::String` | Plain text (URLs, usernames) |
| `FieldType::Secret` | Sensitive data (API keys, tokens) |
| `FieldType::Boolean` | Feature toggles |
| `FieldType::Integer` | Numeric settings |

### Environment Variables

Users can set credentials via environment variables. Define the `env_var` field:

```rust
ConnectorConfigField {
    env_var: Some("GITHUB_TOKEN".to_string()),
    // ...
}
```

### No Auth Required

For connectors that don't need auth:

```rust
fn config_schema(&self) -> ConnectorConfigSchema {
    ConnectorConfigSchema { fields: vec![] }
}

async fn test_auth(&self) -> Result<(), ConnectorError> {
    Ok(())  // Always succeeds
}
```

---

## Feature Flags and Registration

### Step 1: Add Feature Flag

In `rzn_tools_core/Cargo.toml`:

```toml
[features]
# Add to all-connectors list
all-connectors = [
    # ... existing connectors
    "myconnector"
]

# Define the feature
myconnector = ["dep:some-crate"]  # Or just [] if no extra deps
```

### Step 2: Add Module Declaration

In `rzn_tools_core/src/connectors/mod.rs`:

```rust
#[cfg(feature = "myconnector")]
pub mod myconnector;
```

### Step 3: Register in Registry

In `rzn_tools_core/src/lib.rs`, add to `build_registry_enabled_only()`:

```rust
#[cfg(feature = "myconnector")]
{
    if let Ok(connector) =
        connectors::myconnector::MyConnector::new(auth::AuthDetails::new()).await
    {
        registry.register_provider(Box::new(connector));
    }
}
```

### Step 4: Forward Feature Flags

In `rzn_tools_cli/Cargo.toml`:

```toml
[features]
myconnector = ["rzn_tools_core/myconnector"]

full = [
    # ... existing features
    "myconnector"
]
```

In `rzn_tools_mcp/Cargo.toml`:

```toml
[features]
myconnector = ["rzn_tools_core/myconnector"]

full = [
    # ... existing features
    "myconnector"
]
```

---

## Smart Resolver Integration

The Smart Resolver enables `rzn-tools fetch <url-or-id>` to automatically route to your connector.

### Add Patterns

In `rzn_tools_core/src/resolver.rs`, add patterns to `PATTERNS`:

```rust
// Full URL pattern
InputPattern {
    id: "myconnector_url",
    connector: "myconnector",
    tool: "get_item",
    pattern: Regex::new(
        r"^https?://(?:www\.)?myservice\.com/items/(?P<item_id>[a-zA-Z0-9]+)"
    ).unwrap(),
    captures: &["item_id"],
    arg_mapping: &[("item_id", "id")],
    priority: 100,
    description: "MyService item URL",
},

// Shorthand pattern (e.g., ms:abc123)
InputPattern {
    id: "myconnector_shorthand",
    connector: "myconnector",
    tool: "get_item",
    pattern: Regex::new(r"^ms:(?P<item_id>[a-zA-Z0-9]+)$").unwrap(),
    captures: &["item_id"],
    arg_mapping: &[("item_id", "id")],
    priority: 90,
    description: "MyService item shorthand (ms:ID)",
},
```

### Add Example

In `get_pattern_example()`:

```rust
"myconnector_url" => "https://myservice.com/items/abc123",
"myconnector_shorthand" => "ms:abc123",
```

### Priority Guidelines

| Priority | Pattern Type |
|----------|--------------|
| 100 | Full URLs with domain |
| 90 | IDs with prefix (ms:xxx) |
| 80 | Shorthand notations (r/xxx) |
| 50 | Bare IDs (ambiguous) |
| 1 | Generic fallback |

---

## Federated Search Integration

If your connector has a search tool, add it to a federated search profile.

### Add to Existing Profile

In `rzn_tools_core/src/federated/profiles.rs`:

```rust
SearchProfile {
    name: "research".to_string(),
    connectors: vec![
        "pubmed".to_string(),
        "arxiv".to_string(),
        "myconnector".to_string(),  // Add here
    ],
    // ...
},
```

### Create New Profile

```rust
SearchProfile {
    name: "myprofile".to_string(),
    description: Some("Search across my services".to_string()),
    extends: None,
    connectors: vec![
        "myconnector".to_string(),
        "other-connector".to_string(),
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
```

### Update CLI Help

In `rzn_tools_cli/src/cli.rs`, update the built-in profiles help text:

```rust
\x1b[1;33mBuilt-in Profiles:\x1b[0m
  research    - pubmed, arxiv, semantic-scholar, google-scholar, myconnector
```

### Search Tool Compatibility

For federated search to work, your search tool must:

1. Have "search" or "query" in the name
2. Accept `query` and `limit` parameters
3. Return results with `id`, `title`, and optionally `url`, `snippet`

The federated search engine looks for these fields to normalize results:

```rust
// In rzn_tools_core/src/federated/engine.rs
// Results array detection
for field in &["results", "articles", "papers", "items", "stories", "posts", "videos"] {
    // ...
}

// ID extraction (add your connector if needed)
match source {
    "myconnector" => item.get("item_id").and_then(|v| v.as_str()).map(|s| format!("ms:{}", s)),
    // ...
}
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connector_name() {
        let connector = MyConnector::new(AuthDetails::new()).await.unwrap();
        assert_eq!(connector.name(), "myconnector");
    }

    #[tokio::test]
    async fn test_list_tools() {
        let connector = MyConnector::new(AuthDetails::new()).await.unwrap();
        let result = connector.list_tools(None).await.unwrap();

        assert!(!result.tools.is_empty());
        assert!(result.tools.iter().any(|t| t.name == "search"));
    }

    #[tokio::test]
    async fn test_search_requires_query() {
        let connector = MyConnector::new(AuthDetails::new()).await.unwrap();

        let request = CallToolRequestParam {
            name: "search".into(),
            arguments: Some(serde_json::Map::new()),  // Missing query
        };

        let result = connector.call_tool(request).await;
        assert!(matches!(result, Err(ConnectorError::InvalidParams(_))));
    }
}
```

### Integration Tests

Create `rzn_tools_core/examples/myconnector.rs`:

```rust
use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::myconnector::MyConnector;
use rzn_tools_core::{CallToolRequestParam, Connector};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = MyConnector::new(AuthDetails::new()).await?;

    let request = CallToolRequestParam {
        name: "search".into(),
        arguments: Some(json!({"query": "test", "limit": 5}).as_object().unwrap().clone()),
    };

    let result = connector.call_tool(request).await?;
    println!("{:#?}", result);

    Ok(())
}
```

Add to `Cargo.toml`:

```toml
[[example]]
name = "myconnector"
required-features = ["examples", "myconnector"]
```

Run with:

```bash
cargo run --example myconnector --features "examples,myconnector"
```

---

## Documentation

### Update README.md

Add your connector to the appropriate table:

```markdown
### No Authentication Required

| Connector | Description |
|-----------|-------------|
| <img src="https://www.google.com/s2/favicons?domain=myservice.com&sz=16" width="16" height="16" /> MyService | Search and retrieve content |
```

### Create Connector Docs (Optional)

Create `docs/connectors/myconnector.md`:

```markdown
# MyConnector — Design Spec

Status: Implemented

## Overview

Search and retrieve content from MyService.

## Tools

- `search`: Search for content by keyword
  - Inputs: `query` (string), `limit` (int, default 10)
  - Output: List of results with id, title, url, snippet

- `get_item`: Get item details by ID
  - Inputs: `id` (string)
  - Output: Full item details

## Authentication

API key required. Get one at https://myservice.com/settings

Environment variable: `MYSERVICE_API_KEY`

## Rate Limits

- 100 requests per minute
- 1000 requests per day

## Examples

\`\`\`bash
# Using connector subcommands (recommended)
rzn-tools myconnector search --query "rust programming"
rzn-tools myconnector get --id abc123

# Using generic commands
rzn-tools search myconnector "rust programming"
rzn-tools fetch ms:abc123
\`\`\`
```

---

## Complete Example

Here's a minimal but complete connector:

```rust
// rzn_tools_core/src/connectors/example/mod.rs

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;
use async_trait::async_trait;
use rmcp::model::*;
use serde::Deserialize;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 { 10 }

pub struct ExampleConnector;

impl ExampleConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self)
    }
}

#[async_trait]
impl Connector for ExampleConnector {
    fn name(&self) -> &'static str { "example" }
    fn description(&self) -> &'static str { "Example connector for documentation" }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities::default()
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> { Ok(()) }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
    }

    async fn initialize(&self, _: InitializeRequestParam) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: Some("Example".to_string()),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: None,
        })
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParam>) -> Result<ListToolsResult, ConnectorError> {
        Ok(ListToolsResult {
            tools: vec![Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search".to_string()),
                description: Some(Cow::Borrowed("Search for items")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "limit": { "type": "integer", "description": "Max results", "default": 10 }
                    },
                    "required": ["query"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            }],
            next_cursor: None,
        })
    }

    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult, ConnectorError> {
        match request.name.as_ref() {
            "search" => {
                let args: SearchArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())?
                ).map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let data = json!({
                    "query": args.query,
                    "count": 0,
                    "results": []
                });

                structured_result_with_text(&data, Some(serde_json::to_string_pretty(&data)?))
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_resources(&self, _: Option<PaginatedRequestParam>) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult { resources: vec![], next_cursor: None })
    }

    async fn read_resource(&self, _: ReadResourceRequestParam) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_prompts(&self, _: Option<PaginatedRequestParam>) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult { prompts: vec![], next_cursor: None })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!("Prompt '{}' not found", name)))
    }
}
```

---

## Getting Help

- Check existing connectors in `rzn_tools_core/src/connectors/` for reference
- The `google_scholar` connector is a good simple example
- The `hackernews` connector shows complex tool implementations
- The `github` connector demonstrates authentication patterns

Questions? Open an issue at https://github.com/srv1n/rzn-tools/issues
