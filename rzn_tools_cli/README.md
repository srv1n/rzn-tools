# RZN Integrations CLI (`rzn-tools`)

## Overview

The `rzn-tools` binary is the CLI entrypoint for **RZN Integrations**. It gives you one command
surface for 20+ external systems, with structured output and multiple export formats.

## Installation

### Prerequisites
- Rust 1.70+ and Cargo

### Quick Install
```bash
git clone https://github.com/srv1n/rzn-tools.git
cd rzn-tools
cargo build --release

# Add to PATH (optional)
sudo cp target/release/rzn-tools /usr/local/bin/
```

### Verify Installation
```bash
rzn-tools --version
rzn-tools --help
```

## Quick Start

### 1. List Available Data Sources
```bash
rzn-tools list
```
```
Available Data Sources

┌─────────────────────┬──────────────────────────────────────────────┐
│ Name                │ Description                                  │
├─────────────────────┼──────────────────────────────────────────────┤
│ youtube            │ A connector for interacting with YouTube.    │
│ hackernews         │ Hacker News via Firebase and Algolia API     │
│ wikipedia          │ Wikipedia article search and retrieval       │
│ arxiv              │ Academic papers from arXiv.org               │
│ pubmed             │ Medical literature database                  │
└─────────────────────┴──────────────────────────────────────────────┘
```

### 2. Search for Content
```bash
rzn-tools search youtube "rust programming" --limit 3
```
```
Search Results: rust programming
Connector: youtube

1. Learn Rust Programming - Complete Course
   https://www.youtube.com/watch?v=BpPEoZW5IiY
   In this comprehensive Rust course for beginners...

2. Rust Programming Full Course | Learn in 2024
   https://www.youtube.com/watch?v=rQ_J9WH6CGk
   Duration: 3 hours and 5 minutes...
```

### 3. Get Detailed Content
```bash
rzn-tools get youtube BpPEoZW5IiY
```
```
Resource: BpPEoZW5IiY (youtube)

Title: Learn Rust Programming - Complete Course

Description:
In this comprehensive Rust course for beginners, you will learn about
the core concepts of the language and underlying mechanisms in theory.

Chapters:
  * 0:00 - Introduction & Learning Resources
      Welcome to this rust programming course for beginners...
  * 6:19 - Variables
      Variables are assigned using the let keyword...
  * 27:07 - Numbers & Binary System
      Numbers in Rust come in different types...
```

## Core Commands

### `rzn-tools list` - Show Available Connectors
Lists all available data source connectors with descriptions.

**Usage:**
```bash
rzn-tools list [OPTIONS]
```

**Options:**
- `--output FORMAT` - Output format (pretty, json, yaml, text, markdown)

**Examples:**
```bash
rzn-tools list                    # Pretty table view
rzn-tools list --output json      # JSON format
rzn-tools list --output markdown  # Markdown format
```

### `rzn-tools search` - Search for Content
Search for content using a specific data source connector.

**Usage:**
```bash
rzn-tools search <CONNECTOR> <QUERY> [OPTIONS]
```

**Arguments:**
- `CONNECTOR` - Name of the connector (youtube, reddit, hackernews, etc.)
- `QUERY` - Search query string

**Options:**
- `--limit NUMBER` - Maximum number of results (default: 10)
- `--output FORMAT` - Output format

**Examples:**
```bash
# Search YouTube videos
rzn-tools search youtube "machine learning" --limit 5

# Search academic papers
rzn-tools search arxiv "quantum computing" --limit 3

# Search Hacker News
rzn-tools search hackernews "rust language" --limit 10

# Export results as JSON
rzn-tools search reddit "programming" --output json --limit 5
```

### `rzn-tools get` - Fetch Specific Content
Retrieve detailed information about a specific resource.

**Usage:**
```bash
rzn-tools get <CONNECTOR> <ID> [OPTIONS]
```

**Arguments:**
- `CONNECTOR` - Name of the connector
- `ID` - Resource ID or URL

**Options:**
- `--output FORMAT` - Output format

**Examples:**
```bash
# Get YouTube video with transcript
rzn-tools get youtube dQw4w9WgXcQ
rzn-tools get youtube "https://www.youtube.com/watch?v=dQw4w9WgXcQ"

# Get Wikipedia article
rzn-tools get wikipedia "Rust (programming language)"

# Get research paper
rzn-tools get arxiv "2301.07041"

# Export as JSON
rzn-tools get youtube BpPEoZW5IiY --output json
```

### `rzn-tools connectors` - Detailed Connector Information
Show comprehensive information about all connectors including status and capabilities.

**Usage:**
```bash
rzn-tools connectors [OPTIONS]
```

**Examples:**
```bash
rzn-tools connectors                  # Show all connector details
rzn-tools connectors --output yaml    # Export as YAML
```

### Connector Subcommands (Recommended)

Each connector has its own subcommand with proper CLI flags:

```bash
# YouTube
rzn-tools youtube search --query "rust programming" --limit 10
rzn-tools youtube video --id dQw4w9WgXcQ
rzn-tools youtube transcript --id dQw4w9WgXcQ

# Hacker News
rzn-tools hackernews top --limit 20
rzn-tools hackernews search --query "rust" --limit 10
rzn-tools hn story --id 38500000

# arXiv
rzn-tools arxiv search --query "transformer architecture" --limit 10
rzn-tools arxiv paper --id 2301.07041

# GitHub
rzn-tools github search-repos --query "rust cli"
rzn-tools gh issues --repo rust-lang/rust --state open

# Local filesystem
rzn-tools localfs list-files --path ~/Documents --recursive
rzn-tools localfs extract-text --path ~/paper.pdf

# Use --help on any subcommand
rzn-tools hackernews --help
rzn-tools youtube search --help
```

### Tool Discovery

Use `rzn-tools tools <connector>` to see what each connector exposes and then use the connector's
subcommand wrappers (recommended):

```bash
rzn-tools tools reddit
rzn-tools reddit --help
```

### `rzn-tools config` - Manage Authentication

Manage authentication credentials for connectors.

**Usage:**
```bash
rzn-tools config <ACTION> [OPTIONS]
```

**Actions:**
- `show` - Display current configuration
- `set` - Configure authentication
- `test` - Test authentication
- `remove` - Remove authentication

**Examples:**
```bash
# Show current config
rzn-tools config show

# Use auth profiles to manage multiple accounts/proxies
rzn-tools --auth-profile work config show

# If you omit --auth-profile, rzn-tools will use `default` when present, otherwise it will
# fall back to the first configured profile for that connector.

# Set API key authentication
rzn-tools config set github --value "ghp_your_token"

# Set browser cookie authentication
rzn-tools config set x --auth-type browser --browser chrome

# Set a per-profile proxy (where supported by the connector)
rzn-tools --auth-profile work config set reddit --auth-type proxy --value "http://127.0.0.1:8080"

# Test authentication
rzn-tools config test github

# Remove authentication
rzn-tools config remove reddit
```

## Available Data Sources

### Media & Entertainment

#### YouTube (`youtube`)
- **Search videos** by keywords
- **Get video details** with full transcripts organized by chapters
- **No authentication required**

```bash
rzn-tools search youtube "rust tutorial"
rzn-tools get youtube dQw4w9WgXcQ
```

#### Reddit (`reddit`) *[Requires Auth]*
- **Search posts** and comments
- **Get full thread hierarchies**
- **Authentication:** Client ID & Secret

```bash
# Set up authentication first
export REDDIT_CLIENT_ID="your_client_id"
export REDDIT_CLIENT_SECRET="your_client_secret"

rzn-tools search reddit "programming tips"
rzn-tools get reddit "post_id_here"
```

### Academic & Research

#### ArXiv (`arxiv`)
- **Search academic preprints**
- **Get full paper metadata** and abstracts
- **No authentication required**

```bash
rzn-tools search arxiv "machine learning"
rzn-tools get arxiv "2301.07041"
```

#### PubMed (`pubmed`)
- **Search medical literature**
- **Get article abstracts** with MeSH terms
- **No authentication required**

```bash
rzn-tools search pubmed "covid vaccine efficacy"
rzn-tools get pubmed "34762503"
```

#### Semantic Scholar (`semantic_scholar`)
- **Search academic papers** with citation data
- **Get citation graphs** and influence metrics
- **No authentication required**

```bash
rzn-tools search semantic_scholar "neural networks"
rzn-tools get semantic_scholar "paper_id"
```

### Web & Social

#### Hacker News (`hackernews`)
- **Search tech news** and discussions
- **Get comment threads** with user karma
- **No authentication required**

```bash
rzn-tools search hackernews "artificial intelligence"
rzn-tools get hackernews "story_id"
```

#### Wikipedia (`wikipedia`)
- **Search encyclopedia articles**
- **Get full article content** with references
- **No authentication required**

```bash
rzn-tools search wikipedia "quantum computing"
rzn-tools get wikipedia "Rust (programming language)"
```

#### X/Twitter (`x`) *[Requires Auth]*
- **Official X API** for tweets, profiles, timelines, likes, bookmarks, lists, DMs, and media
- **Authentication:** bearer for public reads, OAuth 2.0 for user-context, OAuth 1.0a fallback
- **Not xAI:** use `xai-search` for `web_search` / `x_search`

```bash
# Guided import
rzn-tools setup x
rzn-tools x auth-status

# Public reads
rzn-tools config set x --key bearer_token --value "<X_BEARER_TOKEN>"
rzn-tools search x "machine learning"

# User-context auth
rzn-tools config set x --key oauth2_access_token --value "<ACCESS_TOKEN>"
rzn-tools config set x --key oauth2_refresh_token --value "<REFRESH_TOKEN>"
rzn-tools config set x --key client_id --value "<CLIENT_ID>"
rzn-tools x whoami
```

### Web Scraping

#### Web (`web`)
- **General web scraping** with CSS selectors
- **Form handling** and custom requests
- **No authentication required**

```bash
rzn-tools tools web  # See available scraping tools
```

## Output Formats

### Pretty (Default)
Human-readable output with colors, tables, and formatting.
```bash
rzn-tools list  # Uses pretty format by default
```

### JSON
Machine-readable structured data.
```bash
rzn-tools search youtube "rust" --output json
```
```json
{
  "type": "SearchResults",
  "data": {
    "connector": "youtube",
    "query": "rust",
    "results": {
      "videos": [...]
    }
  }
}
```

### YAML
YAML format for configuration files.
```bash
rzn-tools connectors --output yaml
```

### Text
Plain text without formatting.
```bash
rzn-tools list --output text
```

### Markdown
Markdown format for documentation.
```bash
rzn-tools tools youtube --output markdown
```

## Global Options

All global options must be placed **before** the subcommand.

| Option | Short | Description |
|--------|-------|-------------|
| `--output <FORMAT>` | | Output format: `pretty`, `json`, `yaml`, `text`, `markdown` |
| `--copy` | `-c` | Copy output to system clipboard |
| `--no-color` | | Disable colored output |
| `--verbose` | `-v` | Verbose output (can repeat: `-vv`, `-vvv`) |
| `--tui` | | Launch interactive TUI mode *(Coming Soon)* |

### Examples
```bash
# Copy results to clipboard
rzn-tools --copy fetch https://arxiv.org/abs/2301.07041
rzn-tools -c search youtube "rust tutorial"

# Output as JSON and copy to clipboard
rzn-tools --copy --output json hackernews search --query "rust"

# Verbose mode for debugging
rzn-tools -vv search youtube "test"
```

**Note:** Global flags must come before the subcommand:
```bash
rzn-tools --copy fetch hn:12345678    # ✓ Correct
rzn-tools fetch --copy hn:12345678    # ✗ Won't work
```

## Authentication Setup

### Environment Variables Method (Recommended)

#### Reddit
```bash
export REDDIT_CLIENT_ID="your_reddit_client_id"
export REDDIT_CLIENT_SECRET="your_reddit_client_secret"
```

### Browser Cookie Method

For services like X/Twitter, you can extract cookies from your browser:

```bash
rzn-tools config set x --auth-type browser --browser chrome
```

Supported browsers: `chrome`, `firefox`, `safari`, `brave`

## Common Use Cases

### Content Research
```bash
# Find educational videos
rzn-tools search youtube "rust programming tutorial" --limit 5

# Get full transcript for analysis
rzn-tools get youtube BpPEoZW5IiY --output json > transcript.json

# Cross-reference with academic papers
rzn-tools search arxiv "rust programming language"
```

### Market Research
```bash
# Track discussions about a topic
rzn-tools search hackernews "artificial intelligence" --limit 20
rzn-tools search reddit "AI tools" --limit 15

# Monitor news coverage
rzn-tools search hackernews "AI startup funding"
```

### Academic Research
```bash
# Literature review
rzn-tools search pubmed "cancer immunotherapy" --limit 50 --output json
rzn-tools search arxiv "machine learning medicine" --limit 30
rzn-tools search semantic_scholar "deep learning healthcare"

# Get specific papers
rzn-tools get arxiv "2301.07041" --output markdown > paper_summary.md
```

### Data Collection Pipelines
```bash
# Collect and export data
rzn-tools search youtube "data science" --output json > youtube_results.json
rzn-tools search arxiv "data science" --output json > arxiv_results.json
rzn-tools search hackernews "data science" --output json > hn_results.json

# Combine results with jq
jq -s '.' *.json > combined_results.json
```

## Tips & Best Practices

### Performance
- Use `--limit` to control result size
- Export large datasets as JSON for processing
- Run time-consuming operations in background

### Security
- Store API keys in environment variables
- Use browser cookie method for personal accounts
- Rotate credentials regularly

### Automation
- Use JSON output for scripting: `--output json`
- Combine with `jq` for data processing
- Set up aliases for common searches

### Documentation
- Use `rzn-tools tools <connector>` to see available options
- Check connector status with `rzn-tools connectors`
- Export schemas with `--output markdown`

## Troubleshooting

### Common Issues

#### "Connector not found"
```bash
rzn-tools list  # Check available connectors
rzn-tools connectors  # Check connector status
```

#### Authentication errors
```bash
rzn-tools config show  # Check current config
rzn-tools config test <connector>  # Test specific connector
```

#### Network timeouts
```bash
rzn-tools -vv search youtube "test"  # Enable verbose logging
```

#### Missing results
```bash
rzn-tools tools <connector>  # Check available tools
```

### Debug Mode
```bash
RUST_LOG=debug rzn-tools search youtube "test"
```

### Report Issues
If you encounter bugs or have feature requests:
1. Check existing issues at GitHub repository
2. Include command that failed
3. Include error output with `-vv` flag
4. Include system information (OS, Rust version)

## Advanced Usage

### Shell Integration

#### Shell Aliases
```bash
alias yt="rzn-tools search youtube"
alias arxiv="rzn-tools search arxiv"
alias hn="rzn-tools search hackernews"

# Usage
yt "rust tutorial"
arxiv "machine learning"
hn "programming news"
```

### Configuration File *(Coming Soon)*
```toml
# ~/.config/rzn-tools/config.toml
[default]
output_format = "pretty"
verbosity = 1

[connectors.youtube]
default_limit = 10

[connectors.github]
token = "ghp_your_token"
```

### Cross-Platform Usage

#### Windows PowerShell
```powershell
$env:GITHUB_TOKEN="ghp_your_token"
rzn-tools search hackernews "windows tutorial"
```

#### macOS/Linux
```bash
export GITHUB_TOKEN="ghp_your_token"
rzn-tools search hackernews "macos tutorial"
```

This user guide provides comprehensive documentation for effectively using the rzn-tools CLI tool across various data sources and use cases.
