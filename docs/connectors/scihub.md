# SciHub (Open Access) Connector

Status: Stable

## Overview

The SciHub connector provides best-effort open-access paper lookup by DOI. It queries **OpenAlex** (and optionally **Unpaywall**) to find freely available versions of academic papers.

**Important:** This connector does NOT bypass paywalls. It only returns openly available locations (e.g., preprints, author manuscripts, or publisher open-access versions) when they exist.

## Key Use Cases

- Find open-access PDF links for papers you have the DOI for
- Get paper metadata (title, authors, year, journal)
- Check if a paper has any freely available version before purchasing
- Build citation/reference workflows that link to free versions when available

## Tools

### `get` / `get_paper`

Best-effort open-access lookup by DOI.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `doi` | string | Yes | The DOI (Digital Object Identifier) of the paper |

**Response:**
```json
{
  "doi": "10.1371/journal.pone.0000308",
  "pdf_url": "https://journals.plos.org/plosone/article/file?id=10.1371/journal.pone.0000308&type=printable",
  "title": "Sharing Detailed Research Data Is Associated with Increased Citation Rate",
  "authors": "Heather Piwowar, Roger Day, Douglas B. Fridsma",
  "journal": null,
  "year": "2007",
  "success": true,
  "message": "Found open-access PDF via OpenAlex"
}
```

**Fields:**
| Field | Description |
|-------|-------------|
| `doi` | The DOI that was queried |
| `pdf_url` | Direct link to PDF if found (null otherwise) |
| `title` | Paper title |
| `authors` | Comma-separated author names |
| `journal` | Journal name (may be null) |
| `year` | Publication year |
| `success` | `true` if an open-access PDF was found |
| `message` | Status message (source of result or reason for failure) |

### `search`

Search for papers by title, author, or keywords via OpenAlex.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `query` | string | Yes | — | Search query (title, author, keywords) |
| `limit` | integer | No | 10 | Maximum results to return (max 200) |
| `page` | integer | No | 1 | Page number for pagination |
| `oa_only` | boolean | No | false | If true, only return open-access works |

**Response:** Array of `SciHubResult` objects (same fields as `get`).

### `batch_get`

Look up multiple DOIs concurrently. Failed DOIs return `success: false` instead of aborting the batch.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `dois` | array of strings | Yes | DOIs to look up (max 50) |

**Response:** Array of `SciHubResult` objects (same fields as `get`).

## Authentication

**No authentication required** for basic usage via OpenAlex.

For improved results, you can optionally configure an email for the Unpaywall API:

### Option 1: Environment Variable
```bash
export UNPAYWALL_EMAIL="your.email@example.com"
```

### Option 2: Interactive Setup
```bash
rzn-tools setup scihub
```

### Option 3: Direct Configuration
```bash
rzn-tools config set scihub --key unpaywall_email --value "your.email@example.com"
```

When an Unpaywall email is configured, the connector queries Unpaywall first (better open-access coverage) and falls back to OpenAlex if Unpaywall fails.

## CLI Usage

### Basic Lookup
```bash
# Look up a paper by DOI
rzn-tools scihub paper --doi "10.1371/journal.pone.0000308"

# Output as JSON
rzn-tools scihub paper --doi "10.1038/nature12373" --output json

# Copy PDF URL to clipboard
rzn-tools scihub paper --doi "10.48550/arXiv.1706.03762" --copy
```

### Search
```bash
# Search by topic
rzn-tools scihub search --query "attention mechanism" --limit 5

# Search open-access only
rzn-tools scihub search --query "CRISPR" --oa-only --limit 10

# Paginate results
rzn-tools scihub search --query "machine learning" --page 2
```

### Batch Lookup
```bash
# Look up multiple DOIs at once
rzn-tools scihub batch --dois "10.1038/nature12373,10.1371/journal.pone.0000308"
```

### Example Output
```
Tool scihub.get

doi: 10.1371/journal.pone.0000308
pdf_url: https://journals.plos.org/plosone/article/file?id=10.1371/journal.pone.0000308&type=printable
title: Sharing Detailed Research Data Is Associated with Increased Citation Rate
authors: Heather Piwowar, Roger Day, Douglas B. Fridsma
journal: -
year: 2007
success: true
message: Found open-access PDF via OpenAlex
```

## MCP Usage

When using rzn-tools as an MCP server, the tool is exposed as `scihub/get`:

```json
{
  "name": "scihub/get",
  "arguments": {
    "doi": "10.1371/journal.pone.0000308"
  }
}
```

## Data Sources

### OpenAlex (Default)
- Free, open scholarly metadata database
- Covers 250M+ works
- No authentication required
- Rate limits: 100,000 requests/day (polite pool with email)

### Unpaywall (Optional)
- Requires email registration (free)
- Higher-quality open-access detection
- Better coverage of author manuscripts and repository copies
- Rate limits: 100,000 requests/day

## Common DOI Formats

The connector accepts DOIs in various formats:

```bash
# Standard DOI
rzn-tools scihub paper --doi "10.1038/nature12373"

# arXiv DOI
rzn-tools scihub paper --doi "10.48550/arXiv.1706.03762"

# Journal-specific DOI
rzn-tools scihub paper --doi "10.1016/j.cell.2023.01.001"
```

## Error Handling

| Scenario | Response |
|----------|----------|
| Invalid DOI | `success: false`, message explains the error |
| DOI not found | `success: false`, "OpenAlex lookup failed: HTTP status 404" |
| No open-access version | `success: false`, "No open-access copy found via OpenAlex" |
| OA found but no PDF URL | `success: false`, "Open-access work found, but no direct PDF URL available" |

## Programmatic Usage (Rust)

```rust
use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::connectors::scihub::SciHubConnector;
use rzn_tools_core::{CallToolRequestParam, Connector};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connector = SciHubConnector::new(AuthDetails::new()).await?;

    let response = connector
        .call_tool(CallToolRequestParam {
            name: "get".into(),
            arguments: Some(
                json!({ "doi": "10.1371/journal.pone.0000308" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        })
        .await?;

    println!("{:?}", response.structured_content);
    Ok(())
}
```

## Build Requirements

The SciHub connector is included in the `full` feature set:

```bash
# Build with all connectors
cargo build --release --package rzn_tools_cli --features "full"

# Or build with just scihub
cargo build --release --package rzn_tools_cli --features "scihub"
```

## Limitations

- **Does not bypass paywalls**: Only returns legally available open-access copies
- **PDF availability varies**: Even open-access papers may not have direct PDF links
- **Journal field often null**: OpenAlex uses a different venue model; journal name may be missing
- **Batch limit**: Maximum 50 DOIs per batch request

## Related Connectors

For a complete academic research workflow, combine with:

| Connector | Use Case |
|-----------|----------|
| `arxiv` | Search and get preprints by arXiv ID |
| `pubmed` | Search biomedical literature |
| `semantic_scholar` | Citation graphs and related papers |
| `google_scholar` | Broad paper search (unofficial) |

## See Also

- [OpenAlex API Documentation](https://docs.openalex.org/)
- [Unpaywall API Documentation](https://unpaywall.org/products/api)
- [rzn-tools Connector Reference](../CONNECTORS.md)
