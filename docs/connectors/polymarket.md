# Polymarket Connector (`polymarket`)

The `polymarket` connector exposes public read-only Polymarket data for discovery, retrieval,
discussion, and market analysis.

It is intentionally task-shaped rather than endpoint-shaped. The connector covers the public data
agents actually use:
- discovery across tags, events, markets, and series
- direct entity reads for events, markets, and series
- comment and discussion context
- market microstructure such as order books, price history, and public holder data
- one bundled high-context tool that assembles the important market context in a single response

It does not expose trading, order placement, wallet actions, or other authenticated account flows.

## Tool Belt

| Group | Tools | When to use |
|------|-------|-------------|
| Discovery | `search`, `list_tags`, `list_events`, `list_markets`, `list_series` | Find relevant markets by topic, discover valid tag slugs, browse by series or tag, or expand one event into all related contracts |
| Entity Reads | `get`, `get_market`, `get_series` | Open one event, market, or series when you already know the identifier |
| Discussion | `list_comments` | Pull recent comments tied to an event, market, or series |
| Market Analysis | `order_book`, `price_history`, `market_positions` | Inspect liquidity, trajectory, and public holder positioning |
| Bundled Context | `get_market_context` | Pull one market plus linked event, order books, price history, and optional positions in a single call |

## Task Mapping

| Need | Tool |
|------|------|
| Search by topic or entity name | `search` |
| Discover valid tag slugs before filtering | `list_tags` |
| Browse active events in a tag or series | `list_events` |
| Flatten an event or series into all of its tradable markets | `list_markets` |
| Understand a recurring category such as inflation or a sports league | `list_series`, `get_series` |
| Open an event URL like `https://polymarket.com/event/<slug>` | `get` |
| Inspect one exact contract | `get_market` |
| Pull discussion context before summarizing sentiment | `list_comments` |
| Check book depth or best bid/ask by outcome | `order_book` |
| Review price movement over time | `price_history` |
| Inspect public holder concentration and PnL | `market_positions` |
| Hand an agent one analysis-ready bundle | `get_market_context` |

## Quick Start

```bash
rzn-tools search polymarket "bitcoin" --limit 10
rzn-tools get polymarket 312712
rzn-tools fetch https://polymarket.com/event/cbb-pur-arz-2026-03-28
rzn-tools tools polymarket
rzn-tools polymarket list-tags --limit 20
rzn-tools polymarket market-context --slug cbb-pur-arz-2026-03-28 --include-positions
```

## Wrapped Tools

These tools intentionally wrap low-level Polymarket API details:

- `list_comments` resolves event URLs, slugs, and normalized `item_ref`s before calling the
  comments API.
- `order_book` and `price_history` resolve outcome token ids from the market metadata, so callers
  can work at the market or outcome level instead of dealing with raw CLOB token ids.
- `get_market_context` composes market detail, parent event detail, order books, price history, and
  optional public holder data into one response that is easier for agents to reason over.

## CLI Workflows

For the richer Polymarket-only flows, use the dedicated wrapper:

```bash
rzn-tools polymarket list-tags --limit 20
rzn-tools polymarket list-events --tag-slug crypto --active --limit 10
rzn-tools polymarket list-markets --event-slug cbb-pur-arz-2026-03-28 --limit 20
rzn-tools polymarket order-book --slug cbb-pur-arz-2026-03-28 --depth 5
rzn-tools polymarket price-history --slug cbb-pur-arz-2026-03-28 --interval 1d --fidelity 60
rzn-tools polymarket market-context --slug cbb-pur-arz-2026-03-28 --include-positions
```

## Example Calls

From MCP or launcher tool-calling, the most useful Polymarket calls usually look like:

```json
{
  "tool": "polymarket/list_tags",
  "arguments": {
    "limit": 20
  }
}
```

```json
{
  "tool": "polymarket/list_events",
  "arguments": {
    "tag_slug": "crypto",
    "active": true,
    "limit": 10
  }
}
```

```json
{
  "tool": "polymarket/list_markets",
  "arguments": {
    "event_slug": "cbb-pur-arz-2026-03-28",
    "limit": 20
  }
}
```

```json
{
  "tool": "polymarket/get_market_context",
  "arguments": {
    "slug": "cbb-pur-arz-2026-03-28",
    "include_positions": true,
    "positions_limit": 10
  }
}
```

## Notes

- No authentication is required.
- `search` paginates internally because Polymarket's public search currently returns small fixed
  pages.
- `list_tags`, `list_events`, `list_markets`, `list_series`, and `list_comments` expose opaque cursors for
  pagination.
- `rzn-tools polymarket ...` exposes the non-generic list and analysis flows directly from the CLI.
- `get` accepts frontend event URLs, event slugs, numeric ids, and normalized `item_ref` values like `polymarket:event:312712`.
- `get_market` accepts market slugs, numeric ids, and normalized `item_ref` values like `polymarket:market:1739838`.
- Generic `rzn-tools get polymarket polymarket:market:<id>` routes to `get_market`, while
  `polymarket:series:<id>` routes to `get_series`.
- Generic `rzn-tools get polymarket <numeric-id>` now probes event, market, and series reads and only
  resolves automatically when the match is unique. If the numeric id exists in multiple Polymarket
  namespaces, rzn-tools now returns an explicit ambiguity error instead of guessing.
- Use `output_format=normalized_v1` or `display_v1` when integrating with ingestion/UI pipelines.
