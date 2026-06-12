# RZN DataSourcer CLI - Technical Documentation

## Overview

The RZN DataSourcer CLI is a modern Rust-based command-line interface that provides unified access to 20+ data sources through a beautiful, performant terminal application. Built following senior architect recommendations for CLI design patterns.

## Architecture

### Workspace Structure

```
rzn-tools/
├── rzn_tools_core/     # Core library crate
│   ├── src/
│   │   ├── lib.rs           # Connector trait & ProviderRegistry
│   │   ├── connectors/      # 20+ data source connectors
│   │   ├── error.rs         # Centralized error handling
│   │   ├── auth.rs          # Authentication management
│   │   ├── mcp_server.rs    # MCP protocol implementation
│   │   └── utils.rs         # Cookie extraction & utilities
│   └── Cargo.toml
├── rzn_tools_cli/      # CLI binary crate
│   ├── src/
│   │   ├── main.rs          # Entry point & arg parsing
│   │   ├── cli.rs           # Clap command definitions
│   │   ├── commands/        # Command implementations
│   │   │   ├── list.rs      # Connector listing
│   │   │   ├── search.rs    # Search functionality
│   │   │   ├── get.rs       # Resource fetching
│   │   │   ├── config.rs    # Authentication config
│   │   │   ├── connectors.rs # Detailed connector info
│   │   │   └── tools.rs     # Tool documentation
│   │   ├── output/          # Output formatting
│   │   └── tui/             # TUI mode (placeholder)
│   └── Cargo.toml
├── rzn_tools_mcp/      # MCP server binary
│   └── src/main.rs
└── Cargo.toml               # Workspace configuration
```

### Design Principles

**Separation of Concerns**
- Core library (`rzn_tools_core`) handles all data source logic
- CLI (`rzn_tools_cli`) focuses purely on user interface
- MCP server (`rzn_tools_mcp`) provides protocol-compliant server

**Modern Rust CLI Patterns**
- Uses `clap` v4 with derive macros for argument parsing
- Async/await throughout with `tokio` runtime
- Comprehensive error handling with `thiserror`
- Beautiful styling with recommended crates

**Extensibility**
- Plugin-like connector architecture
- Easy to add new commands
- Multiple output formats supported
- Ready for TUI mode integration

## Key Dependencies

### Core CLI Libraries
```toml
clap = { version = "4.0", features = ["derive", "env"] }
owo-colors = "4.0"           # Zero-cost terminal colors
indicatif = "0.17"           # Progress bars & spinners
comfy-table = "7.0"          # Beautiful ASCII tables
serde_yaml = "0.9"           # YAML output support
```

### Async Runtime
```toml
tokio = { version = "1.36", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### Optional TUI (Feature-gated)
```toml
ratatui = { version = "0.24", optional = true }
crossterm = { version = "0.27", optional = true }
```

## Command Architecture

### Command Structure
```rust
#[derive(Parser)]
#[command(name = "rzn-tools")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true)]
    pub output: OutputFormat,    // --output json|yaml|pretty|text|markdown

    #[arg(long, global = true)]
    pub tui: bool,              // --tui flag

    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,            // -v, -vv, -vvv
}

#[derive(Subcommand)]
pub enum Commands {
    List,                       // rzn-tools list
    Search { connector, query, limit },  // rzn-tools search youtube "rust"
    Get { connector, id },      // rzn-tools get youtube dQw4w9WgXcQ
    Config { action },          // rzn-tools config show|set|test
    Connectors,                 // rzn-tools connectors
    Tools { connector },        // rzn-tools tools youtube
}
```

### Error Handling Strategy
```rust
#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Connector '{0}' not found")]
    ConnectorNotFound(String),

    #[error("Tool '{0}' not found for connector '{1}'")]
    ToolNotFound(String, String),

    #[error("Authentication required for connector '{0}'")]
    AuthenticationRequired(String),

    #[error("Core library error: {0}")]
    Core(#[from] rzn_tools_core::error::ConnectorError),

    // ... more error variants
}
```

## Output Formatting System

### Multi-Format Support
```rust
#[derive(ValueEnum)]
pub enum OutputFormat {
    Pretty,    // Human-readable with colors & tables
    Json,      // Machine-readable JSON
    Yaml,      // YAML format
    Text,      // Plain text
    Markdown,  // Markdown format
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum OutputData {
    ConnectorList(Vec<ServerInfo>),
    SearchResults { connector, query, results },
    ResourceData { connector, id, data },
    ToolsList { connector, tools },
    ConfigInfo(Value),
}
```

### Pretty Output Features
- **Unicode Tables**: UTF-8 rounded corners with `comfy-table`
- **Color Coding**: Status indicators, syntax highlighting
- **Progress Indicators**: Spinners for async operations
- **Categorization**: Grouped connector displays
- **Responsive Layout**: Adapts to terminal width

## Connector Integration

### Registry Pattern
```rust
pub struct ProviderRegistry {
    pub providers: HashMap<String, Arc<Box<dyn Connector>>>,
}

impl ProviderRegistry {
    pub fn register_provider(&mut self, provider: Box<dyn Connector>) {
        self.providers.insert(provider.name().to_string(), Arc::new(provider));
    }

    pub fn get_provider(&self, name: &str) -> Option<&Arc<Box<dyn Connector>>> {
        self.providers.get(name)
    }
}
```

### Dynamic Connector Loading
- No-auth connectors: YouTube, HackerNews, Wikipedia, ArXiv, PubMed, etc.
- Auth-required connectors: Google Search, Reddit, Brave Search (env vars)
- Thread-safe access with `Arc<Box<dyn Connector>>`
- Graceful handling of missing credentials

### Authentication Flow
```rust
// Environment variable detection
if std::env::var("GOOGLE_API_KEY").is_ok() && std::env::var("GOOGLE_CSE_ID").is_ok() {
    let mut auth = AuthDetails::new();
    auth.insert("api_key".to_string(), std::env::var("GOOGLE_API_KEY").unwrap());
    auth.insert("cse_id".to_string(), std::env::var("GOOGLE_CSE_ID").unwrap());

    if let Ok(connector) = GoogleSearchConnector::new(auth).await {
        registry.register_provider(Box::new(connector));
    }
}
```

## Performance Optimizations

### Async Architecture
- All I/O operations use `async/await`
- Concurrent connector initialization
- Non-blocking progress indicators
- Parallel tool calls where possible

### Memory Management
- `Arc` for shared connector instances
- Lazy evaluation of expensive operations
- Efficient string handling with `Cow<str>`
- Minimal allocations in hot paths

### User Experience
- **Sub-100ms startup time** for most commands
- **Live progress indicators** for slow operations
- **Intelligent caching** of connector metadata
- **Graceful degradation** when connectors fail

## Testing Strategy

### Unit Tests
```bash
cargo test                           # All tests
cargo test --package rzn_tools_cli  # CLI-specific tests
```

### Integration Tests
- Mock connector implementations
- End-to-end command testing
- Output format validation
- Error handling verification

### Manual Testing
```bash
# Test all commands
rzn-tools list
rzn-tools connectors
rzn-tools tools youtube
rzn-tools search youtube "test"
rzn-tools get youtube dQw4w9WgXcQ

# Test output formats
rzn-tools list --output json
rzn-tools list --output yaml
rzn-tools list --output markdown
```

## Build & Distribution

### Development Build
```bash
cargo build                          # Debug build
cargo build --release                # Release build
```

### Feature Gates
```bash
cargo build --features tui           # Include TUI mode
cargo build --no-default-features    # Minimal build
```

### Cross-Compilation
```toml
# Cargo.toml workspace config enables:
# - Windows (x86_64-pc-windows-gnu)
# - macOS (x86_64-apple-darwin, aarch64-apple-darwin)
# - Linux (x86_64-unknown-linux-gnu, x86_64-unknown-linux-musl)
```

### Binary Optimization
```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit
panic = "abort"     # Smaller binary size
```

## Extension Points

### Adding New Commands
1. Add command variant to `Commands` enum in `cli.rs`
2. Create implementation file in `commands/`
3. Add match arm in `main.rs`
4. Implement `Result<()>` returning async function

### Adding New Output Formats
1. Add variant to `OutputFormat` enum
2. Implement formatting in `output/mod.rs`
3. Update help documentation

### Custom Connectors
1. Implement `Connector` trait in core library
2. Add to registry initialization in `commands/list.rs`
3. Connector automatically available in CLI

## Security Considerations

### Credential Handling
- Environment variables only (no files)
- No credentials logged or displayed
- Secure cookie extraction via `rookie` crate
- Authentication validation before usage

### Input Validation
- All user inputs validated via JSON schema
- SQL injection prevention (no direct SQL)
- URL validation for web scraping
- Rate limiting respect

### Network Security
- HTTPS-only for all external requests
- Certificate validation enabled
- Request signing where supported
- User-agent rotation for scraping

## Monitoring & Observability

### Logging
```bash
RUST_LOG=rzn_tools_cli=debug rzn-tools search youtube "test"
RUST_LOG=rzn_tools_core=info rzn-tools list
```

### Metrics
- Command execution time
- Connector success/failure rates
- Network request latency
- Error categorization

### Debugging
```bash
rzn-tools -vvv search youtube "test"       # Maximum verbosity
rzn-tools --output json list 2>/dev/null  # Suppress warnings
```

## Future Enhancements

### Planned Features
- **Interactive TUI Mode**: Full-screen dashboard with `ratatui`
- **Configuration Files**: TOML-based config support
- **Shell Completions**: Bash, Zsh, Fish completion scripts
- **Man Pages**: Automated documentation generation
- **Plugin System**: Dynamic connector loading

### Performance Improvements
- Connection pooling for HTTP clients
- Response caching layer
- Parallel connector operations
- Background data prefetching

### Platform Integration
- **Homebrew Formula**: macOS package manager
- **AUR Package**: Arch Linux user repository
- **Snap/AppImage**: Universal Linux packages
- **Chocolatey**: Windows package manager

This technical documentation provides a comprehensive overview of the CLI architecture, implementation patterns, and extension points for developers working with the RZN DataSourcer CLI codebase.
