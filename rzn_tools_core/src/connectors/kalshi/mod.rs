use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat, Partial,
    Relationship, Source,
};
use crate::utils::{build_reqwest_client, structured_result, structured_result_with_text};
use crate::{
    CallToolRequestParam, CallToolResult, Connector, Implementation, InitializeRequestParam,
    InitializeResult, ListPromptsResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParam, Prompt, ProtocolVersion, ReadResourceRequestParam, ResourceContents,
    ServerCapabilities, Tool, URLParamExtraction, URLPatternSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const USER_AGENT: &str = "rzn-tools-kalshi-connector/0.1.0";
const API_BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
const DEFAULT_LIST_LIMIT: u32 = 20;
const DEFAULT_SEARCH_LIMIT: u32 = 10;
const DEFAULT_ORDER_BOOK_DEPTH: usize = 10;
const DEFAULT_PERIOD_INTERVAL: u32 = 60;
const DEFAULT_TRADES_LIMIT: u32 = 20;
const DEFAULT_RELATED_EVENTS_LIMIT: u32 = 10;
const MAX_LIST_LIMIT: u32 = 100;
const MAX_SEARCH_LIMIT: u32 = 50;
const MAX_SEARCH_SCAN_PAGES: u32 = 3;
const CONTEXT_WINDOW_SECONDS: i64 = 86_400;

static KALSHI_EVENT_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:https?://)?(?:www\.)?kalshi\.com/markets(?:/[^/?#]+)+/?(?:[?#].*)?$")
        .expect("valid kalshi event url regex")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeriesCursor {
    offset: usize,
    status: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum MarketSource {
    Live,
    Historical,
}

impl MarketSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Historical => "historical",
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: u32,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct ListSeriesArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetSeriesArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default = "default_related_events_limit")]
    events_limit: u32,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct ListEventsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    series_ticker: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    multivariate: bool,
    #[serde(default)]
    collection_ticker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetEventArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct EventMetadataArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventCandlesticksArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    series_ticker: Option<String>,
    start_ts: i64,
    #[serde(default)]
    end_ts: Option<i64>,
    #[serde(default = "default_period_interval")]
    period_interval: u32,
}

#[derive(Debug, Deserialize)]
struct ListMarketsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    series_ticker: Option<String>,
    #[serde(default)]
    event_ticker: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    historical: bool,
}

#[derive(Debug, Deserialize)]
struct GetMarketArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct OrderBookArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default = "default_order_book_depth")]
    depth: usize,
}

#[derive(Debug, Deserialize)]
struct MarketCandlesticksArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    start_ts: i64,
    #[serde(default)]
    end_ts: Option<i64>,
    #[serde(default = "default_period_interval")]
    period_interval: u32,
}

#[derive(Debug, Deserialize)]
struct ListTradesArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default = "default_trades_limit")]
    limit: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    min_ts: Option<i64>,
    #[serde(default)]
    max_ts: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MarketContextArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    start_ts: Option<i64>,
    #[serde(default)]
    end_ts: Option<i64>,
    #[serde(default = "default_period_interval")]
    period_interval: u32,
    #[serde(default = "default_order_book_depth")]
    orderbook_depth: usize,
    #[serde(default = "default_trades_limit")]
    trades_limit: u32,
    #[serde(default = "default_true")]
    include_event_metadata: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HistoricalCutoff {
    market_settled_ts: String,
    orders_updated_ts: String,
    trades_created_ts: String,
}

fn default_true() -> bool {
    true
}

fn default_list_limit() -> u32 {
    DEFAULT_LIST_LIMIT
}

fn default_search_limit() -> u32 {
    DEFAULT_SEARCH_LIMIT
}

fn default_order_book_depth() -> usize {
    DEFAULT_ORDER_BOOK_DEPTH
}

fn default_period_interval() -> u32 {
    DEFAULT_PERIOD_INTERVAL
}

fn default_trades_limit() -> u32 {
    DEFAULT_TRADES_LIMIT
}

fn default_related_events_limit() -> u32 {
    DEFAULT_RELATED_EVENTS_LIMIT
}

pub struct KalshiConnector {
    client: Client,
}

impl KalshiConnector {
    pub async fn new() -> Result<Self, ConnectorError> {
        let client = build_reqwest_client(|| {
            Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(USER_AGENT)
        })?;
        Ok(Self { client })
    }

    fn clamp_limit(limit: u32) -> u32 {
        limit.clamp(1, MAX_LIST_LIMIT)
    }

    fn clamp_search_limit(limit: u32) -> u32 {
        limit.clamp(1, MAX_SEARCH_LIMIT)
    }

    async fn execute_json(
        &self,
        request: reqwest::RequestBuilder,
        context: &str,
    ) -> Result<Value, ConnectorError> {
        let response = request.send().await.map_err(ConnectorError::HttpRequest)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Kalshi {} failed with status {}",
                context,
                response.status()
            )));
        }
        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_series_all(&self, status: Option<&str>) -> Result<Vec<Value>, ConnectorError> {
        let mut request = self.client.get(format!("{API_BASE_URL}/series"));
        if let Some(status) = status.filter(|value| !value.trim().is_empty()) {
            request = request.query(&[("status", status)]);
        }
        let response = self.execute_json(request, "series listing").await?;
        Ok(response
            .get("series")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    async fn fetch_events_page(
        &self,
        args: &ListEventsArgs,
    ) -> Result<(Vec<Value>, Option<String>), ConnectorError> {
        let endpoint = if args.multivariate {
            format!("{API_BASE_URL}/events/multivariate")
        } else {
            format!("{API_BASE_URL}/events")
        };
        let mut query: Vec<(String, String)> = vec![(
            "limit".to_string(),
            Self::clamp_limit(args.limit).to_string(),
        )];
        if let Some(cursor) = args.cursor.as_ref() {
            query.push(("cursor".to_string(), cursor.clone()));
        }
        if let Some(series_ticker) = args.series_ticker.as_ref() {
            query.push(("series_ticker".to_string(), normalize_ticker(series_ticker)));
        }
        if let Some(status) = args.status.as_ref() {
            query.push(("status".to_string(), status.clone()));
        }
        if let Some(collection_ticker) = args.collection_ticker.as_ref() {
            query.push((
                "collection_ticker".to_string(),
                normalize_ticker(collection_ticker),
            ));
        }

        let response = self
            .execute_json(self.client.get(endpoint).query(&query), "events listing")
            .await?;
        Ok((
            response
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            string_field(&response, "cursor"),
        ))
    }

    async fn fetch_event(&self, ticker: &str) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client.get(format!(
                "{API_BASE_URL}/events/{}",
                normalize_ticker(ticker)
            )),
            "event lookup",
        )
        .await
    }

    async fn fetch_event_metadata(&self, ticker: &str) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client.get(format!(
                "{API_BASE_URL}/events/{}/metadata",
                normalize_ticker(ticker)
            )),
            "event metadata lookup",
        )
        .await
    }

    async fn fetch_event_candlesticks(
        &self,
        series_ticker: &str,
        event_ticker: &str,
        start_ts: i64,
        end_ts: i64,
        period_interval: u32,
    ) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!(
                    "{API_BASE_URL}/series/{}/events/{}/candlesticks",
                    normalize_ticker(series_ticker),
                    normalize_ticker(event_ticker)
                ))
                .query(&[
                    ("start_ts", start_ts.to_string()),
                    ("end_ts", end_ts.to_string()),
                    ("period_interval", period_interval.to_string()),
                ]),
            "event candlesticks lookup",
        )
        .await
    }

    async fn fetch_markets_page_live(
        &self,
        args: &ListMarketsArgs,
    ) -> Result<(Vec<Value>, Option<String>), ConnectorError> {
        let mut query: Vec<(String, String)> = vec![(
            "limit".to_string(),
            Self::clamp_limit(args.limit).to_string(),
        )];
        if let Some(cursor) = args.cursor.as_ref() {
            query.push(("cursor".to_string(), cursor.clone()));
        }
        if let Some(series_ticker) = args.series_ticker.as_ref() {
            query.push(("series_ticker".to_string(), normalize_ticker(series_ticker)));
        }
        if let Some(event_ticker) = args.event_ticker.as_ref() {
            query.push(("event_ticker".to_string(), normalize_ticker(event_ticker)));
        }
        if let Some(status) = args.status.as_ref() {
            query.push(("status".to_string(), status.clone()));
        }

        let response = self
            .execute_json(
                self.client
                    .get(format!("{API_BASE_URL}/markets"))
                    .query(&query),
                "markets listing",
            )
            .await?;
        Ok((
            response
                .get("markets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            string_field(&response, "cursor"),
        ))
    }

    async fn fetch_markets_page_historical(
        &self,
        args: &ListMarketsArgs,
    ) -> Result<(Vec<Value>, Option<String>), ConnectorError> {
        let mut query: Vec<(String, String)> = vec![(
            "limit".to_string(),
            Self::clamp_limit(args.limit).to_string(),
        )];
        if let Some(cursor) = args.cursor.as_ref() {
            query.push(("cursor".to_string(), cursor.clone()));
        }
        if let Some(series_ticker) = args.series_ticker.as_ref() {
            query.push(("series_ticker".to_string(), normalize_ticker(series_ticker)));
        }
        if let Some(event_ticker) = args.event_ticker.as_ref() {
            query.push(("event_ticker".to_string(), normalize_ticker(event_ticker)));
        }
        if let Some(status) = args.status.as_ref() {
            query.push(("status".to_string(), status.clone()));
        }

        let response = self
            .execute_json(
                self.client
                    .get(format!("{API_BASE_URL}/historical/markets"))
                    .query(&query),
                "historical markets listing",
            )
            .await?;
        Ok((
            response
                .get("markets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            string_field(&response, "cursor"),
        ))
    }

    async fn fetch_market_live(&self, ticker: &str) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client.get(format!(
                "{API_BASE_URL}/markets/{}",
                normalize_ticker(ticker)
            )),
            "market lookup",
        )
        .await
    }

    async fn fetch_market_historical(&self, ticker: &str) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client.get(format!(
                "{API_BASE_URL}/historical/markets/{}",
                normalize_ticker(ticker)
            )),
            "historical market lookup",
        )
        .await
    }

    async fn fetch_market_any(
        &self,
        ticker: &str,
    ) -> Result<(Value, MarketSource), ConnectorError> {
        match self.fetch_market_live(ticker).await {
            Ok(response) => Ok((response, MarketSource::Live)),
            Err(ConnectorError::ResourceNotFound) => {
                let historical = self.fetch_market_historical(ticker).await?;
                Ok((historical, MarketSource::Historical))
            }
            Err(err) => Err(err),
        }
    }

    async fn fetch_order_book(&self, ticker: &str) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client.get(format!(
                "{API_BASE_URL}/markets/{}/orderbook",
                normalize_ticker(ticker)
            )),
            "order book lookup",
        )
        .await
    }

    async fn fetch_market_candlesticks_live(
        &self,
        ticker: &str,
        start_ts: i64,
        end_ts: i64,
        period_interval: u32,
    ) -> Result<Value, ConnectorError> {
        let response = self
            .execute_json(
                self.client
                    .get(format!("{API_BASE_URL}/markets/candlesticks"))
                    .query(&[
                        ("market_tickers", normalize_ticker(ticker)),
                        ("start_ts", start_ts.to_string()),
                        ("end_ts", end_ts.to_string()),
                        ("period_interval", period_interval.to_string()),
                    ]),
                "market candlesticks lookup",
            )
            .await?;

        let first_market = response
            .get("markets")
            .and_then(Value::as_array)
            .and_then(|markets| markets.first())
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "market_ticker": normalize_ticker(ticker),
                    "candlesticks": []
                })
            });
        Ok(first_market)
    }

    async fn fetch_market_candlesticks_historical(
        &self,
        ticker: &str,
        start_ts: i64,
        end_ts: i64,
        period_interval: u32,
    ) -> Result<Value, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!(
                    "{API_BASE_URL}/historical/markets/{}/candlesticks",
                    normalize_ticker(ticker)
                ))
                .query(&[
                    ("start_ts", start_ts.to_string()),
                    ("end_ts", end_ts.to_string()),
                    ("period_interval", period_interval.to_string()),
                ]),
            "historical market candlesticks lookup",
        )
        .await
    }

    async fn fetch_trades_live(
        &self,
        ticker: &str,
        limit: u32,
        cursor: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
    ) -> Result<Value, ConnectorError> {
        let mut query = vec![
            ("ticker".to_string(), normalize_ticker(ticker)),
            ("limit".to_string(), Self::clamp_limit(limit).to_string()),
        ];
        if let Some(cursor) = cursor {
            query.push(("cursor".to_string(), cursor.to_string()));
        }
        if let Some(min_ts) = min_ts {
            query.push(("min_ts".to_string(), min_ts.to_string()));
        }
        if let Some(max_ts) = max_ts {
            query.push(("max_ts".to_string(), max_ts.to_string()));
        }

        self.execute_json(
            self.client
                .get(format!("{API_BASE_URL}/markets/trades"))
                .query(&query),
            "trades lookup",
        )
        .await
    }

    async fn fetch_trades_historical(
        &self,
        ticker: &str,
        limit: u32,
        cursor: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
    ) -> Result<Value, ConnectorError> {
        let mut query = vec![
            ("ticker".to_string(), normalize_ticker(ticker)),
            ("limit".to_string(), Self::clamp_limit(limit).to_string()),
        ];
        if let Some(cursor) = cursor {
            query.push(("cursor".to_string(), cursor.to_string()));
        }
        if let Some(min_ts) = min_ts {
            query.push(("min_ts".to_string(), min_ts.to_string()));
        }
        if let Some(max_ts) = max_ts {
            query.push(("max_ts".to_string(), max_ts.to_string()));
        }

        self.execute_json(
            self.client
                .get(format!("{API_BASE_URL}/historical/trades"))
                .query(&query),
            "historical trades lookup",
        )
        .await
    }

    async fn fetch_historical_cutoff(&self) -> Result<HistoricalCutoff, ConnectorError> {
        let value = self
            .execute_json(
                self.client.get(format!("{API_BASE_URL}/historical/cutoff")),
                "historical cutoff lookup",
            )
            .await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::Other(e.to_string()))
    }

    fn series_summary(series: &Value) -> Value {
        json!({
            "entity_type": "series",
            "item_ref": format!("kalshi:series:{}", string_field(series, "ticker").unwrap_or_default()),
            "ticker": string_field(series, "ticker"),
            "title": string_field(series, "title"),
            "category": string_field(series, "category"),
            "tags": array_of_strings(series.get("tags")),
            "frequency": string_field(series, "frequency"),
            "last_updated_ts": string_field(series, "last_updated_ts"),
        })
    }

    fn event_summary(event: &Value) -> Value {
        json!({
            "entity_type": "event",
            "item_ref": format!("kalshi:event:{}", string_field(event, "event_ticker").unwrap_or_default()),
            "ticker": string_field(event, "event_ticker"),
            "series_ticker": string_field(event, "series_ticker"),
            "title": string_field(event, "title"),
            "subtitle": string_field(event, "sub_title"),
            "category": string_field(event, "category"),
            "mutually_exclusive": event.get("mutually_exclusive").and_then(Value::as_bool),
            "last_updated_ts": string_field(event, "last_updated_ts"),
        })
    }

    fn market_summary(market: &Value) -> Value {
        json!({
            "entity_type": "market",
            "item_ref": format!("kalshi:market:{}", string_field(market, "ticker").unwrap_or_default()),
            "ticker": string_field(market, "ticker"),
            "event_ticker": string_field(market, "event_ticker"),
            "title": string_field(market, "title"),
            "market_type": string_field(market, "market_type"),
            "status": string_field(market, "status"),
            "result": string_field(market, "result"),
            "yes_bid_dollars": string_field(market, "yes_bid_dollars"),
            "yes_ask_dollars": string_field(market, "yes_ask_dollars"),
            "no_bid_dollars": string_field(market, "no_bid_dollars"),
            "no_ask_dollars": string_field(market, "no_ask_dollars"),
            "last_price_dollars": string_field(market, "last_price_dollars"),
            "volume_fp": string_field(market, "volume_fp"),
            "volume_24h_fp": string_field(market, "volume_24h_fp"),
            "open_interest_fp": string_field(market, "open_interest_fp"),
            "close_time": string_field(market, "close_time"),
            "settlement_ts": string_field(market, "settlement_ts"),
        })
    }

    fn entity_text(summary: &Value) -> String {
        let entity_type =
            string_field(summary, "entity_type").unwrap_or_else(|| "entity".to_string());
        let title = string_field(summary, "title").unwrap_or_else(|| {
            string_field(summary, "ticker").unwrap_or_else(|| "Untitled".to_string())
        });
        let ticker = string_field(summary, "ticker").unwrap_or_default();
        let mut parts = vec![title];
        if !ticker.is_empty() {
            parts.push(format!("[{ticker}]"));
        }
        if let Some(subtitle) = string_field(summary, "subtitle").filter(|value| !value.is_empty())
        {
            parts.push(subtitle);
        }
        if let Some(category) = string_field(summary, "category").filter(|value| !value.is_empty())
        {
            parts.push(format!("category: {category}"));
        }
        if let Some(status) = string_field(summary, "status").filter(|value| !value.is_empty()) {
            parts.push(format!("status: {status}"));
        }
        if let Some(series_ticker) =
            string_field(summary, "series_ticker").filter(|value| !value.is_empty())
        {
            parts.push(format!("series: {series_ticker}"));
        }
        if let Some(event_ticker) =
            string_field(summary, "event_ticker").filter(|value| !value.is_empty())
        {
            parts.push(format!("event: {event_ticker}"));
        }
        format!("{entity_type}: {}", parts.join(" | "))
    }

    fn summary_to_item(summary: &Value) -> Result<ContentItem, ConnectorError> {
        let item_ref = string_field(summary, "item_ref")
            .ok_or_else(|| ConnectorError::Other("Missing item_ref".to_string()))?;
        let entity_type =
            string_field(summary, "entity_type").unwrap_or_else(|| "entity".to_string());
        let title = string_field(summary, "title")
            .or_else(|| string_field(summary, "ticker"))
            .or_else(|| Some("Untitled".to_string()));

        let mut tags = array_of_strings(summary.get("tags"));
        if let Some(category) = string_field(summary, "category").filter(|value| !value.is_empty())
        {
            tags.push(category);
        }
        if let Some(status) = string_field(summary, "status").filter(|value| !value.is_empty()) {
            tags.push(status);
        }
        tags.push("prediction-markets".to_string());
        tags.push("kalshi".to_string());
        tags.sort();
        tags.dedup();

        let mut relationships = Vec::new();
        if entity_type == "event" {
            if let Some(series_ticker) =
                string_field(summary, "series_ticker").filter(|value| !value.is_empty())
            {
                relationships.push(Relationship {
                    rel: "part_of".to_string(),
                    from: item_ref.clone(),
                    to: format!("kalshi:series:{series_ticker}"),
                });
            }
        } else if entity_type == "market" {
            if let Some(event_ticker) =
                string_field(summary, "event_ticker").filter(|value| !value.is_empty())
            {
                relationships.push(Relationship {
                    rel: "part_of".to_string(),
                    from: item_ref.clone(),
                    to: format!("kalshi:event:{event_ticker}"),
                });
            }
        }

        Ok(ContentItem {
            item_ref: item_ref.clone(),
            kind: format!("kalshi_{entity_type}"),
            canonical_url: None,
            title,
            created_at: None,
            source_updated_at: string_field(summary, "last_updated_ts")
                .or_else(|| string_field(summary, "close_time")),
            authors: Vec::new(),
            tags,
            metadata: Some(summary.clone()),
            blocks: vec![ContentBlock {
                block_ref: format!("{item_ref}:summary"),
                block_kind: "summary".to_string(),
                text: Self::entity_text(summary),
                author: None,
                created_at: None,
                reply_to: None,
                position: None,
                score: summary.get("score").and_then(Value::as_f64),
                attachments: Vec::new(),
                metadata: Some(summary.clone()),
            }],
            relationships,
            truncation: None,
        })
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Value>, ConnectorError> {
        let mut results = Vec::new();
        let desired = Self::clamp_search_limit(limit) as usize;

        for series in self.fetch_series_all(None).await? {
            if let Some(score) =
                score_candidate(query, &[&series], &["title", "ticker", "category"])
            {
                let mut summary = Self::series_summary(&series);
                if let Some(object) = summary.as_object_mut() {
                    object.insert("score".to_string(), json!(score));
                }
                results.push(summary);
            }
        }

        let mut event_cursor = None;
        for _ in 0..MAX_SEARCH_SCAN_PAGES {
            let page_args = ListEventsArgs {
                limit: MAX_LIST_LIMIT,
                cursor: event_cursor.clone(),
                series_ticker: None,
                status: None,
                multivariate: false,
                collection_ticker: None,
            };
            let (events, next_cursor) = self.fetch_events_page(&page_args).await?;
            for event in events {
                if let Some(score) = score_candidate(
                    query,
                    &[&event],
                    &[
                        "title",
                        "sub_title",
                        "event_ticker",
                        "series_ticker",
                        "category",
                    ],
                ) {
                    let mut summary = Self::event_summary(&event);
                    if let Some(object) = summary.as_object_mut() {
                        object.insert("score".to_string(), json!(score));
                    }
                    results.push(summary);
                }
            }
            if next_cursor.is_none() {
                break;
            }
            event_cursor = next_cursor;
        }

        let mut market_cursor = None;
        for _ in 0..MAX_SEARCH_SCAN_PAGES {
            let page_args = ListMarketsArgs {
                limit: MAX_LIST_LIMIT,
                cursor: market_cursor.clone(),
                series_ticker: None,
                event_ticker: None,
                status: None,
                historical: false,
            };
            let (markets, next_cursor) = self.fetch_markets_page_live(&page_args).await?;
            for market in markets {
                if let Some(score) = score_candidate(
                    query,
                    &[&market],
                    &[
                        "title",
                        "ticker",
                        "event_ticker",
                        "market_type",
                        "status",
                        "rules_primary",
                        "rules_secondary",
                    ],
                ) {
                    let mut summary = Self::market_summary(&market);
                    if let Some(object) = summary.as_object_mut() {
                        object.insert("score".to_string(), json!(score));
                    }
                    results.push(summary);
                }
            }
            if next_cursor.is_none() {
                break;
            }
            market_cursor = next_cursor;
        }

        results.sort_by(|a, b| {
            let a_score = a.get("score").and_then(Value::as_f64).unwrap_or_default();
            let b_score = b.get("score").and_then(Value::as_f64).unwrap_or_default();
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(desired);
        Ok(results)
    }

    async fn build_get_series_payload(
        &self,
        ticker: &str,
        events_limit: u32,
    ) -> Result<Value, ConnectorError> {
        let series_list = self.fetch_series_all(None).await?;
        let series = series_list
            .into_iter()
            .find(|candidate| {
                string_field(candidate, "ticker")
                    .map(|value| value == normalize_ticker(ticker))
                    .unwrap_or(false)
            })
            .ok_or(ConnectorError::ResourceNotFound)?;

        let related_args = ListEventsArgs {
            limit: events_limit.clamp(1, MAX_LIST_LIMIT),
            cursor: None,
            series_ticker: Some(normalize_ticker(ticker)),
            status: None,
            multivariate: false,
            collection_ticker: None,
        };
        let (events, next_cursor) = self.fetch_events_page(&related_args).await?;

        Ok(json!({
            "series": Self::series_summary(&series),
            "related_events": events.iter().map(Self::event_summary).collect::<Vec<_>>(),
            "related_events_pagination": {
                "next_cursor": next_cursor,
                "has_more": next_cursor.is_some(),
            },
            "raw": {
                "series": series,
            }
        }))
    }

    async fn build_get_event_payload(&self, ticker: &str) -> Result<Value, ConnectorError> {
        let response = self.fetch_event(ticker).await?;
        let event = response.get("event").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi event response missing event".to_string())
        })?;
        let markets = response
            .get("markets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(json!({
            "event": Self::event_summary(&event),
            "markets": markets.iter().map(Self::market_summary).collect::<Vec<_>>(),
            "raw": response,
        }))
    }

    async fn build_get_market_payload(&self, ticker: &str) -> Result<Value, ConnectorError> {
        let (response, source) = self.fetch_market_any(ticker).await?;
        let market = response.get("market").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi market response missing market".to_string())
        })?;
        Ok(json!({
            "market": Self::market_summary(&market),
            "source": source.as_str(),
            "raw": response,
        }))
    }

    async fn build_market_candlesticks_payload(
        &self,
        ticker: &str,
        start_ts: i64,
        end_ts: i64,
        period_interval: u32,
    ) -> Result<Value, ConnectorError> {
        let (market_response, market_source) = self.fetch_market_any(ticker).await?;
        let market = market_response.get("market").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi market response missing market".to_string())
        })?;
        let cutoff = self.fetch_historical_cutoff().await?;
        let use_historical = should_use_historical_market_data(&market, &cutoff, end_ts);

        let candles = if use_historical || matches!(market_source, MarketSource::Historical) {
            self.fetch_market_candlesticks_historical(ticker, start_ts, end_ts, period_interval)
                .await?
        } else {
            self.fetch_market_candlesticks_live(ticker, start_ts, end_ts, period_interval)
                .await?
        };

        Ok(json!({
            "market": Self::market_summary(&market),
            "source": if use_historical || matches!(market_source, MarketSource::Historical) {
                "historical"
            } else {
                "live"
            },
            "start_ts": start_ts,
            "end_ts": end_ts,
            "period_interval": period_interval,
            "historical_cutoff": cutoff,
            "candlesticks": candles,
        }))
    }

    async fn build_trades_payload(
        &self,
        ticker: &str,
        limit: u32,
        cursor: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
    ) -> Result<Value, ConnectorError> {
        let (market_response, market_source) = self.fetch_market_any(ticker).await?;
        let market = market_response.get("market").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi market response missing market".to_string())
        })?;
        let cutoff = self.fetch_historical_cutoff().await?;
        let effective_end = max_ts.unwrap_or_else(|| Utc::now().timestamp());
        let use_historical = should_use_historical_market_data(&market, &cutoff, effective_end);

        let trades = if use_historical || matches!(market_source, MarketSource::Historical) {
            self.fetch_trades_historical(ticker, limit, cursor, min_ts, max_ts)
                .await?
        } else {
            self.fetch_trades_live(ticker, limit, cursor, min_ts, max_ts)
                .await?
        };

        Ok(json!({
            "market": Self::market_summary(&market),
            "source": if use_historical || matches!(market_source, MarketSource::Historical) {
                "historical"
            } else {
                "live"
            },
            "pagination": {
                "cursor": string_field(&trades, "cursor"),
                "has_more": trades.get("cursor").is_some(),
            },
            "trades": trades.get("trades").and_then(Value::as_array).cloned().unwrap_or_default(),
            "historical_cutoff": cutoff,
        }))
    }

    async fn build_market_context_payload(
        &self,
        ticker: &str,
        args: &MarketContextArgs,
    ) -> Result<Value, ConnectorError> {
        let (market_response, market_source) = self.fetch_market_any(ticker).await?;
        let market = market_response.get("market").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi market response missing market".to_string())
        })?;
        let event_ticker = string_field(&market, "event_ticker").ok_or_else(|| {
            ConnectorError::Other("Kalshi market missing event_ticker".to_string())
        })?;

        let event_response = self.fetch_event(&event_ticker).await?;
        let event = event_response.get("event").cloned().ok_or_else(|| {
            ConnectorError::Other("Kalshi event response missing event".to_string())
        })?;
        let series_ticker = string_field(&event, "series_ticker").ok_or_else(|| {
            ConnectorError::Other("Kalshi event missing series_ticker".to_string())
        })?;
        let series_payload = self.build_get_series_payload(&series_ticker, 5).await?;

        let (start_ts, end_ts) = default_market_window(&market, args.start_ts, args.end_ts)?;
        let cutoff = self.fetch_historical_cutoff().await?;
        let use_historical = should_use_historical_market_data(&market, &cutoff, end_ts);

        let order_book = if matches!(market_source, MarketSource::Live) && !use_historical {
            self.fetch_order_book(ticker)
                .await
                .ok()
                .map(|book| truncate_order_book(&book, args.orderbook_depth))
        } else {
            None
        };

        let candlesticks = self
            .build_market_candlesticks_payload(ticker, start_ts, end_ts, args.period_interval)
            .await?;
        let trades = self
            .build_trades_payload(ticker, args.trades_limit, None, None, Some(end_ts))
            .await?;
        let event_metadata = if args.include_event_metadata {
            Some(self.fetch_event_metadata(&event_ticker).await?)
        } else {
            None
        };

        Ok(json!({
            "market": Self::market_summary(&market),
            "event": Self::event_summary(&event),
            "series": series_payload.get("series").cloned(),
            "related_series_events": series_payload.get("related_events").cloned(),
            "routing": {
                "market_source": market_source.as_str(),
                "candlesticks_source": candlesticks.get("source").cloned(),
                "trades_source": trades.get("source").cloned(),
            },
            "window": {
                "start_ts": start_ts,
                "end_ts": end_ts,
                "period_interval": args.period_interval,
            },
            "order_book": order_book,
            "candlesticks": candlesticks.get("candlesticks").cloned(),
            "recent_trades": trades.get("trades").cloned(),
            "event_metadata": event_metadata,
            "historical_cutoff": cutoff,
            "raw": {
                "market": market_response,
                "event": event_response,
            }
        }))
    }

    fn resolve_series_ticker(args: &GetSeriesArgs) -> Result<String, ConnectorError> {
        if let Some(item_ref) = args.item_ref.as_deref() {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "kalshi") {
                return match kind.as_str() {
                    "series" => Ok(normalize_ticker(&id)),
                    "event" => Err(ConnectorError::InvalidParams(
                        "Item ref points to an event. Use get instead.".to_string(),
                    )),
                    "market" => Err(ConnectorError::InvalidParams(
                        "Item ref points to a market. Use get_market instead.".to_string(),
                    )),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported kalshi item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }
        if let Some(ticker) = args.ticker.as_ref() {
            return Ok(normalize_ticker(ticker));
        }
        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref or ticker".to_string(),
        ))
    }

    fn resolve_event_ticker(
        item_ref: Option<&str>,
        ticker: Option<&str>,
        url: Option<&str>,
    ) -> Result<String, ConnectorError> {
        if let Some(item_ref) = item_ref {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "kalshi") {
                return match kind.as_str() {
                    "event" => Ok(normalize_ticker(&id)),
                    "series" => Err(ConnectorError::InvalidParams(
                        "Item ref points to a series. Use get_series instead.".to_string(),
                    )),
                    "market" => Err(ConnectorError::InvalidParams(
                        "Item ref points to a market. Use get_market instead.".to_string(),
                    )),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported kalshi item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }
        if let Some(url) = url {
            return event_ticker_from_url(url);
        }
        if let Some(ticker) = ticker {
            return Ok(normalize_ticker(ticker));
        }
        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref, ticker, or url".to_string(),
        ))
    }

    fn resolve_market_ticker(
        item_ref: Option<&str>,
        ticker: Option<&str>,
    ) -> Result<String, ConnectorError> {
        if let Some(item_ref) = item_ref {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "kalshi") {
                return match kind.as_str() {
                    "market" => Ok(normalize_ticker(&id)),
                    "event" => Err(ConnectorError::InvalidParams(
                        "Item ref points to an event. Use get instead.".to_string(),
                    )),
                    "series" => Err(ConnectorError::InvalidParams(
                        "Item ref points to a series. Use get_series instead.".to_string(),
                    )),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported kalshi item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }
        if let Some(ticker) = ticker {
            return Ok(normalize_ticker(ticker));
        }
        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref or ticker".to_string(),
        ))
    }
}

#[async_trait]
impl Connector for KalshiConnector {
    fn name(&self) -> &'static str {
        "kalshi"
    }

    fn description(&self) -> &'static str {
        "Public read-only access to Kalshi series, events, markets, books, and price history"
    }

    fn display_name(&self) -> &'static str {
        "Kalshi"
    }

    fn icon(&self) -> &'static str {
        "kalshi"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["prediction-markets", "finance", "news"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: KALSHI_EVENT_URL_RE.as_str().to_string(),
            default_tool: "get".to_string(),
            description: "Kalshi event page URL".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 0,
                param_name: "url".to_string(),
                use_full_url: true,
            }],
        }]
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: Some("https://kalshi.com".to_string()),
            },
            instructions: Some(
                "Use search/list_series/list_events/list_markets for discovery, get/get_series/get_market for core entities, event_metadata and event_candlesticks for event context, order_book/market_candlesticks/list_trades for market microstructure, and get_market_context when you want the important market, event, and routing context assembled in one response. This connector is read-only and does not require authentication."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search Kalshi series, events, and markets using best-effort ranking over the public discovery endpoints.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query for Kalshi markets, events, or series." },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1/display_v1 for ingest/display pipelines."
                            }
                        },
                        "required": ["query"],
                        "examples": [
                            { "description": "Search election-related contracts", "input": { "query": "fed rates", "limit": 10 } },
                            { "description": "Search as normalized output", "input": { "query": "elon mars", "limit": 8, "output_format": "normalized_v1" } }
                        ],
                        "_meta": {
                            "category": "search",
                            "tags": ["prediction-markets", "finance", "news"],
                            "auth_required": false,
                            "supports_output_format": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_series"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Kalshi series. This wraps the public series catalog with client-side cursor pagination.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_series response." },
                            "status": { "type": "string", "description": "Optional Kalshi series status filter." }
                        },
                        "examples": [
                            { "description": "Browse the first page of series", "input": { "limit": 20 } },
                            { "description": "Continue from a cursor", "input": { "cursor": "<opaque>" } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "series", "finance"],
                            "auth_required": false,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_series"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get one Kalshi series by ticker or normalized item_ref, with recent related events bundled in.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized item_ref such as kalshi:series:KXELONMARS." },
                            "ticker": { "type": "string", "description": "Kalshi series ticker." },
                            "events_limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 10 },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw"
                            }
                        },
                        "examples": [
                            { "description": "Fetch one series", "input": { "ticker": "KXELONMARS" } },
                            { "description": "Fetch as normalized output", "input": { "item_ref": "kalshi:series:KXELONMARS", "output_format": "normalized_v1" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "series", "finance"],
                            "auth_required": false,
                            "supports_output_format": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_events"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Kalshi events, including multivariate collections when requested.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_events response." },
                            "series_ticker": { "type": "string", "description": "Filter to one series ticker." },
                            "status": { "type": "string", "description": "Optional Kalshi event status filter." },
                            "multivariate": { "type": "boolean", "default": false, "description": "Use the multivariate events endpoint." },
                            "collection_ticker": { "type": "string", "description": "Multivariate collection ticker." }
                        },
                        "examples": [
                            { "description": "List events for a series", "input": { "series_ticker": "KXELONMARS", "limit": 10 } },
                            { "description": "List multivariate events", "input": { "multivariate": true, "collection_ticker": "KXMVESPORTSMULTIGAMEEXTENDED-R", "limit": 10 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "events", "finance"],
                            "auth_required": false,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get one Kalshi event by ticker, event page URL, or normalized item_ref.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized item_ref such as kalshi:event:KXELONMARS-99." },
                            "ticker": { "type": "string", "description": "Kalshi event ticker." },
                            "url": { "type": "string", "description": "Kalshi event page URL." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw"
                            }
                        },
                        "examples": [
                            { "description": "Fetch by event ticker", "input": { "ticker": "KXELONMARS-99" } },
                            { "description": "Fetch by URL", "input": { "url": "https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "events", "finance"],
                            "auth_required": false,
                            "supports_output_format": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_event_metadata"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get Kalshi event metadata such as settlement sources, imagery, and market details.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "url": { "type": "string" }
                        },
                        "examples": [
                            { "description": "Fetch event metadata", "input": { "ticker": "KXELONMARS-99" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "metadata", "finance"],
                            "auth_required": false
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("event_candlesticks"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get event-level candlesticks grouped by market ticker. This is useful when an event has multiple child markets.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "url": { "type": "string" },
                            "series_ticker": { "type": "string", "description": "Optional series ticker override. The connector can infer it from the event when omitted." },
                            "start_ts": { "type": "integer", "description": "Unix start timestamp." },
                            "end_ts": { "type": "integer", "description": "Unix end timestamp. Defaults to now." },
                            "period_interval": { "type": "integer", "minimum": 1, "default": 60 }
                        },
                        "required": ["start_ts"],
                        "examples": [
                            { "description": "Fetch one hour candles over the last day", "input": { "ticker": "KXELONMARS-99", "start_ts": 1774620000, "end_ts": 1774706400, "period_interval": 60 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "candlesticks", "finance"],
                            "auth_required": false
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_markets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Kalshi markets, optionally across historical archives.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_markets response." },
                            "series_ticker": { "type": "string" },
                            "event_ticker": { "type": "string" },
                            "status": { "type": "string" },
                            "historical": { "type": "boolean", "default": false, "description": "Query the historical markets catalog instead of the live catalog." }
                        },
                        "examples": [
                            { "description": "List markets for an event", "input": { "event_ticker": "KXELONMARS-99", "limit": 20 } },
                            { "description": "List historical markets", "input": { "historical": true, "limit": 20 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "markets", "finance"],
                            "auth_required": false,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_market"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get one Kalshi market by ticker or normalized item_ref. This automatically falls back to historical storage when needed.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized item_ref such as kalshi:market:KX... ." },
                            "ticker": { "type": "string", "description": "Kalshi market ticker." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw"
                            }
                        },
                        "examples": [
                            { "description": "Fetch a live market", "input": { "ticker": "KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "markets", "finance"],
                            "auth_required": false,
                            "supports_output_format": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("order_book"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get the top of book for a market. This is only available for live markets.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "depth": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                        },
                        "examples": [
                            { "description": "Get one market order book", "input": { "ticker": "KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3", "depth": 10 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "order-book", "finance"],
                            "auth_required": false
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("market_candlesticks"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get candlesticks for one market, automatically routing to live or historical storage.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "start_ts": { "type": "integer" },
                            "end_ts": { "type": "integer", "description": "Defaults to now." },
                            "period_interval": { "type": "integer", "minimum": 1, "default": 60 }
                        },
                        "required": ["start_ts"],
                        "examples": [
                            { "description": "Fetch market candles", "input": { "ticker": "KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3", "start_ts": 1774620000, "end_ts": 1774706400, "period_interval": 60 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "candlesticks", "finance"],
                            "auth_required": false
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_trades"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List trades for one market, automatically routing to live or historical storage.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_trades response." },
                            "min_ts": { "type": "integer" },
                            "max_ts": { "type": "integer" }
                        },
                        "examples": [
                            { "description": "Fetch recent trades", "input": { "ticker": "KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3", "limit": 20 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "trades", "finance"],
                            "auth_required": false,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_market_context"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a market plus parent event, parent series, book snapshot, trades, candles, and routing metadata. This is the high-context Kalshi analysis tool for agents.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string" },
                            "ticker": { "type": "string" },
                            "start_ts": { "type": "integer", "description": "Defaults to a sensible 24h window around the market's latest active period or settlement." },
                            "end_ts": { "type": "integer" },
                            "period_interval": { "type": "integer", "minimum": 1, "default": 60 },
                            "orderbook_depth": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
                            "trades_limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "include_event_metadata": { "type": "boolean", "default": true }
                        },
                        "examples": [
                            { "description": "Fetch bundled market context", "input": { "ticker": "KXMVESPORTSMULTIGAMEEXTENDED-S2026C6FFFC3D8E5-5E99704F1C3" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "market-context", "finance"],
                            "auth_required": false
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let tool_name = request.name.as_ref();
        let args_map = request.arguments.unwrap_or_default();

        match tool_name {
            "search" => {
                let args: SearchArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let query = args.query.trim();
                if query.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "query must not be empty".to_string(),
                    ));
                }
                let results = self.search(query, args.limit).await?;

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let items = results
                        .iter()
                        .map(Self::summary_to_item)
                        .collect::<Result<Vec<_>, _>>()?;
                    let page = NormalizedPageV1::new(
                        items,
                        None,
                        false,
                        Partial::complete(Some(ingest::limits_max_items(results.len() as u64))),
                        Source::new("kalshi", "search"),
                    );
                    return structured_result(&page);
                }

                structured_result_with_text(
                    &json!({
                        "query": query,
                        "results": results,
                        "pagination": {
                            "has_more": false,
                            "next_cursor": null,
                        }
                    }),
                    None,
                )
            }
            "list_series" => {
                let args: ListSeriesArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let decoded = if let Some(cursor) = args.cursor.as_deref() {
                    let decoded =
                        ingest::decode_cursor::<SeriesCursor>(cursor).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?;
                    if decoded.status != args.status {
                        return Err(ConnectorError::InvalidParams(
                            "Cursor status does not match requested status".to_string(),
                        ));
                    }
                    decoded
                } else {
                    SeriesCursor {
                        offset: 0,
                        status: args.status.clone(),
                    }
                };

                let all_series = self.fetch_series_all(args.status.as_deref()).await?;
                let offset = decoded.offset.min(all_series.len());
                let limit = Self::clamp_limit(args.limit) as usize;
                let next_offset = (offset + limit).min(all_series.len());
                let has_more = next_offset < all_series.len();
                let next_cursor = if has_more {
                    Some(ingest::encode_cursor(&SeriesCursor {
                        offset: next_offset,
                        status: args.status.clone(),
                    })?)
                } else {
                    None
                };

                structured_result_with_text(
                    &json!({
                        "series": all_series[offset..next_offset]
                            .iter()
                            .map(Self::series_summary)
                            .collect::<Vec<_>>(),
                        "pagination": {
                            "offset": offset,
                            "next_cursor": next_cursor,
                            "has_more": has_more,
                            "total": all_series.len(),
                        },
                        "filters": {
                            "status": args.status,
                        }
                    }),
                    None,
                )
            }
            "get_series" => {
                let args: GetSeriesArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker = Self::resolve_series_ticker(&args)?;
                let payload = self
                    .build_get_series_payload(&ticker, args.events_limit)
                    .await?;

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let item = Self::summary_to_item(
                        payload
                            .get("series")
                            .ok_or_else(|| ConnectorError::Other("Missing series".to_string()))?,
                    )?;
                    return structured_result(&NormalizedItemV1::complete(
                        item,
                        Source::new("kalshi", "get_series"),
                    ));
                }

                structured_result_with_text(&payload, None)
            }
            "list_events" => {
                let args: ListEventsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let (events, next_cursor) = self.fetch_events_page(&args).await?;
                structured_result_with_text(
                    &json!({
                        "events": events.iter().map(Self::event_summary).collect::<Vec<_>>(),
                        "pagination": {
                            "next_cursor": next_cursor,
                            "has_more": next_cursor.is_some(),
                        },
                        "filters": {
                            "series_ticker": args.series_ticker.map(|value| normalize_ticker(&value)),
                            "status": args.status,
                            "multivariate": args.multivariate,
                            "collection_ticker": args.collection_ticker.map(|value| normalize_ticker(&value)),
                        }
                    }),
                    None,
                )
            }
            "get" => {
                let args: GetEventArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker = Self::resolve_event_ticker(
                    args.item_ref.as_deref(),
                    args.ticker.as_deref(),
                    args.url.as_deref(),
                )?;
                let payload = self.build_get_event_payload(&ticker).await?;

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let item = Self::summary_to_item(
                        payload
                            .get("event")
                            .ok_or_else(|| ConnectorError::Other("Missing event".to_string()))?,
                    )?;
                    return structured_result(&NormalizedItemV1::complete(
                        item,
                        Source::new("kalshi", "get"),
                    ));
                }

                structured_result_with_text(&payload, None)
            }
            "get_event_metadata" => {
                let args: EventMetadataArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker = Self::resolve_event_ticker(
                    args.item_ref.as_deref(),
                    args.ticker.as_deref(),
                    args.url.as_deref(),
                )?;
                let event_payload = self.build_get_event_payload(&ticker).await?;
                let metadata = self.fetch_event_metadata(&ticker).await?;
                structured_result_with_text(
                    &json!({
                        "event": event_payload.get("event").cloned(),
                        "metadata": metadata,
                    }),
                    None,
                )
            }
            "event_candlesticks" => {
                let args: EventCandlesticksArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                    ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                })?;
                let ticker = Self::resolve_event_ticker(
                    args.item_ref.as_deref(),
                    args.ticker.as_deref(),
                    args.url.as_deref(),
                )?;
                let end_ts = args.end_ts.unwrap_or_else(|| Utc::now().timestamp());
                if args.start_ts >= end_ts {
                    return Err(ConnectorError::InvalidParams(
                        "start_ts must be less than end_ts".to_string(),
                    ));
                }

                let event_payload = self.build_get_event_payload(&ticker).await?;
                let inferred_series = event_payload
                    .get("event")
                    .and_then(|event| string_field(event, "series_ticker"));
                let series_ticker = args
                    .series_ticker
                    .map(|value| normalize_ticker(&value))
                    .or(inferred_series)
                    .ok_or_else(|| {
                        ConnectorError::Other(
                            "Could not determine series_ticker for event".to_string(),
                        )
                    })?;
                let candles = self
                    .fetch_event_candlesticks(
                        &series_ticker,
                        &ticker,
                        args.start_ts,
                        end_ts,
                        args.period_interval,
                    )
                    .await?;
                structured_result_with_text(
                    &json!({
                        "event": event_payload.get("event").cloned(),
                        "series_ticker": series_ticker,
                        "start_ts": args.start_ts,
                        "end_ts": end_ts,
                        "period_interval": args.period_interval,
                        "candlesticks": candles,
                    }),
                    None,
                )
            }
            "list_markets" => {
                let args: ListMarketsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                if args.event_ticker.is_some() && !args.historical && args.cursor.is_none() {
                    let event = self
                        .fetch_event(args.event_ticker.as_deref().unwrap_or_default())
                        .await?;
                    let markets = event
                        .get("markets")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let limit = Self::clamp_limit(args.limit) as usize;
                    let has_more = markets.len() > limit;
                    structured_result_with_text(
                        &json!({
                            "markets": markets.into_iter().take(limit).map(|market| Self::market_summary(&market)).collect::<Vec<_>>(),
                            "pagination": {
                                "next_cursor": Value::Null,
                                "has_more": has_more,
                            },
                            "filters": {
                                "event_ticker": args.event_ticker.map(|value| normalize_ticker(&value)),
                                "series_ticker": args.series_ticker.map(|value| normalize_ticker(&value)),
                                "status": args.status,
                                "historical": false,
                            }
                        }),
                        None,
                    )
                } else {
                    let (markets, next_cursor) = if args.historical {
                        self.fetch_markets_page_historical(&args).await?
                    } else {
                        self.fetch_markets_page_live(&args).await?
                    };
                    structured_result_with_text(
                        &json!({
                            "markets": markets.iter().map(Self::market_summary).collect::<Vec<_>>(),
                            "pagination": {
                                "next_cursor": next_cursor,
                                "has_more": next_cursor.is_some(),
                            },
                            "filters": {
                                "event_ticker": args.event_ticker.map(|value| normalize_ticker(&value)),
                                "series_ticker": args.series_ticker.map(|value| normalize_ticker(&value)),
                                "status": args.status,
                                "historical": args.historical,
                            }
                        }),
                        None,
                    )
                }
            }
            "get_market" => {
                let args: GetMarketArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker =
                    Self::resolve_market_ticker(args.item_ref.as_deref(), args.ticker.as_deref())?;
                let payload = self.build_get_market_payload(&ticker).await?;

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let item = Self::summary_to_item(
                        payload
                            .get("market")
                            .ok_or_else(|| ConnectorError::Other("Missing market".to_string()))?,
                    )?;
                    return structured_result(&NormalizedItemV1::complete(
                        item,
                        Source::new("kalshi", "get_market"),
                    ));
                }

                structured_result_with_text(&payload, None)
            }
            "order_book" => {
                let args: OrderBookArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker =
                    Self::resolve_market_ticker(args.item_ref.as_deref(), args.ticker.as_deref())?;
                let market_payload = self.build_get_market_payload(&ticker).await?;
                let order_book = self.fetch_order_book(&ticker).await?;
                structured_result_with_text(
                    &json!({
                        "market": market_payload.get("market").cloned(),
                        "depth": args.depth,
                        "order_book": truncate_order_book(&order_book, args.depth),
                    }),
                    None,
                )
            }
            "market_candlesticks" => {
                let args: MarketCandlesticksArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker =
                    Self::resolve_market_ticker(args.item_ref.as_deref(), args.ticker.as_deref())?;
                let end_ts = args.end_ts.unwrap_or_else(|| Utc::now().timestamp());
                if args.start_ts >= end_ts {
                    return Err(ConnectorError::InvalidParams(
                        "start_ts must be less than end_ts".to_string(),
                    ));
                }
                let payload = self
                    .build_market_candlesticks_payload(
                        &ticker,
                        args.start_ts,
                        end_ts,
                        args.period_interval,
                    )
                    .await?;
                structured_result_with_text(&payload, None)
            }
            "list_trades" => {
                let args: ListTradesArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker =
                    Self::resolve_market_ticker(args.item_ref.as_deref(), args.ticker.as_deref())?;
                let payload = self
                    .build_trades_payload(
                        &ticker,
                        args.limit,
                        args.cursor.as_deref(),
                        args.min_ts,
                        args.max_ts,
                    )
                    .await?;
                structured_result_with_text(&payload, None)
            }
            "get_market_context" => {
                let args: MarketContextArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {e}"))
                    })?;
                let ticker =
                    Self::resolve_market_ticker(args.item_ref.as_deref(), args.ticker.as_deref())?;
                let payload = self.build_market_context_payload(&ticker, &args).await?;
                structured_result_with_text(&payload, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt '{}' not found",
            name
        )))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: Vec::new() }
    }
}

fn normalize_ticker(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .filter(|value| !value.is_empty())
}

fn array_of_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn score_candidate(query: &str, values: &[&Value], fields: &[&str]) -> Option<f64> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let query_normalized = query.to_ascii_lowercase();
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return None;
    }

    let mut haystacks = Vec::new();
    for value in values {
        for field in fields {
            if let Some(text) = string_field(value, field) {
                haystacks.push(text);
            }
        }
    }
    if haystacks.is_empty() {
        return None;
    }

    let combined = haystacks.join(" ").to_ascii_lowercase();
    let title = haystacks
        .first()
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let combined_tokens = tokenize(&combined);
    let title_tokens = tokenize(&title);

    let mut score = 0.0;
    if title == query_normalized {
        score += 100.0;
    } else if title.contains(&query_normalized) && query_normalized.len() >= 3 {
        score += 45.0;
    }
    if combined.contains(&query_normalized) && query_normalized.len() >= 3 {
        score += 20.0;
    }

    for token in &query_tokens {
        if title_tokens.iter().any(|candidate| candidate == token) {
            score += 14.0;
        } else if token.len() >= 3
            && title_tokens
                .iter()
                .any(|candidate| candidate.starts_with(token))
        {
            score += 6.0;
        }

        if combined_tokens.iter().any(|candidate| candidate == token) {
            score += 8.0;
        } else if token.len() >= 3
            && combined_tokens
                .iter()
                .any(|candidate| candidate.starts_with(token))
        {
            score += 3.0;
        }
    }

    (score > 0.0).then_some(score)
}

fn event_ticker_from_url(url: &str) -> Result<String, ConnectorError> {
    let parsed = Url::parse(url).map_err(|e| {
        ConnectorError::InvalidParams(format!("Invalid Kalshi URL '{}': {}", url, e))
    })?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "kalshi.com" && host != "www.kalshi.com" {
        return Err(ConnectorError::InvalidParams(
            "Kalshi URLs must point to kalshi.com".to_string(),
        ));
    }
    let segments = parsed
        .path_segments()
        .ok_or_else(|| ConnectorError::InvalidParams("Kalshi URL has no path".to_string()))?
        .collect::<Vec<_>>();
    if segments.len() < 2 || segments.first() != Some(&"markets") {
        return Err(ConnectorError::InvalidParams(
            "Kalshi event URLs must look like https://kalshi.com/markets/.../<event-ticker>"
                .to_string(),
        ));
    }
    let ticker = segments
        .last()
        .copied()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ConnectorError::InvalidParams("Kalshi URL missing event ticker".to_string())
        })?;
    Ok(normalize_ticker(ticker))
}

fn parse_rfc3339_timestamp(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|parsed| parsed.timestamp())
}

fn should_use_historical_market_data(
    market: &Value,
    cutoff: &HistoricalCutoff,
    end_ts: i64,
) -> bool {
    let cutoff_ts = parse_rfc3339_timestamp(Some(&cutoff.market_settled_ts)).unwrap_or_default();
    let settlement_ts = parse_rfc3339_timestamp(string_field(market, "settlement_ts").as_deref())
        .unwrap_or(i64::MAX);
    settlement_ts <= cutoff_ts || end_ts <= cutoff_ts
}

fn default_market_window(
    market: &Value,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<(i64, i64), ConnectorError> {
    let inferred_end = end_ts.unwrap_or_else(|| {
        parse_rfc3339_timestamp(string_field(market, "settlement_ts").as_deref())
            .or_else(|| parse_rfc3339_timestamp(string_field(market, "close_time").as_deref()))
            .unwrap_or_else(|| Utc::now().timestamp())
    });
    let inferred_start = start_ts.unwrap_or(inferred_end.saturating_sub(CONTEXT_WINDOW_SECONDS));
    if inferred_start >= inferred_end {
        return Err(ConnectorError::InvalidParams(
            "start_ts must be less than end_ts".to_string(),
        ));
    }
    Ok((inferred_start, inferred_end))
}

fn truncate_order_book(order_book: &Value, depth: usize) -> Value {
    let Some(book) = order_book.get("orderbook_fp").and_then(Value::as_object) else {
        return order_book.clone();
    };

    let truncate_side = |key: &str| {
        book.get(key)
            .and_then(Value::as_array)
            .map(|levels| levels.iter().take(depth).cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    };

    json!({
        "orderbook_fp": {
            "yes_dollars": truncate_side("yes_dollars"),
            "no_dollars": truncate_side("no_dollars"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalshi_event_url_parses_last_segment() {
        let ticker = event_ticker_from_url(
            "https://kalshi.com/markets/elon-mars/will-elon-musk-visit-mars-in-his-lifetime/kxelonmars-99",
        )
        .expect("ticker");
        assert_eq!(ticker, "KXELONMARS-99");
    }

    #[test]
    fn score_candidate_prefers_exact_tokens_not_short_substrings() {
        let ai = json!({ "title": "AI chip demand" });
        let taiwan = json!({ "title": "Taiwan election odds" });

        let ai_score = score_candidate("ai", &[&ai], &["title"]).expect("ai score should exist");
        let taiwan_score = score_candidate("ai", &[&taiwan], &["title"]).unwrap_or_default();

        assert!(ai_score > taiwan_score);
    }

    #[test]
    fn historical_routing_uses_cutoff() {
        let market = json!({ "settlement_ts": "2025-12-27T23:35:31.681533Z" });
        let cutoff = HistoricalCutoff {
            market_settled_ts: "2025-12-28T00:00:00Z".to_string(),
            orders_updated_ts: "2025-12-28T00:00:00Z".to_string(),
            trades_created_ts: "2025-12-28T00:00:00Z".to_string(),
        };

        assert!(should_use_historical_market_data(
            &market,
            &cutoff,
            1_767_000_000
        ));
    }
}
