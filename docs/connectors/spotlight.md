# Spotlight

Connector name: `spotlight`

Spotlight is available only on macOS.

| Tool | Use |
| --- | --- |
| `search` | Search by content, name, kind, recent date, or a raw Spotlight query. |
| `get_metadata` | Read Spotlight metadata for one path. |

Older names such as `search_content`, `search_by_name`, and
`search_by_kind` remain call aliases. They are not the canonical catalog.

`recent` uses calendar-day query syntax, not a rolling 24-hour window.
Spotlight is separate from `localfs`: Spotlight queries the macOS metadata
index; localfs reads the supplied path directly.

Code: `rzn_tools_core/src/connectors/spotlight/mod.rs`
