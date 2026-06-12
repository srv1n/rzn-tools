# Kalshi Connector (`kalshi`)

The `kalshi` connector exposes public read-only Kalshi data for discovery, retrieval, market
microstructure analysis, and bundled high-context market reads.

It is intentionally task-shaped rather than endpoint-shaped. The connector covers the public data
agents actually use:
- discovery across series, events, and markets
- direct entity reads for series, events, and markets
- event metadata such as settlement sources and imagery
- market microstructure such as order books, candles, and recent trades
- one bundled high-context tool that assembles market, event, series, and routing context in a
  single response

It does not expose trading, order placement, wallet/account actions, or authenticated portfolio
flows.

## Tool Belt

| Group | Tools | When to use |
|------|-------|-------------|
| Discovery | `search`, `list_series`, `list_events`, `list_markets` | Find relevant contracts by topic, browse the series catalog, inspect event catalogs, or browse live/historical markets |
| Entity Reads | `get_series`, `get`, `get_market` | Open one exact series, event, or market when you already know the identifier |
| Event Context | `get_event_metadata`, `event_candlesticks` | Pull settlement sources, imagery, and event-level price movement |
| Market Analysis | `order_book`, `market_candlesticks`, `list_trades` | Inspect liquidity, price movement, and recent public trades |
| Bundled Context | `get_market_context` | Pull one market plus parent event, parent series, candles, trades, and routing metadata in a single call |

## Task Mapping

| Need | Tool |
|------|------|
| Search by topic or entity name | `search` |
| Browse the series catalog | `list_series` |
| Open a recurring category with recent child events | `get_series` |
| Browse events for one series | `list_events` |
| Open a Kalshi event page URL | `get` |
| Pull event settlement sources and imagery | `get_event_metadata` |
| Browse all markets under one event or series | `list_markets` |
| Inspect one exact contract | `get_market` |
| Check live bid/ask depth | `order_book` |
| Review market-level price movement | `market_candlesticks` |
| Review event-level candles across child markets | `event_candlesticks` |
| Inspect recent trade flow | `list_trades` |
| Hand an agent one analysis-ready bundle | `get_market_context` |

## Quick Start

```bash
rzn-tools search kalshi "elon mars" --limit 10
rzn-tools get kalshi KXELONMARS
rzn-tools fetch https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99
rzn-tools tools kalshi
rzn-tools kalshi get-event --ticker KXELONMARS-99
rzn-tools kalshi market-context --ticker KXELONMARS-99
```

## Wrapped Tools

These tools intentionally hide some lower-level Kalshi protocol details:

- `get_market` falls back from the live market endpoint to the historical market endpoint when a
  contract has moved behind Kalshi's public historical cutoff.
- `market_candlesticks` and `list_trades` route between live and historical storage automatically,
  so callers usually do not need to reason about cutoff timestamps themselves.
- `get_market_context` composes the market, parent event, parent series, candles, recent trades,
  live book snapshot, and optional event metadata into one response that is easier for agents to
  reason over.

## CLI Workflows

For the richer Kalshi-only flows, use the dedicated wrapper:

```bash
rzn-tools kalshi search --query "fed rates" --limit 10
rzn-tools kalshi list-series --limit 20
rzn-tools kalshi list-events --series-ticker KXELONMARS --limit 10
rzn-tools kalshi get-event --ticker KXELONMARS-99
rzn-tools kalshi event-metadata --ticker KXELONMARS-99
rzn-tools kalshi list-markets --event-ticker KXELONMARS-99 --limit 20
rzn-tools kalshi order-book --ticker KXELONMARS-99 --depth 10
rzn-tools kalshi list-trades --ticker KXELONMARS-99 --limit 20
rzn-tools kalshi market-context --ticker KXELONMARS-99
```

## Example Calls

From MCP or launcher tool-calling, the most useful Kalshi calls usually look like:

```json
{
  "tool": "kalshi/list_series",
  "arguments": {
    "limit": 20
  }
}
```

```json
{
  "tool": "kalshi/get",
  "arguments": {
    "url": "https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99"
  }
}
```

```json
{
  "tool": "kalshi/list_markets",
  "arguments": {
    "event_ticker": "KXELONMARS-99",
    "limit": 20
  }
}
```

```json
{
  "tool": "kalshi/get_market_context",
  "arguments": {
    "ticker": "KXELONMARS-99",
    "trades_limit": 20,
    "include_event_metadata": true
  }
}
```

## Notes

- No authentication is required.
- `list_series` uses client-side cursor pagination because the public series catalog is exposed as
  one list rather than a cursor-paginated endpoint.
- `list_events`, `list_markets`, and `list_trades` expose opaque cursors for pagination.
- `get` accepts Kalshi event URLs, raw event tickers, and normalized `item_ref` values like
  `kalshi:event:KXELONMARS-99`.
- `get_market` accepts raw market tickers and normalized `item_ref` values like
  `kalshi:market:KXELONMARS-99`.
- Generic `rzn-tools get kalshi kalshi:market:<ticker>` routes to `get_market`, while
  `kalshi:series:<ticker>` routes to `get_series`.
- Generic `rzn-tools get kalshi <ticker>` now probes event, market, and series reads and only resolves
  automatically when the match is unique. If a ticker exists in multiple Kalshi namespaces, rzn-tools
  returns an explicit ambiguity error instead of guessing.
- Use `output_format=normalized_v1` or `display_v1` when integrating with ingestion/UI pipelines.
