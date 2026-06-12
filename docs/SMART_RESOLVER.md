# Smart Resolver

The Smart Resolver is a pattern-matching system that automatically detects URLs, IDs, and identifiers and routes them to the appropriate connector and tool.

## Overview

Instead of manually specifying which connector to use, you can paste any supported URL or ID and rzn-tools will figure out what to do with it.

```bash
# These all work automatically
rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ
rzn-tools fetch arXiv:2301.07041
rzn-tools fetch PMID:12345678
rzn-tools fetch rust-lang/rust
rzn-tools fetch r/rust
```

## CLI Usage

### Fetch Content

```bash
rzn-tools fetch <input>
```

The `fetch` command (alias: `rzn-tools f`) auto-detects the input type and fetches the content.

**Examples:**

```bash
# YouTube
rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ
rzn-tools fetch https://youtu.be/dQw4w9WgXcQ
rzn-tools fetch dQw4w9WgXcQ

# Hacker News
rzn-tools fetch https://news.ycombinator.com/item?id=38500000
rzn-tools fetch hn:38500000
rzn-tools fetch 38500000

# ArXiv
rzn-tools fetch https://arxiv.org/abs/2301.07041
rzn-tools fetch arXiv:2301.07041
rzn-tools fetch 2301.07041

# PubMed
rzn-tools fetch https://pubmed.ncbi.nlm.nih.gov/12345678
rzn-tools fetch PMID:12345678

# DOI
rzn-tools fetch https://doi.org/10.1038/nature12373
rzn-tools fetch 10.1038/nature12373

# GitHub
rzn-tools fetch https://github.com/rust-lang/rust
rzn-tools fetch https://github.com/rust-lang/rust/issues/12345
rzn-tools fetch https://github.com/rust-lang/rust/pull/12345
rzn-tools fetch rust-lang/rust

# Reddit
rzn-tools fetch https://www.reddit.com/r/rust
rzn-tools fetch r/rust
rzn-tools fetch --output-format display_v1 https://www.reddit.com/user/spez/

# Play Store
rzn-tools fetch --output-format display_v1 https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US

# X/Twitter
rzn-tools fetch https://x.com/elonmusk/status/1234567890
rzn-tools fetch https://x.com/elonmusk
rzn-tools fetch @elonmusk

# Wikipedia
rzn-tools fetch https://en.wikipedia.org/wiki/Rust_(programming_language)

# Any URL (falls back to web scraper)
rzn-tools fetch https://example.com/some/page
```

### View Supported Patterns

```bash
rzn-tools formats
# or
rzn-tools patterns
```

Shows all supported input patterns grouped by connector.

### Handling Ambiguous Inputs

Some inputs may match multiple patterns. For example, an 8-digit number could be either a Hacker News ID or a PubMed ID.

In interactive mode, you'll be prompted to choose:

```
$ rzn-tools fetch 12345678

Ambiguous: Input '12345678' matches multiple patterns:

  [1] hackernews → get_post (Hacker News item ID)
  [2] pubmed → get (PubMed ID)

Select option [1-2]: 1

Detected: Hacker News item ID
  Routing to: hackernews → get_post
```

To avoid ambiguity, use prefixes:
- `hn:12345678` for Hacker News
- `PMID:12345678` for PubMed
- `arXiv:2301.07041` for ArXiv

## Supported Patterns

### YouTube

| Pattern | Example | Tool |
|---------|---------|------|
| Watch URL | `https://www.youtube.com/watch?v=dQw4w9WgXcQ` | `get` |
| Short URL | `https://youtu.be/dQw4w9WgXcQ` | `get` |
| Embed URL | `https://www.youtube.com/embed/dQw4w9WgXcQ` | `get` |
| Video ID | `dQw4w9WgXcQ` | `get` |

Note: playlist/channel URL resolution is not currently implemented in the resolver; use
`youtube/search` with `search_type="playlist"` or `search_type="channel"`.

### Hacker News

| Pattern | Example | Tool |
|---------|---------|------|
| Item URL | `https://news.ycombinator.com/item?id=38500000` | `get_post` |
| Item ID | `38500000` or `hn:38500000` | `get_post` |

### ArXiv

| Pattern | Example | Tool |
|---------|---------|------|
| Paper URL | `https://arxiv.org/abs/2301.07041` | `get` |
| New-style ID | `2301.07041` or `arXiv:2301.07041` | `get` |
| Old-style ID | `hep-th/9901001` | `get` |

### PubMed

| Pattern | Example | Tool |
|---------|---------|------|
| Article URL | `https://pubmed.ncbi.nlm.nih.gov/12345678` | `get` |
| PMID | `12345678` or `PMID:12345678` | `get` |

### DOI / Semantic Scholar

| Pattern | Example | Tool |
|---------|---------|------|
| DOI URL | `https://doi.org/10.1038/nature12373` | `get_paper` |
| Bare DOI | `10.1038/nature12373` | `get_paper` |
| Semantic Scholar URL | `https://www.semanticscholar.org/paper/.../abc123` | `get_paper` |

> **Tip:** For open-access PDF lookup, use `rzn-tools scihub paper --doi "10.1038/nature12373"` instead. The SciHub connector queries OpenAlex/Unpaywall to find freely available versions.

### GitHub

| Pattern | Example | Tool |
|---------|---------|------|
| Repository URL | `https://github.com/rust-lang/rust` | `get_repository` |
| Issue URL | `https://github.com/rust-lang/rust/issues/12345` | `get_issue` |
| Pull Request URL | `https://github.com/rust-lang/rust/pull/12345` | `get_pull_request` |
| Shorthand | `rust-lang/rust` | `get_repository` |

### Reddit

| Pattern | Example | Tool |
|---------|---------|------|
| Post URL | `https://www.reddit.com/r/rust/comments/abc123` | `get` |
| User URL | `https://www.reddit.com/user/spez/` | `user` |
| Subreddit URL | `https://www.reddit.com/r/rust` | `list` |
| Shorthand | `r/rust` | `list` |

### Play Store

| Pattern | Example | Tool |
|---------|---------|------|
| App details URL | `https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US` | `app` |

### X (Twitter)

| Pattern | Example | Tool |
|---------|---------|------|
| Tweet URL | `https://x.com/user/status/1234567890` | `get_tweet` |
| Profile URL | `https://x.com/elonmusk` | `get_profile` |
| Handle | `@elonmusk` | `get_profile` |

Notes:
- The resolver routes X URLs to connector `x` (official API). This requires a bearer token.
- If you only have browser-cookie access, call `x-browser/get_tweet` or `x-browser/get_profile` directly.

### Wikipedia

| Pattern | Example | Tool |
|---------|---------|------|
| Article URL | `https://en.wikipedia.org/wiki/Rust_(programming_language)` | `get_page` |

### Generic Web

| Pattern | Example | Tool |
|---------|---------|------|
| Any URL | `https://example.com/page` | `fetch` |

## Library Usage

The Smart Resolver can be used programmatically for downstream applications.

### Basic Usage

```rust
use rzn_tools_core::resolver::SmartResolver;

let resolver = SmartResolver::new();

// Resolve a single input (returns best match)
if let Some(action) = resolver.resolve("https://youtube.com/watch?v=dQw4w9WgXcQ") {
    println!("Connector: {}", action.connector);  // "youtube"
    println!("Tool: {}", action.tool);            // "get"
    println!("Args: {:?}", action.arguments);     // {"video_id": "dQw4w9WgXcQ"}
}
```

### Handling Multiple Matches

```rust
use rzn_tools_core::resolver::SmartResolver;

let resolver = SmartResolver::new();

// Get all possible matches for ambiguous input
let actions = resolver.resolve_all("12345678");

if actions.len() > 1 {
    // Present options to user
    for (i, action) in actions.iter().enumerate() {
        println!("[{}] {} → {}", i + 1, action.connector, action.tool);
    }
    // Let user select...
}
```

### Check If Input Is Supported

```rust
let resolver = SmartResolver::new();

if resolver.can_resolve("https://youtube.com/watch?v=xxx") {
    // Input matches a known pattern
}
```

### List All Patterns

```rust
let resolver = SmartResolver::new();

for pattern in resolver.list_patterns() {
    println!("{}: {} → {}", pattern.connector, pattern.example, pattern.tool);
}
```

### Types

```rust
/// A resolved action ready to be executed
pub struct ResolvedAction {
    /// The connector to use (e.g., "youtube", "pubmed")
    pub connector: String,
    /// The tool to call (e.g., "get_video_details")
    pub tool: String,
    /// Arguments extracted from the input
    pub arguments: HashMap<String, serde_json::Value>,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Human-readable description
    pub description: String,
}

/// Pattern information for documentation
pub struct PatternInfo {
    pub id: String,
    pub connector: String,
    pub tool: String,
    pub description: String,
    pub example: String,
}
```

## Integration with Desktop Apps

For desktop applications, the Smart Resolver enables a "universal search bar" experience:

```rust
use rzn_tools_core::resolver::SmartResolver;
use rzn_tools_core::{build_registry_enabled_only, CallToolRequestParam};

async fn handle_user_input(input: &str) -> Result<String, Error> {
    let resolver = SmartResolver::new();

    // Try to resolve the input
    let actions = resolver.resolve_all(input);

    if actions.is_empty() {
        return Err(Error::UnrecognizedInput);
    }

    // If multiple matches, show picker UI to user
    let action = if actions.len() > 1 {
        show_picker_dialog(&actions).await?
    } else {
        actions.into_iter().next().unwrap()
    };

    // Execute against the registry
    let registry = build_registry_enabled_only().await;
    let provider = registry.get_provider(&action.connector)
        .ok_or(Error::ConnectorNotFound)?;

    let connector = provider.lock().await;
    let request = CallToolRequestParam {
        name: action.tool.into(),
        arguments: Some(action.arguments.into_iter().collect()),
    };

    let result = connector.call_tool(request).await?;
    Ok(format_result(result))
}
```

## Adding New Patterns

Patterns are defined in `rzn_tools_core/src/resolver.rs`. To add a new pattern:

```rust
InputPattern {
    id: "myconnector_url",           // Unique identifier
    connector: "myconnector",        // Connector name
    tool: "get_item",                // Tool to call
    pattern: Regex::new(r"...").unwrap(),  // Regex with named captures
    captures: &["item_id"],          // Names of capture groups
    arg_mapping: &[("item_id", "id")],     // Map captures to tool args
    priority: 100,                   // Higher = checked first
    description: "My connector URL", // Human-readable description
}
```

Then add an example in `get_pattern_example()`.

## Priority System

Patterns are checked in priority order (highest first). This ensures:

1. Specific patterns (like `github.com/owner/repo`) match before generic ones
2. URLs with full domains match before shorthand notations
3. The generic web scraper (`https://...`) only matches when nothing else does

Default priorities:
- Full URLs: 100
- IDs with prefixes (e.g., `arXiv:xxx`): 90
- Shorthand notations (e.g., `r/rust`): 80
- Bare IDs (e.g., `12345678`): 50
- Generic web URL: 1
