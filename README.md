# RZN Integrations (`rzn-tools`)

[![CI](https://github.com/srv1n/rzn-tools/actions/workflows/ci.yml/badge.svg)](https://github.com/srv1n/rzn-tools/actions/workflows/ci.yml)
[![Release](https://github.com/srv1n/rzn-tools/actions/workflows/release.yml/badge.svg)](https://github.com/srv1n/rzn-tools/releases)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

A Rust CLI, MCP runtime, and library for pulling usable context out of external systems without
building a pile of one-off integrations.

`rzn-tools` stays the stable repo slug, package namespace, and binary name. The human-facing name for
this capability is `RZN Integrations`; in diagrams, use `Integrations`.

`rzn-tools` exists for a very specific annoying problem: the data you need is scattered across
links, APIs, SaaS tools, search providers, and local apps. The default alternatives are all
mediocre in their own special way: manual copy-paste into an LLM, one script per provider, or a
graveyard of single-purpose MCP servers.

This repo gives you one local runtime that can be used three ways:

| Surface | Use it for |
|---------|------------|
| `rzn-tools` CLI | shell workflows, quick fetches, scripting |
| `rzn-tools-mcp` or `rzn-tools serve` | Claude, ChatGPT, Codex, and other MCP clients |
| `rzn_tools_core` | embedding the same connector model in a Rust app |

This is not a hosted SaaS. It is a local-first integration layer: install it once, then search,
fetch, and normalize across many systems through one consistent surface.

```text
Links, IDs, queries, auth
          |
      rzn-tools
 CLI | MCP | Rust crate
          |
smart routing + auth + output shaping
          |
YouTube | GitHub | Slack | PubMed | Reddit | GSC | X | local files | ...
```

## Why Install This

| If you do this today | What usually sucks | What `rzn-tools` changes |
|----------------------|--------------------|--------------------------|
| Copy transcripts, docs, issues, or threads into an LLM by hand | Slow, lossy, and impossible to repeat cleanly | `fetch` and connector commands pull clean content directly from URLs, IDs, and APIs |
| Write one script per provider | You rebuild auth, pagination, and response shaping every time | Connectors share one tool model across CLI, MCP, and library use |
| Install one MCP server per source | Setup sprawl, uneven quality, and inconsistent tool naming | One MCP runtime exposes a broad, curated connector catalog |
| Feed raw provider payloads into agents or ingestion jobs | Massive responses, weird schemas, cleanup hell | `response_format`, `output_format=normalized_v1`, and `display_v1` shape results for the job |

## Best Fit

- Use it if you want the same integration surface for shell workflows, agent tool-calling, and app code.
- Use it if your work spans more than one source and you are sick of rebuilding the same glue.
- Use it if you care about structured or normalized output, not just raw HTML and ad hoc JSON.
- Skip it if you only need one provider and that provider's SDK already solves the whole problem.
- Skip it if you want a managed cloud product; this repo is the runtime, not the service.

## Quick Start

```bash
# Install the CLI + bundled starter assets
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash

# Smart fetch: paste a link or ID
rzn-tools fetch https://arxiv.org/abs/2301.07041
rzn-tools fetch https://github.com/rust-lang/rust/issues/12345
rzn-tools fetch PMID:12345678

# Federated research across multiple sources
rzn-tools search "CRISPR gene therapy" --profile research

# Give the same connector surface to an MCP client
rzn-tools serve --local-only

# Install the repo skill for agent clients in this project
rzn-tools skills install --scope project
```

## Agent Skills

`rzn-tools` ships a bundled Agent Skill for working on this repo. It can symlink that skill into
Claude Code, Gemini, generic `.agents/skills`, and Codex skill directories.

```bash
# Show target paths and current link status
rzn-tools skills status --scope project

# Project install: .claude/skills, .gemini/skills, .agents/skills
rzn-tools skills install --scope project --clients all

# Global install: ~/.claude/skills, ~/.gemini/skills, ~/.agents/skills, ~/.codex/skills
rzn-tools skills install --scope global --clients claude,gemini,agent,codex

# Refresh managed release-copy installs or relink repo-checkout installs
rzn-tools skills update --scope global --clients all

# Remove symlinks created by the installer
rzn-tools skills remove --scope global --clients all
```

When run from a repo checkout, the installer links directly to `.agents/skills/rzn-tools`, so skill
updates from git are picked up immediately. When run from an installed release binary outside a
checkout, it writes the release-versioned embedded skill into the managed `rzn-tools` data directory
and links clients to that copy.

## Connector Coverage

Connector breadth is proof, not the pitch. The pitch is that all of these sources show up through
one consistent interface.

Connector SVGs are bundled under `resources/icons/connectors/`, indexed by
`resources/icons/connectors/manifest.json`, and surfaced in MCP metadata via
`connectors/list.icon_url` and `tools/list.icons`. Downstream repos can reuse the raw files or the
bundled `data:image/svg+xml` URLs directly.

### No Authentication Required

These connectors work immediately after installation.

| Connector | Description |
|-----------|-------------|
| <img src="resources/icons/connectors/arxiv.svg" width="16" height="16" /> ArXiv | Search and retrieve academic preprints |
| <img src="resources/icons/connectors/biorxiv.svg" width="16" height="16" /> bioRxiv/medRxiv | Biology and medicine preprints |
| <img src="resources/icons/connectors/pubmed.svg" width="16" height="16" /> PubMed | Search biomedical and life sciences literature |
| <img src="resources/icons/connectors/semantic_scholar.svg" width="16" height="16" /> Semantic Scholar | Academic paper search, citations, references |
| <img src="resources/icons/connectors/google_scholar.svg" width="16" height="16" /> Google Scholar | Academic paper search |
| <img src="resources/icons/connectors/wikipedia.svg" width="16" height="16" /> Wikipedia | Article content and search |
| <img src="resources/icons/connectors/hackernews.svg" width="16" height="16" /> Hacker News | Stories, comments, user profiles |
| <img src="resources/icons/connectors/youtube.svg" width="16" height="16" /> YouTube | Video metadata, transcripts, search |
| <img src="resources/icons/connectors/rss.svg" width="16" height="16" /> RSS | Fetch and parse RSS/Atom feeds |
| <img src="resources/icons/connectors/weather.svg" width="16" height="16" /> Weather | Current weather + short forecast via wttr.in |
| <img src="resources/icons/connectors/polymarket.svg" width="16" height="16" /> Polymarket | Public prediction-market tag/event discovery, market context, and order-book analysis |
| <img src="resources/icons/connectors/kalshi.svg" width="16" height="16" /> Kalshi | Public prediction-market series/event discovery, market microstructure, and high-context market analysis |
| <img src="resources/icons/connectors/google_play.svg" width="16" height="16" /> Play Store | Best-effort Google Play app metadata (public HTML) |
| <img src="resources/icons/connectors/app_store.svg" width="16" height="16" /> App Store | Public App Store app metadata (iTunes Search API) + reviews |
| <img src="resources/icons/connectors/scihub.svg" width="16" height="16" /> SciHub | Open-access paper lookup by DOI (via OpenAlex/Unpaywall) |
| <img src="resources/icons/connectors/web.svg" width="16" height="16" /> Web Scraper | HTML content extraction with CSS selectors |

### Optional Authentication

These connectors work without credentials but offer additional functionality when authenticated.

| Connector | Without Auth | With Auth |
|-----------|--------------|-----------|
| <img src="resources/icons/connectors/reddit.svg" width="16" height="16" /> Reddit | Public subreddit browsing | Post to subreddits, access private content |
| <img src="resources/icons/connectors/github.svg" width="16" height="16" /> GitHub | Public repo search | Private repos, higher rate limits |
| <img src="resources/icons/connectors/semantic_scholar.svg" width="16" height="16" /> Semantic Scholar | Basic search | Higher rate limits |

### Authentication Required

| Connector | Auth Type | Description |
|-----------|-----------|-------------|
| <img src="resources/icons/connectors/slack.svg" width="16" height="16" /> Slack | Bot token | Channels, messages, users |
| <img src="resources/icons/connectors/discord.svg" width="16" height="16" /> Discord | Bot token | Servers, channels, messages |
| <img src="resources/icons/connectors/message-circle.svg" width="16" height="16" /> WhatsApp | Local WuzAPI + QR login | Messages, contacts, and groups |
| <img src="resources/icons/connectors/send.svg" width="16" height="16" /> Telegram | `api_id` + `api_hash` + local session | Dialogs, messages, and sending |
| <img src="resources/icons/connectors/atlassian.svg" width="16" height="16" /> Atlassian | API token | Jira issues, Confluence pages |
| <img src="resources/icons/connectors/app_store.svg" width="16" height="16" /> App Store Connect | API key (JWT) | Apps, App Analytics reports, Sales & Finance reports |
| <img src="resources/icons/connectors/apple.svg" width="16" height="16" /> Apple Search Ads | OAuth client creds + ES256 key | Keyword recommendations + reporting |
| <img src="resources/icons/connectors/google_drive.svg" width="16" height="16" /> Google Drive | OAuth2 | Files and folders |
| <img src="resources/icons/connectors/gmail.svg" width="16" height="16" /> Gmail | OAuth2 | Email access |
| <img src="resources/icons/connectors/google_calendar.svg" width="16" height="16" /> Google Calendar | OAuth2 | Calendar events |
| <img src="resources/icons/connectors/calendar.svg" width="16" height="16" /> CalDAV | Basic/Bearer | Calendar discovery + event read/write |
| <img src="resources/icons/connectors/google_people.svg" width="16" height="16" /> Google Contacts | OAuth2 | People/contacts |
| <img src="resources/icons/connectors/google_search_console.svg" width="16" height="16" /> Google Search Console | OAuth2 | SEO performance, sitemaps, URL inspection |
| <img src="resources/icons/connectors/bing.svg" width="16" height="16" /> Bing Webmaster Tools | API key | SEO performance stats + URL submission |
| <img src="resources/icons/connectors/linkedin.svg" width="16" height="16" /> LinkedIn | OAuth2 / OIDC token import | Auth status, member identity, official posting APIs, raw authenticated requests |
| <img src="resources/icons/connectors/microsoft.svg" width="16" height="16" /> Microsoft Graph | OAuth2 | OneDrive, Outlook, Calendar |
| <img src="resources/icons/connectors/imap.svg" width="16" height="16" /> IMAP | Server credentials | Email retrieval |
| <img src="resources/icons/connectors/mailgun.svg" width="16" height="16" /> SMTP | Server credentials | Outbound email sending |
| <img src="resources/icons/connectors/x.svg" width="16" height="16" /> X (Twitter) API | Bearer token | Official X API v2: tweets, profiles, recent search |
| <img src="resources/icons/connectors/x.svg" width="16" height="16" /> X (Twitter) Browser Cookies | Browser cookies | Threads, profiles, search (scraper-based) |

### Search Providers

These connectors query AI-powered or traditional search APIs.

| Connector | Auth Type |
|-----------|-----------|
| <img src="resources/icons/connectors/perplexity.svg" width="16" height="16" /> Perplexity | API Key |
| <img src="resources/icons/connectors/exa.svg" width="16" height="16" /> Exa | API Key |
| <img src="resources/icons/connectors/tavily.svg" width="16" height="16" /> Tavily | API Key |
| <img src="resources/icons/connectors/serpapi.svg" width="16" height="16" /> SerpApi | API Key |
| <img src="resources/icons/connectors/serper.svg" width="16" height="16" /> Serper | API Key |
| <img src="resources/icons/connectors/firecrawl.svg" width="16" height="16" /> Firecrawl | API Key |
| <img src="resources/icons/connectors/anthropic.svg" width="16" height="16" /> Anthropic | API Key |
| <img src="resources/icons/connectors/openai.svg" width="16" height="16" /> OpenAI | API Key |
| <img src="resources/icons/connectors/gemini.svg" width="16" height="16" /> Gemini | API Key |
| <img src="resources/icons/connectors/parallel.svg" width="16" height="16" /> Parallel AI | API Key |
| <img src="resources/icons/connectors/xai.svg" width="16" height="16" /> xAI | API Key |

### macOS Native

| Connector | Description |
|-----------|-------------|
| <img src="resources/icons/connectors/macos.svg" width="16" height="16" /> macOS Automation | Control Mail, Calendar, Safari via JXA (requires permissions) |
| <img src="resources/icons/connectors/spotlight.svg" width="16" height="16" /> Spotlight | Search files by content, name, type, or metadata (macOS only) |

## Smart Resolver

rzn-tools includes a pattern-matching system that automatically detects URLs, IDs, and identifiers and routes them to the appropriate connector.

```bash
# YouTube - any URL format or video ID
rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ
rzn-tools fetch https://youtu.be/dQw4w9WgXcQ
rzn-tools fetch dQw4w9WgXcQ

# Academic papers
rzn-tools fetch https://arxiv.org/abs/2301.07041
rzn-tools fetch arXiv:2301.07041
rzn-tools fetch PMID:12345678
rzn-tools fetch 10.1038/nature12373              # DOI → open-access lookup via SciHub

# GitHub - repos, issues, PRs
rzn-tools fetch https://github.com/rust-lang/rust
rzn-tools fetch https://github.com/rust-lang/rust/issues/12345
rzn-tools fetch rust-lang/rust

# Social platforms
rzn-tools fetch https://news.ycombinator.com/item?id=38500000
rzn-tools fetch hn:38500000
rzn-tools fetch r/rust
rzn-tools fetch @elonmusk
rzn-tools fetch https://polymarket.com/event/cbb-pur-arz-2026-03-28

# Local files (macOS Spotlight)
rzn-tools fetch ~/Documents/report.pdf
rzn-tools fetch /Users/me/Downloads/data.csv
rzn-tools fetch "spotlight:machine learning"

# Any URL falls back to web scraper
rzn-tools fetch https://example.com/some/page
```

**Ambiguous inputs** are handled interactively. For example, an 8-digit number could be a Hacker News ID or PubMed ID:

```
$ rzn-tools fetch 12345678

Ambiguous: Input '12345678' matches multiple patterns:

  [1] hackernews → get (Hacker News item ID)
  [2] pubmed → get (PubMed ID)

Select option [1-2]:
```

Use prefixes to avoid ambiguity: `hn:12345678`, `PMID:12345678`, `arXiv:2301.07041`.

**Shell quoting:** URLs containing `?` (like YouTube watch URLs) need to be quoted:

```bash
# This will fail in zsh/bash - the ? is interpreted as a glob
rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ  # Error!

# Quote the URL to make it work
rzn-tools fetch "https://www.youtube.com/watch?v=dQw4w9WgXcQ"  # Works!
```

```bash
# View all supported patterns
rzn-tools formats
```

See [Smart Resolver Documentation](docs/SMART_RESOLVER.md) for the complete list of patterns and library usage.

## Installation

### Pre-built Binaries

Ready-made binaries are available from the [GitHub Releases](https://github.com/srv1n/rzn-tools/releases) page. Choose one of the following methods:

#### Install Script (Recommended)

The install script downloads the correct release binary for your platform and the bundled
workflow/example/icon asset pack:

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
```

Check what was installed:

```bash
rzn-tools --version
rzn-tools workflows list
```

Refresh only the published workflow/example/icon bundle later:

```bash
rzn-tools workflows sync --remote
```

Uninstall the default user install:

```bash
rm -f ~/.local/bin/rzn-tools
rm -rf ~/.local/share/rzn-tools
rm -rf ~/.config/rzn-tools
```

### Build from Source

If you prefer to build from source or need to customize which connectors are included:

```bash
git clone https://github.com/srv1n/rzn-tools.git
cd rzn-tools

# Build, validate bundled workflows/examples, and install locally
make install
```

`make install` builds the release CLI, compiles the Rust examples, validates bundled workflow
metadata, and installs both the binary and starter workflow/icon assets.

More detail lives in [INSTALLATION.md](INSTALLATION.md).

## Federated Search

Search multiple data sources simultaneously with a single command using built-in profiles or custom connector lists.

### Built-in Profiles

| Profile | Connectors | Description |
|---------|------------|-------------|
| `research` | pubmed, arxiv, semantic-scholar, google-scholar | Academic papers |
| `enterprise` | slack, atlassian, github | Work documents and code |
| `social` | reddit, hackernews | Community discussions |
| `code` | github | Code search |
| `web` | perplexity, exa, tavily | AI-powered web search |
| `media` | youtube, wikipedia | Video and reference content |

### Usage

```bash
# Search using a profile
rzn-tools search "CRISPR gene therapy" --profile research
rzn-tools search "kubernetes deployment" -p enterprise
rzn-tools search "rust async" --profile social

# Custom connector list
rzn-tools search "machine learning" -s arxiv,pubmed,hackernews

# Merge modes
rzn-tools search "attention mechanisms" -p research --merge grouped    # Group by source (default)
rzn-tools search "attention mechanisms" -p research --merge interleaved # Interleave results

# Modify profiles on the fly
rzn-tools search "CRISPR" -p research --add wikipedia --exclude pubmed
```

### Output

Results are displayed grouped by source with timing information:

```
Federated Search: CRISPR gene therapy
Profile: research

━━ pubmed (10 results)
   1. CRISPR/Cas9 Immune System as a Tool for Genome Engineering
      https://pubmed.ncbi.nlm.nih.gov/12345678
   2. Advances in therapeutic CRISPR/Cas9 genome editing
      ...

━━ arxiv (10 results)
   1. Investigating the genomic background of CRISPR-Cas genomes
      CRISPR-Cas systems are an adaptive immunity that protects prokaryotes...
   ...

Completed in 1234ms
```

## CLI Usage

### Connector Subcommands (Recommended)

Each connector has its own subcommand with proper CLI flags:

```bash
# Local filesystem - text extraction from PDF, EPUB, DOCX, HTML, code
rzn-tools localfs list-files --path ~/Documents --recursive --extensions pdf,md
rzn-tools localfs extract-text --path ~/paper.pdf
rzn-tools localfs structure --path ~/book.epub
rzn-tools localfs section --path ~/doc.pdf --section page:5
rzn-tools localfs search --path ~/code.rs --query "async fn"

# YouTube
rzn-tools youtube search --query "rust programming" --limit 10
rzn-tools youtube video --id dQw4w9WgXcQ
rzn-tools youtube transcript --id dQw4w9WgXcQ

# Hacker News
rzn-tools hackernews top --limit 20
rzn-tools hackernews search --query "rust" --limit 10
rzn-tools hackernews thread --id 38500000
rzn-tools fetch "https://news.ycombinator.com/item?id=38500000"

# arXiv
rzn-tools arxiv search --query "transformer architecture" --limit 10
rzn-tools arxiv paper --id 2301.07041

# GitHub
rzn-tools github search-repos --query "rust cli"
rzn-tools github search-code --query "async fn" --repo tokio-rs/tokio
rzn-tools github issues --repo rust-lang/rust --state open

# Reddit
rzn-tools reddit search --query "rust" --subreddit programming
rzn-tools reddit hot --subreddit rust --limit 200 --output-format normalized_v1
rzn-tools reddit media --id https://www.reddit.com/comments/abc123
rzn-tools reddit user --username spez --output-format display_v1

# Play Store (best-effort)
rzn-tools play-store app --id com.whatsapp --output-format normalized_v1
rzn-tools fetch --output-format display_v1 "https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US"

# AI-powered search
rzn-tools perplexity-search search --query "best practices for rust async"
rzn-tools exa search --query "rust async programming" --num-results 10
rzn-tools openai-search search --query "machine learning"
rzn-tools anthropic-search search --query "AI safety"

# Google services (requires OAuth setup)
rzn-tools google-calendar list-events
rzn-tools google-drive list-files --query "project report"
rzn-tools google-gmail search --query "from:boss@company.com"

# Microsoft 365 (requires OAuth setup)
rzn-tools microsoft-graph list-drive-items
rzn-tools microsoft-graph list-mail --filter "isRead eq false"

# SMTP (requires setup)
rzn-tools smtp test-connection
rzn-tools smtp send-mail --to user@example.com --subject "Hello" --body "Test"

# Academic research
rzn-tools pubmed search --query "CRISPR gene therapy" --limit 10
rzn-tools semantic-scholar search --query "attention mechanism"
rzn-tools biorxiv search --query "protein folding"
rzn-tools scihub paper --doi "10.1038/nature12373"  # Open-access lookup
rzn-tools scihub search --query "attention mechanism"  # Search papers
rzn-tools scihub batch --dois "10.1038/nature12373,10.1371/journal.pone.0000308"

# Use --help on any subcommand for all options
rzn-tools localfs list-files --help
rzn-tools hackernews --help
```

### Generic Commands

```bash
# List available connectors
rzn-tools list

# Show tools for a connector
rzn-tools tools youtube
rzn-tools tools pubmed

# Smart fetch - auto-detects URL/ID type
rzn-tools fetch https://arxiv.org/abs/2301.07041
rzn-tools fetch https://news.ycombinator.com/item?id=38500000
rzn-tools fetch hn:38500000
rzn-tools fetch --output-format display_v1 https://www.reddit.com/user/spez/
rzn-tools fetch --output-format display_v1 https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US

# Search (single connector)
rzn-tools search arxiv "attention mechanism"
rzn-tools search hackernews "rust" --limit 20

# Get specific content
rzn-tools get hackernews 12345678
rzn-tools get youtube dQw4w9WgXcQ

# Connector subcommands (recommended)
rzn-tools github search-repos --query "language:rust stars:>1000" --limit 10
rzn-tools slack channels --limit 100

# Output formats
rzn-tools --output json arxiv search --query "llm" | jq '.results[0]'

# Copy output to clipboard
rzn-tools --copy fetch hn:38500000
```

### All Connector Subcommands

| Connector | Aliases | Description |
|-----------|---------|-------------|
| `localfs` | `fs`, `file` | Local filesystem text extraction |
| `youtube` | `yt` | Video metadata, transcripts, search |
| `hackernews` | `hn` | Stories, comments, search |
| `arxiv` | | Academic preprints |
| `github` | `gh` | Repositories, issues, PRs, code |
| `reddit` | | Posts, comments, subreddits |
| `play-store` | `playstore` | Google Play Store app metadata (best-effort) |
| `web` | | Web page scraping |
| `wikipedia` | `wiki` | Article search and retrieval |
| `pubmed` | | Medical literature |
| `semantic-scholar` | `scholar` | Academic paper search |
| `slack` | | Workspace messages, channels |
| `discord` | | Servers, channels, messages |
| `x` | `twitter` | Tweets, profiles, search |
| `rss` | | RSS/Atom feed reader |
| `biorxiv` | | Biology/medicine preprints |
| `scihub` | | Open-access paper lookup by DOI |
| `google-calendar` | | Calendar events |
| `google-drive` | | File management |
| `google-gmail` | | Email access |
| `google-people` | | Contacts |
| `google-scholar` | | Academic search |
| `microsoft-graph` | | Microsoft 365 services |
| `atlassian` | | Jira + Confluence |
| `imap` | | Email retrieval |
| `smtp` | `mailer` | Outbound email sending |
| `macos` | | macOS automation |
| `spotlight` | | File search (macOS) |
| `openai-search` | | OpenAI web search |
| `anthropic-search` | | Anthropic web search |
| `gemini-search` | | Gemini web search |
| `perplexity-search` | | Perplexity search |
| `xai-search` | | xAI `web_search` + `x_search` |
| `exa` | | Neural search |
| `tavily-search` | | Tavily search |
| `serper-search` | | Serper search |
| `serpapi-search` | | SerpAPI search |
| `firecrawl-search` | | Firecrawl scraping |
| `parallel-search` | | Parallel AI search |

### Response Format

Most connectors support a `response_format` parameter to control output verbosity:

- **`concise`** (default): Returns only essential fields for token efficiency
- **`detailed`**: Returns full metadata including all available fields

```bash
# Concise output (default) - minimal fields, fewer tokens
rzn-tools hackernews top --limit 5

# Some connectors support a `--response-format` flag (e.g., `concise` or `detailed`)
rzn-tools openai-search search --query "What is Rust?" --response-format detailed
```

This is particularly useful when integrating with AI agents where token usage matters. The concise format reduces response size while preserving the most important information.

### Global Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--copy` | `-c` | Copy output to system clipboard |
| `--output <format>` | | Output format: `pretty`, `json`, `yaml`, `text`, `markdown` |
| `--no-color` | | Disable colored output |
| `--verbose` | `-v` | Verbose output (can be repeated: `-vv`, `-vvv`) |

Note: Global flags must be placed **before** the subcommand:
```bash
rzn-tools --copy fetch https://arxiv.org/abs/2301.07041  # Correct
rzn-tools fetch --copy https://arxiv.org/abs/2301.07041  # Won't work
```

## Configuration

rzn-tools provides an interactive setup wizard that guides you through configuring each connector.

### Interactive Setup

```bash
# Launch the setup wizard
rzn-tools setup

# Or configure a specific connector
rzn-tools setup slack
```

The wizard will:
1. Show you where to obtain credentials (with clickable URLs)
2. Walk you through each step
3. Securely prompt for tokens (hidden input)
4. Test the connection automatically
5. Save credentials to `~/.config/rzn-tools/auth.json`

Example session:

```
$ rzn-tools setup slack

Setting up Slack
Workspace messages and channels

How to get credentials:
  https://api.slack.com/apps

  1. Create a new app or select an existing one
  2. Go to 'OAuth & Permissions' in the sidebar
  3. Add required scopes: channels:read, channels:history, users:read
  4. Install the app to your workspace
  5. Copy the 'Bot User OAuth Token' (starts with xoxb-)

Configuration options:

  Option 1: Set environment variables:
    export SLACK_BOT_TOKEN="<Bot Token>"

  Option 2: Enter credentials now (stored in ~/.config/rzn-tools/auth.json):

Enter credentials now? [y/N] y
  Bot Token (starts with xoxb-): ****

Saved! Credentials saved for Slack

Testing connection... Success!

You're all set! Try:
  rzn-tools search slack "test query"
```

### Managing Credentials

```bash
# View configured connectors
rzn-tools config show

# Test authentication
rzn-tools config test github

# Remove credentials
rzn-tools config remove slack

# Set credentials directly
rzn-tools config set github --value "ghp_xxxx"
```

### Environment Variables

You can also configure connectors via environment variables:

```bash
export GITHUB_TOKEN="ghp_..."
export SLACK_BOT_TOKEN="xoxb-..."
export REDDIT_CLIENT_ID="..."
export REDDIT_CLIENT_SECRET="..."
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
```

### WhatsApp & Telegram (Chat Connectors)

WhatsApp and Telegram are easiest to use through the MCP server (`rzn-tools-mcp`), since they expose
interactive tools (QR login, phone-code login) rather than simple `search/get` flows.

**Telegram (MTProto):**

```bash
export TG_ID="123456"                    # from https://my.telegram.org/apps
export TG_HASH="0123456789abcdef..."     # from https://my.telegram.org/apps
export TG_SESSION_FILE="$HOME/.config/rzn-tools/telegram.session"  # optional
```

Then call (from your MCP client):
- `telegram/start_login` with `{ "phone": "+15551234567" }`
- `telegram/complete_login` with `{ "code": "12345", "password": "..." }` (password only for 2FA)

See `docs/connectors/telegram.md`.

**WhatsApp (WuzAPI sidecar):**

1. Install + run WuzAPI (recommended): `brew install asternic/wuzapi/wuzapi` then `wuzapi`
2. Configure rzn-tools:

```bash
export WUZAPI_BASE_URL="http://127.0.0.1:8080"  # default
export WUZAPI_TOKEN="your_wuzapi_user_token"    # required for normal endpoints (/session/*, /chat/*, ...)
```

Then call `whatsapp/connect` and scan the QR code in your WhatsApp mobile app.

See `docs/connectors/whatsapp.md`.

### Browser Cookie Extraction

For services like X (Twitter), rzn-tools can extract session cookies directly from your browser:

```bash
rzn-tools setup x-browser
```

The wizard will prompt you to select your browser (Chrome, Firefox, Safari, or Brave) and automatically extract cookies after you confirm you're logged in.

### X Official API Setup

`x` and `xai-search` are separate on purpose:

| Connector | Use for | Auth |
|------|------|------|
| `x` | official X platform API: profiles, tweets, timelines, likes, bookmarks, lists, DMs, media | bearer and/or OAuth |
| `xai-search` | xAI search tools only: `web_search`, `x_search` | `XAI_API_KEY` |

For the official X connector:

```bash
rzn-tools setup x
rzn-tools x auth-status
```

Recommended auth order:

1. `bearer_token` for public reads
2. `oauth2_*` for user-context reads and writes
3. `oauth1_*` only for legacy import or endpoint fallback

For user-context validation:

```bash
rzn-tools x whoami
rzn-tools x refresh-oauth2
```

### OAuth Setup

For Google and Microsoft services, rzn-tools supports the OAuth device authorization flow:

```bash
rzn-tools setup google-drive
```

You'll receive a code to enter at a URL in your browser. Once authorized, tokens are saved and refreshed automatically.

## Library Usage

```toml
[dependencies]
rzn_tools_core = { version = "0.1", features = ["arxiv", "pubmed"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
serde_json = "1"
```

```rust
use rzn_tools_core::{build_registry_enabled_only, CallToolRequestParam};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = build_registry_enabled_only().await;
    let pubmed = registry.get_provider("pubmed").unwrap();
    let connector = pubmed.lock().await;

    let request = CallToolRequestParam {
        name: "search_articles".into(),
        arguments: Some(json!({"query": "CRISPR"}).as_object().unwrap().clone()),
    };

    let result = connector.call_tool(request).await?;
    println!("{:?}", result);
    Ok(())
}
```

## MCP Server

rzn-tools includes a [Model Context Protocol](https://modelcontextprotocol.io/) server for integration with MCP-compatible clients like Claude Desktop.

```bash
cargo build --release -p rzn_tools_mcp --features full
./target/release/rzn-tools-mcp

# Native HTTP transport for tunnels / Workers / remote proxies
./target/release/rzn-tools-mcp http --bind 127.0.0.1:8000

# CLI wrappers for the same flow
rzn-tools configure cloudflare guide
rzn-tools configure cloudflare tunnel --hostname rzn-tools-origin.example.com --tunnel-name rzn-tools-mcp
rzn-tools serve

# Local-only mode if you do not want Cloudflare
rzn-tools serve --local-only
```

For Cloudflare tunnel mode:

- `cloudflared` is required.
- `wrangler` is optional and only needed if you also deploy a Worker.
- `rzn-tools serve` auto-starts `cloudflared` when a tunnel name is configured.
- The remote HTTP catalog is curated for agents: it lists canonical task tools and hides compatibility aliases plus `auth/*` setup helpers.
- The hostname in `serve.json`, the hostname in `~/.cloudflared/config.yml`, and the `Host` header accepted by the running origin all need to match.
- Use `rzn-tools configure cloudflare doctor` for troubleshooting, not as a required startup step.
- The tunnel itself still lives in Cloudflare / `cloudflared`; rzn-tools just serves MCP locally and saves defaults in `serve.json`.

Full operator guide:

- [`docs/integrations/REMOTE_MCP_SETUP.md`](docs/integrations/REMOTE_MCP_SETUP.md) covers Cloudflare Tunnel, domain setup, and remote-client setup for ChatGPT and Claude.

Claude Desktop configuration (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "rzn-tools": {
      "command": "/path/to/rzn-tools-mcp",
      "env": {
        "GITHUB_TOKEN": "...",
        "SLACK_BOT_TOKEN": "...",
        "TG_ID": "123456",
        "TG_HASH": "0123456789abcdef0123456789abcdef",
        "WUZAPI_BASE_URL": "http://127.0.0.1:8080",
        "WUZAPI_TOKEN": "your_wuzapi_user_token",
        "WUZAPI_ADMIN_TOKEN": "your_wuzapi_admin_token"
      }
    }
  }
}
```

## Feature Flags

Enable only the connectors you need to reduce binary size:

```toml
# Research
rzn_tools_core = { version = "0.1", features = ["arxiv", "pubmed", "semantic-scholar"] }

# Social
rzn_tools_core = { version = "0.1", features = ["reddit", "hackernews", "youtube"] }

# Chat
rzn_tools_core = { version = "0.1", features = ["telegram", "whatsapp"] }

# Enterprise
rzn_tools_core = { version = "0.1", features = ["slack", "github", "atlassian"] }

# Everything
rzn_tools_core = { version = "0.1", features = ["full"] }
```

## Architecture

```
rzn-tools/
├── rzn_tools_core/           # Package: rzn_tools_core
├── rzn_tools_cli/            # Package: rzn_tools_cli
├── rzn_tools_mcp/            # Package: rzn_tools_mcp
└── scrapable_derive/ # Proc-macro for HTML parsing
```

All connectors implement a common trait:

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn name(&self) -> &'static str;
    async fn list_tools(&self, request: Option<PaginatedRequestParam>) -> Result<ListToolsResult, ConnectorError>;
    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult, ConnectorError>;
    // ...
}
```

## Adding a Connector

See the **[Connector Development Guide](docs/CONNECTOR_DEVELOPMENT.md)** for a complete walkthrough including:

- Step-by-step implementation with code examples
- Tool design guidelines and naming conventions
- Authentication patterns
- Smart Resolver and Federated Search integration
- Testing and documentation

Quick overview:
1. Create a module in `rzn_tools_core/src/connectors/`
2. Implement the `Connector` trait
3. Add a feature flag to `Cargo.toml`
4. Register in `build_registry_enabled_only()`

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.

## Documentation

- [Connector Development Guide](docs/CONNECTOR_DEVELOPMENT.md) - Complete guide for adding connectors
- [CLI Usage Guide](rzn_tools_cli/README.md)
- [Smart Resolver](docs/SMART_RESOLVER.md) - URL/ID auto-detection and routing
- [Federated Search](docs/FEDERATED_SEARCH.md) - Multi-source search architecture
- [Authentication Design](docs/AUTH_DESIGN.md) - Auth patterns for downstream apps
- [Installation Options](INSTALLATION.md)
- [Connector Documentation](docs/CONNECTORS.md)
- [Authentication Setup](docs/auth/README.md)

## Downstream Integrations

- Downstream tool API + migration notes: `docs/integrations/DOWNSTREAM_UPGRADE.md`
- LLM-facing tool surface notes: `docs/integrations/TOOLS.md`
- `rzn-tools` plugin release path: `python3 scripts/publish_rzn_tools_release.py --platform macos_arm64 --channel stable --targets all`

## Maintainer Release Process

```bash
make release VERSION=0.2.17
```

That command is intentionally strict. It refuses to cut a release from a dirty tree, from the wrong
branch, or from a local commit that is not already the exact `origin/main` tip. When it passes, it
creates and pushes `v0.2.17`; GitHub Actions then builds Linux, Windows, macOS Intel, macOS Apple
Silicon, generates release notes from the delta since the previous tag, and publishes the GitHub Release.

To clean up old GitHub release titles that still use the legacy brand:

```bash
GITHUB_TOKEN=... make release-retitle-legacy
```

## Acknowledgements

rzn-tools is built on the shoulders of excellent open-source crates:

| Crate | Used For |
|-------|----------|
| [roux](https://crates.io/crates/roux) | Reddit API client |
| [octocrab](https://crates.io/crates/octocrab) | GitHub API client |
| [wikipedia](https://crates.io/crates/wikipedia) | Wikipedia API client |
| [yt-transcript-rs](https://crates.io/crates/yt-transcript-rs) | YouTube transcript extraction |
| [rusty_ytdl](https://crates.io/crates/rusty_ytdl) | YouTube video metadata |
| [agent-twitter-client](https://crates.io/crates/agent-twitter-client) | X (Twitter) client |
| [rookie](https://crates.io/crates/rookie) | Browser cookie extraction |
| [graph-rs-sdk](https://crates.io/crates/graph-rs-sdk) | Microsoft Graph API |
| [google-drive3](https://crates.io/crates/google-drive3) | Google Drive API |
| [google-gmail1](https://crates.io/crates/google-gmail1) | Gmail API |
| [google-calendar3](https://crates.io/crates/google-calendar3) | Google Calendar API |
| [lettre](https://crates.io/crates/lettre) | SMTP client for outbound email |
| [scraper](https://crates.io/crates/scraper) | HTML parsing |
| [htmd](https://crates.io/crates/htmd) | HTML to Markdown conversion |
| [quick-xml](https://crates.io/crates/quick-xml) | XML parsing for ArXiv/PubMed |
| [rmcp](https://crates.io/crates/rmcp) | Model Context Protocol |
| [reqwest](https://crates.io/crates/reqwest) | HTTP client |
| [tokio](https://crates.io/crates/tokio) | Async runtime |

Thank you to all the maintainers and contributors of these projects.

## License

Licensed under the GNU Affero General Public License v3.0 only ([LICENSE](LICENSE)).
