use crate::commands::{CommandError, Result};
use serde_json::{json, Map, Value};

pub fn generic_get_tool_and_args(
    connector: &str,
    id: &str,
) -> Result<(&'static str, Map<String, Value>)> {
    match connector {
        "youtube" => Ok((
            "get",
            json!({ "video_id": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "reddit" => {
            let post_url = if id.starts_with("http://") || id.starts_with("https://") {
                id.to_string()
            } else {
                format!("https://www.reddit.com/comments/{}", id)
            };
            Ok((
                "get",
                json!({ "post_url": post_url })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ))
        }
        "play-store" | "playstore" | "play_store" => Ok((
            "app",
            json!({ "id": id, "hl": "en", "gl": "US" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "app-store" | "appstore" | "itunes" => {
            let track_id = id.parse::<u64>().map_err(|_| {
                CommandError::InvalidInput("App Store track IDs must be numeric.".to_string())
            })?;
            Ok((
                "lookup",
                json!({ "track_id": track_id, "country": "US" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ))
        }
        "app-store-connect" | "asc" | "appstoreconnect" | "app_store_connect" => Ok((
            "get_app",
            json!({ "app_id": id }).as_object().expect("json object").clone(),
        )),
        "hackernews" => {
            let parsed = id.parse::<u64>().map_err(|_| {
                CommandError::InvalidInput("Hacker News IDs must be numeric.".to_string())
            })?;
            Ok((
                "get_thread",
                json!({ "id": parsed, "max_comments": 20, "response_format": "compact" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ))
        }
        "wikipedia" => Ok((
            "get",
            json!({ "title": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "weather" | "wttr" => Ok((
            "get_weather",
            json!({ "location": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "arxiv" => Ok((
            "get",
            json!({ "paper_id": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "pubmed" => Ok((
            "get",
            json!({ "pmid": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "polymarket" => {
            let (tool, args) = if id.starts_with("http://") || id.starts_with("https://") {
                ("get", json!({ "url": id, "output_format": "display_v1" }))
            } else if let Some(series_id) = id.strip_prefix("polymarket:series:") {
                ("get_series", json!({ "id": series_id }))
            } else if id.starts_with("polymarket:market:") {
                (
                    "get_market",
                    json!({ "item_ref": id, "output_format": "display_v1" }),
                )
            } else if id.starts_with("polymarket:event:") {
                ("get", json!({ "item_ref": id, "output_format": "display_v1" }))
            } else if id.starts_with("polymarket:") {
                return Err(CommandError::InvalidInput(format!(
                    "Unsupported Polymarket item_ref '{}'. Expected polymarket:event:<id>, polymarket:market:<id>, or polymarket:series:<id>.",
                    id
                )));
            } else if id.chars().all(|c| c.is_ascii_digit()) {
                ("get", json!({ "id": id, "output_format": "display_v1" }))
            } else {
                ("get", json!({ "slug": id, "output_format": "display_v1" }))
            };
            Ok((tool, args.as_object().expect("json object").clone()))
        }
        "kalshi" => {
            let (tool, args) = if id.starts_with("http://") || id.starts_with("https://") {
                ("get", json!({ "url": id, "output_format": "display_v1" }))
            } else if let Some(series_ticker) = id.strip_prefix("kalshi:series:") {
                ("get_series", json!({ "ticker": series_ticker, "output_format": "display_v1" }))
            } else if id.starts_with("kalshi:market:") {
                (
                    "get_market",
                    json!({ "item_ref": id, "output_format": "display_v1" }),
                )
            } else if id.starts_with("kalshi:event:") {
                ("get", json!({ "item_ref": id, "output_format": "display_v1" }))
            } else if id.starts_with("kalshi:") {
                return Err(CommandError::InvalidInput(format!(
                    "Unsupported Kalshi item_ref '{}'. Expected kalshi:event:<ticker>, kalshi:market:<ticker>, or kalshi:series:<ticker>.",
                    id
                )));
            } else {
                ("get", json!({ "ticker": id, "output_format": "display_v1" }))
            };
            Ok((tool, args.as_object().expect("json object").clone()))
        }
        "semantic-scholar" | "semantic_scholar" => Ok((
            "get_paper_details",
            json!({ "paper_id": id, "response_format": "detailed" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "github" => {
            let (owner, repo) = id.split_once('/').ok_or_else(|| {
                CommandError::InvalidInput("GitHub IDs must be in owner/repo form.".to_string())
            })?;
            Ok((
                "get_repository",
                json!({ "owner": owner, "repo": repo })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ))
        }
        _ => Err(CommandError::InvalidInput(format!(
            "Connector '{}' is not supported by the generic `get` command. Use `rzn-tools {0} --help` or `rzn-tools tools {0}`.",
            connector
        ))),
    }
}

pub fn generic_search_tool_and_args(
    connector: &str,
    query: &str,
    limit: u32,
) -> Result<(&'static str, Map<String, Value>)> {
    match connector {
        "youtube" => Ok((
            "search",
            json!({ "query": query, "limit": limit })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "reddit" => Ok((
            "search",
            json!({ "query": query, "limit": limit })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "hackernews" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "response_format": "compact" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "wikipedia" => Ok((
            "search",
            json!({ "query": query, "limit": limit })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "arxiv" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "response_format": "concise" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "pubmed" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "response_format": "concise" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "polymarket" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "output_format": "display_v1" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "kalshi" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "output_format": "display_v1" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "semantic-scholar" | "semantic_scholar" => Ok((
            "search_papers",
            json!({ "query": query, "limit": limit, "response_format": "concise" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "github" => Ok((
            "search_repositories",
            json!({ "query": query, "per_page": limit, "response_format": "concise" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "app-store" | "appstore" | "itunes" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "country": "US" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "x" | "twitter" => Ok((
            "search_recent_tweets",
            json!({ "query": query, "limit": limit })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        "xai-search" => Ok((
            "search",
            json!({ "query": query, "limit": limit, "response_format": "concise" })
                .as_object()
                .expect("json object")
                .clone(),
        )),
        _ => Err(CommandError::InvalidInput(format!(
            "Connector '{}' is not supported by the generic `search` command. Use `rzn-tools {0} --help` or `rzn-tools tools {0}`.",
            connector
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polymarket_market_item_ref_routes_to_get_market() {
        let (tool, args) = generic_get_tool_and_args("polymarket", "polymarket:market:1739838")
            .expect("polymarket market route");

        assert_eq!(tool, "get_market");
        assert_eq!(
            args.get("item_ref").and_then(Value::as_str),
            Some("polymarket:market:1739838")
        );
    }

    #[test]
    fn polymarket_series_item_ref_routes_to_get_series() {
        let (tool, args) = generic_get_tool_and_args("polymarket", "polymarket:series:10007")
            .expect("polymarket series route");

        assert_eq!(tool, "get_series");
        assert_eq!(args.get("id").and_then(Value::as_str), Some("10007"));
    }

    #[test]
    fn kalshi_market_item_ref_routes_to_get_market() {
        let (tool, args) = generic_get_tool_and_args("kalshi", "kalshi:market:KXELONMARS-99")
            .expect("kalshi market route");

        assert_eq!(tool, "get_market");
        assert_eq!(
            args.get("item_ref").and_then(Value::as_str),
            Some("kalshi:market:KXELONMARS-99")
        );
    }

    #[test]
    fn kalshi_series_item_ref_routes_to_get_series() {
        let (tool, args) = generic_get_tool_and_args("kalshi", "kalshi:series:KXELONMARS")
            .expect("kalshi series route");

        assert_eq!(tool, "get_series");
        assert_eq!(
            args.get("ticker").and_then(Value::as_str),
            Some("KXELONMARS")
        );
    }
}
