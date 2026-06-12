use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Relationship, Source,
};
use crate::utils::{build_reqwest_client, structured_result, structured_result_with_text};
use crate::{
    CallToolRequestParam, Connector, Implementation, InitializeRequestParam, InitializeResult,
    ListPromptsResult, ListResourcesResult, ListToolsResult, PaginatedRequestParam, Prompt,
    ProtocolVersion, ReadResourceRequestParam, ResourceContents, ServerCapabilities, Tool,
    URLParamExtraction, URLPatternSpec,
};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

const USER_AGENT: &str = "rzn-tools-polymarket-connector/0.1.0";
const GAMMA_BASE_URL: &str = "https://gamma-api.polymarket.com";
const CLOB_BASE_URL: &str = "https://clob.polymarket.com";
const DATA_API_BASE_URL: &str = "https://data-api.polymarket.com/v1";
const DEFAULT_SEARCH_LIMIT: u32 = 10;
const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_SEARCH_LIMIT: u32 = 100;
const MAX_LIST_LIMIT: u32 = 100;
const MAX_SEARCH_REQUESTS: u32 = 20;
const MAX_EVENT_SCAN_REQUESTS: u32 = 20;
const DEFAULT_ORDER_BOOK_DEPTH: usize = 5;

static EVENT_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:https?://)?(?:www\.)?polymarket\.com/event/(?P<slug>[a-zA-Z0-9][a-zA-Z0-9_-]*)(?:[/?#].*)?$",
    )
    .expect("valid polymarket event url regex")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchCursor {
    query: String,
    page: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: u32,
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: u32,
    #[serde(default = "default_search_page")]
    page: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct GetEventArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct GetMarketArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct ListEventsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    series_id: Option<String>,
    #[serde(default)]
    series_slug: Option<String>,
    #[serde(default)]
    tag_slug: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    featured: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListMarketsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    event_item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    event_id: Option<String>,
    #[serde(default)]
    event_slug: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    series_id: Option<String>,
    #[serde(default)]
    series_slug: Option<String>,
    #[serde(default)]
    tag_slug: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListSeriesArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    featured: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListTagsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetSeriesArgs {
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListCommentsArgs {
    #[serde(default = "default_list_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    event_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    event_id: Option<String>,
    #[serde(default)]
    event_slug: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    market_id: Option<String>,
    #[serde(default)]
    market_slug: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    series_id: Option<String>,
    #[serde(default)]
    series_slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrderBookArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    token_id: Option<String>,
    #[serde(default = "default_order_book_depth")]
    depth: usize,
}

#[derive(Debug, Deserialize)]
struct PriceHistoryArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    token_id: Option<String>,
    #[serde(default = "default_price_history_interval")]
    interval: String,
    #[serde(default = "default_price_history_fidelity")]
    fidelity: u32,
}

#[derive(Debug, Deserialize)]
struct MarketPositionsArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default = "default_list_limit")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct GetMarketContextArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default, deserialize_with = "de_opt_stringish")]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default = "default_order_book_depth")]
    depth: usize,
    #[serde(default = "default_price_history_interval")]
    interval: String,
    #[serde(default = "default_price_history_fidelity")]
    fidelity: u32,
    #[serde(default)]
    include_positions: bool,
    #[serde(default = "default_list_limit")]
    positions_limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct GammaSearchResponse {
    #[serde(default)]
    events: Vec<GammaEvent>,
    #[serde(default)]
    pagination: GammaPagination,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaPagination {
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    total_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaSeriesSummary {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    series_type: Option<String>,
    #[serde(default)]
    recurrence: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    featured: bool,
    #[serde(default, rename = "commentCount", deserialize_with = "de_opt_u64")]
    comment_count: Option<u64>,
    #[serde(default, rename = "volume24hr", deserialize_with = "de_opt_f64")]
    volume_24hr: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    liquidity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaTag {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    requires_translation: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEventRef {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    tags: Vec<GammaTag>,
    #[serde(default)]
    series: Vec<GammaSeriesSummary>,
    #[serde(default)]
    markets: Vec<GammaMarket>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    resolution_source: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    comment_count: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    liquidity: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    open_interest: Option<f64>,
    #[serde(default, rename = "volume24hr", deserialize_with = "de_opt_f64")]
    volume_24hr: Option<f64>,
    #[serde(default, rename = "volume1wk", deserialize_with = "de_opt_f64")]
    volume_1wk: Option<f64>,
    #[serde(default, rename = "volume1mo", deserialize_with = "de_opt_f64")]
    volume_1mo: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    volume: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaMarket {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    accepting_orders: Option<bool>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default)]
    resolution_source: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    events: Vec<GammaEventRef>,
    #[serde(default)]
    outcomes: Option<Value>,
    #[serde(default)]
    outcome_prices: Option<Value>,
    #[serde(default)]
    clob_token_ids: Option<Value>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    last_trade_price: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    best_bid: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    best_ask: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    liquidity: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    volume: Option<f64>,
    #[serde(default, rename = "volume24hr", deserialize_with = "de_opt_f64")]
    volume_24hr: Option<f64>,
    #[serde(default, rename = "volume1wk", deserialize_with = "de_opt_f64")]
    volume_1wk: Option<f64>,
    #[serde(default, rename = "volume1mo", deserialize_with = "de_opt_f64")]
    volume_1mo: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaSeries {
    id: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    series_type: Option<String>,
    #[serde(default)]
    recurrence: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    featured: bool,
    #[serde(default)]
    restricted: bool,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default, rename = "commentCount", deserialize_with = "de_opt_u64")]
    comment_count: Option<u64>,
    #[serde(default, rename = "volume24hr", deserialize_with = "de_opt_f64")]
    volume_24hr: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    volume: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    liquidity: Option<f64>,
    #[serde(default)]
    events: Vec<GammaEventRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaCommentProfile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    pseudonym: Option<String>,
    #[serde(default)]
    profile_image: Option<String>,
    #[serde(default)]
    display_username_public: Option<bool>,
    #[serde(default)]
    verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaComment {
    id: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    parent_entity_type: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    parent_entity_id: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    reaction_count: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    report_count: Option<u64>,
    #[serde(default)]
    user_address: Option<String>,
    #[serde(default)]
    profile: Option<GammaCommentProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClobOrderLevel {
    price: String,
    size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClobOrderBook {
    #[serde(default)]
    market: Option<String>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    bids: Vec<ClobOrderLevel>,
    #[serde(default)]
    asks: Vec<ClobOrderLevel>,
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    last_trade_price: Option<String>,
    #[serde(default)]
    min_order_size: Option<String>,
    #[serde(default)]
    tick_size: Option<String>,
    #[serde(default)]
    neg_risk: Option<bool>,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClobPricePoint {
    t: i64,
    p: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClobPriceHistory {
    #[serde(default)]
    history: Vec<ClobPricePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataMarketPosition {
    #[serde(default)]
    proxy_wallet: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    profile_image: Option<String>,
    #[serde(default)]
    verified: Option<bool>,
    #[serde(default)]
    asset: Option<String>,
    #[serde(default)]
    condition_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    avg_price: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    size: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    curr_price: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    current_value: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    cash_pnl: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    total_bought: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    realized_pnl: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    total_pnl: Option<f64>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    outcome_index: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataMarketPositionGroup {
    token: String,
    #[serde(default)]
    positions: Vec<DataMarketPosition>,
}

#[derive(Debug, Clone, Serialize)]
struct SearchResponseView {
    query: String,
    events: Vec<EventSummary>,
    pagination: SearchPaginationView,
}

#[derive(Debug, Clone, Serialize)]
struct SearchPaginationView {
    page: u32,
    next_cursor: Option<String>,
    has_more: bool,
    total_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct EventSummary {
    id: String,
    slug: Option<String>,
    title: String,
    url: Option<String>,
    active: bool,
    closed: bool,
    start_date: Option<String>,
    end_date: Option<String>,
    updated_at: Option<String>,
    comment_count: Option<u64>,
    liquidity: Option<f64>,
    open_interest: Option<f64>,
    volume_24hr: Option<f64>,
    volume_1mo: Option<f64>,
    tags: Vec<String>,
    series: Vec<SeriesSummaryView>,
    top_market: Option<MarketSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct EventDetail {
    id: String,
    slug: Option<String>,
    title: String,
    description: Option<String>,
    url: Option<String>,
    active: bool,
    closed: bool,
    start_date: Option<String>,
    end_date: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    comment_count: Option<u64>,
    liquidity: Option<f64>,
    open_interest: Option<f64>,
    volume_24hr: Option<f64>,
    volume_1wk: Option<f64>,
    volume_1mo: Option<f64>,
    volume: Option<f64>,
    resolution_source: Option<String>,
    tags: Vec<String>,
    series: Vec<SeriesSummaryView>,
    markets: Vec<MarketSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketSummary {
    id: String,
    slug: Option<String>,
    question: String,
    url: Option<String>,
    active: bool,
    closed: bool,
    accepting_orders: Option<bool>,
    start_date: Option<String>,
    end_date: Option<String>,
    updated_at: Option<String>,
    last_trade_price: Option<f64>,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    liquidity: Option<f64>,
    volume_24hr: Option<f64>,
    volume_1wk: Option<f64>,
    volume_1mo: Option<f64>,
    outcomes: Vec<OutcomeSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketDetail {
    id: String,
    slug: Option<String>,
    question: String,
    description: Option<String>,
    url: Option<String>,
    active: bool,
    closed: bool,
    accepting_orders: Option<bool>,
    start_date: Option<String>,
    end_date: Option<String>,
    updated_at: Option<String>,
    condition_id: Option<String>,
    resolution_source: Option<String>,
    last_trade_price: Option<f64>,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    liquidity: Option<f64>,
    volume: Option<f64>,
    volume_24hr: Option<f64>,
    volume_1wk: Option<f64>,
    volume_1mo: Option<f64>,
    clob_token_ids: Vec<String>,
    outcomes: Vec<OutcomeSummary>,
    events: Vec<LinkedEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct OutcomeSummary {
    name: String,
    price: f64,
}

#[derive(Debug, Clone, Serialize)]
struct LinkedEvent {
    id: Option<String>,
    slug: Option<String>,
    title: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EventListResponseView {
    events: Vec<EventSummary>,
    pagination: OffsetPaginationView,
    filters: EventListFiltersView,
}

#[derive(Debug, Clone, Serialize)]
struct MarketListResponseView {
    markets: Vec<MarketListItemView>,
    pagination: OffsetPaginationView,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
struct MarketListItemView {
    market: MarketSummary,
    parent_event: Option<LinkedEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct SeriesListResponseView {
    series: Vec<SeriesSummaryView>,
    pagination: OffsetPaginationView,
}

#[derive(Debug, Clone, Serialize)]
struct TagListResponseView {
    tags: Vec<TagSummaryView>,
    pagination: OffsetPaginationView,
}

#[derive(Debug, Clone, Serialize)]
struct SeriesDetailView {
    id: String,
    slug: Option<String>,
    ticker: Option<String>,
    title: String,
    recurrence: Option<String>,
    active: bool,
    closed: bool,
    archived: bool,
    featured: bool,
    restricted: bool,
    comment_count: Option<u64>,
    volume_24hr: Option<f64>,
    volume: Option<f64>,
    liquidity: Option<f64>,
    events: Vec<LinkedEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct SeriesSummaryView {
    id: String,
    slug: Option<String>,
    ticker: Option<String>,
    title: String,
    recurrence: Option<String>,
    active: bool,
    closed: bool,
    featured: bool,
    comment_count: Option<u64>,
    volume_24hr: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct TagSummaryView {
    id: Option<String>,
    label: String,
    slug: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    requires_translation: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct CommentListResponseView {
    target: CommentTargetView,
    comments: Vec<CommentSummaryView>,
    pagination: OffsetPaginationView,
}

#[derive(Debug, Clone, Serialize)]
struct CommentTargetView {
    entity_type: String,
    entity_id: String,
    entity_slug: Option<String>,
    entity_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CommentSummaryView {
    id: String,
    body: String,
    created_at: Option<String>,
    parent_entity_type: Option<String>,
    parent_entity_id: Option<u64>,
    reaction_count: Option<u64>,
    user_address: Option<String>,
    profile: Option<CommentProfileView>,
}

#[derive(Debug, Clone, Serialize)]
struct CommentProfileView {
    name: Option<String>,
    pseudonym: Option<String>,
    profile_image: Option<String>,
    display_username_public: Option<bool>,
    verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct OffsetPaginationView {
    offset: u32,
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
struct EventListFiltersView {
    series_id: Option<String>,
    series_slug: Option<String>,
    tag_slug: Option<String>,
    active: Option<bool>,
    closed: Option<bool>,
    archived: Option<bool>,
    featured: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct OrderBookResponseView {
    market: MarketDetail,
    books: Vec<OutcomeOrderBookView>,
}

#[derive(Debug, Clone, Serialize)]
struct OutcomeOrderBookView {
    outcome: String,
    token_id: String,
    best_bid: Option<OrderLevelView>,
    best_ask: Option<OrderLevelView>,
    midpoint: Option<f64>,
    spread: Option<f64>,
    min_order_size: Option<f64>,
    tick_size: Option<f64>,
    last_trade_price: Option<f64>,
    neg_risk: Option<bool>,
    timestamp: Option<String>,
    top_bids: Vec<OrderLevelView>,
    top_asks: Vec<OrderLevelView>,
}

#[derive(Debug, Clone, Serialize)]
struct OrderLevelView {
    price: f64,
    size: f64,
}

#[derive(Debug, Clone, Serialize)]
struct PriceHistoryResponseView {
    market: MarketDetail,
    interval: String,
    fidelity: u32,
    history: Vec<OutcomePriceHistoryView>,
}

#[derive(Debug, Clone, Serialize)]
struct OutcomePriceHistoryView {
    outcome: String,
    token_id: String,
    points: Vec<PricePointView>,
    summary: Option<PriceHistorySummaryView>,
}

#[derive(Debug, Clone, Serialize)]
struct PricePointView {
    timestamp: i64,
    price: f64,
}

#[derive(Debug, Clone, Serialize)]
struct PriceHistorySummaryView {
    first_price: f64,
    last_price: f64,
    min_price: f64,
    max_price: f64,
    change_abs: f64,
    change_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketPositionsResponseView {
    market: MarketDetail,
    positions: Vec<MarketPositionGroupView>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketPositionGroupView {
    token_id: String,
    outcome: Option<String>,
    positions: Vec<MarketPositionView>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketPositionView {
    proxy_wallet: Option<String>,
    name: Option<String>,
    profile_image: Option<String>,
    verified: Option<bool>,
    avg_price: Option<f64>,
    size: Option<f64>,
    curr_price: Option<f64>,
    current_value: Option<f64>,
    cash_pnl: Option<f64>,
    realized_pnl: Option<f64>,
    total_pnl: Option<f64>,
    outcome: Option<String>,
    outcome_index: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct MarketContextView {
    market: MarketDetail,
    event: Option<EventDetail>,
    order_books: Vec<OutcomeOrderBookView>,
    price_history: Vec<OutcomePriceHistoryView>,
    positions: Option<Vec<MarketPositionGroupView>>,
}

#[derive(Debug, Clone)]
struct OutcomeTokenInfo {
    name: String,
    price: Option<f64>,
    token_id: Option<String>,
}

pub struct PolymarketConnector {
    client: Client,
}

impl PolymarketConnector {
    pub async fn new() -> Result<Self, ConnectorError> {
        let client = build_reqwest_client(|| {
            Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(USER_AGENT)
        })?;

        Ok(Self { client })
    }

    fn clamp_list_limit(limit: u32) -> u32 {
        limit.clamp(1, MAX_LIST_LIMIT)
    }

    async fn execute_json<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
        context: &str,
    ) -> Result<T, ConnectorError> {
        let response = request.send().await.map_err(ConnectorError::HttpRequest)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket {} failed with status {}",
                context,
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_events_page(
        &self,
        limit: u32,
        offset: u32,
        args: &ListEventsArgs,
    ) -> Result<Vec<GammaEvent>, ConnectorError> {
        let mut params = vec![
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];

        if let Some(value) = args.series_id.as_ref() {
            params.push(("series_id".to_string(), value.clone()));
        }
        if let Some(value) = args.series_slug.as_ref() {
            params.push(("series_slug".to_string(), value.clone()));
        }
        if let Some(value) = args.tag_slug.as_ref() {
            params.push(("tag_slug".to_string(), value.clone()));
        }
        if let Some(value) = args.active {
            params.push(("active".to_string(), value.to_string()));
        }
        if let Some(value) = args.closed {
            params.push(("closed".to_string(), value.to_string()));
        }
        if let Some(value) = args.archived {
            params.push(("archived".to_string(), value.to_string()));
        }
        if let Some(value) = args.featured {
            params.push(("featured".to_string(), value.to_string()));
        }

        self.execute_json(
            self.client
                .get(format!("{}/events", GAMMA_BASE_URL))
                .query(&params),
            "events listing",
        )
        .await
    }

    async fn fetch_markets_page_direct(
        &self,
        limit: u32,
        offset: u32,
        args: &ListMarketsArgs,
    ) -> Result<Vec<GammaMarket>, ConnectorError> {
        let mut params = vec![
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];
        if let Some(value) = args.slug.as_ref() {
            params.push(("slug".to_string(), value.clone()));
        }
        if let Some(value) = args.active {
            params.push(("active".to_string(), value.to_string()));
        }
        if let Some(value) = args.closed {
            params.push(("closed".to_string(), value.to_string()));
        }

        self.execute_json(
            self.client
                .get(format!("{}/markets", GAMMA_BASE_URL))
                .query(&params),
            "markets listing",
        )
        .await
    }

    async fn fetch_series_page(
        &self,
        limit: u32,
        offset: u32,
        args: &ListSeriesArgs,
    ) -> Result<Vec<GammaSeries>, ConnectorError> {
        let mut params = vec![
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];
        if let Some(value) = args.slug.as_ref() {
            params.push(("slug".to_string(), value.clone()));
        }
        if let Some(value) = args.active {
            params.push(("active".to_string(), value.to_string()));
        }
        if let Some(value) = args.closed {
            params.push(("closed".to_string(), value.to_string()));
        }
        if let Some(value) = args.featured {
            params.push(("featured".to_string(), value.to_string()));
        }

        self.execute_json(
            self.client
                .get(format!("{}/series", GAMMA_BASE_URL))
                .query(&params),
            "series listing",
        )
        .await
    }

    async fn fetch_series_by_id(&self, id: &str) -> Result<GammaSeries, ConnectorError> {
        self.execute_json(
            self.client.get(format!("{}/series/{}", GAMMA_BASE_URL, id)),
            "series lookup",
        )
        .await
    }

    async fn fetch_series_by_slug(&self, slug: &str) -> Result<GammaSeries, ConnectorError> {
        let args = ListSeriesArgs {
            limit: 1,
            offset: 0,
            cursor: None,
            slug: Some(slug.to_string()),
            active: None,
            closed: None,
            featured: None,
        };
        let results = self.fetch_series_page(1, 0, &args).await?;
        results
            .into_iter()
            .next()
            .ok_or(ConnectorError::ResourceNotFound)
    }

    async fn fetch_tags_page(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<GammaTag>, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!("{}/tags", GAMMA_BASE_URL))
                .query(&[("limit", limit.to_string()), ("offset", offset.to_string())]),
            "tags listing",
        )
        .await
    }

    async fn fetch_comments_page(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<GammaComment>, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!("{}/comments", GAMMA_BASE_URL))
                .query(&[
                    ("parent_entity_type", entity_type),
                    ("parent_entity_id", entity_id),
                    ("limit", &limit.to_string()),
                    ("offset", &offset.to_string()),
                ]),
            "comments listing",
        )
        .await
    }

    async fn fetch_order_book(&self, token_id: &str) -> Result<ClobOrderBook, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!("{}/book", CLOB_BASE_URL))
                .query(&[("token_id", token_id)]),
            "order book lookup",
        )
        .await
    }

    async fn fetch_price_history(
        &self,
        token_id: &str,
        interval: &str,
        fidelity: u32,
    ) -> Result<ClobPriceHistory, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!("{}/prices-history", CLOB_BASE_URL))
                .query(&[
                    ("market", token_id),
                    ("interval", interval),
                    ("fidelity", &fidelity.to_string()),
                ]),
            "price history lookup",
        )
        .await
    }

    async fn fetch_market_positions(
        &self,
        condition_id: &str,
        limit: u32,
    ) -> Result<Vec<DataMarketPositionGroup>, ConnectorError> {
        self.execute_json(
            self.client
                .get(format!("{}/market-positions", DATA_API_BASE_URL))
                .query(&[("market", condition_id), ("limit", &limit.to_string())]),
            "market positions lookup",
        )
        .await
    }

    async fn fetch_search_page(
        &self,
        query: &str,
        page: u32,
    ) -> Result<GammaSearchResponse, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/public-search", GAMMA_BASE_URL))
            .query(&[
                ("q", query),
                ("page", &page.to_string()),
                ("events_status", "active"),
                ("keep_closed_markets", "0"),
            ])
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket search failed with status {}",
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_event_by_id(&self, id: &str) -> Result<GammaEvent, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/events/{}", GAMMA_BASE_URL, id))
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket event lookup failed with status {}",
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_event_by_slug(&self, slug: &str) -> Result<GammaEvent, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/events/slug/{}", GAMMA_BASE_URL, slug))
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket event lookup failed with status {}",
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_market_by_id(&self, id: &str) -> Result<GammaMarket, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/markets/{}", GAMMA_BASE_URL, id))
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket market lookup failed with status {}",
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn fetch_market_by_slug(&self, slug: &str) -> Result<GammaMarket, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/markets/slug/{}", GAMMA_BASE_URL, slug))
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !response.status().is_success() {
            return Err(ConnectorError::Other(format!(
                "Polymarket market lookup failed with status {}",
                response.status()
            )));
        }

        response.json().await.map_err(ConnectorError::HttpRequest)
    }

    async fn search_events(
        &self,
        query: &str,
        start_page: u32,
        limit: u32,
    ) -> Result<(Vec<GammaEvent>, Option<String>, bool, Option<u64>), ConnectorError> {
        let desired = limit.clamp(1, MAX_SEARCH_LIMIT);
        let first_page = start_page.max(1);
        let mut items = Vec::new();
        let mut total_results = None;
        let mut has_more = false;
        let mut next_cursor = None;

        for page in first_page..first_page.saturating_add(MAX_SEARCH_REQUESTS) {
            let response = self.fetch_search_page(query, page).await?;
            if total_results.is_none() {
                total_results = response.pagination.total_results;
            }

            has_more = response.pagination.has_more;
            items.extend(response.events);

            if items.len() as u32 >= desired {
                items.truncate(desired as usize);
                if has_more {
                    next_cursor = Some(ingest::encode_cursor(&SearchCursor {
                        query: query.to_string(),
                        page: page + 1,
                    })?);
                }
                break;
            }

            if !has_more {
                break;
            }

            next_cursor = Some(ingest::encode_cursor(&SearchCursor {
                query: query.to_string(),
                page: page + 1,
            })?);
        }

        if !has_more {
            next_cursor = None;
        }

        Ok((items, next_cursor, has_more, total_results))
    }

    async fn fetch_market_by_slug_via_list(
        &self,
        slug: &str,
    ) -> Result<GammaMarket, ConnectorError> {
        let args = ListMarketsArgs {
            limit: 1,
            offset: 0,
            cursor: None,
            slug: Some(slug.to_string()),
            event_item_ref: None,
            event_id: None,
            event_slug: None,
            series_id: None,
            series_slug: None,
            tag_slug: None,
            active: None,
            closed: None,
        };
        let results = self.fetch_markets_page_direct(1, 0, &args).await?;
        results
            .into_iter()
            .next()
            .ok_or(ConnectorError::ResourceNotFound)
    }

    async fn enrich_market(&self, market: GammaMarket) -> Result<GammaMarket, ConnectorError> {
        if market.events.is_empty() {
            if let Some(slug) = market.slug.as_deref() {
                if let Ok(enriched) = self.fetch_market_by_slug_via_list(slug).await {
                    return Ok(enriched);
                }
            }
        }
        Ok(market)
    }

    async fn fetch_event_resolved(
        &self,
        event_ref: EventRef,
    ) -> Result<GammaEvent, ConnectorError> {
        match event_ref {
            EventRef::Id(id) => self.fetch_event_by_id(&id).await,
            EventRef::Slug(slug) => self.fetch_event_by_slug(&slug).await,
        }
    }

    async fn fetch_market_resolved(
        &self,
        market_ref: MarketRef,
    ) -> Result<GammaMarket, ConnectorError> {
        let market = match market_ref {
            MarketRef::Id(id) => self.fetch_market_by_id(&id).await?,
            MarketRef::Slug(slug) => self.fetch_market_by_slug(&slug).await?,
        };
        self.enrich_market(market).await
    }

    async fn fetch_series_resolved(
        &self,
        series_ref: SeriesRef,
    ) -> Result<GammaSeries, ConnectorError> {
        match series_ref {
            SeriesRef::Id(id) => self.fetch_series_by_id(&id).await,
            SeriesRef::Slug(slug) => self.fetch_series_by_slug(&slug).await,
        }
    }

    fn resolve_event_ref(args: &GetEventArgs) -> Result<EventRef, ConnectorError> {
        if let Some(item_ref) = args.item_ref.as_deref() {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "polymarket") {
                return match kind.as_str() {
                    "event" => Ok(EventRef::Id(id)),
                    "market" => Err(ConnectorError::InvalidParams(
                        "Item ref points to a market. Use get_market instead.".to_string(),
                    )),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported polymarket item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }

        if let Some(url) = args.url.as_deref() {
            if let Some(captures) = EVENT_URL_RE.captures(url) {
                if let Some(slug) = captures.name("slug") {
                    return Ok(EventRef::Slug(slug.as_str().to_string()));
                }
            }
            return Err(ConnectorError::InvalidParams(
                "Polymarket event URLs must look like https://polymarket.com/event/<slug>"
                    .to_string(),
            ));
        }

        if let Some(slug) = args.slug.as_ref() {
            return Ok(EventRef::Slug(slug.clone()));
        }
        if let Some(id) = args.id.as_ref() {
            return Ok(EventRef::Id(id.clone()));
        }

        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref, url, slug, or id".to_string(),
        ))
    }

    fn resolve_market_ref(args: &GetMarketArgs) -> Result<MarketRef, ConnectorError> {
        if let Some(item_ref) = args.item_ref.as_deref() {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "polymarket") {
                return match kind.as_str() {
                    "market" => Ok(MarketRef::Id(id)),
                    "event" => Err(ConnectorError::InvalidParams(
                        "Item ref points to an event. Use get instead.".to_string(),
                    )),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported polymarket item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }

        if let Some(slug) = args.slug.as_ref() {
            return Ok(MarketRef::Slug(slug.clone()));
        }
        if let Some(id) = args.id.as_ref() {
            return Ok(MarketRef::Id(id.clone()));
        }

        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref, slug, or id".to_string(),
        ))
    }

    fn resolve_series_ref(args: &GetSeriesArgs) -> Result<SeriesRef, ConnectorError> {
        if let Some(slug) = args.slug.as_ref() {
            return Ok(SeriesRef::Slug(slug.clone()));
        }
        if let Some(id) = args.id.as_ref() {
            return Ok(SeriesRef::Id(id.clone()));
        }

        Err(ConnectorError::InvalidParams(
            "Provide one of: slug or id".to_string(),
        ))
    }

    fn parse_offset(offset: u32, cursor: Option<&str>) -> Result<u32, ConnectorError> {
        if let Some(cursor) = cursor {
            let decoded = ingest::decode_cursor::<OffsetCursor>(cursor)
                .ok_or_else(|| ConnectorError::InvalidParams("Invalid cursor".to_string()))?;
            Ok(decoded.offset)
        } else {
            Ok(offset)
        }
    }

    fn encode_next_offset_cursor(
        next_offset: u32,
        has_more: bool,
    ) -> Result<Option<String>, ConnectorError> {
        if has_more {
            Ok(Some(ingest::encode_cursor(&OffsetCursor {
                offset: next_offset,
            })?))
        } else {
            Ok(None)
        }
    }

    fn parse_string_vec(value: &Option<Value>) -> Vec<String> {
        match value {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            Some(Value::String(raw)) => {
                serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    fn parse_float_vec(value: &Option<Value>) -> Vec<f64> {
        match value {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|value| match value {
                    Value::Number(number) => number.as_f64(),
                    Value::String(string) => string.parse::<f64>().ok(),
                    _ => None,
                })
                .collect(),
            Some(Value::String(raw)) => serde_json::from_str::<Vec<String>>(raw)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.parse::<f64>().ok())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn outcome_infos(market: &GammaMarket) -> Vec<OutcomeTokenInfo> {
        let names = Self::parse_string_vec(&market.outcomes);
        let prices = Self::parse_float_vec(&market.outcome_prices);
        let token_ids = Self::parse_string_vec(&market.clob_token_ids);
        let len = names.len().max(prices.len()).max(token_ids.len());

        let mut infos = Vec::with_capacity(len);
        for index in 0..len {
            infos.push(OutcomeTokenInfo {
                name: names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("Outcome {}", index + 1)),
                price: prices.get(index).copied(),
                token_id: token_ids.get(index).cloned(),
            });
        }
        infos
    }

    fn parse_outcomes(market: &GammaMarket) -> Vec<OutcomeSummary> {
        let mut outcomes = Self::outcome_infos(market)
            .into_iter()
            .filter_map(|info| {
                info.price.map(|price| OutcomeSummary {
                    name: info.name,
                    price,
                })
            })
            .collect::<Vec<_>>();

        outcomes.sort_by(|a, b| {
            b.price
                .partial_cmp(&a.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        outcomes
    }

    fn canonical_event_url(slug: Option<&str>) -> Option<String> {
        slug.map(|value| format!("https://polymarket.com/event/{}", value))
    }

    fn series_summaries(event: &GammaEvent) -> Vec<SeriesSummaryView> {
        event.series.iter().map(Self::to_series_summary).collect()
    }

    fn tag_labels(event: &GammaEvent) -> Vec<String> {
        event
            .tags
            .iter()
            .filter_map(|tag| tag.label.clone().or_else(|| tag.slug.clone()))
            .collect()
    }

    fn event_title(event: &GammaEvent) -> String {
        event
            .title
            .clone()
            .unwrap_or_else(|| format!("Polymarket event {}", event.id))
    }

    fn market_question(market: &GammaMarket) -> String {
        market
            .question
            .clone()
            .unwrap_or_else(|| format!("Polymarket market {}", market.id))
    }

    fn series_title(series: &GammaSeries) -> String {
        series
            .title
            .clone()
            .unwrap_or_else(|| format!("Polymarket series {}", series.id))
    }

    fn tag_title(tag: &GammaTag) -> String {
        tag.label
            .clone()
            .or_else(|| tag.slug.clone())
            .or_else(|| tag.id.clone())
            .unwrap_or_else(|| "Polymarket tag".to_string())
    }

    fn to_market_summary(market: &GammaMarket) -> MarketSummary {
        let outcomes = Self::parse_outcomes(market);
        MarketSummary {
            id: market.id.clone(),
            slug: market.slug.clone(),
            question: Self::market_question(market),
            url: Self::canonical_event_url(market.slug.as_deref()),
            active: market.active,
            closed: market.closed,
            accepting_orders: market.accepting_orders,
            start_date: market.start_date.clone(),
            end_date: market.end_date.clone(),
            updated_at: market.updated_at.clone(),
            last_trade_price: market.last_trade_price,
            best_bid: market.best_bid,
            best_ask: market.best_ask,
            liquidity: market.liquidity,
            volume_24hr: market.volume_24hr,
            volume_1wk: market.volume_1wk,
            volume_1mo: market.volume_1mo,
            outcomes,
        }
    }

    fn to_series_summary(series: &GammaSeriesSummary) -> SeriesSummaryView {
        SeriesSummaryView {
            id: series.id.clone(),
            slug: series.slug.clone(),
            ticker: series.ticker.clone(),
            title: series
                .title
                .clone()
                .unwrap_or_else(|| format!("Polymarket series {}", series.id)),
            recurrence: series.recurrence.clone(),
            active: series.active,
            closed: series.closed,
            featured: series.featured,
            comment_count: series.comment_count,
            volume_24hr: series.volume_24hr,
        }
    }

    fn to_tag_summary(tag: &GammaTag) -> TagSummaryView {
        TagSummaryView {
            id: tag.id.clone(),
            label: Self::tag_title(tag),
            slug: tag.slug.clone(),
            created_at: tag.created_at.clone(),
            updated_at: tag.updated_at.clone(),
            requires_translation: tag.requires_translation,
        }
    }

    fn to_event_summary(event: &GammaEvent) -> EventSummary {
        let top_market = event
            .markets
            .iter()
            .filter(|market| market.active && !market.closed)
            .max_by(|left, right| {
                left.volume_24hr
                    .unwrap_or_default()
                    .partial_cmp(&right.volume_24hr.unwrap_or_default())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(Self::to_market_summary);

        EventSummary {
            id: event.id.clone(),
            slug: event.slug.clone(),
            title: Self::event_title(event),
            url: Self::canonical_event_url(event.slug.as_deref()),
            active: event.active,
            closed: event.closed,
            start_date: event.start_date.clone(),
            end_date: event.end_date.clone(),
            updated_at: event.updated_at.clone(),
            comment_count: event.comment_count,
            liquidity: event.liquidity,
            open_interest: event.open_interest,
            volume_24hr: event.volume_24hr,
            volume_1mo: event.volume_1mo,
            tags: Self::tag_labels(event),
            top_market,
            series: Self::series_summaries(event),
        }
    }

    fn to_event_detail(event: &GammaEvent) -> EventDetail {
        let mut markets: Vec<MarketSummary> =
            event.markets.iter().map(Self::to_market_summary).collect();
        markets.sort_by(|left, right| {
            right
                .volume_24hr
                .unwrap_or_default()
                .partial_cmp(&left.volume_24hr.unwrap_or_default())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        EventDetail {
            id: event.id.clone(),
            slug: event.slug.clone(),
            title: Self::event_title(event),
            description: event.description.clone(),
            url: Self::canonical_event_url(event.slug.as_deref()),
            active: event.active,
            closed: event.closed,
            start_date: event.start_date.clone(),
            end_date: event.end_date.clone(),
            created_at: event.created_at.clone(),
            updated_at: event.updated_at.clone(),
            comment_count: event.comment_count,
            liquidity: event.liquidity,
            open_interest: event.open_interest,
            volume_24hr: event.volume_24hr,
            volume_1wk: event.volume_1wk,
            volume_1mo: event.volume_1mo,
            volume: event.volume,
            resolution_source: event.resolution_source.clone(),
            tags: Self::tag_labels(event),
            markets,
            series: Self::series_summaries(event),
        }
    }

    fn to_market_detail(market: &GammaMarket) -> MarketDetail {
        let events = market
            .events
            .iter()
            .map(|event| LinkedEvent {
                id: event.id.clone(),
                slug: event.slug.clone(),
                title: event.title.clone(),
                url: Self::canonical_event_url(event.slug.as_deref()),
            })
            .collect();

        MarketDetail {
            id: market.id.clone(),
            slug: market.slug.clone(),
            question: Self::market_question(market),
            description: market.description.clone(),
            url: Self::canonical_event_url(market.slug.as_deref()),
            active: market.active,
            closed: market.closed,
            accepting_orders: market.accepting_orders,
            start_date: market.start_date.clone(),
            end_date: market.end_date.clone(),
            updated_at: market.updated_at.clone(),
            condition_id: market.condition_id.clone(),
            resolution_source: market.resolution_source.clone(),
            last_trade_price: market.last_trade_price,
            best_bid: market.best_bid,
            best_ask: market.best_ask,
            liquidity: market.liquidity,
            volume: market.volume,
            volume_24hr: market.volume_24hr,
            volume_1wk: market.volume_1wk,
            volume_1mo: market.volume_1mo,
            clob_token_ids: Self::parse_string_vec(&market.clob_token_ids),
            outcomes: Self::parse_outcomes(market),
            events,
        }
    }

    fn to_series_detail(series: &GammaSeries) -> SeriesDetailView {
        SeriesDetailView {
            id: series.id.clone(),
            slug: series.slug.clone(),
            ticker: series.ticker.clone(),
            title: Self::series_title(series),
            recurrence: series.recurrence.clone(),
            active: series.active,
            closed: series.closed,
            archived: series.archived,
            featured: series.featured,
            restricted: series.restricted,
            comment_count: series.comment_count,
            volume_24hr: series.volume_24hr,
            volume: series.volume,
            liquidity: series.liquidity,
            events: series
                .events
                .iter()
                .map(|event| LinkedEvent {
                    id: event.id.clone(),
                    slug: event.slug.clone(),
                    title: event.title.clone(),
                    url: Self::canonical_event_url(event.slug.as_deref()),
                })
                .collect(),
        }
    }

    fn to_comment_summary(comment: &GammaComment) -> CommentSummaryView {
        CommentSummaryView {
            id: comment.id.clone(),
            body: comment.body.clone().unwrap_or_default(),
            created_at: comment.created_at.clone(),
            parent_entity_type: comment.parent_entity_type.clone(),
            parent_entity_id: comment.parent_entity_id,
            reaction_count: comment.reaction_count,
            user_address: comment.user_address.clone(),
            profile: comment.profile.as_ref().map(|profile| CommentProfileView {
                name: profile.name.clone(),
                pseudonym: profile.pseudonym.clone(),
                profile_image: profile.profile_image.clone(),
                display_username_public: profile.display_username_public,
                verified: profile.verified,
            }),
        }
    }

    fn parse_level(level: &ClobOrderLevel) -> Option<OrderLevelView> {
        Some(OrderLevelView {
            price: level.price.parse::<f64>().ok()?,
            size: level.size.parse::<f64>().ok()?,
        })
    }

    fn to_order_book_view(
        outcome: &OutcomeTokenInfo,
        book: &ClobOrderBook,
        depth: usize,
    ) -> OutcomeOrderBookView {
        let best_bid = book.bids.first().and_then(Self::parse_level);
        let best_ask = book.asks.first().and_then(Self::parse_level);
        let midpoint = match (&best_bid, &best_ask) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2.0),
            _ => None,
        };
        let spread = match (&best_bid, &best_ask) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        };

        OutcomeOrderBookView {
            outcome: outcome.name.clone(),
            token_id: outcome.token_id.clone().unwrap_or_default(),
            best_bid,
            best_ask,
            midpoint,
            spread,
            min_order_size: book
                .min_order_size
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok()),
            tick_size: book
                .tick_size
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok()),
            last_trade_price: book
                .last_trade_price
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok()),
            neg_risk: book.neg_risk,
            timestamp: book.timestamp.clone(),
            top_bids: book
                .bids
                .iter()
                .take(depth)
                .filter_map(Self::parse_level)
                .collect(),
            top_asks: book
                .asks
                .iter()
                .take(depth)
                .filter_map(Self::parse_level)
                .collect(),
        }
    }

    fn to_price_history_view(
        outcome: &OutcomeTokenInfo,
        history: ClobPriceHistory,
    ) -> OutcomePriceHistoryView {
        let points = history
            .history
            .into_iter()
            .map(|point| PricePointView {
                timestamp: point.t,
                price: point.p,
            })
            .collect::<Vec<_>>();

        let summary = if points.is_empty() {
            None
        } else {
            let first_price = points.first().map(|point| point.price).unwrap_or_default();
            let last_price = points.last().map(|point| point.price).unwrap_or_default();
            let min_price = points
                .iter()
                .map(|point| point.price)
                .fold(f64::INFINITY, f64::min);
            let max_price = points
                .iter()
                .map(|point| point.price)
                .fold(f64::NEG_INFINITY, f64::max);
            let change_abs = last_price - first_price;
            let change_pct = if first_price.abs() > f64::EPSILON {
                Some(change_abs / first_price)
            } else {
                None
            };

            Some(PriceHistorySummaryView {
                first_price,
                last_price,
                min_price,
                max_price,
                change_abs,
                change_pct,
            })
        };

        OutcomePriceHistoryView {
            outcome: outcome.name.clone(),
            token_id: outcome.token_id.clone().unwrap_or_default(),
            points,
            summary,
        }
    }

    fn to_market_position_groups(
        groups: Vec<DataMarketPositionGroup>,
    ) -> Vec<MarketPositionGroupView> {
        groups
            .into_iter()
            .map(|group| MarketPositionGroupView {
                outcome: group
                    .positions
                    .first()
                    .and_then(|position| position.outcome.clone()),
                token_id: group.token,
                positions: group
                    .positions
                    .into_iter()
                    .map(|position| MarketPositionView {
                        proxy_wallet: position.proxy_wallet,
                        name: position.name,
                        profile_image: position.profile_image,
                        verified: position.verified,
                        avg_price: position.avg_price,
                        size: position.size,
                        curr_price: position.curr_price,
                        current_value: position.current_value,
                        cash_pnl: position.cash_pnl,
                        realized_pnl: position.realized_pnl,
                        total_pnl: position.total_pnl,
                        outcome: position.outcome,
                        outcome_index: position.outcome_index,
                    })
                    .collect(),
            })
            .collect()
    }

    fn event_list_text(events: &[EventSummary]) -> String {
        events
            .iter()
            .take(5)
            .map(Self::event_search_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn market_list_text(markets: &[MarketListItemView]) -> String {
        markets
            .iter()
            .take(5)
            .map(|item| {
                if let Some(parent) = item
                    .parent_event
                    .as_ref()
                    .and_then(|event| event.title.as_ref())
                {
                    format!("{} | event: {}", item.market.question, parent)
                } else {
                    item.market.question.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn series_list_text(series: &[SeriesSummaryView]) -> String {
        series
            .iter()
            .take(5)
            .map(|item| {
                let mut parts = vec![item.title.clone()];
                if let Some(recurrence) = item.recurrence.as_deref() {
                    parts.push(recurrence.to_string());
                }
                if let Some(volume_24hr) = item.volume_24hr {
                    parts.push(format!("24h volume ${:.0}", volume_24hr));
                }
                parts.join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tag_list_text(tags: &[TagSummaryView]) -> String {
        tags.iter()
            .take(20)
            .map(|tag| match tag.slug.as_deref() {
                Some(slug) if slug != tag.label => format!("{} ({})", tag.label, slug),
                _ => tag.label.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn comments_list_text(comments: &[CommentSummaryView]) -> String {
        comments
            .iter()
            .take(5)
            .map(|comment| {
                let author = comment
                    .profile
                    .as_ref()
                    .and_then(|profile| profile.name.as_ref())
                    .cloned()
                    .unwrap_or_else(|| "anonymous".to_string());
                format!("{}: {}", author, comment.body.replace('\n', " "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn order_book_text(books: &[OutcomeOrderBookView]) -> String {
        books
            .iter()
            .map(|book| {
                format!(
                    "{} | bid {} | ask {} | mid {}",
                    book.outcome,
                    book.best_bid
                        .as_ref()
                        .map(|level| format!("{:.3}", level.price))
                        .unwrap_or_else(|| "-".to_string()),
                    book.best_ask
                        .as_ref()
                        .map(|level| format!("{:.3}", level.price))
                        .unwrap_or_else(|| "-".to_string()),
                    book.midpoint
                        .map(|value| format!("{:.3}", value))
                        .unwrap_or_else(|| "-".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn price_history_text(history: &[OutcomePriceHistoryView]) -> String {
        history
            .iter()
            .map(|series| {
                if let Some(summary) = series.summary.as_ref() {
                    format!(
                        "{} | last {:.3} | change {}",
                        series.outcome,
                        summary.last_price,
                        summary
                            .change_pct
                            .map(|value| format!("{:.1}%", value * 100.0))
                            .unwrap_or_else(|| "n/a".to_string())
                    )
                } else {
                    format!("{} | no price history", series.outcome)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn positions_text(groups: &[MarketPositionGroupView]) -> String {
        groups
            .iter()
            .map(|group| {
                let top = group.positions.first();
                let holder = top
                    .and_then(|position| position.name.as_ref())
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let size = top
                    .and_then(|position| position.size)
                    .map(|value| format!("{:.0}", value))
                    .unwrap_or_else(|| "-".to_string());
                format!(
                    "{} | top holder {} | size {}",
                    group
                        .outcome
                        .clone()
                        .unwrap_or_else(|| group.token_id.clone()),
                    holder,
                    size
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn build_market_list(
        &self,
        args: &ListMarketsArgs,
    ) -> Result<MarketListResponseView, ConnectorError> {
        let offset = Self::parse_offset(args.offset, args.cursor.as_deref())?;
        let limit = Self::clamp_list_limit(args.limit);

        if let Some(slug) = args.slug.as_ref() {
            let markets = self
                .fetch_markets_page_direct(
                    1,
                    0,
                    &ListMarketsArgs {
                        limit: 1,
                        offset: 0,
                        cursor: None,
                        slug: Some(slug.clone()),
                        event_item_ref: None,
                        event_id: None,
                        event_slug: None,
                        series_id: None,
                        series_slug: None,
                        tag_slug: None,
                        active: None,
                        closed: None,
                    },
                )
                .await?;
            let items = markets
                .into_iter()
                .map(|market| MarketListItemView {
                    parent_event: market.events.first().map(|event| LinkedEvent {
                        id: event.id.clone(),
                        slug: event.slug.clone(),
                        title: event.title.clone(),
                        url: Self::canonical_event_url(event.slug.as_deref()),
                    }),
                    market: Self::to_market_summary(&market),
                })
                .collect::<Vec<_>>();
            let has_more = false;
            let next_cursor = None;
            return Ok(MarketListResponseView {
                markets: items,
                pagination: OffsetPaginationView {
                    offset,
                    next_cursor,
                    has_more,
                },
                source: "direct_market_lookup".to_string(),
            });
        }

        if args.event_item_ref.is_some()
            || args.event_id.is_some()
            || args.event_slug.is_some()
            || args.series_id.is_some()
            || args.series_slug.is_some()
            || args.tag_slug.is_some()
        {
            let mut collected = Vec::new();
            let target = offset + limit;

            if args.event_item_ref.is_some() || args.event_id.is_some() || args.event_slug.is_some()
            {
                let event_ref = if let Some(item_ref) = args.event_item_ref.as_deref() {
                    let event_args = GetEventArgs {
                        item_ref: Some(item_ref.to_string()),
                        url: None,
                        id: None,
                        slug: None,
                        output_format: OutputFormat::Raw,
                    };
                    Self::resolve_event_ref(&event_args)?
                } else {
                    args.event_slug
                        .clone()
                        .map(EventRef::Slug)
                        .or_else(|| args.event_id.clone().map(EventRef::Id))
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams(
                                "Provide one of: event_item_ref, event_id, or event_slug"
                                    .to_string(),
                            )
                        })?
                };
                let event = self.fetch_event_resolved(event_ref).await?;
                let total_markets = event.markets.len();
                for market in &event.markets {
                    collected.push(MarketListItemView {
                        market: Self::to_market_summary(market),
                        parent_event: Some(LinkedEvent {
                            id: Some(event.id.clone()),
                            slug: event.slug.clone(),
                            title: event.title.clone(),
                            url: Self::canonical_event_url(event.slug.as_deref()),
                        }),
                    });
                }
                collected.sort_by(|left, right| {
                    right
                        .market
                        .volume_24hr
                        .unwrap_or_default()
                        .partial_cmp(&left.market.volume_24hr.unwrap_or_default())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let page = collected
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect::<Vec<_>>();
                let has_more = (offset as usize + page.len()) < total_markets;
                let next_cursor =
                    Self::encode_next_offset_cursor(offset + page.len() as u32, has_more)?;
                return Ok(MarketListResponseView {
                    markets: page,
                    pagination: OffsetPaginationView {
                        offset,
                        next_cursor,
                        has_more,
                    },
                    source: "event_markets".to_string(),
                });
            }

            let mut event_offset = 0u32;
            for _ in 0..MAX_EVENT_SCAN_REQUESTS {
                let events = self
                    .fetch_events_page(
                        Self::clamp_list_limit(limit.max(20)),
                        event_offset,
                        &ListEventsArgs {
                            limit,
                            offset: 0,
                            cursor: None,
                            series_id: args.series_id.clone(),
                            series_slug: args.series_slug.clone(),
                            tag_slug: args.tag_slug.clone(),
                            active: args.active,
                            closed: args.closed,
                            archived: None,
                            featured: None,
                        },
                    )
                    .await?;
                if events.is_empty() {
                    break;
                }
                for event in &events {
                    for market in &event.markets {
                        collected.push(MarketListItemView {
                            market: Self::to_market_summary(market),
                            parent_event: Some(LinkedEvent {
                                id: Some(event.id.clone()),
                                slug: event.slug.clone(),
                                title: event.title.clone(),
                                url: Self::canonical_event_url(event.slug.as_deref()),
                            }),
                        });
                        if collected.len() as u32 >= target {
                            break;
                        }
                    }
                    if collected.len() as u32 >= target {
                        break;
                    }
                }
                if collected.len() as u32 >= target
                    || events.len() < Self::clamp_list_limit(limit.max(20)) as usize
                {
                    let has_more = collected.len() as u32 > offset + limit
                        || events.len() == Self::clamp_list_limit(limit.max(20)) as usize;
                    let page = collected
                        .into_iter()
                        .skip(offset as usize)
                        .take(limit as usize)
                        .collect::<Vec<_>>();
                    let next_cursor =
                        Self::encode_next_offset_cursor(offset + page.len() as u32, has_more)?;
                    return Ok(MarketListResponseView {
                        markets: page,
                        pagination: OffsetPaginationView {
                            offset,
                            next_cursor,
                            has_more,
                        },
                        source: "flattened_event_markets".to_string(),
                    });
                }
                event_offset += Self::clamp_list_limit(limit.max(20));
            }

            let has_more = collected.len() as u32 > offset + limit;
            let page = collected
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect::<Vec<_>>();
            let next_cursor =
                Self::encode_next_offset_cursor(offset + page.len() as u32, has_more)?;
            return Ok(MarketListResponseView {
                markets: page,
                pagination: OffsetPaginationView {
                    offset,
                    next_cursor,
                    has_more,
                },
                source: "flattened_event_markets".to_string(),
            });
        }

        let markets = self.fetch_markets_page_direct(limit, offset, args).await?;
        let has_more = markets.len() == limit as usize;
        let next_cursor = Self::encode_next_offset_cursor(offset + markets.len() as u32, has_more)?;
        Ok(MarketListResponseView {
            markets: markets
                .into_iter()
                .map(|market| MarketListItemView {
                    parent_event: market.events.first().map(|event| LinkedEvent {
                        id: event.id.clone(),
                        slug: event.slug.clone(),
                        title: event.title.clone(),
                        url: Self::canonical_event_url(event.slug.as_deref()),
                    }),
                    market: Self::to_market_summary(&market),
                })
                .collect(),
            pagination: OffsetPaginationView {
                offset,
                next_cursor,
                has_more,
            },
            source: "global_markets".to_string(),
        })
    }

    async fn resolve_comment_target(
        &self,
        args: &ListCommentsArgs,
    ) -> Result<CommentTarget, ConnectorError> {
        if let Some(item_ref) = args.item_ref.as_deref() {
            if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "polymarket") {
                return match kind.as_str() {
                    "event" => {
                        let event = self.fetch_event_by_id(&id).await?;
                        Ok(CommentTarget {
                            entity_type: "Event".to_string(),
                            entity_id: event.id,
                            entity_slug: event.slug.clone(),
                            entity_title: event.title.clone(),
                        })
                    }
                    "market" => {
                        let market = self.fetch_market_resolved(MarketRef::Id(id)).await?;
                        Ok(CommentTarget {
                            entity_type: "Market".to_string(),
                            entity_id: market.id.clone(),
                            entity_slug: market.slug.clone(),
                            entity_title: market.question.clone(),
                        })
                    }
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Unsupported polymarket item_ref kind '{}'",
                        kind
                    ))),
                };
            }
        }

        if let Some(url) = args.event_url.as_deref() {
            let event_args = GetEventArgs {
                item_ref: None,
                url: Some(url.to_string()),
                id: None,
                slug: None,
                output_format: OutputFormat::Raw,
            };
            let event = self
                .fetch_event_resolved(Self::resolve_event_ref(&event_args)?)
                .await?;
            return Ok(CommentTarget {
                entity_type: "Event".to_string(),
                entity_id: event.id,
                entity_slug: event.slug.clone(),
                entity_title: event.title.clone(),
            });
        }

        if let Some(id) = args.event_id.as_ref() {
            let event = self.fetch_event_by_id(id).await?;
            return Ok(CommentTarget {
                entity_type: "Event".to_string(),
                entity_id: event.id,
                entity_slug: event.slug.clone(),
                entity_title: event.title.clone(),
            });
        }
        if let Some(slug) = args.event_slug.as_ref() {
            let event = self.fetch_event_by_slug(slug).await?;
            return Ok(CommentTarget {
                entity_type: "Event".to_string(),
                entity_id: event.id,
                entity_slug: event.slug.clone(),
                entity_title: event.title.clone(),
            });
        }
        if let Some(id) = args.market_id.as_ref() {
            let market = self
                .fetch_market_resolved(MarketRef::Id(id.clone()))
                .await?;
            return Ok(CommentTarget {
                entity_type: "Market".to_string(),
                entity_id: market.id,
                entity_slug: market.slug.clone(),
                entity_title: market.question.clone(),
            });
        }
        if let Some(slug) = args.market_slug.as_ref() {
            let market = self
                .fetch_market_resolved(MarketRef::Slug(slug.clone()))
                .await?;
            return Ok(CommentTarget {
                entity_type: "Market".to_string(),
                entity_id: market.id,
                entity_slug: market.slug.clone(),
                entity_title: market.question.clone(),
            });
        }
        if let Some(id) = args.series_id.as_ref() {
            let series = self.fetch_series_by_id(id).await?;
            return Ok(CommentTarget {
                entity_type: "Series".to_string(),
                entity_id: series.id,
                entity_slug: series.slug.clone(),
                entity_title: series.title.clone(),
            });
        }
        if let Some(slug) = args.series_slug.as_ref() {
            let series = self.fetch_series_by_slug(slug).await?;
            return Ok(CommentTarget {
                entity_type: "Series".to_string(),
                entity_id: series.id,
                entity_slug: series.slug.clone(),
                entity_title: series.title.clone(),
            });
        }

        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref, event_url, event_id, event_slug, market_id, market_slug, series_id, or series_slug".to_string(),
        ))
    }

    fn select_outcomes<'a>(
        infos: &'a [OutcomeTokenInfo],
        outcome: Option<&str>,
        token_id: Option<&str>,
    ) -> Result<Vec<&'a OutcomeTokenInfo>, ConnectorError> {
        if let Some(token_id) = token_id {
            let info = infos
                .iter()
                .find(|info| info.token_id.as_deref() == Some(token_id))
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(format!(
                        "No outcome matched token_id '{}'",
                        token_id
                    ))
                })?;
            return Ok(vec![info]);
        }

        if let Some(outcome) = outcome {
            let normalized = outcome.to_lowercase();
            let info = infos
                .iter()
                .find(|info| info.name.to_lowercase() == normalized)
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(format!("No outcome matched '{}'", outcome))
                })?;
            return Ok(vec![info]);
        }

        Ok(infos.iter().collect())
    }

    async fn build_order_books(
        &self,
        market: &GammaMarket,
        outcome: Option<&str>,
        token_id: Option<&str>,
        depth: usize,
    ) -> Result<Vec<OutcomeOrderBookView>, ConnectorError> {
        let infos = Self::outcome_infos(market);
        let selected = Self::select_outcomes(&infos, outcome, token_id)?;
        let mut books = Vec::new();

        for info in selected {
            let Some(token_id) = info.token_id.as_deref() else {
                continue;
            };
            let book = self.fetch_order_book(token_id).await?;
            books.push(Self::to_order_book_view(info, &book, depth));
        }

        Ok(books)
    }

    async fn build_price_history(
        &self,
        market: &GammaMarket,
        outcome: Option<&str>,
        token_id: Option<&str>,
        interval: &str,
        fidelity: u32,
    ) -> Result<Vec<OutcomePriceHistoryView>, ConnectorError> {
        let infos = Self::outcome_infos(market);
        let selected = Self::select_outcomes(&infos, outcome, token_id)?;
        let mut history = Vec::new();

        for info in selected {
            let Some(token_id) = info.token_id.as_deref() else {
                continue;
            };
            let series = self
                .fetch_price_history(token_id, interval, fidelity)
                .await?;
            history.push(Self::to_price_history_view(info, series));
        }

        Ok(history)
    }

    async fn build_positions(
        &self,
        market: &GammaMarket,
        limit: u32,
    ) -> Result<Vec<MarketPositionGroupView>, ConnectorError> {
        let condition_id = market.condition_id.as_deref().ok_or_else(|| {
            ConnectorError::Other("Market does not include a condition_id".to_string())
        })?;
        let positions = self.fetch_market_positions(condition_id, limit).await?;
        Ok(Self::to_market_position_groups(positions))
    }

    async fn resolve_parent_event_detail(
        &self,
        market: &GammaMarket,
    ) -> Result<Option<EventDetail>, ConnectorError> {
        if let Some(event_ref) = market.events.first() {
            let event = if let Some(id) = event_ref.id.as_deref() {
                self.fetch_event_by_id(id).await?
            } else if let Some(slug) = event_ref.slug.as_deref() {
                self.fetch_event_by_slug(slug).await?
            } else {
                return Ok(None);
            };
            return Ok(Some(Self::to_event_detail(&event)));
        }

        if let Some(slug) = market.slug.as_deref() {
            let enriched = self.fetch_market_by_slug_via_list(slug).await?;
            if let Some(event_ref) = enriched.events.first() {
                let event = if let Some(id) = event_ref.id.as_deref() {
                    self.fetch_event_by_id(id).await?
                } else if let Some(event_slug) = event_ref.slug.as_deref() {
                    self.fetch_event_by_slug(event_slug).await?
                } else {
                    return Ok(None);
                };
                return Ok(Some(Self::to_event_detail(&event)));
            }
        }

        Ok(None)
    }

    fn event_search_text(summary: &EventSummary) -> String {
        let mut parts = vec![summary.title.clone()];
        if let Some(top_market) = &summary.top_market {
            parts.push(format!("top market: {}", top_market.question));
            if !top_market.outcomes.is_empty() {
                let outcomes = top_market
                    .outcomes
                    .iter()
                    .take(3)
                    .map(|outcome| format!("{} {:.1}%", outcome.name, outcome.price * 100.0))
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("outcomes: {}", outcomes));
            }
        }
        if let Some(volume_24hr) = summary.volume_24hr {
            parts.push(format!("24h volume ${:.0}", volume_24hr));
        }
        if !summary.tags.is_empty() {
            parts.push(format!("tags: {}", summary.tags.join(", ")));
        }
        parts.join(" | ")
    }

    fn event_detail_text(detail: &EventDetail) -> String {
        let mut parts = vec![detail.title.clone()];
        if let Some(description) = detail.description.as_deref() {
            if !description.trim().is_empty() {
                parts.push(description.trim().to_string());
            }
        }
        if !detail.markets.is_empty() {
            let preview = detail
                .markets
                .iter()
                .take(3)
                .map(|market| {
                    if market.outcomes.is_empty() {
                        market.question.clone()
                    } else {
                        let outcomes = market
                            .outcomes
                            .iter()
                            .take(2)
                            .map(|outcome| {
                                format!("{} {:.1}%", outcome.name, outcome.price * 100.0)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{} ({})", market.question, outcomes)
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            parts.push(preview);
        }
        parts.join("\n\n")
    }

    fn market_detail_text(detail: &MarketDetail) -> String {
        let mut parts = vec![detail.question.clone()];
        if !detail.outcomes.is_empty() {
            let outcomes = detail
                .outcomes
                .iter()
                .take(5)
                .map(|outcome| format!("{} {:.1}%", outcome.name, outcome.price * 100.0))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("Outcomes: {}", outcomes));
        }
        if let Some(volume_24hr) = detail.volume_24hr {
            parts.push(format!("24h volume ${:.0}", volume_24hr));
        }
        if let Some(liquidity) = detail.liquidity {
            parts.push(format!("liquidity ${:.0}", liquidity));
        }
        parts.join(" | ")
    }

    fn event_summary_item(summary: &EventSummary) -> Result<ContentItem, ConnectorError> {
        let block = ContentBlock {
            block_ref: format!("polymarket:event:{}:summary", summary.id),
            block_kind: "summary".to_string(),
            text: Self::event_search_text(summary),
            author: None,
            created_at: None,
            reply_to: None,
            position: Some(json!({ "section": "search_result" })),
            score: summary.volume_24hr,
            attachments: Vec::new(),
            metadata: Some(
                serde_json::to_value(summary).map_err(|e| ConnectorError::Other(e.to_string()))?,
            ),
        };

        Ok(ContentItem {
            item_ref: format!("polymarket:event:{}", summary.id),
            kind: "prediction_event".to_string(),
            canonical_url: summary.url.clone(),
            title: Some(summary.title.clone()),
            created_at: summary.start_date.clone(),
            source_updated_at: summary.updated_at.clone(),
            authors: Vec::new(),
            tags: summary.tags.clone(),
            metadata: Some(
                serde_json::to_value(summary).map_err(|e| ConnectorError::Other(e.to_string()))?,
            ),
            blocks: vec![block],
            relationships: Vec::new(),
            truncation: None,
        })
    }

    fn event_detail_item(detail: &EventDetail) -> Result<ContentItem, ConnectorError> {
        let mut blocks = Vec::new();
        blocks.push(ContentBlock {
            block_ref: format!("polymarket:event:{}:summary", detail.id),
            block_kind: "summary".to_string(),
            text: detail.title.clone(),
            author: None,
            created_at: detail.created_at.clone(),
            reply_to: None,
            position: Some(json!({ "section": "header" })),
            score: detail.volume_24hr,
            attachments: Vec::new(),
            metadata: Some(json!({
                "liquidity": detail.liquidity,
                "open_interest": detail.open_interest,
                "volume_24hr": detail.volume_24hr,
                "volume_1mo": detail.volume_1mo,
                "comment_count": detail.comment_count,
            })),
        });

        if let Some(description) = detail.description.as_ref() {
            if !description.trim().is_empty() {
                blocks.push(ContentBlock {
                    block_ref: format!("polymarket:event:{}:description", detail.id),
                    block_kind: "description".to_string(),
                    text: description.trim().to_string(),
                    author: None,
                    created_at: None,
                    reply_to: None,
                    position: Some(json!({ "section": "description" })),
                    score: None,
                    attachments: Vec::new(),
                    metadata: None,
                });
            }
        }

        let market_limit = 5usize;
        for (index, market) in detail.markets.iter().take(market_limit).enumerate() {
            blocks.push(ContentBlock {
                block_ref: format!("polymarket:event:{}:market:{}", detail.id, market.id),
                block_kind: "market".to_string(),
                text: Self::market_detail_text(&MarketDetail {
                    id: market.id.clone(),
                    slug: market.slug.clone(),
                    question: market.question.clone(),
                    description: None,
                    url: market.url.clone(),
                    active: market.active,
                    closed: market.closed,
                    accepting_orders: market.accepting_orders,
                    start_date: market.start_date.clone(),
                    end_date: market.end_date.clone(),
                    updated_at: market.updated_at.clone(),
                    condition_id: None,
                    resolution_source: None,
                    last_trade_price: market.last_trade_price,
                    best_bid: market.best_bid,
                    best_ask: market.best_ask,
                    liquidity: market.liquidity,
                    volume: None,
                    volume_24hr: market.volume_24hr,
                    volume_1wk: market.volume_1wk,
                    volume_1mo: market.volume_1mo,
                    clob_token_ids: Vec::new(),
                    outcomes: market.outcomes.clone(),
                    events: Vec::new(),
                }),
                author: None,
                created_at: None,
                reply_to: None,
                position: Some(json!({ "section": "markets", "index": index })),
                score: market.volume_24hr,
                attachments: Vec::new(),
                metadata: Some(
                    serde_json::to_value(market)
                        .map_err(|e| ConnectorError::Other(e.to_string()))?,
                ),
            });
        }

        let truncation = if detail.markets.len() > market_limit {
            Some(crate::ingest::Truncation {
                is_truncated: true,
                reason: format!(
                    "Included the top {} markets by 24h volume out of {} total markets.",
                    market_limit,
                    detail.markets.len()
                ),
                total_blocks_hint: Some(
                    detail.markets.len() as u64 + blocks.len() as u64 - market_limit as u64,
                ),
                returned_blocks: blocks.len() as u64,
                policy: Some("top_markets_by_volume_24hr".to_string()),
            })
        } else {
            None
        };

        Ok(ContentItem {
            item_ref: format!("polymarket:event:{}", detail.id),
            kind: "prediction_event".to_string(),
            canonical_url: detail.url.clone(),
            title: Some(detail.title.clone()),
            created_at: detail
                .created_at
                .clone()
                .or_else(|| detail.start_date.clone()),
            source_updated_at: detail.updated_at.clone(),
            authors: Vec::new(),
            tags: detail.tags.clone(),
            metadata: Some(
                serde_json::to_value(detail).map_err(|e| ConnectorError::Other(e.to_string()))?,
            ),
            blocks,
            relationships: Vec::new(),
            truncation,
        })
    }

    fn market_detail_item(detail: &MarketDetail) -> Result<ContentItem, ConnectorError> {
        let mut relationships = Vec::new();
        for event in &detail.events {
            if let Some(event_id) = event.id.as_ref() {
                relationships.push(Relationship {
                    rel: "belongs_to".to_string(),
                    from: format!("polymarket:market:{}", detail.id),
                    to: format!("polymarket:event:{}", event_id),
                });
            }
        }

        let authors = detail
            .events
            .iter()
            .filter_map(|event| event.title.as_ref())
            .map(|title| Author {
                name: title.clone(),
                id: None,
            })
            .collect::<Vec<_>>();

        let blocks = vec![ContentBlock {
            block_ref: format!("polymarket:market:{}:summary", detail.id),
            block_kind: "market".to_string(),
            text: Self::market_detail_text(detail),
            author: authors.first().cloned(),
            created_at: detail.start_date.clone(),
            reply_to: None,
            position: Some(json!({ "section": "market" })),
            score: detail.volume_24hr,
            attachments: Vec::new(),
            metadata: Some(
                serde_json::to_value(detail).map_err(|e| ConnectorError::Other(e.to_string()))?,
            ),
        }];

        Ok(ContentItem {
            item_ref: format!("polymarket:market:{}", detail.id),
            kind: "prediction_market".to_string(),
            canonical_url: detail.url.clone(),
            title: Some(detail.question.clone()),
            created_at: detail.start_date.clone(),
            source_updated_at: detail.updated_at.clone(),
            authors,
            tags: vec!["polymarket".to_string()],
            metadata: Some(
                serde_json::to_value(detail).map_err(|e| ConnectorError::Other(e.to_string()))?,
            ),
            blocks,
            relationships,
            truncation: None,
        })
    }
}

enum EventRef {
    Id(String),
    Slug(String),
}

enum MarketRef {
    Id(String),
    Slug(String),
}

enum SeriesRef {
    Id(String),
    Slug(String),
}

struct CommentTarget {
    entity_type: String,
    entity_id: String,
    entity_slug: Option<String>,
    entity_title: Option<String>,
}

fn default_search_limit() -> u32 {
    DEFAULT_SEARCH_LIMIT
}

fn default_search_page() -> u32 {
    1
}

fn default_list_limit() -> u32 {
    DEFAULT_LIST_LIMIT
}

fn default_order_book_depth() -> usize {
    DEFAULT_ORDER_BOOK_DEPTH
}

fn default_price_history_interval() -> String {
    "1d".to_string()
}

fn default_price_history_fidelity() -> u32 {
    60
}

fn de_opt_stringish<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(string)) => Ok(Some(string)),
        Some(Value::Number(number)) => Ok(Some(number.to_string())),
        Some(Value::Bool(boolean)) => Ok(Some(boolean.to_string())),
        Some(other) => Err(de::Error::custom(format!(
            "expected string/number/bool, got {}",
            other
        ))),
    }
}

fn de_opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| de::Error::custom("invalid numeric value"))
            .map(Some),
        Some(Value::String(string)) => string
            .parse::<f64>()
            .map(Some)
            .map_err(|_| de::Error::custom(format!("invalid float string '{}'", string))),
        Some(other) => Err(de::Error::custom(format!(
            "expected float-compatible value, got {}",
            other
        ))),
    }
}

fn de_opt_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .ok_or_else(|| de::Error::custom("invalid integer value"))
            .map(Some),
        Some(Value::String(string)) => string
            .parse::<u64>()
            .map(Some)
            .map_err(|_| de::Error::custom(format!("invalid integer string '{}'", string))),
        Some(other) => Err(de::Error::custom(format!(
            "expected integer-compatible value, got {}",
            other
        ))),
    }
}

#[async_trait]
impl Connector for PolymarketConnector {
    fn name(&self) -> &'static str {
        "polymarket"
    }

    fn description(&self) -> &'static str {
        "Public read-only access to Polymarket events and markets via the Gamma API"
    }

    fn display_name(&self) -> &'static str {
        "Polymarket"
    }

    fn icon(&self) -> &'static str {
        "polymarket"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["prediction-markets", "news", "finance"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: EVENT_URL_RE.as_str().to_string(),
            default_tool: "get".to_string(),
            description: "Fetch a Polymarket event by frontend event URL".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "slug".to_string(),
                use_full_url: false,
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
                website_url: Some("https://polymarket.com".to_string()),
            },
            instructions: Some(
                "Use search/list_tags/list_events/list_series for discovery, get/get_market/get_series for core entities, list_comments for discussion context, order_book/price_history/market_positions for market analysis, and get_market_context when you want the important market context assembled in one response. This connector is read-only and does not require authentication."
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
                    "Search active Polymarket prediction events. Use this to discover current markets by keyword or topic. Example: query=\"bitcoin\" limit=10.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query for Polymarket events."
                            },
                            "page": {
                                "type": "integer",
                                "minimum": 1,
                                "default": 1,
                                "description": "Starting page number for direct paging. Cursor takes precedence when provided."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 100,
                                "default": 10,
                                "description": "Total number of events to return. The connector paginates internally because Polymarket currently returns 5 events per page."
                            },
                            "cursor": {
                                "type": ["string", "null"],
                                "description": "Opaque cursor from a previous normalized response."
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                                "default": "raw"
                            }
                        },
                        "required": ["query"],
                        "examples": [
                            {
                                "description": "Search crypto-related markets",
                                "input": { "query": "bitcoin", "limit": 10 }
                            },
                            {
                                "description": "Search election markets as normalized output",
                                "input": { "query": "US election", "limit": 15, "output_format": "normalized_v1" }
                            }
                        ],
                        "_meta": {
                            "category": "search",
                            "tags": ["prediction-markets", "news", "finance"],
                            "auth_required": false,
                            "supports_output_format": true,
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
                name: Cow::Borrowed("list_tags"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Polymarket tags. Use this before list_events/list_markets when you need to discover valid tag slugs instead of guessing.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "offset": { "type": "integer", "minimum": 0, "default": 0 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_tags response." }
                        },
                        "examples": [
                            { "description": "Browse the first page of tags", "input": { "limit": 20 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "tags", "finance"],
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
                name: Cow::Borrowed("list_events"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Polymarket events with structured filters like series_slug, tag_slug, and status flags. Use this when keyword search is too fuzzy.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "offset": { "type": "integer", "minimum": 0, "default": 0 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_events response." },
                            "series_id": { "type": ["string", "integer"], "description": "Filter to a series id." },
                            "series_slug": { "type": "string", "description": "Filter to a series slug like ncaa-cbb." },
                            "tag_slug": { "type": "string", "description": "Filter to a tag slug like crypto or sports." },
                            "active": { "type": "boolean" },
                            "closed": { "type": "boolean" },
                            "archived": { "type": "boolean" },
                            "featured": { "type": "boolean" }
                        },
                        "examples": [
                            { "description": "Browse active crypto events", "input": { "tag_slug": "crypto", "active": true, "limit": 10 } },
                            { "description": "Browse a series", "input": { "series_slug": "ncaa-cbb", "limit": 10 } }
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
                name: Cow::Borrowed("list_markets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse markets directly or flatten markets from an event, series, or tag. Use this when you need all submarkets, spreads, totals, or related contracts.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "offset": { "type": "integer", "minimum": 0, "default": 0 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_markets response." },
                            "slug": { "type": "string", "description": "Direct market slug lookup." },
                            "event_item_ref": { "type": "string", "description": "Normalized event item_ref like polymarket:event:312712." },
                            "event_id": { "type": ["string", "integer"], "description": "Event id whose markets should be listed." },
                            "event_slug": { "type": "string", "description": "Event slug whose markets should be listed." },
                            "series_id": { "type": ["string", "integer"], "description": "Flatten markets from events in a series." },
                            "series_slug": { "type": "string", "description": "Flatten markets from events in a series." },
                            "tag_slug": { "type": "string", "description": "Flatten markets from events under a tag." },
                            "active": { "type": "boolean" },
                            "closed": { "type": "boolean" }
                        },
                        "examples": [
                            { "description": "List all markets in one event", "input": { "event_id": 312712, "limit": 20 } },
                            { "description": "Browse active markets directly", "input": { "active": true, "closed": false, "limit": 10 } }
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
                name: Cow::Borrowed("list_series"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Browse Polymarket series such as recurring sports, macro, or category-level groupings.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "offset": { "type": "integer", "minimum": 0, "default": 0 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_series response." },
                            "slug": { "type": "string", "description": "Filter to one series slug." },
                            "active": { "type": "boolean" },
                            "closed": { "type": "boolean" },
                            "featured": { "type": "boolean" }
                        },
                        "examples": [
                            { "description": "Browse active series", "input": { "active": true, "limit": 10 } },
                            { "description": "Lookup one series by slug", "input": { "slug": "us-annual-inflation" } }
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
                    "Get one Polymarket series by slug or id, including linked event stubs.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": ["string", "integer"], "description": "Series id." },
                            "slug": { "type": "string", "description": "Series slug like us-annual-inflation." }
                        },
                        "examples": [
                            { "description": "Fetch a series by slug", "input": { "slug": "us-annual-inflation" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "series", "finance"],
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
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a Polymarket event by event URL, slug, numeric id, or normalized item_ref. Use this for event pages like https://polymarket.com/event/<slug>.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": {
                                "type": "string",
                                "description": "Normalized item_ref such as polymarket:event:312712."
                            },
                            "url": {
                                "type": "string",
                                "description": "Frontend Polymarket event URL."
                            },
                            "slug": {
                                "type": "string",
                                "description": "Event slug from polymarket.com/event/<slug>."
                            },
                            "id": {
                                "type": ["string", "integer"],
                                "description": "Numeric Polymarket event id."
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                                "default": "raw"
                            }
                        },
                        "examples": [
                            {
                                "description": "Fetch an event by URL",
                                "input": { "url": "https://polymarket.com/event/cbb-pur-arz-2026-03-28" }
                            },
                            {
                                "description": "Fetch an event by slug as normalized output",
                                "input": { "slug": "cbb-pur-arz-2026-03-28", "output_format": "normalized_v1" }
                            }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "news", "finance"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": false
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
                    "Get an individual Polymarket market by slug, numeric id, or normalized item_ref. Use this when you already know the exact market identifier.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": {
                                "type": "string",
                                "description": "Normalized item_ref such as polymarket:market:1739838."
                            },
                            "slug": {
                                "type": "string",
                                "description": "Market slug."
                            },
                            "id": {
                                "type": ["string", "integer"],
                                "description": "Numeric Polymarket market id."
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                                "default": "raw"
                            }
                        },
                        "examples": [
                            {
                                "description": "Fetch a market by id",
                                "input": { "id": 1739838 }
                            },
                            {
                                "description": "Fetch a market by slug",
                                "input": { "slug": "cbb-pur-arz-2026-03-28", "output_format": "normalized_v1" }
                            }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "news", "finance"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": false
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
                name: Cow::Borrowed("list_comments"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List recent comments for an event, market, or series. This resolves slugs and item_refs for you so agents can pull discussion context in one step.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                            "offset": { "type": "integer", "minimum": 0, "default": 0 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous list_comments response." },
                            "item_ref": { "type": "string", "description": "Normalized event/market item_ref." },
                            "event_url": { "type": "string", "description": "Polymarket event URL." },
                            "event_id": { "type": ["string", "integer"] },
                            "event_slug": { "type": "string" },
                            "market_id": { "type": ["string", "integer"] },
                            "market_slug": { "type": "string" },
                            "series_id": { "type": ["string", "integer"] },
                            "series_slug": { "type": "string" }
                        },
                        "examples": [
                            { "description": "List comments on a series", "input": { "series_slug": "us-annual-inflation", "limit": 10 } },
                            { "description": "List comments on an event URL", "input": { "event_url": "https://polymarket.com/event/cbb-pur-arz-2026-03-28", "limit": 10 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["prediction-markets", "comments", "social"],
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
                name: Cow::Borrowed("order_book"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get top-of-book depth for one market outcome or all outcomes in a market. This resolves outcome token ids automatically from the market metadata.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized market item_ref like polymarket:market:1739838." },
                            "id": { "type": ["string", "integer"], "description": "Market id." },
                            "slug": { "type": "string", "description": "Market slug." },
                            "outcome": { "type": "string", "description": "Optional outcome name to narrow to one side." },
                            "token_id": { "type": "string", "description": "Optional direct CLOB token id." },
                            "depth": { "type": "integer", "minimum": 1, "maximum": 20, "default": 5 }
                        },
                        "examples": [
                            { "description": "Get full order book context for a market", "input": { "id": 1739838, "depth": 5 } },
                            { "description": "Get one outcome only", "input": { "slug": "cbb-pur-arz-2026-03-28", "outcome": "Arizona Wildcats" } }
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
                name: Cow::Borrowed("price_history"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get price history for one market outcome or all outcomes in a market. This wraps the token-level history endpoint into market-level context.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized market item_ref like polymarket:market:1739838." },
                            "id": { "type": ["string", "integer"], "description": "Market id." },
                            "slug": { "type": "string", "description": "Market slug." },
                            "outcome": { "type": "string", "description": "Optional outcome name to narrow to one side." },
                            "token_id": { "type": "string", "description": "Optional direct CLOB token id." },
                            "interval": { "type": "string", "default": "1d", "description": "History interval, for example 1d or max." },
                            "fidelity": { "type": "integer", "minimum": 1, "default": 60, "description": "Sampling fidelity in seconds." }
                        },
                        "examples": [
                            { "description": "Get recent history for a market", "input": { "id": 1739838, "interval": "1d", "fidelity": 60 } },
                            { "description": "Get a longer history window", "input": { "slug": "cbb-pur-arz-2026-03-28", "interval": "max", "fidelity": 300 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "price-history", "finance"],
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
                name: Cow::Borrowed("market_positions"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get public holder/position data for a market, grouped by token/outcome.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized market item_ref like polymarket:market:1739838." },
                            "id": { "type": ["string", "integer"], "description": "Market id." },
                            "slug": { "type": "string", "description": "Market slug." },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20, "description": "Maximum number of positions per token to request." }
                        },
                        "examples": [
                            { "description": "Inspect public market holders", "input": { "id": 1739838, "limit": 10 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["prediction-markets", "positions", "finance"],
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
                name: Cow::Borrowed("get_market_context"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a market plus its linked event, order-book summary, price-history summary, and optionally public holder data. This is the high-context analysis tool for agents.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Normalized market item_ref like polymarket:market:1739838." },
                            "id": { "type": ["string", "integer"], "description": "Market id." },
                            "slug": { "type": "string", "description": "Market slug." },
                            "depth": { "type": "integer", "minimum": 1, "maximum": 20, "default": 5, "description": "Book depth per outcome." },
                            "interval": { "type": "string", "default": "1d", "description": "History interval for embedded price history." },
                            "fidelity": { "type": "integer", "minimum": 1, "default": 60, "description": "Sampling fidelity in seconds." },
                            "include_positions": { "type": "boolean", "default": false, "description": "Also fetch public holder/position data." },
                            "positions_limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 }
                        },
                        "examples": [
                            { "description": "Fetch full market context", "input": { "id": 1739838 } },
                            { "description": "Fetch full market context with public holders", "input": { "slug": "cbb-pur-arz-2026-03-28", "include_positions": true, "positions_limit": 10 } }
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
    ) -> Result<crate::CallToolResult, ConnectorError> {
        let tool_name = request.name.as_ref();
        let args_map = request.arguments.unwrap_or_default();

        match tool_name {
            "search" => {
                let args: SearchArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;

                let query = args.query.trim();
                if query.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "query must not be empty".to_string(),
                    ));
                }

                let start_page = if let Some(cursor) = args.cursor.as_deref() {
                    let decoded =
                        ingest::decode_cursor::<SearchCursor>(cursor).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?;
                    if decoded.query != query {
                        return Err(ConnectorError::InvalidParams(
                            "Cursor query does not match requested query".to_string(),
                        ));
                    }
                    decoded.page
                } else {
                    args.page.max(1)
                };

                let (events, next_cursor, has_more, total_results) =
                    self.search_events(query, start_page, args.limit).await?;
                let summaries: Vec<EventSummary> =
                    events.iter().map(Self::to_event_summary).collect();

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let items = summaries
                        .iter()
                        .map(Self::event_summary_item)
                        .collect::<Result<Vec<_>, _>>()?;
                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor.clone(),
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(args.limit as u64))),
                        Source::new("polymarket", "search"),
                    );
                    return structured_result(&page);
                }

                let payload = SearchResponseView {
                    query: query.to_string(),
                    events: summaries,
                    pagination: SearchPaginationView {
                        page: start_page,
                        next_cursor,
                        has_more,
                        total_results,
                    },
                };
                structured_result_with_text(&payload, None)
            }
            "list_events" => {
                let args: ListEventsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let offset = Self::parse_offset(args.offset, args.cursor.as_deref())?;
                let limit = Self::clamp_list_limit(args.limit);
                let events = self.fetch_events_page(limit, offset, &args).await?;
                let has_more = events.len() == limit as usize;
                let next_cursor =
                    Self::encode_next_offset_cursor(offset + events.len() as u32, has_more)?;
                let payload = EventListResponseView {
                    events: events.iter().map(Self::to_event_summary).collect(),
                    pagination: OffsetPaginationView {
                        offset,
                        next_cursor,
                        has_more,
                    },
                    filters: EventListFiltersView {
                        series_id: args.series_id,
                        series_slug: args.series_slug,
                        tag_slug: args.tag_slug,
                        active: args.active,
                        closed: args.closed,
                        archived: args.archived,
                        featured: args.featured,
                    },
                };
                structured_result_with_text(&payload, Some(Self::event_list_text(&payload.events)))
            }
            "list_tags" => {
                let args: ListTagsArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let offset = Self::parse_offset(args.offset, args.cursor.as_deref())?;
                let limit = Self::clamp_list_limit(args.limit);
                let tags = self.fetch_tags_page(limit, offset).await?;
                let has_more = tags.len() == limit as usize;
                let next_cursor =
                    Self::encode_next_offset_cursor(offset + tags.len() as u32, has_more)?;
                let payload = TagListResponseView {
                    tags: tags.iter().map(Self::to_tag_summary).collect(),
                    pagination: OffsetPaginationView {
                        offset,
                        next_cursor,
                        has_more,
                    },
                };
                structured_result_with_text(&payload, Some(Self::tag_list_text(&payload.tags)))
            }
            "list_markets" => {
                let args: ListMarketsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let payload = self.build_market_list(&args).await?;
                structured_result_with_text(
                    &payload,
                    Some(Self::market_list_text(&payload.markets)),
                )
            }
            "list_series" => {
                let args: ListSeriesArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let offset = Self::parse_offset(args.offset, args.cursor.as_deref())?;
                let limit = Self::clamp_list_limit(args.limit);
                let series = self.fetch_series_page(limit, offset, &args).await?;
                let has_more = series.len() == limit as usize;
                let next_cursor =
                    Self::encode_next_offset_cursor(offset + series.len() as u32, has_more)?;
                let payload = SeriesListResponseView {
                    series: series
                        .iter()
                        .map(|series| {
                            Self::to_series_summary(&GammaSeriesSummary {
                                id: series.id.clone(),
                                slug: series.slug.clone(),
                                ticker: series.ticker.clone(),
                                title: series.title.clone(),
                                series_type: series.series_type.clone(),
                                recurrence: series.recurrence.clone(),
                                active: series.active,
                                closed: series.closed,
                                archived: series.archived,
                                featured: series.featured,
                                comment_count: series.comment_count,
                                volume_24hr: series.volume_24hr,
                                liquidity: series.liquidity,
                            })
                        })
                        .collect(),
                    pagination: OffsetPaginationView {
                        offset,
                        next_cursor,
                        has_more,
                    },
                };
                structured_result_with_text(&payload, Some(Self::series_list_text(&payload.series)))
            }
            "get_series" => {
                let args: GetSeriesArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let series = self
                    .fetch_series_resolved(Self::resolve_series_ref(&args)?)
                    .await?;
                let detail = Self::to_series_detail(&series);
                structured_result_with_text(&detail, Some(detail.title.clone()))
            }
            "get" => {
                let args: GetEventArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let event = match Self::resolve_event_ref(&args)? {
                    EventRef::Id(id) => self.fetch_event_by_id(&id).await?,
                    EventRef::Slug(slug) => self.fetch_event_by_slug(&slug).await?,
                };
                let detail = Self::to_event_detail(&event);

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let item = Self::event_detail_item(&detail)?;
                    let normalized =
                        NormalizedItemV1::complete(item, Source::new("polymarket", "get"));
                    return structured_result(&normalized);
                }

                structured_result_with_text(&detail, Some(Self::event_detail_text(&detail)))
            }
            "get_market" => {
                let args: GetMarketArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let market = self
                    .fetch_market_resolved(Self::resolve_market_ref(&args)?)
                    .await?;
                let detail = Self::to_market_detail(&market);

                if args.output_format.is_normalized() || args.output_format.is_display() {
                    let item = Self::market_detail_item(&detail)?;
                    let normalized =
                        NormalizedItemV1::complete(item, Source::new("polymarket", "get_market"));
                    return structured_result(&normalized);
                }

                structured_result_with_text(&detail, Some(Self::market_detail_text(&detail)))
            }
            "list_comments" => {
                let args: ListCommentsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let offset = Self::parse_offset(args.offset, args.cursor.as_deref())?;
                let limit = Self::clamp_list_limit(args.limit);
                let target = self.resolve_comment_target(&args).await?;
                let comments = self
                    .fetch_comments_page(&target.entity_type, &target.entity_id, limit, offset)
                    .await?;
                let has_more = comments.len() == limit as usize;
                let next_cursor =
                    Self::encode_next_offset_cursor(offset + comments.len() as u32, has_more)?;
                let payload = CommentListResponseView {
                    target: CommentTargetView {
                        entity_type: target.entity_type,
                        entity_id: target.entity_id,
                        entity_slug: target.entity_slug,
                        entity_title: target.entity_title,
                    },
                    comments: comments.iter().map(Self::to_comment_summary).collect(),
                    pagination: OffsetPaginationView {
                        offset,
                        next_cursor,
                        has_more,
                    },
                };
                structured_result_with_text(
                    &payload,
                    Some(Self::comments_list_text(&payload.comments)),
                )
            }
            "order_book" => {
                let args: OrderBookArgs =
                    serde_json::from_value(Value::Object(args_map)).map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let market = self
                    .fetch_market_resolved(Self::resolve_market_ref(&GetMarketArgs {
                        item_ref: args.item_ref.clone(),
                        id: args.id.clone(),
                        slug: args.slug.clone(),
                        output_format: OutputFormat::Raw,
                    })?)
                    .await?;
                let detail = Self::to_market_detail(&market);
                let books = self
                    .build_order_books(
                        &market,
                        args.outcome.as_deref(),
                        args.token_id.as_deref(),
                        args.depth,
                    )
                    .await?;
                let payload = OrderBookResponseView {
                    market: detail,
                    books,
                };
                structured_result_with_text(&payload, Some(Self::order_book_text(&payload.books)))
            }
            "price_history" => {
                let args: PriceHistoryArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let market = self
                    .fetch_market_resolved(Self::resolve_market_ref(&GetMarketArgs {
                        item_ref: args.item_ref.clone(),
                        id: args.id.clone(),
                        slug: args.slug.clone(),
                        output_format: OutputFormat::Raw,
                    })?)
                    .await?;
                let detail = Self::to_market_detail(&market);
                let history = self
                    .build_price_history(
                        &market,
                        args.outcome.as_deref(),
                        args.token_id.as_deref(),
                        &args.interval,
                        args.fidelity,
                    )
                    .await?;
                let payload = PriceHistoryResponseView {
                    market: detail,
                    interval: args.interval,
                    fidelity: args.fidelity,
                    history,
                };
                structured_result_with_text(
                    &payload,
                    Some(Self::price_history_text(&payload.history)),
                )
            }
            "market_positions" => {
                let args: MarketPositionsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let market = self
                    .fetch_market_resolved(Self::resolve_market_ref(&GetMarketArgs {
                        item_ref: args.item_ref.clone(),
                        id: args.id.clone(),
                        slug: args.slug.clone(),
                        output_format: OutputFormat::Raw,
                    })?)
                    .await?;
                let detail = Self::to_market_detail(&market);
                let positions = self.build_positions(&market, args.limit).await?;
                let payload = MarketPositionsResponseView {
                    market: detail,
                    positions,
                };
                structured_result_with_text(
                    &payload,
                    Some(Self::positions_text(&payload.positions)),
                )
            }
            "get_market_context" => {
                let args: GetMarketContextArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid arguments: {}", e))
                    })?;
                let market = self
                    .fetch_market_resolved(Self::resolve_market_ref(&GetMarketArgs {
                        item_ref: args.item_ref.clone(),
                        id: args.id.clone(),
                        slug: args.slug.clone(),
                        output_format: OutputFormat::Raw,
                    })?)
                    .await?;
                let detail = Self::to_market_detail(&market);
                let event = self.resolve_parent_event_detail(&market).await?;
                let order_books = self
                    .build_order_books(&market, None, None, args.depth)
                    .await?;
                let price_history = self
                    .build_price_history(&market, None, None, &args.interval, args.fidelity)
                    .await?;
                let positions = if args.include_positions {
                    Some(self.build_positions(&market, args.positions_limit).await?)
                } else {
                    None
                };

                let payload = MarketContextView {
                    market: detail,
                    event,
                    order_books,
                    price_history,
                    positions,
                };

                let mut parts = vec![Self::market_detail_text(&payload.market)];
                if !payload.order_books.is_empty() {
                    parts.push(Self::order_book_text(&payload.order_books));
                }
                if !payload.price_history.is_empty() {
                    parts.push(Self::price_history_text(&payload.price_history));
                }
                if let Some(positions) = payload
                    .positions
                    .as_ref()
                    .filter(|positions| !positions.is_empty())
                {
                    parts.push(Self::positions_text(positions));
                }
                structured_result_with_text(&payload, Some(parts.join("\n\n")))
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
        let _ = self.fetch_search_page("bitcoin", 1).await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_outcomes_from_json_strings() {
        let market = GammaMarket {
            id: "1".to_string(),
            slug: Some("market".to_string()),
            question: Some("Will X happen?".to_string()),
            description: None,
            active: true,
            closed: false,
            accepting_orders: Some(true),
            image: None,
            icon: None,
            condition_id: None,
            resolution_source: None,
            start_date: None,
            end_date: None,
            updated_at: None,
            events: Vec::new(),
            outcomes: Some(Value::String("[\"Yes\",\"No\"]".to_string())),
            outcome_prices: Some(Value::String("[\"0.6\",\"0.4\"]".to_string())),
            clob_token_ids: None,
            last_trade_price: None,
            best_bid: None,
            best_ask: None,
            liquidity: None,
            volume: None,
            volume_24hr: None,
            volume_1wk: None,
            volume_1mo: None,
        };

        let outcomes = PolymarketConnector::parse_outcomes(&market);
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].name, "Yes");
        assert!((outcomes[0].price - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn resolves_event_slug_from_url() {
        let args = GetEventArgs {
            item_ref: None,
            url: Some("https://polymarket.com/event/cbb-pur-arz-2026-03-28".to_string()),
            id: None,
            slug: None,
            output_format: OutputFormat::Raw,
        };

        let resolved = PolymarketConnector::resolve_event_ref(&args).expect("event ref");
        match resolved {
            EventRef::Slug(slug) => assert_eq!(slug, "cbb-pur-arz-2026-03-28"),
            EventRef::Id(_) => panic!("expected slug ref"),
        }
    }

    #[test]
    fn event_summary_item_uses_polymarket_item_ref() {
        let summary = EventSummary {
            id: "312712".to_string(),
            slug: Some("cbb-pur-arz-2026-03-28".to_string()),
            title: "Purdue Boilermakers vs. Arizona Wildcats".to_string(),
            url: Some("https://polymarket.com/event/cbb-pur-arz-2026-03-28".to_string()),
            active: true,
            closed: false,
            start_date: Some("2026-03-28".to_string()),
            end_date: Some("2026-03-29".to_string()),
            updated_at: Some("2026-03-27T00:00:00Z".to_string()),
            comment_count: Some(4),
            liquidity: Some(1000.0),
            open_interest: Some(250.0),
            volume_24hr: Some(500.0),
            volume_1mo: Some(5000.0),
            tags: vec!["sports".to_string(), "basketball".to_string()],
            series: Vec::new(),
            top_market: None,
        };

        let item = PolymarketConnector::event_summary_item(&summary).expect("content item");
        assert_eq!(item.item_ref, "polymarket:event:312712");
        assert_eq!(item.kind, "prediction_event");
        assert_eq!(
            item.title.as_deref(),
            Some("Purdue Boilermakers vs. Arizona Wildcats")
        );
    }

    #[test]
    fn search_cursor_round_trip() {
        let cursor = SearchCursor {
            query: "bitcoin".to_string(),
            page: 3,
        };
        let encoded = ingest::encode_cursor(&cursor).expect("encode cursor");
        let decoded: SearchCursor = ingest::decode_cursor(&encoded).expect("decode cursor");
        assert_eq!(decoded.query, "bitcoin");
        assert_eq!(decoded.page, 3);
    }

    #[tokio::test]
    async fn list_tools_exposes_task_shaped_polymarket_toolbelt() {
        let connector = PolymarketConnector::new().await.expect("connector");
        let tools = connector.list_tools(None).await.expect("list tools");
        let tool_names = tools
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();

        for expected in [
            "search",
            "list_tags",
            "list_events",
            "list_markets",
            "list_series",
            "get_series",
            "get",
            "get_market",
            "list_comments",
            "order_book",
            "price_history",
            "market_positions",
            "get_market_context",
        ] {
            assert!(
                tool_names.contains(&expected),
                "missing expected tool {expected}"
            );
        }

        let context_tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == "get_market_context")
            .expect("context tool");
        assert!(context_tool
            .description
            .as_deref()
            .is_some_and(|description| description.contains("high-context")));
        assert_eq!(
            context_tool
                .input_schema
                .get("_meta")
                .and_then(|meta| meta.get("category"))
                .and_then(Value::as_str),
            Some("read")
        );
    }

    #[test]
    fn tag_summary_falls_back_to_slug_when_label_missing() {
        let tag = GammaTag {
            id: Some("42".to_string()),
            label: None,
            slug: Some("crypto".to_string()),
            created_at: None,
            updated_at: None,
            requires_translation: Some(false),
        };

        let summary = PolymarketConnector::to_tag_summary(&tag);
        assert_eq!(summary.label, "crypto");
        assert_eq!(summary.slug.as_deref(), Some("crypto"));
    }
}
