# Architecture Overview

This document summarizes the repository structure and core runtime architecture.

## Project Overview

**RZN Integrations** is the human-facing name for the `rzn-tools` repository. It is a Rust
integration layer that provides a unified interface for interacting with external systems through an
MCP-compliant connector architecture.

## Development Commands

- `cargo build` - Build the library
- `cargo test` - Run tests
- `cargo run --example <example_name>` - Run specific examples (see examples/ directory)
- `cargo run -p rzn_tools_mcp --bin rzn-tools-mcp` - Run the MCP server
- `cargo doc` - Generate API documentation
- `cargo fmt` - Format code
- `cargo clippy` - Run linter

## Core Architecture

### Central Abstractions

**Connector Trait** (`src/lib.rs`): The foundational interface that all data source connectors must implement. Key methods include:
- `capabilities()` - MCP server capabilities
- `list_tools()` / `call_tool()` - Tool-based operations
- `list_resources()` / `read_resource()` - Resource management
- `set_auth_details()` / `test_auth()` - Authentication handling
- `config_schema()` - Dynamic configuration schema definition

**ProviderRegistry** (`src/lib.rs`): Thread-safe connector management using `Arc<Box<dyn Connector>>` for concurrent access and dynamic connector discovery.

### Authentication & Configuration

The library uses a schema-driven approach where connectors define their requirements via `ConnectorConfigSchema` (`src/capabilities.rs`). Authentication credentials are provided as `HashMap<String, String>` with validation logic in `set_auth_details()`. The embedding application handles secure credential storage.

### Error Handling

Centralized error management via `ConnectorError` enum (`src/error.rs`) with automatic conversion from common error types and MCP-compliant error mapping through `to_jsonrpc_error()`.

## Connector Development Pattern

All connectors follow this structure:

1. **Struct Definition**: Contains HTTP client and any connector-specific state
2. **Connector Trait Implementation**: Implements all required MCP interface methods
3. **Tool Definition**: JSON Schema-based tool definitions with argument validation
4. **Authentication**: Schema-driven auth with multiple method support
5. **Data Serialization**: Returns data as `ToolResponseContent::Text` with JSON serialization

### Key Design Patterns

- **MCP Compliance**: Uses `rmcp` crate for protocol data structures
- **Tool-Centric Interface**: Primary interactions through "tools" representing actions
- **Async-First**: All I/O operations use async/await with tokio
- **JSON Schema Validation**: Tool inputs defined with JSON Schema for client understanding

## Connector Locations

Connectors are organized in `src/connectors/` with each having its own module:
- Social/News: `x/`, `reddit/`, `hackernews/`, `youtube/`
- Academic: `arxiv/`, `pubmed/`, `semantic_scholar/`, `scihub/`
- Web: `web/`
- Reference: `wikipedia/`
- Productivity: `slack/`, `github/`, `atlassian/`, `microsoft/`, `google_drive/`, `google_gmail/`, `google_calendar/`, `google_people/`
- LLM Search: `openai_search/`, `anthropic_search/`, `gemini_search/`, `perplexity_search/`, `xai_search/`, `exa_search/`, `firecrawl_search/`, `serper_search/`, `tavily_search/`, `serpapi_search/`

## Utility Components

**Cookie Management** (`src/utils.rs`): Browser cookie extraction using `rookie` crate with support for Chrome, Firefox, Safari, Brave.

**Derive Macros** (`scrapable_derive/`): `#[derive(Scrapable)]` for automatic HTML parsing with CSS selector-based field extraction.

## Development Guidelines

### Adding New Connectors

1. Create new module in `src/connectors/`
2. Implement `Connector` trait with all required methods
3. Define tools using JSON Schema in `list_tools()`
4. Implement `config_schema()` for authentication/configuration requirements
5. Handle authentication in `set_auth_details()` with proper validation
6. Return all data as JSON strings in `ToolResponseContent::Text`
7. Use `ConnectorError` enum for consistent error handling

### Authentication Implementation

- Define schema using `ConnectorConfigSchema` with appropriate field types
- Use `FieldType::Secret` for sensitive information
- Implement multiple authentication pathways in `set_auth_details()`
- Validate credentials and return appropriate errors
- Implement `test_auth()` for authentication verification

### Code Conventions

- Use `async`/`await` for all I/O operations
- Never use `unwrap()` or `expect()` - always handle errors gracefully
- Validate all inputs to prevent injection attacks
- Use `serde_json` for JSON serialization/deserialization
- Follow Rust coding conventions (`cargo fmt`)
- Map external library errors to `ConnectorError` variants

### Required Dependencies

- **Core**: `async-trait`, `serde`, `tokio`, `reqwest`
- **MCP**: `rmcp` for protocol compliance
- **Error Handling**: `thiserror`
- **Authentication**: `rookie` for cookie management (optional, behind `browser-cookies` feature)
- **Web Scraping**: `scraper`, `htmd` for HTML parsing

## MCP Server

The project includes a fully compliant MCP server that exposes all connectors via the Model Context Protocol:

- **Binary**: `rzn_tools_mcp/src/main.rs` - Main MCP server executable for the `rzn-tools-mcp` binary
- **Transport**: JSON-RPC over stdio for standard MCP compliance
- **Server Implementation**: `rzn_tools_core/src/mcp_server.rs` - Core MCP server logic
- **Transport Layer**: `rzn_tools_core/src/transport.rs` - Stdio transport implementation

### Running the MCP Server

```bash
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp
```

The server aggregates all connectors and exposes them through:
- Tools (prefixed with connector name, e.g., `hackernews/search`)
- Resources (connector-specific URIs)
- Prompts (connector-specific prompt templates)

### Environment Configuration

Set environment variables to enable authenticated connectors:
- `REDDIT_CLIENT_ID` & `REDDIT_CLIENT_SECRET` for Reddit
- `GITHUB_TOKEN` for GitHub
- `SLACK_BOT_TOKEN` for Slack
- `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc. for LLM search connectors

## Testing

- Write unit tests for individual functions
- Write integration tests with mocking where possible
- Test error handling and edge cases thoroughly
- Avoid making real API calls during testing
- Test MCP compliance with `examples/mcp_client_example.rs`

## Examples

The `examples/` directory contains comprehensive usage examples for each connector. Use `cargo run --example <name>` to run them. These demonstrate authentication setup, tool usage, and proper error handling patterns.

Key examples:
- `mcp_client_example.rs` - Demonstrates MCP client interaction
- Individual connector examples for direct usage
