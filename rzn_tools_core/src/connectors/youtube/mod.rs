// src/connectors/youtube/mod.rs

use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Relationship, Source,
};
use crate::utils::{
    clean_html_entities, get_cookies, match_browser, structured_result, structured_result_with_text,
};
use crate::{auth::AuthDetails, Connector, URLParamExtraction, URLPatternSpec};
use async_trait::async_trait;
use chrono::TimeZone;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use futures::FutureExt;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client as HttpClient;
use rmcp::model::*;
use rusty_ytdl::search::{SearchOptions, SearchResult, SearchType, YouTube};
use rusty_ytdl::{RequestOptions, Video, VideoError, VideoOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::process::Command;
use url::{form_urlencoded, Url};
use yt_transcript_rs::errors::{CouldNotRetrieveTranscript, CouldNotRetrieveTranscriptReason};
use yt_transcript_rs::YouTubeTranscriptApi;

fn inline_output_format_property(schema: &mut Value) {
    let schema_obj = schema
        .as_object_mut()
        .expect("Input schema must be a JSON object");
    let properties = schema_obj
        .get_mut("properties")
        .and_then(|v| v.as_object_mut())
        .expect("Input schema must have properties");
    properties.insert(
        "output_format".to_string(),
        json!({
            "type": "string",
            "enum": ["raw", "normalized_v1", "display_v1"],
            "default": "raw",
            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
        }),
    );

    if let Some(required) = schema_obj
        .get_mut("required")
        .and_then(|v| v.as_array_mut())
    {
        required.retain(|v| v.as_str() != Some("output_format"));
    }
}

// Input/Output structs for tools
/// Response format for controlling output verbosity
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Minimal response for token efficiency - only essential fields (default)
    #[default]
    Concise,
    /// Full response with all metadata
    Detailed,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetVideoDetailsInput {
    /// YouTube video ID/URL, playlist ID/URL, or channel handle/URL.
    #[serde(default)]
    pub video_id: Option<String>,
    /// Normalized item_ref (e.g., youtube:video:dQw4w9WgXcQ, youtube:playlist:PL..., youtube:channel:UC...)
    #[serde(default)]
    pub item_ref: Option<String>,
    /// Canonical URL for the video, playlist, or channel
    #[serde(default)]
    pub url: Option<String>,
    /// Response verbosity: 'concise' returns only title and transcript/chapters, 'detailed' includes description and all metadata
    #[serde(default)]
    pub response_format: ResponseFormat,
    /// Output format: 'raw' (default) or 'normalized_v1' for ingestion pipelines
    #[serde(default)]
    pub output_format: OutputFormat,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchVideosInput {
    /// Search query string
    pub query: String,
    /// Maximum number of results to return
    #[serde(default = "default_limit")]
    #[schemars(default = "default_limit")]
    pub limit: u64,
    /// Type of content to search for
    #[serde(default = "default_search_category")]
    #[schemars(default = "default_search_category")]
    pub search_type: SearchCategory,
    /// Sort order for results
    #[serde(default)]
    pub sort: Option<SearchSort>,
    /// Filter by upload date
    #[serde(default)]
    pub upload_date: Option<UploadDateFilter>,
    /// Response verbosity: 'concise' returns only id/title/url, 'detailed' includes all metadata
    #[serde(default)]
    pub response_format: ResponseFormat,
    /// Output format: 'raw' (default) or 'normalized_v1' for ingestion pipelines
    #[serde(default)]
    pub output_format: OutputFormat,
}

fn default_limit() -> u64 {
    5
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListSource {
    Channel,
    Playlist,
}

fn default_list_source() -> ListSource {
    ListSource::Channel
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListVideosInput {
    /// What you are listing: a channel's uploads or a playlist's items.
    #[serde(default = "default_list_source")]
    #[schemars(default = "default_list_source")]
    pub source: ListSource,

    /// Channel identifier. Accepts a channel ID (UC...), a channel URL, or a handle like "@hubermanlab".
    #[serde(default)]
    pub channel: Option<String>,

    /// Playlist identifier. Accepts a playlist ID (PL.../UU...) or a playlist URL.
    #[serde(default)]
    pub playlist: Option<String>,

    /// Max number of videos to return. When omitted, the connector paginates until YouTube stops
    /// returning continuation pages.
    #[serde(default)]
    pub limit: Option<u64>,

    /// Optional RFC3339 timestamp; only include videos published at/after this time.
    #[serde(default)]
    pub published_after: Option<String>,

    /// Optional relative filter; only include videos published in the last N days (UTC, relative to now).
    /// If provided, this overrides published_after.
    #[serde(default)]
    pub published_within_days: Option<u32>,

    /// Output format: 'raw' (default) or 'normalized_v1' for ingestion pipelines
    #[serde(default)]
    pub output_format: OutputFormat,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListedVideo {
    /// 1-based position in the returned channel/playlist ordering.
    pub index: u64,
    pub id: String,
    pub title: String,
    pub url: String,
    pub published_at: Option<String>,
    /// UI-friendly channel name. Kept alongside `channel_title` for downstream ergonomics.
    pub channel: Option<String>,
    pub channel_title: Option<String>,
    pub channel_id: Option<String>,
    pub playlist_id: Option<String>,
    pub playlist_title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListVideosOutput {
    /// Ordered videos. This is the canonical enumeration field for downstream scripts.
    pub entries: Vec<ListedVideo>,
    /// Back-compat alias for callers that already consume youtube/list.
    pub videos: Vec<ListedVideo>,
    pub source: ListSource,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub playlist_id: Option<String>,
    pub playlist_title: Option<String>,
}

fn default_resolve_limit() -> u64 {
    5
}

fn default_prefer_verified() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveChannelInput {
    /// Free-text channel query (e.g., "Andrew Huberman"). Returns candidate channels.
    #[serde(default)]
    pub query: Option<String>,

    /// Explicit channel identifier to normalize to a channel ID (UC...) when possible.
    /// Accepts a channel ID, channel URL, or handle like "@hubermanlab".
    #[serde(default)]
    pub channel: Option<String>,

    /// Max candidates to return (default: 5).
    #[serde(default = "default_resolve_limit")]
    #[schemars(default = "default_resolve_limit")]
    pub limit: u64,

    /// Prefer verified channels when ranking candidates (default: true).
    #[serde(default = "default_prefer_verified")]
    #[schemars(default = "default_prefer_verified")]
    pub prefer_verified: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct ChannelCandidate {
    pub channel_id: String,
    pub title: String,
    pub url: String,
    pub verified: bool,
    pub subscribers: u64,
    pub score: f64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveChannelOutput {
    /// Heuristic best guess (may be None if no candidates found).
    pub recommended: Option<ChannelCandidate>,
    /// Ranked candidates (best first).
    pub candidates: Vec<ChannelCandidate>,
    /// When `channel` was provided, this is the normalized UC... ID if resolved.
    pub resolved_channel_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchVideosOutput {
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VideoSearchResult {
    pub id: String,
    pub title: String,
    pub description: String,
    pub thumbnail: String,
    pub url: String,
    pub duration_seconds: u64,
    pub views: u64,
    pub uploaded_at: Option<String>,
    pub channel_name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PlaylistSearchResult {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: String,
    pub channel: ChannelSearchResult,
    pub video_count: u64,
    pub views: u64,
    pub last_update: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ChannelSearchResult {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: String,
    pub verified: bool,
    pub subscribers: u64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SearchResultItem {
    Video(VideoSearchResult),
    Playlist(PlaylistSearchResult),
    Channel(ChannelSearchResult),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchCategory {
    Video,
    Playlist,
    Channel,
    All,
}

fn default_search_category() -> SearchCategory {
    SearchCategory::Video
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    Relevance,
    ViewsDesc,
    ViewsAsc,
    DurationDesc,
    DurationAsc,
    SubscribersDesc,
    SubscribersAsc,
    PlaylistVideosDesc,
    PlaylistVideosAsc,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadDateFilter {
    Any,
    LastHour,
    Today,
    ThisWeek,
    ThisMonth,
    ThisYear,
}

impl From<rusty_ytdl::search::Video> for VideoSearchResult {
    fn from(video: rusty_ytdl::search::Video) -> Self {
        let thumbnail = video
            .thumbnails
            .first()
            .map(|t| t.url.clone())
            .unwrap_or_default();

        Self {
            id: video.id.clone(),
            title: video.title.clone(),
            description: video.description.clone(),
            thumbnail,
            url: format!("https://www.youtube.com/watch?v={}", video.id),
            duration_seconds: video.duration,
            views: video.views,
            uploaded_at: video.uploaded_at.clone(),
            channel_name: video.channel.name.clone(),
        }
    }
}

impl From<rusty_ytdl::search::Channel> for ChannelSearchResult {
    fn from(channel: rusty_ytdl::search::Channel) -> Self {
        let thumbnail = channel
            .icon
            .first()
            .map(|t| t.url.clone())
            .unwrap_or_default();

        Self {
            id: channel.id,
            title: channel.name,
            url: channel.url,
            thumbnail,
            verified: channel.verified,
            subscribers: channel.subscribers,
        }
    }
}

impl From<rusty_ytdl::search::Playlist> for PlaylistSearchResult {
    fn from(playlist: rusty_ytdl::search::Playlist) -> Self {
        let thumbnail = playlist
            .thumbnails
            .first()
            .map(|t| t.url.clone())
            .unwrap_or_default();

        Self {
            id: playlist.id,
            title: playlist.name,
            url: playlist.url,
            thumbnail,
            channel: playlist.channel.clone().into(),
            video_count: playlist.videos.len() as u64,
            views: playlist.views,
            last_update: playlist.last_update,
        }
    }
}

impl From<rusty_ytdl::search::Channel> for SearchResultItem {
    fn from(value: rusty_ytdl::search::Channel) -> Self {
        SearchResultItem::Channel(value.into())
    }
}

impl From<rusty_ytdl::search::Video> for SearchResultItem {
    fn from(value: rusty_ytdl::search::Video) -> Self {
        SearchResultItem::Video(value.into())
    }
}

impl From<rusty_ytdl::search::Playlist> for SearchResultItem {
    fn from(value: rusty_ytdl::search::Playlist) -> Self {
        SearchResultItem::Playlist(value.into())
    }
}

fn to_rusty_search_type(category: SearchCategory) -> SearchType {
    match category {
        SearchCategory::Video => SearchType::Video,
        SearchCategory::Playlist => SearchType::Playlist,
        SearchCategory::Channel => SearchType::Channel,
        SearchCategory::All => SearchType::All,
    }
}

fn apply_sort(
    results: &mut [SearchResultItem],
    sort: Option<SearchSort>,
    category: SearchCategory,
) -> Result<(), ConnectorError> {
    let Some(sort) = sort else {
        return Ok(());
    };

    if sort == SearchSort::Relevance {
        return Ok(());
    }

    match category {
        SearchCategory::Video => sort_videos(results, sort),
        SearchCategory::Playlist => sort_playlists(results, sort),
        SearchCategory::Channel => sort_channels(results, sort),
        SearchCategory::All => Err(ConnectorError::InvalidParams(
            "Sorting is only supported when search_type is video, playlist, or channel".to_string(),
        )),
    }
}

fn sort_videos(results: &mut [SearchResultItem], sort: SearchSort) -> Result<(), ConnectorError> {
    use SearchSort::*;

    match sort {
        ViewsDesc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Video(a), SearchResultItem::Video(b)) => b.views.cmp(&a.views),
                _ => Ordering::Equal,
            });
            Ok(())
        }
        ViewsAsc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Video(a), SearchResultItem::Video(b)) => a.views.cmp(&b.views),
                _ => Ordering::Equal,
            });
            Ok(())
        }
        DurationDesc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Video(a), SearchResultItem::Video(b)) => {
                    b.duration_seconds.cmp(&a.duration_seconds)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        DurationAsc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Video(a), SearchResultItem::Video(b)) => {
                    a.duration_seconds.cmp(&b.duration_seconds)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        SubscribersDesc | SubscribersAsc | PlaylistVideosDesc | PlaylistVideosAsc | Relevance => {
            Err(ConnectorError::InvalidParams(format!(
                "Sort {:?} is not supported for video search",
                sort
            )))
        }
    }
}

fn sort_playlists(
    results: &mut [SearchResultItem],
    sort: SearchSort,
) -> Result<(), ConnectorError> {
    use SearchSort::*;

    match sort {
        ViewsDesc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Playlist(a), SearchResultItem::Playlist(b)) => {
                    b.views.cmp(&a.views)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        ViewsAsc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Playlist(a), SearchResultItem::Playlist(b)) => {
                    a.views.cmp(&b.views)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        PlaylistVideosDesc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Playlist(a), SearchResultItem::Playlist(b)) => {
                    b.video_count.cmp(&a.video_count)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        PlaylistVideosAsc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Playlist(a), SearchResultItem::Playlist(b)) => {
                    a.video_count.cmp(&b.video_count)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        DurationDesc | DurationAsc | SubscribersDesc | SubscribersAsc | Relevance => {
            Err(ConnectorError::InvalidParams(format!(
                "Sort {:?} is not supported for playlist search",
                sort
            )))
        }
    }
}

fn sort_channels(results: &mut [SearchResultItem], sort: SearchSort) -> Result<(), ConnectorError> {
    use SearchSort::*;

    match sort {
        SubscribersDesc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Channel(a), SearchResultItem::Channel(b)) => {
                    b.subscribers.cmp(&a.subscribers)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        SubscribersAsc => {
            results.sort_by(|a, b| match (a, b) {
                (SearchResultItem::Channel(a), SearchResultItem::Channel(b)) => {
                    a.subscribers.cmp(&b.subscribers)
                }
                _ => Ordering::Equal,
            });
            Ok(())
        }
        ViewsDesc | ViewsAsc | DurationDesc | DurationAsc | PlaylistVideosDesc
        | PlaylistVideosAsc | Relevance => Err(ConnectorError::InvalidParams(format!(
            "Sort {:?} is not supported for channel search",
            sort
        ))),
    }
}

fn apply_upload_date_filter(
    results: &mut Vec<SearchResultItem>,
    filter: Option<UploadDateFilter>,
    category: SearchCategory,
) -> Result<(), ConnectorError> {
    let Some(filter) = filter else {
        return Ok(());
    };

    if filter == UploadDateFilter::Any {
        return Ok(());
    }

    if category == SearchCategory::Channel {
        return Err(ConnectorError::InvalidParams(
            "upload_date filter is not supported for channel search".to_string(),
        ));
    }

    let Some(cutoff) = cutoff_for_filter(filter) else {
        return Ok(());
    };

    results.retain(|item| match item {
        SearchResultItem::Video(video) => video
            .uploaded_at
            .as_deref()
            .and_then(parse_uploaded_timestamp)
            .map(|timestamp| timestamp >= cutoff)
            .unwrap_or(false),
        SearchResultItem::Playlist(playlist) => playlist
            .last_update
            .as_deref()
            .and_then(parse_uploaded_timestamp)
            .map(|timestamp| timestamp >= cutoff)
            .unwrap_or(false),
        SearchResultItem::Channel(_) => false,
    });

    Ok(())
}

fn cutoff_for_filter(filter: UploadDateFilter) -> Option<DateTime<Utc>> {
    let now = Utc::now();

    match filter {
        UploadDateFilter::Any => None,
        UploadDateFilter::LastHour => Some(now - Duration::hours(1)),
        UploadDateFilter::Today => {
            let midnight = now.date_naive().and_hms_opt(0, 0, 0)?;
            Some(Utc.from_utc_datetime(&midnight))
        }
        UploadDateFilter::ThisWeek => Some(now - Duration::days(7)),
        UploadDateFilter::ThisMonth => Some(now - Duration::days(30)),
        UploadDateFilter::ThisYear => Some(now - Duration::days(365)),
    }
}

fn parse_uploaded_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }

    let base = cleaned.split('•').next().unwrap_or(cleaned).trim();
    if base.eq_ignore_ascii_case("live") {
        return None;
    }

    let mut normalized = base;

    static PREFIXES: [&str; 8] = [
        "Streamed live on ",
        "Streamed live ",
        "Streamed ",
        "Premiered ",
        "Uploaded ",
        "Live streamed on ",
        "Last updated on ",
        "Last update on ",
    ];

    loop {
        let mut stripped = None;
        for prefix in PREFIXES {
            if normalized.len() >= prefix.len()
                && normalized[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                stripped = Some(normalized[prefix.len()..].trim());
                break;
            }
        }

        if let Some(rest) = stripped {
            normalized = rest;
        } else {
            break;
        }
    }

    // Handle relative expressions like "3 years ago"
    static RELATIVE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)(?P<num>\d+)\s+(?P<unit>second|minute|hour|day|week|month|year)s?\s+ago")
            .unwrap()
    });

    if let Some(caps) = RELATIVE_RE.captures(normalized) {
        let num = caps.name("num")?.as_str().parse::<i64>().ok()?;
        let unit = caps.name("unit")?.as_str().to_lowercase();
        let duration = match unit.as_str() {
            "second" => Duration::seconds(num),
            "minute" => Duration::minutes(num),
            "hour" => Duration::hours(num),
            "day" => Duration::days(num),
            "week" => Duration::weeks(num),
            "month" => Duration::days(num * 30),
            "year" => Duration::days(num * 365),
            _ => return None,
        };
        return Some(Utc::now() - duration);
    }

    // Handle absolute dates like "Jan 1, 2023"
    static DATE_FORMATS: Lazy<Vec<&'static str>> = Lazy::new(|| {
        vec![
            "%b %e, %Y",
            "%b %d, %Y",
            "%B %e, %Y",
            "%B %d, %Y",
            "%b %Y",
            "%B %Y",
            "%Y",
        ]
    });

    for format in DATE_FORMATS.iter() {
        if let Ok(date) = NaiveDate::parse_from_str(normalized, format) {
            let midnight = date.and_hms_opt(0, 0, 0)?;
            return Some(Utc.from_utc_datetime(&midnight));
        }
    }

    None
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct YouTubeContent {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterContent>,
    /// When `transcript` is None and `chapters` is empty, this gives the
    /// agent a short machine-readable reason (e.g. "transcripts_disabled",
    /// "no_english_transcript", "age_restricted", "video_unavailable",
    /// "ip_blocked"). Absent when a transcript was successfully fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_unavailable_reason: Option<String>,
}

/// Concise version of YouTubeContent for token efficiency
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
pub struct YouTubeContentConcise {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterContentConcise>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_unavailable_reason: Option<String>,
}

/// Concise chapter content - just heading and content
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ChapterContentConcise {
    pub heading: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct TimedTextTrack {
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(rename = "languageCode")]
    language_code: Option<String>,
    kind: Option<String>,
}

#[derive(Debug)]
struct TimedTextTranscript {
    text: String,
    language_code: String,
    is_generated: bool,
}

/// Concise video search result - includes key metadata for LLM decision-making
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VideoSearchResultConcise {
    pub id: String,
    pub title: String,
    pub url: String,
    pub channel_name: String,
    pub views: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Concise playlist search result
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PlaylistSearchResultConcise {
    pub id: String,
    pub title: String,
    pub url: String,
}

/// Concise channel search result
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ChannelSearchResultConcise {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SearchResultItemConcise {
    Video(VideoSearchResultConcise),
    Playlist(PlaylistSearchResultConcise),
    Channel(ChannelSearchResultConcise),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchVideosOutputConcise {
    pub results: Vec<SearchResultItemConcise>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ChapterContent {
    pub heading: String,
    pub start_time: i32,
    pub content: String,
}

#[derive(Clone)]
pub struct YouTubeConnector {
    video_options: VideoOptions,
}

impl YouTubeConnector {
    pub async fn new(auth: Option<AuthDetails>) -> Result<Self, ConnectorError> {
        let mut connector = YouTubeConnector {
            video_options: VideoOptions::default(), // Default quality
        };

        if let Some(auth) = auth {
            connector.set_auth_details(auth).await?;
        }

        Ok(connector)
    }
}

#[async_trait]
impl Connector for YouTubeConnector {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn description(&self) -> &'static str {
        "YouTube video retrieval, transcript extraction, channel resolution, and feed search."
    }

    fn display_name(&self) -> &'static str {
        "YouTube"
    }

    fn icon(&self) -> &'static str {
        "youtube"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["video", "media", "social"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![
            URLPatternSpec {
                pattern:
                    r"(?:https?://)?(?:www\.)?youtube\.com/playlist\?(?:[^#\s]*?&)?list=([A-Za-z0-9_-]+)"
                        .to_string(),
                default_tool: "list".to_string(),
                description: "List playlist videos".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "playlist".to_string(),
                    use_full_url: false,
                }],
            },
            URLPatternSpec {
                pattern:
                    r"(?:https?://)?(?:www\.)?(?:youtube\.com/watch\?v=|youtu\.be/)([A-Za-z0-9_-]+)"
                        .to_string(),
                default_tool: "get".to_string(),
                description: "Get video details and transcript".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "video_id".to_string(),
                    use_full_url: false,
                }],
            },
            URLPatternSpec {
                pattern:
                    r"(?:https?://)?(?:www\.)?youtube\.com/(@[A-Za-z0-9_.-]+)(?:/(?:videos|streams|shorts|featured))?"
                        .to_string(),
                default_tool: "list".to_string(),
                description: "List channel uploads by handle".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "channel".to_string(),
                    use_full_url: false,
                }],
            },
            URLPatternSpec {
                pattern: r"(?:https?://)?(?:www\.)?youtube\.com/channel/([A-Za-z0-9_-]+)"
                    .to_string(),
                default_tool: "list".to_string(),
                description: "List channel uploads by ID".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "channel".to_string(),
                    use_full_url: false,
                }],
            },
        ]
    }

    async fn capabilities(&self) -> ServerCapabilities {
        // Define the capabilities according to what your connector supports.
        ServerCapabilities {
            tools: None,
            ..Default::default() // Use default for other capabilities
        }
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        if let Some(browser) = details.get("browser") {
            let browser = match_browser(browser.to_string())
                .await
                .map_err(|e| ConnectorError::Other(e.to_string()))?;
            let cookies = get_cookies(browser, "youtube.com".to_string())
                .await
                .map_err(|e| ConnectorError::Other(e.to_string()))?;

            self.video_options = VideoOptions {
                request_options: RequestOptions {
                    cookies: Some(cookies),
                    ..Default::default()
                },
                ..Default::default()
            };
            return Ok(());
        }

        Ok(()) // No auth
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
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
                website_url: None,
            },
            instructions: Some(
                "Use `get` as the default YouTube read tool: it accepts `url`, `video_id`, or \
`item_ref`; videos return transcript/chapters when available, while playlists and channels return \
ordered `entries`. Use \
`response_format=\"concise\"` with `output_format=\"normalized_v1\"` for machine-friendly \
transcript work. Use `search` to discover videos first, `list` for recent channel or playlist \
uploads, and `resolve_channel` only when you need a stable UC... channel id."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let resources = vec![];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        // let uri_str = request.uri.as_str();

        // if uri_str.starts_with("youtube://video/") {
        //     let parts: Vec<&str> = uri_str.split('/').collect();
        //     if parts.len() < 4 {
        //         return Err(ConnectorError::InvalidInput(format!("Invalid resource URI: {}", uri_str)));
        //     }
        //     let video_id = parts[3];

        //     let video_options = VideoOptions {
        //         quality: self.video_quality.clone(),
        //         filter: VideoSearchOptions::Video, // or Audio, depending on what you need
        //         ..Default::default()
        //     };
        //     let video = Video::new_with_options(format!("https://www.youtube.com/watch?v={}", video_id).as_str(), video_options)
        //         .map_err(|e| ConnectorError::Other(e.to_string()))?;

        //     let video_info = video.get_info().await.map_err(|e| ConnectorError::Other(e.to_string()))?;

        //     let chapters = video_info.video_details.chapters.clone();
        //      let transcript = match YoutubeTranscript::fetch_transcript(&format!("https://www.youtube.com/watch?v={}", video_id), None).await {
        //         Ok(transcript) => {
        //             let chapter_contents = self.group_transcript_by_chapters(&chapters, transcript);
        //             Some(chapter_contents)
        //         }
        //         Err(e) => {
        //             eprintln!("Error fetching transcript: {}", e);
        //             None
        //         }
        //     };

        //     let youtube_content =  YouTubeContent {
        //         title: video_info.video_details.title.clone(),
        //         description: video_info.video_details.description.clone(),
        //         transcript: None, // Populated below if available
        //         chapters: transcript.unwrap_or_default()
        //     };

        Ok(vec![])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let mut get_schema = serde_json::to_value(schemars::schema_for!(GetVideoDetailsInput))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        inline_output_format_property(&mut get_schema);
        if let Some(obj) = get_schema.as_object_mut() {
            obj.insert(
                "examples".to_string(),
                json!([
                    {
                        "description": "Get details by video ID",
                        "input": { "video_id": "dQw4w9WgXcQ", "response_format": "concise" }
                    },
                    {
                        "description": "Get details by URL",
                        "input": {
                            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                            "response_format": "concise",
                            "output_format": "normalized_v1"
                        }
                    },
                    {
                        "description": "Get details by item_ref",
                        "input": { "item_ref": "youtube:video:dQw4w9WgXcQ" }
                    },
                    {
                        "description": "Enumerate a playlist with get",
                        "input": {
                            "url": "https://www.youtube.com/playlist?list=PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n"
                        }
                    },
                    {
                        "description": "Enumerate channel uploads with get",
                        "input": { "video_id": "@hubermanlab" }
                    }
                ]),
            );
            obj.insert(
                "_meta".to_string(),
                json!({
                    "category": "read",
                    "tags": ["video", "media", "social"],
                    "auth_required": false,
                    "supports_output_format": true,
                    "supports_cursor": false
                }),
            );
        }

        let mut search_schema = serde_json::to_value(schemars::schema_for!(SearchVideosInput))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        inline_output_format_property(&mut search_schema);
        if let Some(obj) = search_schema.as_object_mut() {
            obj.insert(
                "examples".to_string(),
                json!([
                    {
                        "description": "Search videos by keyword",
                        "input": {
                            "query": "rust async",
                            "limit": 5,
                            "search_type": "video",
                            "output_format": "normalized_v1"
                        }
                    },
                    {
                        "description": "Search channels",
                        "input": { "query": "Andrew Huberman", "search_type": "channel", "limit": 5 }
                    }
                ]),
            );
            obj.insert(
                "_meta".to_string(),
                json!({
                    "category": "search",
                    "tags": ["video", "media", "social"],
                    "auth_required": false,
                    "supports_output_format": true,
                    "supports_cursor": false
                }),
            );
        }

        let mut list_schema = serde_json::to_value(schemars::schema_for!(ListVideosInput))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        inline_output_format_property(&mut list_schema);
        if let Some(obj) = list_schema.as_object_mut() {
            obj.insert(
                "examples".to_string(),
                json!([
                    {
                        "description": "List all channel uploads, stopping after 30 days",
                        "input": {
                            "source": "channel",
                            "channel": "@hubermanlab",
                            "published_within_days": 30,
                            "output_format": "normalized_v1"
                        }
                    },
                    {
                        "description": "List playlist videos",
                        "input": {
                            "source": "playlist",
                            "playlist": "PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n",
                            "limit": 10,
                            "output_format": "normalized_v1"
                        }
                    }
                ]),
            );
            obj.insert(
                "_meta".to_string(),
                json!({
                    "category": "list",
                    "tags": ["video", "media", "social"],
                    "auth_required": false,
                    "supports_output_format": true,
                    "supports_cursor": false
                }),
            );
        }

        let mut resolve_schema = serde_json::to_value(schemars::schema_for!(ResolveChannelInput))
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if let Some(obj) = resolve_schema.as_object_mut() {
            obj.insert(
                "examples".to_string(),
                json!([
                    {
                        "description": "Resolve by handle",
                        "input": { "channel": "@openai" }
                    },
                    {
                        "description": "Resolve by query",
                        "input": { "query": "OpenAI", "limit": 5, "prefer_verified": true }
                    }
                ]),
            );
            obj.insert(
                "_meta".to_string(),
                json!({
                    "category": "resolve",
                    "tags": ["video", "media", "social"],
                    "auth_required": false,
                    "supports_output_format": false,
                    "supports_cursor": false
                }),
            );
        }

        let tools = vec![
            Tool {
                name: Cow::Borrowed("get"),
                title: Some("Get Video Or Enumerate Playlist/Channel".into()),
                description: Some(Cow::Borrowed(
                    "Default YouTube retrieval tool. Use this for a known video when you want transcript, chapters, and core metadata; playlist IDs/URLs and channel handles/URLs enumerate ordered `entries`. Accepts `url`, `video_id`, or `item_ref`. For agent use, prefer `response_format=\"concise\"` for videos and `output_format=\"normalized_v1\"` for ingestion. Key args: exactly one of url/video_id/item_ref, plus optional response_format and output_format.",
                )),
                input_schema: Arc::new(
                    get_schema
                        .as_object()
                        .expect("Schema object")
                        .clone(),
                ),
                output_schema: None,
                annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search Videos".into()),
                description: Some(Cow::Borrowed(
                    "Search YouTube when you do not already know the target video, playlist, or channel. Default to `search_type=\"video\"`; pass the returned `url` or `item_ref` into `get` for transcript retrieval. Key args: query (required), optional search_type, limit, response_format, output_format.",
                )),
                input_schema: Arc::new(
                    search_schema
                        .as_object()
                        .expect("Schema object")
                        .clone(),
                ),
                output_schema: None,
                annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list"),
                title: Some("List Channel Or Playlist Videos".into()),
                description: Some(Cow::Borrowed(
                    "List channel uploads or playlist videos using native YouTube page data and Innertube continuation pagination. Omit `limit` to paginate until YouTube stops returning videos; pass `limit` for the last N channel uploads or first N playlist items. Key args: channel or playlist, optional limit, published_after or published_within_days, output_format.",
                )),
                input_schema: Arc::new(
                    list_schema
                        .as_object()
                        .expect("Schema object")
                        .clone(),
                ),
                output_schema: None,
                annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("resolve_channel"),
                title: Some("Resolve Channel".into()),
                description: Some(Cow::Borrowed(
                    "Resolve a handle, URL, or channel-name query to a stable UC... channel id. Use this only when you specifically need channel identity normalization; do not call it before `get` for normal video transcript retrieval. Key args: channel or query, optional limit and prefer_verified.",
                )),
                input_schema: Arc::new(
                    resolve_schema
                        .as_object()
                        .expect("Schema object")
                        .clone(),
                ),
                output_schema: None,
                annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
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
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();
        let args_map = serde_json::Map::from_iter(args);

        match name {
            "get" | "get_video_details" => {
                let input: GetVideoDetailsInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let target = resolve_get_target(&input)?;
                let video_id = match target {
                    YouTubeGetTarget::Video(video_id) => video_id,
                    YouTubeGetTarget::Playlist(playlist) => {
                        return list_videos_call_result(ListVideosInput {
                            source: ListSource::Playlist,
                            channel: None,
                            playlist: Some(playlist),
                            limit: None,
                            published_after: None,
                            published_within_days: None,
                            output_format: input.output_format,
                        })
                        .await;
                    }
                    YouTubeGetTarget::Channel(channel) => {
                        return list_videos_call_result(ListVideosInput {
                            source: ListSource::Channel,
                            channel: Some(channel),
                            playlist: None,
                            limit: None,
                            published_after: None,
                            published_within_days: None,
                            output_format: input.output_format,
                        })
                        .await;
                    }
                };
                let want_normalized = input.output_format == OutputFormat::NormalizedV1;

                let video = Video::new_with_options(
                    format!("https://www.youtube.com/watch?v={}", video_id).as_str(),
                    self.video_options.clone(),
                )
                .map_err(|e| classify_video_error(e, &video_id))?;

                // Guard against upstream panics in rusty_ytdl
                let video_info = AssertUnwindSafe(video.get_info())
                    .catch_unwind()
                    .await
                    .map_err(|_| {
                        ConnectorError::Other(format!(
                            "Internal: rusty_ytdl::get_info panicked for video '{}'. \
                             Retry is unlikely to help; report this as a bug.",
                            video_id
                        ))
                    })?
                    .map_err(|e| classify_video_error(e, &video_id))?;

                let chapters = video_info.video_details.chapters.clone();
                let api = YouTubeTranscriptApi::new(None, None, None)
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                // Fetch transcript parts once; we will decide whether to expose
                // chapterized content or a raw transcript, but never both.
                let mut transcript_parts: Option<Vec<yt_transcript_rs::FetchedTranscriptSnippet>> =
                    None;
                let mut transcript_meta: Option<(String, String, bool)> = None;
                // Classified reason the transcript ended up unavailable, if any.
                // `None` means a transcript was successfully produced.
                let mut transcript_unavailable_reason: Option<&'static str> = None;
                let (chapters_out, transcript_out) = match api
                    .fetch_transcript(&video_id, &["en"], false)
                    .await
                {
                    Ok(fetched) => {
                        let parts = fetched.parts();
                        if want_normalized {
                            transcript_parts = Some(parts.to_vec());
                            transcript_meta = Some((
                                fetched.language.clone(),
                                fetched.language_code.clone(),
                                fetched.is_generated,
                            ));
                        }
                        // Build a raw transcript string from parts (cleaned) for fallback.
                        let raw_text = parts
                            .iter()
                            .map(|p| p.text.clone())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let cleaned = clean_html_entities(&raw_text);

                        if cleaned.trim().is_empty() {
                            tracing::warn!(
                                video_id = %video_id,
                                "Transcript API returned empty text; trying TimedText fallback"
                            );
                            match fetch_transcript_from_timedtext(&video_id).await {
                                Ok(fallback) => {
                                    if want_normalized {
                                        transcript_parts = None;
                                        transcript_meta = Some((
                                            fallback.language_code.clone(),
                                            fallback.language_code.clone(),
                                            fallback.is_generated,
                                        ));
                                    }
                                    (Vec::new(), Some(fallback.text))
                                }
                                Err(fallback_err) => {
                                    tracing::warn!(
                                        error = %fallback_err,
                                        video_id = %video_id,
                                        "TimedText fallback transcript fetch failed after empty transcript"
                                    );
                                    transcript_unavailable_reason =
                                        Some("empty_transcript_and_fallback_failed");
                                    (Vec::new(), None)
                                }
                            }
                        } else if !chapters.is_empty() {
                            // Prefer chapterized content when real chapter metadata exists.
                            let grouped = group_transcript_by_chapters_new(&chapters, fetched);
                            if !grouped.is_empty() {
                                (grouped, None)
                            } else if !cleaned.is_empty() {
                                (Vec::new(), Some(cleaned))
                            } else {
                                (Vec::new(), None)
                            }
                        } else if !cleaned.is_empty() {
                            // No chapters metadata → provide raw transcript only.
                            (Vec::new(), Some(cleaned))
                        } else {
                            (Vec::new(), None)
                        }
                    }
                    Err(e) => {
                        let (code, human) = classify_transcript_error(&e);
                        tracing::warn!(
                            reason = code,
                            detail = %human,
                            video_id = %video_id,
                            "Failed to fetch YouTube transcript"
                        );
                        match fetch_transcript_from_timedtext(&video_id).await {
                            Ok(fallback) => {
                                if want_normalized {
                                    transcript_meta = Some((
                                        fallback.language_code.clone(),
                                        fallback.language_code.clone(),
                                        fallback.is_generated,
                                    ));
                                }
                                (Vec::new(), Some(fallback.text))
                            }
                            Err(fallback_err) => {
                                tracing::warn!(
                                    error = %fallback_err,
                                    video_id = %video_id,
                                    "TimedText fallback transcript fetch failed"
                                );
                                transcript_unavailable_reason = Some(code);
                                (Vec::new(), None)
                            }
                        }
                    }
                };

                if want_normalized {
                    let item_ref = format!("youtube:video:{}", video_id);
                    let canonical_url = format!("https://www.youtube.com/watch?v={}", video_id);
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    let mut relationships: Vec<Relationship> = Vec::new();

                    if !video_info.video_details.description.is_empty() {
                        let desc_ref = format!("youtube:description:{}", video_id);
                        blocks.push(ContentBlock {
                            block_ref: desc_ref.clone(),
                            block_kind: "description".to_string(),
                            text: video_info.video_details.description.clone(),
                            author: video_info
                                .video_details
                                .author
                                .as_ref()
                                .map(|author| Author {
                                    name: author.name.clone(),
                                    id: Some(format!("youtube:channel:{}", author.id)),
                                }),
                            created_at: None,
                            reply_to: None,
                            position: None,
                            score: None,
                            attachments: Vec::new(),
                            metadata: None,
                        });
                        relationships.push(Relationship {
                            rel: "has_block".to_string(),
                            from: item_ref.clone(),
                            to: desc_ref,
                        });
                    }

                    if let Some(parts) = transcript_parts {
                        for part in parts {
                            let start_ms = (part.start * 1000.0).round().max(0.0) as u64;
                            let end_ms =
                                ((part.start + part.duration) * 1000.0).round().max(0.0) as u64;
                            let seg_ref =
                                format!("youtube:segment:{}:{}-{}", video_id, start_ms, end_ms);
                            blocks.push(ContentBlock {
                                block_ref: seg_ref.clone(),
                                block_kind: "transcript_segment".to_string(),
                                text: part.text,
                                author: None,
                                created_at: None,
                                reply_to: None,
                                position: Some(json!({
                                    "kind": "time_range",
                                    "start_ms": start_ms,
                                    "end_ms": end_ms,
                                })),
                                score: None,
                                attachments: Vec::new(),
                                metadata: None,
                            });
                            relationships.push(Relationship {
                                rel: "has_block".to_string(),
                                from: item_ref.clone(),
                                to: seg_ref,
                            });
                        }
                    } else if !chapters_out.is_empty() {
                        for (idx, chapter) in chapters_out.iter().enumerate() {
                            let start_ms = (chapter.start_time as i64).max(0) as u64 * 1000;
                            let end_ms = chapters_out
                                .get(idx + 1)
                                .map(|next| (next.start_time as i64).max(0) as u64 * 1000)
                                .unwrap_or(start_ms);
                            let seg_ref =
                                format!("youtube:segment:{}:{}-{}", video_id, start_ms, end_ms);
                            blocks.push(ContentBlock {
                                block_ref: seg_ref.clone(),
                                block_kind: "transcript_segment".to_string(),
                                text: chapter.content.clone(),
                                author: None,
                                created_at: None,
                                reply_to: None,
                                position: Some(json!({
                                    "kind": "time_range",
                                    "start_ms": start_ms,
                                    "end_ms": end_ms,
                                })),
                                score: None,
                                attachments: Vec::new(),
                                metadata: Some(json!({
                                    "chapter": chapter.heading,
                                })),
                            });
                            relationships.push(Relationship {
                                rel: "has_block".to_string(),
                                from: item_ref.clone(),
                                to: seg_ref,
                            });
                        }
                    } else if let Some(transcript) = transcript_out.as_ref() {
                        if !transcript.is_empty() {
                            let seg_ref = format!("youtube:transcript:{}", video_id);
                            blocks.push(ContentBlock {
                                block_ref: seg_ref.clone(),
                                block_kind: "transcript".to_string(),
                                text: transcript.clone(),
                                author: None,
                                created_at: None,
                                reply_to: None,
                                position: None,
                                score: None,
                                attachments: Vec::new(),
                                metadata: None,
                            });
                            relationships.push(Relationship {
                                rel: "has_block".to_string(),
                                from: item_ref.clone(),
                                to: seg_ref,
                            });
                        }
                    }

                    let created_at = parse_video_date(&video_info.video_details.publish_date)
                        .or_else(|| parse_video_date(&video_info.video_details.upload_date));
                    let authors = video_info
                        .video_details
                        .author
                        .as_ref()
                        .map(|author| Author {
                            name: author.name.clone(),
                            id: Some(format!("youtube:channel:{}", author.id)),
                        })
                        .into_iter()
                        .collect::<Vec<_>>();
                    let metadata = json!({
                        "channel_id": video_info.video_details.channel_id.clone(),
                        "channel_name": video_info.video_details.owner_channel_name.clone(),
                        "view_count": video_info.video_details.view_count.clone(),
                        "length_seconds": video_info.video_details.length_seconds.clone(),
                        "keywords": video_info.video_details.keywords.clone(),
                        "is_live_content": video_info.video_details.is_live_content,
                        "transcript": transcript_meta.as_ref().map(|(lang, code, generated)| json!({
                            "language": lang,
                            "language_code": code,
                            "is_generated": generated,
                        })),
                    });

                    let item = ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "video".to_string(),
                        canonical_url: Some(canonical_url),
                        title: Some(video_info.video_details.title.clone()),
                        created_at,
                        source_updated_at: None,
                        authors,
                        tags: Vec::new(),
                        metadata: Some(metadata),
                        blocks,
                        relationships,
                        truncation: None,
                    };

                    let normalized = NormalizedItemV1::new(
                        item,
                        Partial::complete(None),
                        Source::new("youtube", "get"),
                    );
                    return structured_result(&normalized);
                }

                // Only surface a reason when the transcript and chapters
                // both came back empty — if we produced chapterized content,
                // the agent already has transcript-equivalent data.
                let reason_for_response = if transcript_out.is_none() && chapters_out.is_empty() {
                    transcript_unavailable_reason.map(|s| s.to_string())
                } else {
                    None
                };

                // Return concise or detailed based on response_format
                if input.response_format == ResponseFormat::Concise {
                    let concise_chapters: Vec<ChapterContentConcise> = chapters_out
                        .iter()
                        .map(|c| ChapterContentConcise {
                            heading: c.heading.clone(),
                            content: c.content.clone(),
                        })
                        .collect();
                    let youtube_content = YouTubeContentConcise {
                        title: video_info.video_details.title.clone(),
                        transcript: transcript_out,
                        chapters: concise_chapters,
                        transcript_unavailable_reason: reason_for_response,
                    };
                    let text = serde_json::to_string(&youtube_content)?;
                    Ok(structured_result_with_text(&youtube_content, Some(text))?)
                } else {
                    let youtube_content = YouTubeContent {
                        id: video_id,
                        title: video_info.video_details.title.clone(),
                        description: video_info.video_details.description.clone(),
                        transcript: transcript_out,
                        chapters: chapters_out,
                        transcript_unavailable_reason: reason_for_response,
                    };
                    let text = serde_json::to_string(&youtube_content)?;
                    Ok(structured_result_with_text(&youtube_content, Some(text))?)
                }
            }
            "search" | "search_videos" => {
                let input: SearchVideosInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let youtube = YouTube::new().map_err(|e| ConnectorError::Other(e.to_string()))?;

                let search_options = SearchOptions {
                    limit: input.limit,
                    search_type: to_rusty_search_type(input.search_type),
                    ..Default::default()
                };

                // Guard against upstream panics in rusty_ytdl search path
                let results: Vec<SearchResult> =
                    AssertUnwindSafe(youtube.search(&input.query, Some(&search_options)))
                        .catch_unwind()
                        .await
                        .map_err(|_| ConnectorError::Other("YouTube search panicked".to_string()))?
                        .map_err(|e| ConnectorError::Other(e.to_string()))?;

                let mut mapped_results: Vec<SearchResultItem> = results
                    .into_iter()
                    .filter_map(|result| match result {
                        SearchResult::Video(video)
                            if matches!(
                                input.search_type,
                                SearchCategory::Video | SearchCategory::All
                            ) =>
                        {
                            Some(SearchResultItem::from(video))
                        }
                        SearchResult::Playlist(playlist)
                            if matches!(
                                input.search_type,
                                SearchCategory::Playlist | SearchCategory::All
                            ) =>
                        {
                            Some(SearchResultItem::from(playlist))
                        }
                        SearchResult::Channel(channel)
                            if matches!(
                                input.search_type,
                                SearchCategory::Channel | SearchCategory::All
                            ) =>
                        {
                            Some(SearchResultItem::from(channel))
                        }
                        SearchResult::Video(_)
                        | SearchResult::Playlist(_)
                        | SearchResult::Channel(_) => None,
                    })
                    .collect();

                apply_upload_date_filter(
                    &mut mapped_results,
                    input.upload_date,
                    input.search_type,
                )?;

                apply_sort(&mut mapped_results, input.sort, input.search_type)?;

                if input.limit > 0 && mapped_results.len() > input.limit as usize {
                    mapped_results.truncate(input.limit as usize);
                }

                if input.output_format == OutputFormat::NormalizedV1 {
                    let items: Vec<ContentItem> = mapped_results
                        .iter()
                        .map(|r| match r {
                            SearchResultItem::Video(v) => ContentItem {
                                item_ref: format!("youtube:video:{}", v.id),
                                kind: "video".to_string(),
                                canonical_url: Some(v.url.clone()),
                                title: Some(v.title.clone()),
                                created_at: v.uploaded_at.as_deref().and_then(parse_video_date),
                                source_updated_at: None,
                                authors: if v.channel_name.is_empty() {
                                    Vec::new()
                                } else {
                                    vec![Author {
                                        name: v.channel_name.clone(),
                                        id: None,
                                    }]
                                },
                                tags: Vec::new(),
                                metadata: Some(json!({
                                    "duration_seconds": v.duration_seconds,
                                    "views": v.views,
                                    "channel_name": v.channel_name,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            },
                            SearchResultItem::Playlist(p) => ContentItem {
                                item_ref: format!("youtube:playlist:{}", p.id),
                                kind: "playlist".to_string(),
                                canonical_url: Some(p.url.clone()),
                                title: Some(p.title.clone()),
                                created_at: p.last_update.as_deref().and_then(parse_video_date),
                                source_updated_at: None,
                                authors: if p.channel.title.is_empty() {
                                    Vec::new()
                                } else {
                                    vec![Author {
                                        name: p.channel.title.clone(),
                                        id: None,
                                    }]
                                },
                                tags: Vec::new(),
                                metadata: Some(json!({
                                    "video_count": p.video_count,
                                    "views": p.views,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            },
                            SearchResultItem::Channel(c) => ContentItem {
                                item_ref: format!("youtube:channel:{}", c.id),
                                kind: "channel".to_string(),
                                canonical_url: Some(c.url.clone()),
                                title: Some(c.title.clone()),
                                created_at: None,
                                source_updated_at: None,
                                authors: Vec::new(),
                                tags: Vec::new(),
                                metadata: Some(json!({
                                    "verified": c.verified,
                                    "subscribers": c.subscribers,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            },
                        })
                        .collect();

                    let page = NormalizedPageV1::new(
                        items,
                        None,
                        false,
                        Partial::complete(Some(ingest::limits_max_items(input.limit as u64))),
                        Source::new("youtube", "search"),
                    );
                    return structured_result(&page);
                }

                // Return concise or detailed based on response_format
                if input.response_format == ResponseFormat::Concise {
                    let concise_results: Vec<SearchResultItemConcise> = mapped_results
                        .iter()
                        .map(|r| match r {
                            SearchResultItem::Video(v) => {
                                // Create a snippet from description (first ~150 chars)
                                let snippet = if v.description.is_empty() {
                                    None
                                } else {
                                    let clean = v.description.replace('\n', " ");
                                    let truncated: String = clean.chars().take(150).collect();
                                    if clean.chars().count() > 150 {
                                        Some(format!("{}...", truncated))
                                    } else {
                                        Some(truncated)
                                    }
                                };
                                SearchResultItemConcise::Video(VideoSearchResultConcise {
                                    id: v.id.clone(),
                                    title: v.title.clone(),
                                    url: v.url.clone(),
                                    channel_name: v.channel_name.clone(),
                                    views: v.views,
                                    uploaded_at: v.uploaded_at.clone(),
                                    snippet,
                                })
                            }
                            SearchResultItem::Playlist(p) => {
                                SearchResultItemConcise::Playlist(PlaylistSearchResultConcise {
                                    id: p.id.clone(),
                                    title: p.title.clone(),
                                    url: p.url.clone(),
                                })
                            }
                            SearchResultItem::Channel(c) => {
                                SearchResultItemConcise::Channel(ChannelSearchResultConcise {
                                    id: c.id.clone(),
                                    title: c.title.clone(),
                                    url: c.url.clone(),
                                })
                            }
                        })
                        .collect();
                    let output = SearchVideosOutputConcise {
                        results: concise_results,
                    };
                    let text = serde_json::to_string(&output)?;
                    Ok(structured_result_with_text(&output, Some(text))?)
                } else {
                    let output = SearchVideosOutput {
                        results: mapped_results,
                    };
                    let text = serde_json::to_string(&output)?;
                    Ok(structured_result_with_text(&output, Some(text))?)
                }
            }
            "list" | "list_videos" => {
                let input: ListVideosInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                list_videos_call_result(input).await
            }
            "resolve_channel" => {
                let input: ResolveChannelInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let has_channel = input
                    .channel
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                let has_query = input
                    .query
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                if !has_channel && !has_query {
                    return Err(ConnectorError::InvalidParams(
                        "Provide either `channel` (a handle, URL, or UC... id to normalize) \
                         or `query` (free-text channel name to search). Example: \
                         resolve_channel(query=\"Andrew Huberman\") or \
                         resolve_channel(channel=\"@hubermanlab\")."
                            .to_string(),
                    ));
                }

                let limit = input.limit.clamp(1, 10) as usize;

                let client = HttpClient::builder()
                    .user_agent("rzn-datasourcer/0.2.x youtube-connector")
                    .timeout(std::time::Duration::from_secs(20))
                    .build()
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                // If a concrete channel identifier was provided, normalize to UC... when possible.
                let resolved_channel_id = if let Some(ch) = input.channel.as_deref() {
                    resolve_channel_id_best_effort(&client, ch).await
                } else {
                    None
                };

                // If query is provided, return ranked candidates.
                let mut candidates: Vec<ChannelCandidate> = Vec::new();
                if let Some(q) = input.query.as_deref() {
                    let qn = normalize_ws(q);
                    if !qn.is_empty() {
                        let youtube =
                            YouTube::new().map_err(|e| ConnectorError::Other(e.to_string()))?;
                        let search_options = SearchOptions {
                            limit: limit as u64,
                            search_type: SearchType::Channel,
                            ..Default::default()
                        };

                        let results: Vec<SearchResult> =
                            AssertUnwindSafe(youtube.search(&qn, Some(&search_options)))
                                .catch_unwind()
                                .await
                                .map_err(|_| {
                                    ConnectorError::Other("YouTube search panicked".to_string())
                                })?
                                .map_err(|e| ConnectorError::Other(e.to_string()))?;

                        for r in results {
                            if let SearchResult::Channel(channel) = r {
                                let mapped: ChannelSearchResult = channel.into();
                                let score = score_channel_candidate(
                                    &qn,
                                    &mapped.title,
                                    mapped.verified,
                                    mapped.subscribers,
                                    input.prefer_verified,
                                );
                                candidates.push(ChannelCandidate {
                                    channel_id: mapped.id,
                                    title: mapped.title,
                                    url: mapped.url,
                                    verified: mapped.verified,
                                    subscribers: mapped.subscribers,
                                    score,
                                });
                            }
                        }

                        candidates.sort_by(|a, b| {
                            b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal)
                        });
                        candidates.truncate(limit);
                    }
                }

                let recommended = candidates.first().cloned();

                let out = ResolveChannelOutput {
                    recommended,
                    candidates,
                    resolved_channel_id,
                };
                let text = serde_json::to_string(&out)?;
                Ok(structured_result_with_text(&out, Some(text))?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![], // No prompts for now.  Add if you have use cases.
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::MethodNotFound) //  No prompts implemented
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum YouTubeGetTarget {
    Video(String),
    Playlist(String),
    Channel(String),
}

async fn list_videos_call_result(input: ListVideosInput) -> Result<CallToolResult, ConnectorError> {
    let output_format = input.output_format;
    let out = list_videos_output(input).await?;

    if output_format == OutputFormat::NormalizedV1 {
        return list_videos_normalized_result(&out);
    }

    let text = serde_json::to_string(&out)?;
    structured_result_with_text(&out, Some(text))
}

async fn list_videos_output(input: ListVideosInput) -> Result<ListVideosOutput, ConnectorError> {
    let source = effective_list_source(&input)?;
    let client = HttpClient::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| ConnectorError::Other(e.to_string()))?;

    let after = if let Some(days) = input.published_within_days {
        Some(Utc::now() - Duration::days(days as i64))
    } else {
        input.published_after.as_deref().and_then(parse_rfc3339)
    };

    let (mut videos, channel_id, channel_title, playlist_id, playlist_title) = match source {
        ListSource::Channel => {
            let Some(ch) = input.channel.as_deref().filter(|s| !s.trim().is_empty()) else {
                return Err(ConnectorError::InvalidParams(
                    "source='channel' requires 'channel'".to_string(),
                ));
            };
            let cid = resolve_channel_id_best_effort(&client, ch)
                .await
                .ok_or_else(|| {
                    ConnectorError::InvalidInput(
                        "Could not resolve channel_id from channel input. Provide a UC... channel \
                         ID, handle, or channel URL."
                            .to_string(),
                    )
                })?;
            let listed = list_channel_videos_native(&client, &cid, input.limit, after).await?;
            (listed.videos, Some(cid), listed.title, None, None)
        }
        ListSource::Playlist => {
            let Some(pl) = input.playlist.as_deref().filter(|s| !s.trim().is_empty()) else {
                return Err(ConnectorError::InvalidParams(
                    "source='playlist' requires 'playlist'".to_string(),
                ));
            };
            let pid = extract_playlist_id_from_str(pl).ok_or_else(|| {
                ConnectorError::InvalidInput(
                    "Could not parse playlist ID from playlist input. Provide a playlist ID or \
                     playlist URL."
                        .to_string(),
                )
            })?;
            let listed = list_playlist_videos_native(&client, &pid, input.limit, after).await?;
            (listed.videos, None, None, Some(pid), listed.title)
        }
    };

    finalize_list_entries(
        &mut videos,
        source,
        channel_id.as_deref(),
        playlist_id.as_deref(),
        playlist_title.as_deref(),
    );

    Ok(ListVideosOutput {
        entries: videos.clone(),
        videos,
        source,
        channel_id,
        channel_title,
        playlist_id,
        playlist_title,
    })
}

fn effective_list_source(input: &ListVideosInput) -> Result<ListSource, ConnectorError> {
    let has_channel = input
        .channel
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_playlist = input
        .playlist
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    match (has_channel, has_playlist) {
        (true, true) => Err(ConnectorError::InvalidParams(
            "Provide either channel or playlist, not both.".to_string(),
        )),
        (true, false) => Ok(ListSource::Channel),
        (false, true) => Ok(ListSource::Playlist),
        (false, false) => Ok(input.source),
    }
}

fn finalize_list_entries(
    videos: &mut [ListedVideo],
    source: ListSource,
    channel_id: Option<&str>,
    playlist_id: Option<&str>,
    playlist_title: Option<&str>,
) {
    for (idx, video) in videos.iter_mut().enumerate() {
        video.index = (idx + 1) as u64;
        if video.channel.is_none() {
            video.channel = video.channel_title.clone();
        }

        if source == ListSource::Channel && video.channel_id.is_none() {
            video.channel_id = channel_id.map(ToString::to_string);
        }

        if source == ListSource::Playlist {
            video.playlist_id = playlist_id.map(ToString::to_string);
            video.playlist_title = playlist_title.map(ToString::to_string);
        }
    }
}

fn list_videos_normalized_result(out: &ListVideosOutput) -> Result<CallToolResult, ConnectorError> {
    let items: Vec<ContentItem> = out
        .entries
        .iter()
        .map(|v| ContentItem {
            item_ref: format!("youtube:video:{}", v.id),
            kind: "video".to_string(),
            canonical_url: Some(v.url.clone()),
            title: Some(v.title.clone()),
            created_at: v.published_at.as_deref().and_then(parse_video_date),
            source_updated_at: None,
            authors: v
                .channel
                .as_deref()
                .map(|name| {
                    vec![Author {
                        name: name.to_string(),
                        id: v
                            .channel_id
                            .as_deref()
                            .map(|id| format!("youtube:channel:{id}")),
                    }]
                })
                .unwrap_or_default(),
            tags: Vec::new(),
            metadata: Some(json!({
                "index": v.index,
                "channel_id": v.channel_id.clone(),
                "playlist_id": v.playlist_id.clone(),
                "playlist_title": v.playlist_title.clone(),
            })),
            blocks: Vec::new(),
            relationships: Vec::new(),
            truncation: None,
        })
        .collect();

    let page = NormalizedPageV1::new(
        items,
        None,
        false,
        Partial::complete(Some(ingest::limits_max_items(out.entries.len() as u64))),
        Source::new("youtube", "list"),
    );
    structured_result(&page)
}

/// Convert a `rusty_ytdl::VideoError` into a `ConnectorError` with an
/// agent-actionable message. Per Anthropic's tool-writing guidance,
/// opaque upstream errors prevent agents from self-correcting — they
/// need to know *what* failed and *what to try next*.
fn classify_video_error(err: VideoError, video_id: &str) -> ConnectorError {
    match err {
        VideoError::VideoNotFound => ConnectorError::InvalidParams(format!(
            "YouTube video '{}' was not found. Double-check the video_id \
             or URL; the video may have been deleted or the id may be malformed. \
             Use `youtube.search` if you only have a title.",
            video_id
        )),
        VideoError::VideoIsPrivate => ConnectorError::InvalidInput(format!(
            "YouTube video '{}' is private and cannot be fetched without \
             owner authentication. No retry will help; inform the user.",
            video_id
        )),
        VideoError::LiveStreamNotSupported => ConnectorError::InvalidInput(format!(
            "YouTube video '{}' is a live stream, which this tool cannot \
             fetch. Wait for the stream to end and a VOD to be published, \
             or choose a different video.",
            video_id
        )),
        VideoError::VideoPlayerResponseError(detail) => {
            // Covers age-gated, region-locked, unplayable, and similar.
            // The detail string from YouTube often names the specific cause.
            let lower = detail.to_lowercase();
            let hint = if lower.contains("age") {
                "Video appears to be age-restricted. This tool cannot \
                 authenticate; inform the user or try a mirror."
            } else if lower.contains("region") || lower.contains("country") {
                "Video appears to be region-restricted. A different network \
                 or proxy may be required."
            } else if lower.contains("removed") || lower.contains("unavailable") {
                "Video appears unavailable (removed, geo-blocked, or \
                 temporarily inaccessible). Try again later or pick \
                 another video."
            } else {
                "YouTube refused to return player data. Try a different \
                 video; this one is likely restricted or unavailable."
            };
            ConnectorError::InvalidInput(format!(
                "YouTube rejected playback for '{}': {}. {}",
                video_id, detail, hint
            ))
        }
        VideoError::Reqwest(_)
        | VideoError::ReqwestMiddleware(_)
        | VideoError::DownloadError(_) => ConnectorError::Other(format!(
            "Network error fetching YouTube video '{}': {}. Transient — \
             the agent may retry once after a short delay.",
            video_id, err
        )),
        other => ConnectorError::Other(format!(
            "Unexpected YouTube error for '{}': {}.",
            video_id, other
        )),
    }
}

/// Map a transcript-fetch failure to a short, stable reason code the agent
/// can branch on without regex-matching free-form error text. Also returns
/// a human-readable explanation suitable for logs.
fn classify_transcript_error(err: &CouldNotRetrieveTranscript) -> (&'static str, String) {
    match &err.reason {
        Some(CouldNotRetrieveTranscriptReason::TranscriptsDisabled) => (
            "transcripts_disabled",
            "The channel has disabled subtitles for this video.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::NoTranscriptFound { .. }) => (
            "no_english_transcript",
            "No English transcript is available; other languages may exist.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::VideoUnavailable) => (
            "video_unavailable",
            "The video is no longer available (removed or private).".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::VideoUnplayable { reason, .. }) => (
            "video_unplayable",
            reason
                .clone()
                .unwrap_or_else(|| "The video is unplayable.".to_string()),
        ),
        Some(CouldNotRetrieveTranscriptReason::IpBlocked(_)) => (
            "ip_blocked",
            "YouTube is blocking this server's IP address.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::RequestBlocked(_)) => (
            "request_blocked",
            "YouTube is rate-limiting or blocking requests.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::AgeRestricted) => (
            "age_restricted",
            "The video is age-restricted and requires authentication.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::InvalidVideoId) => (
            "invalid_video_id",
            "The provided video id was not recognized by YouTube.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::YouTubeRequestFailed(msg)) => (
            "upstream_request_failed",
            format!("YouTube request failed: {msg}"),
        ),
        Some(CouldNotRetrieveTranscriptReason::YouTubeDataUnparsable(_)) => (
            "youtube_data_unparsable",
            "YouTube's response could not be parsed; may be a transient \
             upstream change."
                .to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::FailedToCreateConsentCookie) => (
            "consent_cookie_failed",
            "Could not establish a consent cookie with YouTube.".to_string(),
        ),
        Some(CouldNotRetrieveTranscriptReason::TranslationUnavailable(_))
        | Some(CouldNotRetrieveTranscriptReason::TranslationLanguageUnavailable(_)) => (
            "translation_unavailable",
            "Requested transcript translation is not available.".to_string(),
        ),
        None => (
            "unknown",
            "Transcript fetch failed without a reason.".to_string(),
        ),
    }
}

fn resolve_get_target(input: &GetVideoDetailsInput) -> Result<YouTubeGetTarget, ConnectorError> {
    if let Some(video_id) = input.video_id.as_deref() {
        if !video_id.trim().is_empty() {
            return Ok(classify_get_identifier(video_id));
        }
    }
    if let Some(item_ref) = input.item_ref.as_deref() {
        if let Some((kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "youtube") {
            return match kind.as_str() {
                "video" => Ok(YouTubeGetTarget::Video(id)),
                "playlist" => Ok(YouTubeGetTarget::Playlist(id)),
                "channel" => Ok(YouTubeGetTarget::Channel(id)),
                _ => Err(ConnectorError::InvalidParams(format!(
                    "Unsupported YouTube item_ref kind '{kind}'. Expected video, playlist, or channel."
                ))),
            };
        }
    }
    if let Some(url) = input.url.as_deref() {
        if !url.trim().is_empty() {
            return Ok(classify_get_identifier(url));
        }
    }
    Err(ConnectorError::InvalidParams(
        "Missing YouTube identifier. Provide video_id, item_ref, or url.".to_string(),
    ))
}

fn classify_get_identifier(input: &str) -> YouTubeGetTarget {
    let trimmed = input.trim();

    if is_youtube_video_url(trimmed) {
        return YouTubeGetTarget::Video(extract_video_id(trimmed));
    }
    if is_youtube_playlist_input(trimmed) {
        return YouTubeGetTarget::Playlist(trimmed.to_string());
    }
    if is_youtube_channel_input(trimmed) {
        return YouTubeGetTarget::Channel(trimmed.to_string());
    }

    YouTubeGetTarget::Video(extract_video_id(trimmed))
}

fn is_youtube_video_url(input: &str) -> bool {
    let Ok(url) = Url::parse(input) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    if host == "youtu.be" {
        return true;
    }
    is_youtube_host(host)
        && (url.path() == "/watch" && url.query_pairs().any(|(key, _)| key == "v")
            || url.path().starts_with("/embed/")
            || url.path().starts_with("/shorts/"))
}

fn is_youtube_playlist_input(input: &str) -> bool {
    extract_playlist_id_from_str(input).is_some() && !is_youtube_video_url(input)
}

fn is_youtube_channel_input(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.starts_with('@') || extract_channel_id_from_str(trimmed).is_some() {
        return true;
    }

    let Ok(url) = Url::parse(trimmed) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if !is_youtube_host(host) {
        return false;
    }

    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    matches!(
        segments.as_slice(),
        [first, ..] if first.starts_with('@') || *first == "channel" || *first == "c" || *first == "user"
    )
}

fn is_youtube_host(host: &str) -> bool {
    host == "youtube.com" || host.ends_with(".youtube.com")
}

// Helper function to extract video ID from either a full URL or just the ID
fn extract_video_id(input: &str) -> String {
    // Check if the input is a URL
    if input.starts_with("http") {
        // Try to parse as URL
        if let Ok(url) = Url::parse(input) {
            // Extract video ID from query parameters (youtube.com/watch?v=VIDEO_ID)
            if let Some(pairs) = url.query_pairs().find(|(key, _)| key == "v") {
                return pairs.1.to_string();
            }

            // Extract from path segments (youtu.be/VIDEO_ID)
            let path = url.path();
            if url.host_str() == Some("youtu.be") && path.len() > 1 {
                return path[1..].to_string();
            }

            if let Some(id) = path.strip_prefix("/embed/") {
                return id.trim_matches('/').to_string();
            }

            if let Some(id) = path.strip_prefix("/shorts/") {
                return id.trim_matches('/').to_string();
            }
        }
    }

    // If not a URL or couldn't extract ID, assume the input is already a video ID
    input.to_string()
}

fn extract_channel_id_from_str(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.starts_with("UC") && trimmed.len() >= 20 {
        return Some(trimmed.to_string());
    }

    if let Ok(url) = Url::parse(trimmed) {
        if let Some(seg) = url.path_segments() {
            let segs: Vec<_> = seg.collect();
            if let Some(idx) = segs.iter().position(|p| *p == "channel") {
                if let Some(cid) = segs.get(idx + 1) {
                    if cid.starts_with("UC") {
                        return Some((*cid).to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_playlist_id_from_str(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.starts_with("PL") || trimmed.starts_with("UU") || trimmed.starts_with("OLAK5") {
        return Some(trimmed.to_string());
    }
    if let Ok(url) = Url::parse(trimmed) {
        for (k, v) in url.query_pairs() {
            if k == "list" && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

async fn resolve_channel_id_best_effort(client: &HttpClient, channel: &str) -> Option<String> {
    if let Some(cid) = extract_channel_id_from_str(channel) {
        return Some(cid);
    }

    let trimmed = channel.trim();
    let url = if trimmed.starts_with("@") {
        format!("https://www.youtube.com/{}", trimmed)
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://www.youtube.com/{}", trimmed)
    };

    let html = client.get(url).send().await.ok()?.text().await.ok()?;

    static CHANNEL_ID_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#""channelId"\s*:\s*"(?P<id>UC[a-zA-Z0-9_-]{10,})""#).expect("channelId regex")
    });
    static CANONICAL_CHANNEL_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"<link\s+rel="canonical"\s+href="https://www\.youtube\.com/channel/(?P<id>UC[a-zA-Z0-9_-]{10,})""#)
            .expect("canonical channel regex")
    });
    static BROWSE_ID_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#""browseId"\s*:\s*"(?P<id>UC[a-zA-Z0-9_-]{10,})""#).expect("browseId regex")
    });

    CANONICAL_CHANNEL_RE
        .captures(&html)
        .or_else(|| CHANNEL_ID_RE.captures(&html))
        .or_else(|| BROWSE_ID_RE.captures(&html))
        .and_then(|c| c.name("id").map(|m| m.as_str().to_string()))
}

const YOUTUBE_WEB_CLIENT_VERSION: &str = "2.20260114.08.00";
const YOUTUBE_LIST_MAX_PAGES: usize = 500;

#[derive(Debug, Clone)]
struct YouTubeInnertubeConfig {
    api_key: String,
    context: Value,
    client_name: String,
    client_version: String,
    visitor_data: Option<String>,
    user_agent: String,
}

#[derive(Debug)]
struct YouTubeListInitialPage {
    config: YouTubeInnertubeConfig,
    root: Value,
    fallback_channel_title: Option<String>,
    list_title: Option<String>,
}

#[derive(Debug)]
struct YouTubeListedVideos {
    videos: Vec<ListedVideo>,
    title: Option<String>,
}

async fn list_channel_videos_native(
    client: &HttpClient,
    channel_id: &str,
    limit: Option<u64>,
    published_after: Option<DateTime<Utc>>,
) -> Result<YouTubeListedVideos, ConnectorError> {
    list_youtube_url_videos_native(
        client,
        &format!("https://www.youtube.com/channel/{channel_id}/videos?hl=en"),
        ListSource::Channel,
        limit,
        published_after,
    )
    .await
}

async fn list_playlist_videos_native(
    client: &HttpClient,
    playlist_id: &str,
    limit: Option<u64>,
    published_after: Option<DateTime<Utc>>,
) -> Result<YouTubeListedVideos, ConnectorError> {
    list_youtube_url_videos_native(
        client,
        &format!("https://www.youtube.com/playlist?list={playlist_id}&hl=en"),
        ListSource::Playlist,
        limit,
        published_after,
    )
    .await
}

async fn list_youtube_url_videos_native(
    client: &HttpClient,
    url: &str,
    source: ListSource,
    limit: Option<u64>,
    published_after: Option<DateTime<Utc>>,
) -> Result<YouTubeListedVideos, ConnectorError> {
    let mut last_error = None;
    for attempt in 0..3 {
        let html = fetch_youtube_html(client, url).await?;
        let initial = parse_youtube_list_initial_page(&html, source)?;
        let title = initial.list_title.clone();
        match collect_youtube_list_pages(client, initial, source, limit, published_after).await {
            Ok(videos) => return Ok(YouTubeListedVideos { videos, title }),
            Err(err) if is_empty_youtube_list_error(&err) && attempt < 2 => {
                last_error = Some(err);
                continue;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ConnectorError::Other("YouTube list page did not contain any video entries".to_string())
    }))
}

fn is_empty_youtube_list_error(err: &ConnectorError) -> bool {
    matches!(
        err,
        ConnectorError::Other(msg) if msg == "YouTube list page did not contain any video entries"
    )
}

async fn fetch_youtube_html(client: &HttpClient, url: &str) -> Result<String, ConnectorError> {
    client
        .get(url)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?
        .error_for_status()
        .map_err(ConnectorError::HttpRequest)?
        .text()
        .await
        .map_err(ConnectorError::HttpRequest)
}

fn parse_youtube_list_initial_page(
    html: &str,
    source: ListSource,
) -> Result<YouTubeListInitialPage, ConnectorError> {
    let initial_data = extract_json_object_after_marker(html, "var ytInitialData = ")
        .or_else(|| extract_json_object_after_marker(html, "window[\"ytInitialData\"] = "))
        .ok_or_else(|| {
            ConnectorError::Other(
                "Could not find ytInitialData in the YouTube list page".to_string(),
            )
        })?;

    let data: Value =
        serde_json::from_str(initial_data).map_err(|e| ConnectorError::Other(e.to_string()))?;
    let config = extract_innertube_config(html)?;
    let fallback_channel_title = data
        .get("metadata")
        .and_then(|value| value.get("channelMetadataRenderer"))
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let list_title = match source {
        ListSource::Channel => fallback_channel_title.clone(),
        ListSource::Playlist => extract_playlist_title(&data),
    };
    let root = match source {
        ListSource::Channel => selected_channel_tab_content(&data).unwrap_or(&data).clone(),
        ListSource::Playlist => selected_playlist_content(&data).unwrap_or(&data).clone(),
    };

    Ok(YouTubeListInitialPage {
        config,
        root,
        fallback_channel_title,
        list_title,
    })
}

#[cfg(test)]
fn parse_channel_videos_from_page(html: &str) -> Result<Vec<ListedVideo>, ConnectorError> {
    let initial_data = extract_json_object_after_marker(html, "var ytInitialData = ")
        .or_else(|| extract_json_object_after_marker(html, "window[\"ytInitialData\"] = "))
        .ok_or_else(|| {
            ConnectorError::Other(
                "Could not find ytInitialData in the YouTube channel videos page".to_string(),
            )
        })?;
    let data: Value =
        serde_json::from_str(initial_data).map_err(|e| ConnectorError::Other(e.to_string()))?;
    let fallback_channel_title = data
        .get("metadata")
        .and_then(|value| value.get("channelMetadataRenderer"))
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let root = selected_channel_tab_content(&data).unwrap_or(&data);

    let mut seen_ids = HashSet::new();
    let mut videos = Vec::new();
    let _ = append_videos_from_value(
        root,
        ListSource::Channel,
        fallback_channel_title.as_deref(),
        None,
        None,
        &mut seen_ids,
        &mut videos,
    );
    if videos.is_empty() {
        return Err(ConnectorError::Other(
            "YouTube channel videos page did not contain any video entries".to_string(),
        ));
    }

    Ok(videos)
}

async fn collect_youtube_list_pages(
    client: &HttpClient,
    initial: YouTubeListInitialPage,
    source: ListSource,
    limit: Option<u64>,
    published_after: Option<DateTime<Utc>>,
) -> Result<Vec<ListedVideo>, ConnectorError> {
    let max_items = limit.map(|limit| limit as usize);
    if max_items == Some(0) {
        return Ok(Vec::new());
    }

    let mut config = initial.config;
    let fallback_channel_title = initial.fallback_channel_title;
    let mut page = initial.root;
    let mut seen_continuations = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut videos = Vec::new();

    for page_num in 0..YOUTUBE_LIST_MAX_PAGES {
        let hit_date_cutoff = append_videos_from_value(
            &page,
            source,
            fallback_channel_title.as_deref(),
            published_after,
            max_items,
            &mut seen_ids,
            &mut videos,
        );
        if max_items.is_some_and(|max| videos.len() >= max) || hit_date_cutoff {
            break;
        }

        let Some(continuation) = extract_continuation_token(&page) else {
            break;
        };
        if !seen_continuations.insert(continuation.clone()) {
            tracing::warn!(
                source = ?source,
                "YouTube list pagination returned a repeated continuation token"
            );
            break;
        }

        page = fetch_youtube_browse_continuation(client, &config, &continuation).await?;
        if let Some(visitor_data) = extract_visitor_data(&page) {
            config.visitor_data = Some(visitor_data);
        }

        if page_num + 1 == YOUTUBE_LIST_MAX_PAGES {
            return Err(ConnectorError::Other(format!(
                "YouTube list pagination exceeded {YOUTUBE_LIST_MAX_PAGES} pages"
            )));
        }
    }

    if videos.is_empty() {
        return Err(ConnectorError::Other(
            "YouTube list page did not contain any video entries".to_string(),
        ));
    }

    Ok(videos)
}

async fn fetch_youtube_browse_continuation(
    client: &HttpClient,
    config: &YouTubeInnertubeConfig,
    continuation: &str,
) -> Result<Value, ConnectorError> {
    let url = format!(
        "https://www.youtube.com/youtubei/v1/browse?key={}&prettyPrint=false",
        config.api_key
    );
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("origin", "https://www.youtube.com")
        .header("x-youtube-client-name", &config.client_name)
        .header("x-youtube-client-version", &config.client_version)
        .header("user-agent", &config.user_agent);
    if let Some(visitor_data) = config.visitor_data.as_deref() {
        request = request.header("x-goog-visitor-id", visitor_data);
    }

    request
        .json(&json!({
            "context": config.context,
            "continuation": continuation,
        }))
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?
        .error_for_status()
        .map_err(ConnectorError::HttpRequest)?
        .json()
        .await
        .map_err(ConnectorError::HttpRequest)
}

fn append_videos_from_value(
    value: &Value,
    source: ListSource,
    fallback_channel_title: Option<&str>,
    published_after: Option<DateTime<Utc>>,
    max_items: Option<usize>,
    seen_ids: &mut HashSet<String>,
    videos: &mut Vec<ListedVideo>,
) -> bool {
    let mut renderers = Vec::new();
    match source {
        ListSource::Channel => collect_video_renderers(value, &mut renderers),
        ListSource::Playlist => collect_playlist_video_renderers(value, &mut renderers),
    }

    let mut dated = 0usize;
    let mut older_than_cutoff = 0usize;

    for renderer in renderers {
        if max_items.is_some_and(|max| videos.len() >= max) {
            break;
        }

        let video = match source {
            ListSource::Channel => listed_video_from_renderer(renderer, fallback_channel_title),
            ListSource::Playlist => listed_video_from_playlist_renderer(renderer),
        };
        let Some(video) = video else {
            continue;
        };

        let published_at = listed_video_published_at(&video);
        if let Some(dt) = published_at {
            dated += 1;
            if published_after.is_some_and(|after| dt < after) {
                older_than_cutoff += 1;
            }
        }
        if let Some(after) = published_after {
            if published_at.map(|dt| dt < after).unwrap_or(true) {
                continue;
            }
        }
        if seen_ids.insert(video.id.clone()) {
            videos.push(video);
        }
    }

    source == ListSource::Channel
        && published_after.is_some()
        && dated > 0
        && dated == older_than_cutoff
}

fn extract_innertube_config(html: &str) -> Result<YouTubeInnertubeConfig, ConnectorError> {
    let ytcfg = extract_json_objects_after_marker(html, "ytcfg.set(")
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<Value>(raw).ok())
        .find(|cfg| {
            cfg.get("INNERTUBE_API_KEY")
                .and_then(Value::as_str)
                .is_some()
        })
        .ok_or_else(|| {
            ConnectorError::Other("Could not find YouTube Innertube config in page".to_string())
        })?;

    let api_key = ytcfg
        .get("INNERTUBE_API_KEY")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ConnectorError::Other("YouTube page did not include INNERTUBE_API_KEY".to_string())
        })?
        .to_string();
    let client_version = ytcfg
        .get("INNERTUBE_CLIENT_VERSION")
        .and_then(Value::as_str)
        .or_else(|| {
            ytcfg
                .get("INNERTUBE_CONTEXT")
                .and_then(|value| value.get("client"))
                .and_then(|value| value.get("clientVersion"))
                .and_then(Value::as_str)
        })
        .unwrap_or(YOUTUBE_WEB_CLIENT_VERSION)
        .to_string();
    let client_name = ytcfg
        .get("INNERTUBE_CONTEXT_CLIENT_NAME")
        .and_then(|value| {
            value
                .as_u64()
                .map(|n| n.to_string())
                .or_else(|| value.as_str().map(ToString::to_string))
        })
        .unwrap_or_else(|| "1".to_string());
    let user_agent = ytcfg
        .get("INNERTUBE_CONTEXT")
        .and_then(|value| value.get("client"))
        .and_then(|value| value.get("userAgent"))
        .and_then(Value::as_str)
        .unwrap_or(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        )
        .to_string();
    let visitor_data = ytcfg
        .get("VISITOR_DATA")
        .and_then(Value::as_str)
        .or_else(|| {
            ytcfg
                .get("INNERTUBE_CONTEXT")
                .and_then(|value| value.get("client"))
                .and_then(|value| value.get("visitorData"))
                .and_then(Value::as_str)
        })
        .map(ToString::to_string);
    let mut context = ytcfg
        .get("INNERTUBE_CONTEXT")
        .cloned()
        .unwrap_or_else(|| default_innertube_context(&client_version));
    normalize_innertube_context(&mut context, &client_version, visitor_data.as_deref());

    Ok(YouTubeInnertubeConfig {
        api_key,
        context,
        client_name,
        client_version,
        visitor_data,
        user_agent,
    })
}

fn default_innertube_context(client_version: &str) -> Value {
    json!({
        "client": {
            "clientName": "WEB",
            "clientVersion": client_version,
            "hl": "en",
            "gl": "US",
            "timeZone": "UTC",
            "utcOffsetMinutes": 0
        }
    })
}

fn normalize_innertube_context(
    context: &mut Value,
    client_version: &str,
    visitor_data: Option<&str>,
) {
    let Some(context_obj) = context.as_object_mut() else {
        *context = default_innertube_context(client_version);
        return;
    };
    let client = context_obj
        .entry("client")
        .or_insert_with(|| json!({}))
        .as_object_mut();
    let Some(client_obj) = client else {
        context_obj.insert(
            "client".to_string(),
            json!({
                "clientName": "WEB",
                "clientVersion": client_version,
                "hl": "en",
                "gl": "US",
                "timeZone": "UTC",
                "utcOffsetMinutes": 0
            }),
        );
        return;
    };
    client_obj
        .entry("clientName")
        .or_insert_with(|| json!("WEB"));
    client_obj
        .entry("clientVersion")
        .or_insert_with(|| json!(client_version));
    client_obj.entry("hl").or_insert_with(|| json!("en"));
    client_obj.entry("gl").or_insert_with(|| json!("US"));
    client_obj.entry("timeZone").or_insert_with(|| json!("UTC"));
    client_obj
        .entry("utcOffsetMinutes")
        .or_insert_with(|| json!(0));
    if let Some(visitor_data) = visitor_data {
        client_obj
            .entry("visitorData")
            .or_insert_with(|| json!(visitor_data));
    }
}

fn extract_visitor_data(value: &Value) -> Option<String> {
    value
        .get("responseContext")
        .and_then(|value| value.get("visitorData"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_json_object_after_marker<'a>(html: &'a str, marker: &str) -> Option<&'a str> {
    let marker_start = html.find(marker)?;
    let search_start = marker_start + marker.len();
    let json_start = search_start + html[search_start..].find('{')?;
    extract_json_object_at(html, json_start)
}

fn extract_json_objects_after_marker<'a>(html: &'a str, marker: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while let Some(marker_start) = html[offset..].find(marker) {
        let search_start = offset + marker_start + marker.len();
        let Some(relative_json_start) = html[search_start..].find('{') else {
            break;
        };
        let json_start = search_start + relative_json_start;
        if let Some(raw) = extract_json_object_at(html, json_start) {
            out.push(raw);
            offset = json_start + raw.len();
        } else {
            offset = search_start;
        }
    }
    out
}

fn extract_json_object_at(html: &str, json_start: usize) -> Option<&str> {
    let bytes = html.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, byte) in bytes.iter().enumerate().skip(json_start) {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(&html[json_start..=idx]);
                }
            }
            _ => {}
        }
    }

    None
}

fn selected_channel_tab_content(data: &Value) -> Option<&Value> {
    let tabs = data
        .get("contents")?
        .get("twoColumnBrowseResultsRenderer")?
        .get("tabs")?
        .as_array()?;

    tabs.iter()
        .find_map(|tab| {
            let renderer = tab.get("tabRenderer")?;
            let is_videos_tab = renderer
                .get("title")
                .and_then(Value::as_str)
                .map(|title| title.eq_ignore_ascii_case("Videos"))
                .unwrap_or(false);
            if is_videos_tab {
                renderer.get("content")
            } else {
                None
            }
        })
        .or_else(|| {
            tabs.iter().find_map(|tab| {
                let renderer = tab.get("tabRenderer")?;
                let is_selected = renderer
                    .get("selected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if is_selected {
                    renderer.get("content")
                } else {
                    None
                }
            })
        })
}

fn selected_playlist_content(data: &Value) -> Option<&Value> {
    data.get("contents")?
        .get("twoColumnBrowseResultsRenderer")?
        .get("tabs")?
        .as_array()?
        .iter()
        .find_map(|tab| {
            let renderer = tab.get("tabRenderer")?;
            let is_selected = renderer
                .get("selected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_selected {
                renderer.get("content")
            } else {
                None
            }
        })
}

fn extract_playlist_title(data: &Value) -> Option<String> {
    data.get("metadata")
        .and_then(|value| value.get("playlistMetadataRenderer"))
        .and_then(|value| value.get("title"))
        .and_then(json_title)
        .or_else(|| find_renderer_title(data, "playlistHeaderRenderer"))
        .or_else(|| find_renderer_title(data, "playlistSidebarPrimaryInfoRenderer"))
}

fn find_renderer_title(value: &Value, renderer_key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(renderer) = map.get(renderer_key) {
                if let Some(title) = renderer.get("title").and_then(json_title) {
                    return Some(title);
                }
            }
            map.values()
                .find_map(|child| find_renderer_title(child, renderer_key))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_renderer_title(child, renderer_key)),
        _ => None,
    }
}

fn collect_video_renderers<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if let Some(renderer) = map.get("videoRenderer") {
                out.push(renderer);
                return;
            }
            if let Some(renderer) = map.get("lockupViewModel") {
                if renderer
                    .get("contentType")
                    .and_then(Value::as_str)
                    .is_some_and(|content_type| content_type == "LOCKUP_CONTENT_TYPE_VIDEO")
                {
                    out.push(renderer);
                    return;
                }
            }
            for child in map.values() {
                collect_video_renderers(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_video_renderers(item, out);
            }
        }
        _ => {}
    }
}

fn collect_playlist_video_renderers<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if let Some(renderer) = map.get("playlistVideoRenderer") {
                out.push(renderer);
                return;
            }
            if let Some(renderer) = map.get("playlistPanelVideoRenderer") {
                out.push(renderer);
                return;
            }
            for child in map.values() {
                collect_playlist_video_renderers(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_playlist_video_renderers(item, out);
            }
        }
        _ => {}
    }
}

fn extract_channel_id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(id) = map.get("browseId").and_then(Value::as_str) {
                if id.starts_with("UC") {
                    return Some(id.to_string());
                }
            }
            for child in map.values() {
                if let Some(id) = extract_channel_id_from_value(child) {
                    return Some(id);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_channel_id_from_value),
        _ => None,
    }
}

fn listed_video_from_renderer(
    renderer: &Value,
    fallback_channel_title: Option<&str>,
) -> Option<ListedVideo> {
    if renderer.get("contentType").and_then(Value::as_str) == Some("LOCKUP_CONTENT_TYPE_VIDEO") {
        return listed_video_from_lockup_view_model(renderer, fallback_channel_title);
    }

    let id = renderer.get("videoId")?.as_str()?.to_string();
    let title = json_text(renderer.get("title")?)?;
    let published_at = renderer
        .get("publishedTimeText")
        .and_then(json_text)
        .or_else(|| {
            renderer
                .get("upcomingEventData")
                .and_then(|value| value.get("startTime"))
                .and_then(Value::as_str)
                .and_then(|raw| raw.parse::<i64>().ok())
                .and_then(|unix_seconds| DateTime::<Utc>::from_timestamp(unix_seconds, 0))
                .map(|dt| dt.to_rfc3339())
        });
    let channel_title = renderer
        .get("ownerText")
        .and_then(json_text)
        .or_else(|| renderer.get("longBylineText").and_then(json_text))
        .or_else(|| fallback_channel_title.map(ToString::to_string));
    let channel_id = extract_channel_id_from_value(renderer);

    Some(ListedVideo {
        index: 0,
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        published_at,
        channel: channel_title.clone(),
        channel_title,
        channel_id,
        playlist_id: None,
        playlist_title: None,
    })
}

fn listed_video_from_lockup_view_model(
    renderer: &Value,
    fallback_channel_title: Option<&str>,
) -> Option<ListedVideo> {
    let id = renderer
        .get("contentId")
        .and_then(Value::as_str)
        .or_else(|| {
            renderer
                .get("rendererContext")
                .and_then(|value| value.get("commandContext"))
                .and_then(|value| value.get("onTap"))
                .and_then(|value| value.get("innertubeCommand"))
                .and_then(|value| value.get("watchEndpoint"))
                .and_then(|value| value.get("videoId"))
                .and_then(Value::as_str)
        })?
        .to_string();
    let title = renderer
        .get("metadata")
        .and_then(|value| value.get("lockupMetadataViewModel"))
        .and_then(|value| value.get("title"))
        .and_then(view_model_text)?;
    let published_at = lockup_metadata_text_parts(renderer)
        .into_iter()
        .find_map(|text| parse_video_date(&text));
    let channel_title = fallback_channel_title.map(ToString::to_string);
    let channel_id = extract_channel_id_from_value(renderer);

    Some(ListedVideo {
        index: 0,
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        published_at,
        channel: channel_title.clone(),
        channel_title,
        channel_id,
        playlist_id: None,
        playlist_title: None,
    })
}

fn listed_video_from_playlist_renderer(renderer: &Value) -> Option<ListedVideo> {
    let id = renderer
        .get("videoId")
        .or_else(|| renderer.get("video_id"))?
        .as_str()?
        .to_string();
    let title = renderer
        .get("title")
        .and_then(json_text)
        .or_else(|| renderer.get("headline").and_then(json_text))?;
    let channel_title = renderer
        .get("shortBylineText")
        .and_then(json_text)
        .or_else(|| renderer.get("longBylineText").and_then(json_text))
        .or_else(|| renderer.get("ownerText").and_then(json_text));
    let channel_id = extract_channel_id_from_value(renderer);
    let published_at = renderer
        .get("publishedTimeText")
        .and_then(json_text)
        .and_then(|raw| parse_video_date(&raw));

    Some(ListedVideo {
        index: 0,
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        published_at,
        channel: channel_title.clone(),
        channel_title,
        channel_id,
        playlist_id: None,
        playlist_title: None,
    })
}

fn extract_continuation_token(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(token) = map
                .get("continuationCommand")
                .and_then(|value| value.get("token"))
                .and_then(Value::as_str)
            {
                return Some(token.to_string());
            }
            if let Some(token) = map
                .get("continuationEndpoint")
                .and_then(|value| value.get("continuationCommand"))
                .and_then(|value| value.get("token"))
                .and_then(Value::as_str)
            {
                return Some(token.to_string());
            }
            for child in map.values() {
                if let Some(token) = extract_continuation_token(child) {
                    return Some(token);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_continuation_token),
        _ => None,
    }
}

fn json_text(value: &Value) -> Option<String> {
    if let Some(simple) = value.get("simpleText").and_then(Value::as_str) {
        let text = normalize_ws(simple);
        if !text.is_empty() {
            return Some(text);
        }
    }

    let runs = value.get("runs")?.as_array()?;
    let text = runs
        .iter()
        .filter_map(|run| run.get("text").and_then(Value::as_str))
        .collect::<String>();
    let text = normalize_ws(&text);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn json_title(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str().map(normalize_ws).filter(|s| !s.is_empty()) {
        return Some(text);
    }

    json_text(value)
}

fn view_model_text(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(Value::as_str)
        .map(normalize_ws)
        .filter(|text| !text.is_empty())
        .or_else(|| json_title(value))
}

fn lockup_metadata_text_parts(renderer: &Value) -> Vec<String> {
    let rows = renderer
        .get("metadata")
        .and_then(|value| value.get("lockupMetadataViewModel"))
        .and_then(|value| value.get("metadata"))
        .and_then(|value| value.get("contentMetadataViewModel"))
        .and_then(|value| value.get("metadataRows"))
        .and_then(Value::as_array);
    let Some(rows) = rows else {
        return Vec::new();
    };

    rows.iter()
        .flat_map(|row| {
            row.get("metadataParts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|part| {
            part.get("text")
                .and_then(view_model_text)
                .or_else(|| {
                    part.get("accessibilityLabel")
                        .and_then(Value::as_str)
                        .map(normalize_ws)
                })
                .filter(|text| !text.is_empty())
        })
        .collect()
}

async fn fetch_transcript_from_timedtext(
    video_id: &str,
) -> Result<TimedTextTranscript, ConnectorError> {
    match fetch_transcript_from_watch_page_timedtext(video_id).await {
        Ok(transcript) => return Ok(transcript),
        Err(e) => {
            tracing::warn!(
                error = %e,
                video_id = %video_id,
                "Watch-page timedtext fallback failed; trying yt-dlp fallback"
            );
        }
    }

    fetch_transcript_via_yt_dlp(video_id).await
}

async fn fetch_transcript_from_watch_page_timedtext(
    video_id: &str,
) -> Result<TimedTextTranscript, ConnectorError> {
    let client = HttpClient::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| ConnectorError::Other(e.to_string()))?;

    let watch_url = format!("https://www.youtube.com/watch?v={}&hl=en", video_id);
    let watch_html = client
        .get(watch_url)
        .send()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?
        .error_for_status()
        .map_err(|e| ConnectorError::Other(e.to_string()))?
        .text()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;

    let tracks = extract_caption_tracks_from_watch_html(&watch_html)?;
    let selected = select_preferred_caption_track(&tracks).ok_or_else(|| {
        ConnectorError::Other("No usable caption track found on watch page".to_string())
    })?;

    let timedtext_url = force_json3_caption_url(
        selected
            .base_url
            .as_deref()
            .ok_or_else(|| ConnectorError::Other("Caption track missing baseUrl".to_string()))?,
    );
    let text = fetch_transcript_text_from_timedtext_url(&client, &timedtext_url).await?;

    let language_code = selected
        .language_code
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let is_generated = selected.kind.as_deref() == Some("asr");

    Ok(TimedTextTranscript {
        text,
        language_code,
        is_generated,
    })
}

async fn fetch_transcript_via_yt_dlp(
    video_id: &str,
) -> Result<TimedTextTranscript, ConnectorError> {
    let mut cmd = Command::new("yt-dlp");
    cmd.arg("-J")
        .arg("--skip-download")
        .arg("--no-warnings")
        .arg(format!("https://www.youtube.com/watch?v={video_id}"))
        .kill_on_drop(true);

    let output = tokio::time::timeout(StdDuration::from_secs(30), cmd.output())
        .await
        .map_err(|_| ConnectorError::Other("yt-dlp timed out while fetching captions".to_string()))?
        .map_err(|e| ConnectorError::Other(format!("Failed to execute yt-dlp: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reason = if stderr.is_empty() {
            format!("yt-dlp exited with status {}", output.status)
        } else {
            format!("yt-dlp failed: {stderr}")
        };
        return Err(ConnectorError::Other(reason));
    }

    let info: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| ConnectorError::Other(e.to_string()))?;
    let (timedtext_url, language_code, is_generated) = select_yt_dlp_caption_url(&info)
        .ok_or_else(|| {
            ConnectorError::Other("yt-dlp output did not include usable caption URLs".to_string())
        })?;

    let client = HttpClient::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    let text = fetch_transcript_text_from_timedtext_url(&client, &timedtext_url).await?;

    Ok(TimedTextTranscript {
        text,
        language_code,
        is_generated,
    })
}

fn extract_caption_tracks_from_watch_html(
    watch_html: &str,
) -> Result<Vec<TimedTextTrack>, ConnectorError> {
    static CAPTION_TRACKS_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?s)"captionTracks":(\[.*?\]),"audioTracks""#)
            .expect("valid captionTracks regex")
    });

    let tracks_json = CAPTION_TRACKS_RE
        .captures(watch_html)
        .and_then(|c| c.get(1).map(|m| m.as_str()))
        .ok_or_else(|| {
            ConnectorError::Other(
                "Could not locate captionTracks in YouTube watch page response".to_string(),
            )
        })?;

    let tracks: Vec<TimedTextTrack> =
        serde_json::from_str(tracks_json).map_err(|e| ConnectorError::Other(e.to_string()))?;

    if tracks.is_empty() {
        return Err(ConnectorError::Other(
            "YouTube watch page did not include any caption tracks".to_string(),
        ));
    }

    Ok(tracks)
}

fn select_preferred_caption_track(tracks: &[TimedTextTrack]) -> Option<&TimedTextTrack> {
    const ENGLISH_CODES: [&str; 5] = ["en", "en-us", "en-gb", "en-in", "en-orig"];

    for preferred in ENGLISH_CODES {
        if let Some(track) = tracks.iter().find(|track| {
            track
                .language_code
                .as_deref()
                .map(|code| code.eq_ignore_ascii_case(preferred))
                .unwrap_or(false)
        }) {
            return Some(track);
        }
    }

    if let Some(track) = tracks.iter().find(|track| {
        track
            .language_code
            .as_deref()
            .map(|code| code.to_ascii_lowercase().starts_with("en"))
            .unwrap_or(false)
    }) {
        return Some(track);
    }

    tracks.first()
}

fn force_json3_caption_url(base_url: &str) -> String {
    if let Ok(mut parsed) = Url::parse(base_url) {
        let mut pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let mut has_fmt = false;
        for (key, value) in &mut pairs {
            if key == "fmt" {
                *value = "json3".to_string();
                has_fmt = true;
            }
        }
        if !has_fmt {
            pairs.push(("fmt".to_string(), "json3".to_string()));
        }

        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish();
        parsed.set_query(Some(&query));
        parsed.to_string()
    } else if base_url.contains('?') {
        format!("{base_url}&fmt=json3")
    } else {
        format!("{base_url}?fmt=json3")
    }
}

fn select_yt_dlp_caption_url(info: &Value) -> Option<(String, String, bool)> {
    const ENGLISH_CODES: [&str; 5] = ["en", "en-us", "en-gb", "en-in", "en-orig"];

    let automatic = info.get("automatic_captions").and_then(|v| v.as_object());
    let subtitles = info.get("subtitles").and_then(|v| v.as_object());

    for preferred in ENGLISH_CODES {
        if let Some((url, lang, generated)) = automatic
            .and_then(|m| select_track_url_for_lang(m, preferred, true))
            .or_else(|| subtitles.and_then(|m| select_track_url_for_lang(m, preferred, false)))
        {
            return Some((url, lang, generated));
        }
    }

    automatic
        .and_then(|m| select_any_track_url(m, true))
        .or_else(|| subtitles.and_then(|m| select_any_track_url(m, false)))
}

fn select_track_url_for_lang(
    tracks_by_lang: &serde_json::Map<String, Value>,
    lang: &str,
    generated: bool,
) -> Option<(String, String, bool)> {
    tracks_by_lang.iter().find_map(|(language_code, tracks)| {
        if !language_code.eq_ignore_ascii_case(lang) {
            return None;
        }
        select_track_url(tracks).map(|url| (url, language_code.clone(), generated))
    })
}

fn select_any_track_url(
    tracks_by_lang: &serde_json::Map<String, Value>,
    generated: bool,
) -> Option<(String, String, bool)> {
    tracks_by_lang.iter().find_map(|(language_code, tracks)| {
        select_track_url(tracks).map(|url| (url, language_code.clone(), generated))
    })
}

fn select_track_url(tracks: &Value) -> Option<String> {
    let entries = tracks.as_array()?;
    for entry in entries {
        let ext = entry
            .get("ext")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if ext.eq_ignore_ascii_case("json3") {
            if let Some(url) = entry.get("url").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
        }
    }
    entries.iter().find_map(|entry| {
        entry
            .get("url")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
    })
}

async fn fetch_transcript_text_from_timedtext_url(
    client: &HttpClient,
    timedtext_url: &str,
) -> Result<String, ConnectorError> {
    let json3: Value = client
        .get(timedtext_url)
        .send()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?
        .error_for_status()
        .map_err(|e| ConnectorError::Other(e.to_string()))?
        .json()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;

    extract_transcript_text_from_json3(&json3).ok_or_else(|| {
        ConnectorError::Other("TimedText transcript response had no text events".to_string())
    })
}

fn extract_transcript_text_from_json3(json3: &Value) -> Option<String> {
    let events = json3.get("events")?.as_array()?;
    let mut chunks: Vec<String> = Vec::new();

    for event in events {
        let Some(segs) = event.get("segs").and_then(|v| v.as_array()) else {
            continue;
        };

        let mut chunk = String::new();
        for seg in segs {
            if let Some(text) = seg.get("utf8").and_then(|v| v.as_str()) {
                chunk.push_str(text);
            }
        }

        let normalized = clean_html_entities(&chunk.replace('\n', " "));
        let normalized = normalize_ws(&normalized);
        if normalized.is_empty() {
            continue;
        }

        if chunks.last() == Some(&normalized) {
            continue;
        }
        chunks.push(normalized);
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(" "))
    }
}

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn listed_video_published_at(video: &ListedVideo) -> Option<DateTime<Utc>> {
    video
        .published_at
        .as_deref()
        .and_then(parse_video_date)
        .as_deref()
        .and_then(parse_rfc3339)
}

fn parse_video_date(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(dt) = parse_rfc3339(trimmed) {
        return Some(dt.to_rfc3339());
    }

    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let midnight = date.and_hms_opt(0, 0, 0)?;
        return Some(Utc.from_utc_datetime(&midnight).to_rfc3339());
    }

    parse_uploaded_timestamp(trimmed).map(|dt| dt.to_rfc3339())
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn string_tokens(s: &str) -> Vec<String> {
    normalize_ws(&s.to_lowercase())
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn token_overlap_score(query: &str, title: &str) -> f64 {
    let q = string_tokens(query);
    let t = string_tokens(title);
    if q.is_empty() || t.is_empty() {
        return 0.0;
    }
    let mut overlap = 0usize;
    for qt in &q {
        if t.iter().any(|tt| tt == qt) {
            overlap += 1;
        }
    }
    overlap as f64 / (q.len() as f64)
}

fn score_channel_candidate(
    query: &str,
    title: &str,
    verified: bool,
    subscribers: u64,
    prefer_verified: bool,
) -> f64 {
    let overlap = token_overlap_score(query, title);
    let mut score = overlap * 10.0;
    if prefer_verified && verified {
        score += 3.0;
    }
    // Subscribers saturate quickly; use log scale.
    let subs = (subscribers as f64).max(1.0);
    score += subs.log10().min(8.0);
    score
}

fn group_transcript_by_chapters_new(
    chapters: &[rusty_ytdl::Chapter],
    transcript: yt_transcript_rs::FetchedTranscript,
) -> Vec<ChapterContent> {
    let parts = transcript.parts();

    if chapters.is_empty() {
        let raw_text = parts
            .iter()
            .map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let cleaned_text = clean_html_entities(&raw_text);
        return vec![ChapterContent {
            heading: "Full Video".to_string(),
            start_time: 0,
            content: cleaned_text,
        }];
    }

    let mut chapter_contents = Vec::new();

    for (i, chapter) in chapters.iter().enumerate() {
        let next_start_time = chapters
            .get(i + 1)
            .map(|next| next.start_time)
            .unwrap_or(i32::MAX);

        let content: Vec<String> = parts
            .iter()
            .filter(|p| {
                let p_time = p.start as i32;
                p_time >= chapter.start_time && p_time < next_start_time
            })
            .map(|p| p.text.clone())
            .collect();

        let raw_text = content.join(" ").replace("\n", " ");
        let cleaned_text = clean_html_entities(&raw_text);

        chapter_contents.push(ChapterContent {
            heading: chapter.title.clone(),
            start_time: chapter.start_time,
            content: cleaned_text,
        });
    }

    chapter_contents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_object_after_marker_handles_nested_json() {
        let html = r#"<script>var ytInitialData = {"a":{"b":"brace } here","c":"quoted \"text\"","d":[{"x":1}]}};</script>"#;

        let extracted = extract_json_object_after_marker(html, "var ytInitialData = ");

        assert_eq!(
            extracted,
            Some(r#"{"a":{"b":"brace } here","c":"quoted \"text\"","d":[{"x":1}]}}"#)
        );
    }

    #[test]
    fn parse_channel_videos_from_page_extracts_video_entries() {
        let html = r#"
        <html>
        <script>
        var ytInitialData = {
          "metadata": {
            "channelMetadataRenderer": {
              "title": "Latent Space"
            }
          },
          "contents": {
            "twoColumnBrowseResultsRenderer": {
              "tabs": [
                {
                  "tabRenderer": {
                    "title": "Home",
                    "selected": true,
                    "content": {
                      "richGridRenderer": {
                        "contents": []
                      }
                    }
                  }
                },
                {
                  "tabRenderer": {
                    "title": "Videos",
                    "selected": false,
                    "content": {
                      "richGridRenderer": {
                        "contents": [
                          {
                            "richItemRenderer": {
                              "content": {
                                "videoRenderer": {
                                  "videoId": "abc123",
                                  "title": {
                                    "runs": [
                                      {
                                        "text": "Newest episode"
                                      }
                                    ]
                                  },
                                  "publishedTimeText": {
                                    "simpleText": "15 hours ago"
                                  }
                                }
                              }
                            }
                          },
                          {
                            "richItemRenderer": {
                              "content": {
                                "videoRenderer": {
                                  "videoId": "def456",
                                  "title": {
                                    "simpleText": "Second episode"
                                  },
                                  "publishedTimeText": {
                                    "simpleText": "2 days ago"
                                  },
                                  "ownerText": {
                                    "runs": [
                                      {
                                        "text": "Guest Channel"
                                      }
                                    ]
                                  }
                                }
                              }
                            }
                          }
                        ]
                      }
                    }
                  }
                }
              ]
            }
          }
        };
        </script>
        </html>
        "#;

        let videos = parse_channel_videos_from_page(html).expect("channel page videos");

        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0].id, "abc123");
        assert_eq!(videos[0].title, "Newest episode");
        assert_eq!(videos[0].channel_title.as_deref(), Some("Latent Space"));
        assert_eq!(videos[1].channel_title.as_deref(), Some("Guest Channel"));
        assert!(listed_video_published_at(&videos[0]).is_some());
    }

    #[test]
    fn parse_channel_videos_from_page_extracts_lockup_view_models() {
        let html = r#"
        <html>
        <script>
        var ytInitialData = {
          "metadata": {
            "channelMetadataRenderer": {
              "title": "Synthet"
            }
          },
          "contents": {
            "twoColumnBrowseResultsRenderer": {
              "tabs": [
                {
                  "tabRenderer": {
                    "title": "Videos",
                    "selected": true,
                    "content": {
                      "richGridRenderer": {
                        "contents": [
                          {
                            "richItemRenderer": {
                              "content": {
                                "lockupViewModel": {
                                  "contentId": "Bkt6-iCbI0o",
                                  "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
                                  "metadata": {
                                    "lockupMetadataViewModel": {
                                      "title": {
                                        "content": "the history of iconic sounds"
                                      },
                                      "metadata": {
                                        "contentMetadataViewModel": {
                                          "metadataRows": [
                                            {
                                              "metadataParts": [
                                                {
                                                  "text": {
                                                    "content": "1.6M views"
                                                  }
                                                },
                                                {
                                                  "text": {
                                                    "content": "13 days ago"
                                                  }
                                                }
                                              ]
                                            }
                                          ]
                                        }
                                      }
                                    }
                                  },
                                  "rendererContext": {
                                    "commandContext": {
                                      "onTap": {
                                        "innertubeCommand": {
                                          "watchEndpoint": {
                                            "videoId": "Bkt6-iCbI0o"
                                          }
                                        }
                                      }
                                    }
                                  }
                                }
                              }
                            }
                          }
                        ]
                      }
                    }
                  }
                }
              ]
            }
          }
        };
        </script>
        </html>
        "#;

        let videos = parse_channel_videos_from_page(html).expect("channel lockup videos");

        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, "Bkt6-iCbI0o");
        assert_eq!(videos[0].title, "the history of iconic sounds");
        assert_eq!(videos[0].channel_title.as_deref(), Some("Synthet"));
        assert!(listed_video_published_at(&videos[0]).is_some());
    }

    #[test]
    fn get_identifier_routes_playlist_and_channel_enumeration_inputs() {
        assert_eq!(
            classify_get_identifier("PLVeoizpP6tgCPDmPf2O8F1di6-5NxFLOY"),
            YouTubeGetTarget::Playlist("PLVeoizpP6tgCPDmPf2O8F1di6-5NxFLOY".to_string())
        );
        assert_eq!(
            classify_get_identifier(
                "https://www.youtube.com/playlist?list=PLVeoizpP6tgCPDmPf2O8F1di6-5NxFLOY"
            ),
            YouTubeGetTarget::Playlist(
                "https://www.youtube.com/playlist?list=PLVeoizpP6tgCPDmPf2O8F1di6-5NxFLOY"
                    .to_string()
            )
        );
        assert_eq!(
            classify_get_identifier("@synthet7"),
            YouTubeGetTarget::Channel("@synthet7".to_string())
        );
        assert_eq!(
            classify_get_identifier("https://www.youtube.com/@synthet7/videos"),
            YouTubeGetTarget::Channel("https://www.youtube.com/@synthet7/videos".to_string())
        );
        assert_eq!(
            classify_get_identifier(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1234567890"
            ),
            YouTubeGetTarget::Video("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn list_source_infers_playlist_when_source_is_omitted() {
        let input = ListVideosInput {
            source: ListSource::Channel,
            channel: None,
            playlist: Some("PLVeoizpP6tgCPDmPf2O8F1di6-5NxFLOY".to_string()),
            limit: None,
            published_after: None,
            published_within_days: None,
            output_format: OutputFormat::Raw,
        };

        assert_eq!(effective_list_source(&input).unwrap(), ListSource::Playlist);
    }

    #[test]
    fn finalize_list_entries_adds_indexes_and_playlist_metadata() {
        let mut videos = vec![
            ListedVideo {
                index: 0,
                id: "abc123xyz00".to_string(),
                title: "First".to_string(),
                url: "https://www.youtube.com/watch?v=abc123xyz00".to_string(),
                published_at: None,
                channel: None,
                channel_title: Some("Creator".to_string()),
                channel_id: Some("UCcreator".to_string()),
                playlist_id: None,
                playlist_title: None,
            },
            ListedVideo {
                index: 0,
                id: "def456xyz00".to_string(),
                title: "Second".to_string(),
                url: "https://www.youtube.com/watch?v=def456xyz00".to_string(),
                published_at: None,
                channel: Some("Creator".to_string()),
                channel_title: Some("Creator".to_string()),
                channel_id: None,
                playlist_id: None,
                playlist_title: None,
            },
        ];

        finalize_list_entries(
            &mut videos,
            ListSource::Playlist,
            None,
            Some("PLtest"),
            Some("Playlist title"),
        );

        assert_eq!(videos[0].index, 1);
        assert_eq!(videos[1].index, 2);
        assert_eq!(videos[0].channel.as_deref(), Some("Creator"));
        assert_eq!(videos[0].playlist_id.as_deref(), Some("PLtest"));
        assert_eq!(videos[1].playlist_title.as_deref(), Some("Playlist title"));
    }

    #[test]
    fn parse_playlist_initial_page_extracts_videos_config_and_continuation() {
        let html = r#"
        <html>
        <script>
        ytcfg.set({
          "INNERTUBE_API_KEY": "test-key",
          "INNERTUBE_CLIENT_VERSION": "2.20260114.08.00",
          "INNERTUBE_CONTEXT_CLIENT_NAME": 1,
          "VISITOR_DATA": "visitor",
          "INNERTUBE_CONTEXT": {
            "client": {
              "clientName": "WEB",
              "clientVersion": "2.20260114.08.00"
            }
          }
        });
        </script>
        <script>
        var ytInitialData = {
          "metadata": {
            "playlistMetadataRenderer": {
              "title": "Playlist title"
            }
          },
          "contents": {
            "twoColumnBrowseResultsRenderer": {
              "tabs": [
                {
                  "tabRenderer": {
                    "selected": true,
                    "content": {
                      "sectionListRenderer": {
                        "contents": [
                          {
                            "itemSectionRenderer": {
                              "contents": [
                                {
                                  "playlistVideoListRenderer": {
                                    "contents": [
                                      {
                                        "playlistVideoRenderer": {
                                          "videoId": "abc123xyz00",
                                          "title": {
                                            "runs": [
                                              {
                                                "text": "Playlist video"
                                              }
                                            ]
                                          },
                                          "shortBylineText": {
                                            "runs": [
                                              {
                                                "text": "Creator"
                                              }
                                            ]
                                          }
                                        }
                                      },
                                      {
                                        "continuationItemRenderer": {
                                          "continuationEndpoint": {
                                            "continuationCommand": {
                                              "token": "next-token"
                                            }
                                          }
                                        }
                                      }
                                    ]
                                  }
                                }
                              ]
                            }
                          }
                        ]
                      }
                    }
                  }
                }
              ]
            }
          }
        };
        </script>
        </html>
        "#;

        let initial =
            parse_youtube_list_initial_page(html, ListSource::Playlist).expect("playlist page");
        let mut seen_ids = HashSet::new();
        let mut videos = Vec::new();

        let hit_cutoff = append_videos_from_value(
            &initial.root,
            ListSource::Playlist,
            None,
            None,
            None,
            &mut seen_ids,
            &mut videos,
        );

        assert!(!hit_cutoff);
        assert_eq!(initial.config.api_key, "test-key");
        assert_eq!(initial.config.client_name, "1");
        assert_eq!(initial.list_title.as_deref(), Some("Playlist title"));
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, "abc123xyz00");
        assert_eq!(videos[0].title, "Playlist video");
        assert_eq!(videos[0].channel_title.as_deref(), Some("Creator"));
        assert_eq!(
            extract_continuation_token(&initial.root).as_deref(),
            Some("next-token")
        );
    }

    #[test]
    fn continuation_response_extracts_playlist_items_and_next_token() {
        let response = json!({
            "onResponseReceivedActions": [
                {
                    "appendContinuationItemsAction": {
                        "continuationItems": [
                            {
                                "playlistVideoRenderer": {
                                    "videoId": "def456xyz00",
                                    "title": {
                                        "simpleText": "Second page video"
                                    },
                                    "shortBylineText": {
                                        "runs": [
                                            {
                                                "text": "Second Creator"
                                            }
                                        ]
                                    }
                                }
                            },
                            {
                                "continuationItemRenderer": {
                                    "continuationEndpoint": {
                                        "continuationCommand": {
                                            "token": "third-page"
                                        }
                                    }
                                }
                            }
                        ]
                    }
                }
            ]
        });
        let mut seen_ids = HashSet::new();
        let mut videos = Vec::new();

        let hit_cutoff = append_videos_from_value(
            &response,
            ListSource::Playlist,
            None,
            None,
            None,
            &mut seen_ids,
            &mut videos,
        );

        assert!(!hit_cutoff);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, "def456xyz00");
        assert_eq!(
            extract_continuation_token(&response).as_deref(),
            Some("third-page")
        );
    }
}
