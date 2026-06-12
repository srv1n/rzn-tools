# Federated Search Design

## Overview

Federated search allows querying multiple connectors with a single query and consolidating results. It's designed with **sensible defaults** so users can get started immediately without configuration.

## Quick Start

```bash
# Just works - uses built-in 'research' profile
rzn-tools search "CRISPR gene therapy" --profile research

# Ad-hoc connector list
rzn-tools search "release notes 6.4" -c slack,confluence,google-drive

# Single connector still works as before
rzn-tools search pubmed "CRISPR gene therapy"
```

**No configuration required.** Built-in profiles work out of the box.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Command                             │
│  rzn-tools search "query" --profile research                        │
│  rzn-tools search "query" -c pubmed,arxiv,biorxiv                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Profile Resolution                           │
│  1. Check user profiles (~/.config/rzn-tools/profiles.yaml)         │
│  2. Fall back to built-in profiles                              │
│  3. Apply CLI overrides (--add, --exclude)                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   FederatedSearch Engine                        │
│  • Execute searches in parallel (tokio::join_all)               │
│  • Per-source timeout with partial results                      │
│  • Normalize to UnifiedSearchResult                             │
│  • Apply weighting and merge strategy                           │
│  • Optional deduplication                                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Output Modes                               │
│  • grouped (default): Results organized by source               │
│  • interleaved: Single ranked list by relevance                 │
└─────────────────────────────────────────────────────────────────┘
```

## Built-in Profiles

These work immediately without any configuration:

| Profile | Connectors | Description |
|---------|------------|-------------|
| `research` | pubmed, arxiv, biorxiv, semantic-scholar | Academic research |
| `enterprise` | slack, confluence, google-drive | Internal documents |
| `social` | reddit, hackernews | Discussions & forums |
| `code` | github | Code search |
| `web` | perplexity, exa, tavily | AI-powered web search |

```bash
# List available profiles
rzn-tools profiles

# Show profile details
rzn-tools profiles show research
```

## CLI Interface

### Basic Usage

```bash
# Using a profile (simplest)
rzn-tools search "machine learning" --profile research

# Short form
rzn-tools search "machine learning" -p research

# Ad-hoc connector list
rzn-tools search "bug fix" -c github,slack

# Combine: profile + modifications
rzn-tools search "query" -p research --add wikipedia --exclude biorxiv
```

### Output Control

```bash
# Grouped by source (default)
rzn-tools search "query" -p research

# Interleaved single list
rzn-tools search "query" -p research --merge interleaved

# Limit per source
rzn-tools search "query" -p research --limit 5

# Output format
rzn-tools search "query" -p research --output json
rzn-tools search "query" -p research --output yaml
```

### Example Output (Grouped - Default)

```
Search: "CRISPR gene therapy"
Profile: research (4 sources)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 PubMed (5 results)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. [PMID:34567890] CRISPR-Cas9 for Genetic Disorders
   https://pubmed.ncbi.nlm.nih.gov/34567890/
   Smith J, et al. · Nature Medicine · 2023

2. [PMID:34567891] Gene Therapy Advances Using CRISPR
   https://pubmed.ncbi.nlm.nih.gov/34567891/
   Johnson A, et al. · Cell · 2023

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 ArXiv (3 results)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. [arXiv:2301.07041] Novel CRISPR Delivery Mechanisms
   https://arxiv.org/abs/2301.07041
   Chen L, et al. · q-bio.GN

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 bioRxiv (2 results)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. [10.1101/2023.01.15.524077] CRISPR Base Editing Efficiency
   https://www.biorxiv.org/content/10.1101/2023.01.15.524077
   Williams K, et al.

──────────────────────────────────────────────────────────────────
Total: 10 results from 3/4 sources
⚠ semantic-scholar: timed out (3s)
```

## Configuration

### User Profiles (Optional)

Create custom profiles in `~/.config/rzn-tools/profiles.yaml`:

```yaml
# Custom profile extending a built-in
my-research:
  extends: research
  add:
    - wikipedia
    - google-scholar
  exclude:
    - biorxiv
  defaults:
    limit: 15
  overrides:
    pubmed:
      start_year: 2020

# Completely custom profile
work-docs:
  description: "Internal documentation search"
  connectors:
    - slack
    - confluence
    - google-drive
    - notion
  defaults:
    limit: 20
    response_format: concise
  weights:
    confluence: 1.5  # Boost Confluence results
    slack: 0.8       # De-prioritize Slack

# Enterprise profile with auth requirements
enterprise-full:
  connectors:
    - slack
    - confluence
    - sharepoint
    - google-drive
  timeout_ms: 10000
  deduplication:
    enabled: true
    strategy: url
```

### Profile Options Reference

```yaml
profile-name:
  # Inheritance (optional)
  extends: base-profile-name

  # Description shown in `rzn-tools profiles`
  description: "What this profile is for"

  # Connectors to search
  connectors:
    - connector-name

  # Add/remove from extended profile
  add:
    - additional-connector
  exclude:
    - removed-connector

  # Default parameters for all connectors
  defaults:
    limit: 10                    # Results per source (default: 10)
    response_format: concise     # concise or detailed (default: concise)

  # Per-source weighting for interleaved merge (default: 1.0)
  weights:
    pubmed: 1.5
    arxiv: 1.0
    biorxiv: 0.8

  # Per-source parameter overrides
  overrides:
    pubmed:
      start_year: 2020
      end_year: 2024
    arxiv:
      max_results: 5

  # Timeout settings
  timeout_ms: 5000              # Per-source timeout (default: 5000)
  global_timeout_ms: 15000      # Total timeout (default: 15000)

  # Deduplication (default: disabled)
  deduplication:
    enabled: false
    strategy: url               # url, doi, or title_fuzzy
    prefer:                     # When duplicate, prefer this source
      - pubmed
      - arxiv
```

## Data Structures

### UnifiedSearchResult

Every result, regardless of source, is normalized to this format:

```rust
pub struct UnifiedSearchResult {
    /// Source connector name
    pub source: String,

    /// Unique ID (e.g., "PMID:12345", "arXiv:2301.07041")
    pub id: String,

    /// Result title
    pub title: String,

    /// Preview/snippet/abstract
    pub snippet: Option<String>,

    /// URL to full content
    pub url: Option<String>,

    /// Publication/creation timestamp
    pub timestamp: Option<DateTime<Utc>>,

    /// Source-specific metadata
    pub metadata: Value,

    /// Federation tracking (for debugging/transparency)
    pub _federation: FederationMeta,
}

pub struct FederationMeta {
    /// Original rank within source (before merge)
    pub source_rank: usize,

    /// Weight applied to this source
    pub weight: f32,

    /// Computed score (if interleaved)
    pub score: Option<f32>,
}
```

### FederatedSearchResult

```rust
pub struct FederatedSearchResult {
    /// The search query
    pub query: String,

    /// Profile used (if any)
    pub profile: Option<String>,

    /// Merge mode used
    pub merge_mode: MergeMode,

    /// Results (grouped or interleaved based on merge_mode)
    pub results: FederatedResults,

    /// Total count across all sources
    pub total_count: usize,

    /// Sources that completed
    pub completed: Vec<String>,

    /// Sources that failed or timed out
    pub errors: Vec<SourceError>,

    /// Whether results are partial (some sources failed/timed out)
    pub partial: bool,
}

pub enum FederatedResults {
    /// Results grouped by source
    Grouped(Vec<SourceResults>),

    /// Results interleaved into single ranked list
    Interleaved(Vec<UnifiedSearchResult>),
}
```

## Merge Strategies

### Grouped (Default)

Results organized by source. Best for:
- Comparing results across sources
- When source matters (e.g., peer-reviewed vs preprint)
- Browsing/exploration

### Interleaved

Single ranked list using weighted scoring. Best for:
- Finding the "best" result regardless of source
- AI agent consumption
- When you want one answer, not multiple lists

Scoring formula:
```
final_score = (1 / source_rank) * source_weight
```

## Error Handling & Partial Results

Federated search is resilient. If some sources fail, you still get results:

```json
{
  "query": "CRISPR",
  "partial": true,
  "completed": ["pubmed", "arxiv"],
  "errors": [
    {"source": "semantic-scholar", "error": "timeout after 5000ms"},
    {"source": "biorxiv", "error": "rate limited"}
  ],
  "results": { ... }
}
```

Timeouts are per-source, so a slow source doesn't block fast ones.

## MCP Integration

Federated search is exposed as a single MCP tool:

```json
{
  "name": "federated_search",
  "description": "Search across multiple data sources simultaneously. Use profiles like 'research' (academic papers), 'enterprise' (internal docs), 'social' (discussions), or specify connectors directly.",
  "input_schema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Search query"
      },
      "profile": {
        "type": "string",
        "description": "Named profile: research, enterprise, social, code, web",
        "enum": ["research", "enterprise", "social", "code", "web"]
      },
      "connectors": {
        "type": "array",
        "items": {"type": "string"},
        "description": "Ad-hoc connector list (alternative to profile)"
      },
      "limit": {
        "type": "integer",
        "description": "Max results per source",
        "default": 10
      },
      "merge": {
        "type": "string",
        "enum": ["grouped", "interleaved"],
        "default": "grouped"
      }
    },
    "required": ["query"]
  }
}
```

AI agents can now search multiple sources with one tool call:

```
User: Find recent papers on attention mechanisms
Agent: [calls federated_search with profile="research", query="attention mechanisms"]
```

## Defaults Philosophy

Everything has sensible defaults:

| Setting | Default | Rationale |
|---------|---------|-----------|
| `limit` | 10 | Enough to be useful, not overwhelming |
| `response_format` | concise | Token-efficient for AI agents |
| `merge` | grouped | Users usually want to see source provenance |
| `timeout_ms` | 5000 | Fast sources return quickly, slow ones don't block |
| `global_timeout_ms` | 15000 | Reasonable total wait time |
| `weight` | 1.0 | All sources equal by default |
| `deduplication` | disabled | Opt-in feature, can be complex |

## File Structure

```
rzn_tools_core/src/federated/
├── mod.rs              # Module exports
├── types.rs            # UnifiedSearchResult, FederatedSearchResult, etc.
├── profiles.rs         # SearchProfile, ProfileStore
├── engine.rs           # FederatedSearch execution
├── normalize.rs        # Per-connector result normalization
└── dedup.rs            # Deduplication strategies

rzn_tools_cli/src/commands/
├── search.rs           # Updated: --profile, -c, --merge flags
└── profiles.rs         # Profile management commands

Config files:
~/.config/rzn-tools/
├── auth.json           # Credentials (existing)
└── profiles.yaml       # User-defined profiles
```

## Implementation Phases

### Phase 1: Core (MVP)
- [x] `UnifiedSearchResult` type with federation metadata
- [x] `SearchProfile` with built-in profiles
- [x] `FederatedSearch` engine with parallel execution
- [x] Grouped output mode
- [ ] CLI `--profile` and `-c` flags
- [ ] Basic timeout handling

### Phase 2: Enhanced
- [ ] Interleaved merge with weighting
- [ ] Per-source timeouts with partial results
- [ ] Profile inheritance (`extends`)
- [ ] `--add`/`--exclude` CLI flags
- [ ] MCP `federated_search` tool

### Phase 3: Advanced
- [ ] Deduplication (URL, DOI, title fuzzy)
- [ ] Result caching
- [ ] Query transformation per source
- [ ] Custom user profiles in YAML
