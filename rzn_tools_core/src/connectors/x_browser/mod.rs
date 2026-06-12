// src/connectors/x_browser/mod.rs

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{get_cookies, match_browser, structured_result_with_text};
use crate::{auth::AuthDetails, Connector, URLParamExtraction, URLPatternSpec};
use agent_twitter_client::api::endpoints::Endpoints;
use agent_twitter_client::api::requests::request_api;
use agent_twitter_client::auth::user_auth::TwitterUserAuth;
use agent_twitter_client::timeline::v1::{QueryProfilesResponse, QueryTweetsResponse};
use agent_twitter_client::timeline::v2::QueryTweetsResponse as V2QueryTweetsResponse;
use agent_twitter_client::timeline::v2::{
    parse_timeline_entry_item_content_raw, ThreadedConversation,
};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde_json::{json, Value};

// Directly use types from agent-twitter-client
use agent_twitter_client::models::{Profile, Tweet};
use agent_twitter_client::scraper::Scraper;
use agent_twitter_client::search::SearchMode;

// use agent_twitter_client::error::Error as AgentError;

use rmcp::model::*;
use rookie::{any_browser, common::enums::CookieToString};

pub struct XConnector {
    scraper: Scraper, // Directly use AgentScraper
}

const X_COOKIE_DOMAINS: [&str; 2] = ["x.com", "twitter.com"];
const X_BROWSER_RELOGIN_HINT: &str = "Log into X in the selected browser, close the browser completely, and rerun `rzn-tools setup x-browser`.";

#[derive(Debug, Clone, Copy)]
enum SortBy {
    Time,
    Engagement,
}

#[derive(Debug, Clone, Copy)]
enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
struct TweetFilters {
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    exclude_replies: bool,
    exclude_retweets: bool,
    min_likes: Option<i64>,
    min_retweets: Option<i64>,
    min_replies: Option<i64>,
    min_views: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct TweetSort {
    by: SortBy,
    order: SortOrder,
}

fn parse_sort_by(raw: Option<&str>) -> Result<SortBy, ConnectorError> {
    match raw.unwrap_or("time") {
        "time" => Ok(SortBy::Time),
        "engagement" => Ok(SortBy::Engagement),
        other => Err(ConnectorError::InvalidParams(format!(
            "Invalid 'sort_by': {other}. Expected one of: time, engagement"
        ))),
    }
}

fn parse_sort_order(raw: Option<&str>) -> Result<SortOrder, ConnectorError> {
    match raw.unwrap_or("desc") {
        "asc" => Ok(SortOrder::Asc),
        "desc" => Ok(SortOrder::Desc),
        other => Err(ConnectorError::InvalidParams(format!(
            "Invalid 'order': {other}. Expected one of: asc, desc"
        ))),
    }
}

fn parse_search_mode(raw: Option<&str>) -> Result<SearchMode, ConnectorError> {
    match raw.unwrap_or("latest") {
        "top" => Ok(SearchMode::Top),
        "latest" => Ok(SearchMode::Latest),
        "photos" => Ok(SearchMode::Photos),
        "videos" => Ok(SearchMode::Videos),
        other => Err(ConnectorError::InvalidParams(format!(
            "Invalid 'mode': {other}. Expected one of: top, latest, photos, videos"
        ))),
    }
}

fn parse_cookie_name_value(raw_cookie: &str) -> Option<(&str, &str)> {
    let (name, value) = raw_cookie.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name, value))
}

fn has_required_x_session_cookies(cookie_header: &str) -> bool {
    let mut has_ct0 = false;
    let mut has_auth_token = false;

    for raw_cookie in cookie_header.split(';') {
        let Some((name, _)) = parse_cookie_name_value(raw_cookie.trim()) else {
            continue;
        };
        match name {
            "ct0" => has_ct0 = true,
            "auth_token" => has_auth_token = true,
            _ => {}
        }
        if has_ct0 && has_auth_token {
            return true;
        }
    }

    false
}

fn merge_cookie_headers(cookie_headers: &[String]) -> String {
    let mut cookie_order = Vec::<String>::new();
    let mut cookie_values = HashMap::<String, String>::new();

    for cookie_header in cookie_headers {
        for raw_cookie in cookie_header.split(';') {
            let raw_cookie = raw_cookie.trim();
            let Some((name, value)) = parse_cookie_name_value(raw_cookie) else {
                continue;
            };

            if !cookie_values.contains_key(name) {
                cookie_order.push(name.to_string());
            }
            cookie_values.insert(name.to_string(), format!("{name}={value}"));
        }
    }

    let merged = cookie_order
        .into_iter()
        .filter_map(|name| cookie_values.remove(&name))
        .collect::<Vec<_>>();

    merged.join("; ")
}

fn push_cookie_candidate(candidates: &mut Vec<String>, cookie_header: String) {
    if cookie_header.is_empty() || !has_required_x_session_cookies(&cookie_header) {
        return;
    }
    if !candidates
        .iter()
        .any(|candidate| candidate == &cookie_header)
    {
        candidates.push(cookie_header);
    }
}

fn build_cookie_candidates(domain_cookies: &HashMap<&'static str, String>) -> Vec<String> {
    let mut candidates = Vec::<String>::new();
    let x_cookie = domain_cookies.get("x.com").cloned();
    let twitter_cookie = domain_cookies.get("twitter.com").cloned();

    if let (Some(x_cookie), Some(twitter_cookie)) = (x_cookie.as_ref(), twitter_cookie.as_ref()) {
        push_cookie_candidate(
            &mut candidates,
            merge_cookie_headers(&[x_cookie.clone(), twitter_cookie.clone()]),
        );
        push_cookie_candidate(
            &mut candidates,
            merge_cookie_headers(&[twitter_cookie.clone(), x_cookie.clone()]),
        );
    }

    if let Some(twitter_cookie) = twitter_cookie {
        push_cookie_candidate(&mut candidates, twitter_cookie);
    }
    if let Some(x_cookie) = x_cookie {
        push_cookie_candidate(&mut candidates, x_cookie);
    }

    candidates
}

fn load_cookie_header_from_db_path(
    cookie_db_path: &Path,
    domain: &str,
) -> Result<String, ConnectorError> {
    let cookie_db_path = cookie_db_path
        .to_str()
        .ok_or_else(|| ConnectorError::Other("Cookie DB path is not valid UTF-8".to_string()))?;
    let cookies = any_browser(cookie_db_path, Some(vec![domain.to_string()]), None)
        .map_err(|error| ConnectorError::Other(error.to_string()))?;
    Ok(cookies.to_string())
}

#[cfg(target_os = "macos")]
fn browser_profile_cookie_db_paths(browser_name: &str) -> Vec<PathBuf> {
    let Some(home_dir) = dirs::home_dir() else {
        return Vec::new();
    };

    let (base_dir, channels): (&str, &[&str]) = match browser_name {
        "chrome" => (
            "Library/Application Support/Google/Chrome",
            &["", "-beta", "-dev", "-nightly"],
        ),
        "brave" => (
            "Library/Application Support/BraveSoftware/Brave-Browser",
            &["", "-beta", "-dev", "-nightly"],
        ),
        "edge" => (
            "Library/Application Support/Microsoft Edge",
            &["", " Beta", " Dev", " Canary"],
        ),
        _ => return Vec::new(),
    };

    let mut cookie_db_paths = Vec::<PathBuf>::new();
    for channel in channels {
        let user_data_dir = home_dir.join(format!("{base_dir}{channel}"));
        if !user_data_dir.exists() {
            continue;
        }

        let default_cookie_db = user_data_dir.join("Default").join("Cookies");
        if default_cookie_db.exists() {
            cookie_db_paths.push(default_cookie_db);
        }

        if let Ok(entries) = std::fs::read_dir(&user_data_dir) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }

                let profile_name = entry.file_name();
                let profile_name = profile_name.to_string_lossy();
                if !profile_name.starts_with("Profile ") {
                    continue;
                }

                let cookie_db = entry.path().join("Cookies");
                if cookie_db.exists() {
                    cookie_db_paths.push(cookie_db);
                }
            }
        }
    }

    cookie_db_paths.sort();
    cookie_db_paths.dedup();
    cookie_db_paths
}

#[cfg(not(target_os = "macos"))]
fn browser_profile_cookie_db_paths(_browser_name: &str) -> Vec<PathBuf> {
    Vec::new()
}

async fn load_x_browser_cookie_candidates(
    browser_name: &str,
) -> Result<Vec<String>, ConnectorError> {
    let browser = match_browser(browser_name.to_string()).await?;
    let mut all_candidates = Vec::<String>::new();
    let mut domain_failures = Vec::<String>::new();

    for cookie_db_path in browser_profile_cookie_db_paths(browser_name) {
        let mut profile_domain_cookies = HashMap::<&'static str, String>::new();
        for domain in X_COOKIE_DOMAINS {
            match load_cookie_header_from_db_path(&cookie_db_path, domain) {
                Ok(cookie_header) if !cookie_header.trim().is_empty() => {
                    profile_domain_cookies.insert(domain, cookie_header);
                }
                Ok(_) => {}
                Err(error) => domain_failures
                    .push(format!("{} ({domain}): {error}", cookie_db_path.display())),
            }
        }

        for candidate in build_cookie_candidates(&profile_domain_cookies) {
            push_cookie_candidate(&mut all_candidates, candidate);
        }
    }

    let mut default_domain_cookies = HashMap::<&'static str, String>::new();
    for domain in X_COOKIE_DOMAINS {
        match get_cookies(browser.clone(), domain.to_string()).await {
            Ok(cookie_header) if !cookie_header.trim().is_empty() => {
                default_domain_cookies.insert(domain, cookie_header);
            }
            Ok(_) => domain_failures.push(format!("{domain}: no cookies found")),
            Err(err) => domain_failures.push(format!("{domain}: {err}")),
        }
    }

    for candidate in build_cookie_candidates(&default_domain_cookies) {
        push_cookie_candidate(&mut all_candidates, candidate);
    }

    if all_candidates.is_empty() && default_domain_cookies.is_empty() {
        let details = if domain_failures.is_empty() {
            String::new()
        } else {
            format!(" Details: {}.", domain_failures.join("; "))
        };
        return Err(ConnectorError::Authentication(format!(
            "Could not load any X session cookies from {browser_name}. {X_BROWSER_RELOGIN_HINT}{details}"
        )));
    }

    if !all_candidates.is_empty() {
        return Ok(all_candidates);
    }

    Err(ConnectorError::Authentication(format!(
        "Could not find the required X session cookies (`ct0` and `auth_token`) in {browser_name} for x.com or twitter.com. {X_BROWSER_RELOGIN_HINT}"
    )))
}

fn map_scraper_error(error: impl ToString) -> ConnectorError {
    let message = error.to_string();

    if message.contains("Missing essential cookies") {
        return ConnectorError::Authentication(format!(
            "Could not find the required X session cookies (`ct0` and `auth_token`). {X_BROWSER_RELOGIN_HINT}"
        ));
    }

    if message.contains("401 Unauthorized") {
        return ConnectorError::Authentication(format!(
            "X rejected the extracted browser session with 401 Unauthorized. {X_BROWSER_RELOGIN_HINT} Upstream error: {message}"
        ));
    }

    ConnectorError::Other(message)
}

fn parse_rfc3339_or_date(
    raw: &str,
    date_is_end_of_day: bool,
) -> Result<DateTime<Utc>, ConnectorError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc));
    }

    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        ConnectorError::InvalidParams(format!(
            "Invalid datetime/date: {raw}. Expected RFC3339 (e.g. 2025-01-02T03:04:05Z) or \
YYYY-MM-DD"
        ))
    })?;

    let time = if date_is_end_of_day {
        NaiveTime::from_hms_opt(23, 59, 59).unwrap()
    } else {
        NaiveTime::MIN
    };
    let naive = date.and_time(time);
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

fn parse_optional_time(
    args: &serde_json::Map<String, Value>,
    key: &str,
    date_is_end_of_day: bool,
) -> Result<Option<DateTime<Utc>>, ConnectorError> {
    match args.get(key).and_then(Value::as_str) {
        Some(raw) if !raw.trim().is_empty() => {
            Ok(Some(parse_rfc3339_or_date(raw.trim(), date_is_end_of_day)?))
        }
        _ => Ok(None),
    }
}

fn parse_optional_date(
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<NaiveDate>, ConnectorError> {
    match args.get(key).and_then(Value::as_str) {
        Some(raw) if !raw.trim().is_empty() => Ok(Some(
            NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| {
                ConnectorError::InvalidParams(format!(
                    "Invalid '{key}': {raw}. Expected YYYY-MM-DD"
                ))
            })?,
        )),
        _ => Ok(None),
    }
}

fn normalize_query_with_date_filters(
    query: &str,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
) -> String {
    let mut out = query.trim().to_string();
    if let Some(d) = since {
        if !out.contains(" since:") && !out.starts_with("since:") {
            out.push_str(&format!(" since:{d}"));
        }
    }
    if let Some(d) = until {
        if !out.contains(" until:") && !out.starts_with("until:") {
            out.push_str(&format!(" until:{d}"));
        }
    }
    out
}

fn tweet_timestamp_secs(tweet: &Tweet) -> Option<i64> {
    tweet
        .timestamp
        .or_else(|| tweet.time_parsed.as_ref().map(|t| t.timestamp()))
        .or_else(|| {
            tweet.created_at.as_deref().and_then(|created_at| {
                chrono::DateTime::parse_from_str(created_at, "%a %b %d %H:%M:%S %z %Y")
                    .ok()
                    .map(|t| t.timestamp())
            })
        })
}

fn engagement_score(tweet: &Tweet) -> i64 {
    let likes = tweet.likes.unwrap_or(0) as i64;
    let retweets = tweet.retweets.unwrap_or(0) as i64;
    let replies = tweet.replies.unwrap_or(0) as i64;
    let quotes = tweet.quote_count.unwrap_or(0) as i64;
    likes + retweets + replies + quotes
}

fn filter_and_sort_tweets(
    mut tweets: Vec<Tweet>,
    filters: TweetFilters,
    sort: TweetSort,
) -> Vec<Tweet> {
    let start_ts = filters.start_time.map(|t| t.timestamp());
    let end_ts = filters.end_time.map(|t| t.timestamp());

    tweets.retain(|t| {
        if filters.exclude_replies && t.is_reply == Some(true) {
            return false;
        }
        if filters.exclude_retweets && t.is_retweet == Some(true) {
            return false;
        }

        if let Some(min) = filters.min_likes {
            if (t.likes.unwrap_or(0) as i64) < min {
                return false;
            }
        }
        if let Some(min) = filters.min_retweets {
            if (t.retweets.unwrap_or(0) as i64) < min {
                return false;
            }
        }
        if let Some(min) = filters.min_replies {
            if (t.replies.unwrap_or(0) as i64) < min {
                return false;
            }
        }
        if let Some(min) = filters.min_views {
            if (t.views.unwrap_or(0) as i64) < min {
                return false;
            }
        }

        if start_ts.is_some() || end_ts.is_some() {
            if let Some(ts) = tweet_timestamp_secs(t) {
                if let Some(start) = start_ts {
                    if ts < start {
                        return false;
                    }
                }
                if let Some(end) = end_ts {
                    if ts > end {
                        return false;
                    }
                }
            }
        }

        true
    });

    match sort.by {
        SortBy::Time => {
            tweets.sort_by_key(|t| tweet_timestamp_secs(t).unwrap_or(0));
            if matches!(sort.order, SortOrder::Desc) {
                tweets.reverse();
            }
        }
        SortBy::Engagement => {
            tweets.sort_by_key(engagement_score);
            if matches!(sort.order, SortOrder::Desc) {
                tweets.reverse();
            }
        }
    }

    tweets
}

impl XConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut connector = XConnector {
            scraper: Scraper::new().await.map_err(map_scraper_error)?,
        };

        // Validate auth details before proceeding
        // connector.validate_auth_details(&auth)?;

        // Set the auth details which will handle either cookie-based or credential-based auth
        connector.set_auth_details(auth).await?;

        Ok(connector)
    }

    async fn verify_authenticated_session(&self) -> Result<(), ConnectorError> {
        let _ = self
            .scraper
            .twitter_client
            .auth
            .as_any()
            .downcast_ref::<TwitterUserAuth>();

        self.scraper
            .get_profile("x")
            .await
            .map(|_| ())
            .map_err(map_scraper_error)
    }

    async fn resolve_user_id(&self, username_or_id: &str) -> Result<String, ConnectorError> {
        let trimmed = username_or_id.trim().trim_start_matches('@');
        if trimmed.chars().all(|c| c.is_ascii_digit()) {
            return Ok(trimmed.to_string());
        }

        let profile = self
            .scraper
            .get_profile(trimmed)
            .await
            .map_err(map_scraper_error)?;
        Ok(profile.id)
    }

    async fn fetch_thread_tweets(&self, tweet_id: &str) -> Result<Vec<Tweet>, ConnectorError> {
        let mut headers = reqwest011::header::HeaderMap::new();
        self.scraper
            .twitter_client
            .auth
            .install_headers(&mut headers)
            .await
            .map_err(map_scraper_error)?;

        let tweet_detail_request = Endpoints::tweet_detail(tweet_id);
        let url = tweet_detail_request.to_request_url();
        let (response, _) = request_api::<Value>(
            &self.scraper.twitter_client.client,
            &url,
            headers,
            reqwest011::Method::GET,
            None,
        )
        .await
        .map_err(map_scraper_error)?;

        let conversation: ThreadedConversation = serde_json::from_value(response)?;

        let mut tweets = Vec::new();
        let instructions = conversation
            .data
            .as_ref()
            .and_then(|data| data.threaded_conversation_with_injections_v2.as_ref())
            .and_then(|conv| conv.instructions.as_deref())
            .unwrap_or(&[]);

        for instruction in instructions {
            let entries = instruction.entries.as_deref().unwrap_or(&[]);
            for entry in entries {
                let Some(content) = &entry.content else {
                    continue;
                };

                if let Some(item_content) = &content.item_content {
                    if let Some(tweet) = parse_timeline_entry_item_content_raw(
                        item_content,
                        entry.entry_id.as_deref().unwrap_or_default(),
                        true,
                    ) {
                        tweets.push(tweet);
                    }
                }

                if let Some(items) = &content.items {
                    for item in items {
                        let Some(item) = &item.item else {
                            continue;
                        };
                        let Some(item_content) = &item.item_content else {
                            continue;
                        };
                        if let Some(tweet) = parse_timeline_entry_item_content_raw(
                            item_content,
                            entry.entry_id.as_deref().unwrap_or_default(),
                            true,
                        ) {
                            tweets.push(tweet);
                        }
                    }
                }
            }
        }

        // Dedupe while preserving first-seen order
        let mut seen = std::collections::HashSet::<String>::new();
        tweets.retain(|t| match t.id.as_ref() {
            Some(id) => seen.insert(id.clone()),
            None => true,
        });

        // Safety cap: TweetDetail can return large conversations.
        let cap = 500usize;
        if tweets.len() > cap {
            tweets.truncate(cap);
        }

        Ok(tweets)
    }
}

#[async_trait]
impl Connector for XConnector {
    fn name(&self) -> &'static str {
        "x-browser"
    }

    fn description(&self) -> &'static str {
        "X (Twitter) connector using browser cookies (scraper-based)."
    }

    fn display_name(&self) -> &'static str {
        "X (Browser Cookies)"
    }

    fn icon(&self) -> &'static str {
        "x"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["social", "news"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?(?:www\.)?(?:x\.com|twitter\.com)/[^/]+/status/(\d+)"
                .to_string(),
            default_tool: "get_tweet".to_string(),
            description: "Fetch tweet by ID".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "tweet_id".to_string(),
                use_full_url: false,
            }],
        }]
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
        // If no auth details provided, skip authentication (allows listing tools without auth)
        if details.is_empty() {
            return Ok(());
        }

        if let Some(cookie_header) = details.get("cookie").or_else(|| details.get("cookies")) {
            self.scraper
                .set_from_cookie_string(cookie_header)
                .await
                .map_err(map_scraper_error)?;
            self.verify_authenticated_session().await?;
            return Ok(());
        }

        // Check for browser-based cookie extraction
        if let Some(browser) = details.get("browser") {
            let cookie_candidates = load_x_browser_cookie_candidates(browser).await?;
            let mut last_error: Option<ConnectorError> = None;

            for cookie_candidate in cookie_candidates {
                match self
                    .scraper
                    .set_from_cookie_string(&cookie_candidate)
                    .await
                    .map_err(map_scraper_error)
                {
                    Ok(()) => {}
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                }

                match self.verify_authenticated_session().await {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = Some(error),
                }
            }

            return Err(last_error.unwrap_or_else(|| {
                ConnectorError::Authentication(format!(
                    "Could not load a usable browser session for {browser}. {X_BROWSER_RELOGIN_HINT}"
                ))
            }));
        }

        // If no cookies, try credentials-based auth
        let username = details.get("username").ok_or_else(|| {
            ConnectorError::InvalidInput("Username is required for credential auth".to_string())
        })?;
        let password = details.get("password").ok_or_else(|| {
            ConnectorError::InvalidInput("Password is required for credential auth".to_string())
        })?;

        // Optional email and 2FA
        let email = details.get("email").map(|s| s.to_string());
        let two_fa = details.get("2fa_secret").map(|s| s.to_string());

        self.scraper
            .login(
                username.to_string(),
                password.to_string(),
                email.map(|s| s.to_string()),
                two_fa.map(|s| s.to_string()),
            )
            .await
            .map_err(map_scraper_error)?;

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        self.verify_authenticated_session().await?;
        Ok(())
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    //Browser
                    name: "browser".to_string(),
                    label: "Browser for Cookie Extraction".to_string(),
                    field_type: FieldType::Select {
                        options: vec![
                            "chrome".to_string(),
                            "firefox".to_string(),
                            "edge".to_string(),
                            "safari".to_string(),
                            "brave".to_string(),
                        ],
                    },
                    required: false, // Only required if using cookie auth, handled by logic
                    description: Some(
                        "Select the browser from which to extract cookies.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "cookie".to_string(),
                    label: "Raw Cookie Header".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional raw Cookie header containing at least `ct0` and `auth_token`. Use this to bypass browser extraction when needed.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    //Username
                    name: "username".to_string(),
                    label: "X Username".to_string(),
                    field_type: FieldType::Text,
                    required: false, // NOT individually required
                    description: Some("Your X username.".to_string()),
                    options: None,
                },
                Field {
                    //Password
                    name: "password".to_string(),
                    label: "X Password".to_string(),
                    field_type: FieldType::Secret,
                    required: false, // NOT individually required
                    description: Some("Your X password.".to_string()),
                    options: None,
                },
                Field {
                    // Bearer token
                    name: "email".to_string(),
                    label: "X Email".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Optional. Used for credential-based auth if X requests it.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    // Bearer token
                    name: "2fa_secret".to_string(),
                    label: "X 2FA Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional. Used for credential-based auth if two-factor is enabled."
                            .to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        // Implement initialization logic (if needed).
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
                "X (Twitter) connector for accessing user profiles, tweets, and social media data"
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        let resources = vec![
            Resource {
                raw: RawResource {
                    uri: "twitter://user/{username}".to_string(),
                    name: "User Profile".to_string(),
                    title: None,
                    description: Some("Represents an X user profile.".to_string()),
                    mime_type: Some("application/vnd.twitter.user+json".to_string()),
                    size: None,
                    icons: None,
                },
                annotations: None,
            },
            Resource {
                raw: RawResource {
                    uri: "twitter://tweet/{tweet_id}".to_string(),
                    name: "Tweet".to_string(),
                    title: None,
                    description: Some("Represents a Tweet.".to_string()),
                    mime_type: Some("application/vnd.twitter.tweet+json".to_string()),
                    size: None,
                    icons: None,
                },
                annotations: None,
            },
        ];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        let uri_str = request.uri.as_str();

        if uri_str.starts_with("twitter://user/") {
            let parts: Vec<&str> = uri_str.split('/').collect();
            if parts.len() < 4 {
                return Err(ConnectorError::InvalidInput(format!(
                    "Invalid resource URI: {}",
                    uri_str
                )));
            }
            let username = parts[3];

            let profile = self
                .scraper
                .get_profile(username)
                .await
                .map_err(map_scraper_error)?;
            let content_text = serde_json::to_string(&profile)?;
            Ok(vec![ResourceContents::text(content_text, uri_str)])
        } else if uri_str.starts_with("twitter://tweet/") {
            let parts: Vec<&str> = uri_str.split('/').collect();

            if parts.len() < 4 {
                return Err(ConnectorError::InvalidInput(format!(
                    "Invalid resource URI: {}",
                    uri_str
                )));
            }
            let tweet_id = parts[3];
            let tweet = self
                .scraper
                .get_tweet(tweet_id)
                .await
                .map_err(map_scraper_error)?;
            let content_text = serde_json::to_string(&tweet)?;
            Ok(vec![ResourceContents::text(content_text, uri_str)])
        } else {
            Err(ConnectorError::ResourceNotFound)
        }
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        let tools = vec![
            Tool {
                name: Cow::Borrowed("get_profile"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a user profile by username (no @). Use when you need bio/follow counts. \
Example: username=\"rustlang\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "username": {
                                "type": "string",
                                "description": "The X username."
                            }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_tweets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search tweets by keyword with optional pagination, time filtering, and \
post-filtering. Examples: query=\"rust lang:en\" limit=20 mode=\"latest\"; \
since=\"2025-01-01\" until=\"2025-01-31\" exclude_retweets=true sort_by=\"engagement\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The search query."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of tweets to return (1..50)."
                            },
                            "cursor": {
                                "type": "string",
                                "description": "Optional cursor for pagination (use next_cursor from the previous call)."
                            },
                            "mode": {
                                "type": "string",
                                "description": "Search mode: top|latest|photos|videos.",
                                "enum": ["top","latest","photos","videos"]
                            },
                            "since": {
                                "type": "string",
                                "description": "Date filter (YYYY-MM-DD). Appended to query as since:YYYY-MM-DD unless query already contains since:."
                            },
                            "until": {
                                "type": "string",
                                "description": "Date filter (YYYY-MM-DD). Appended to query as until:YYYY-MM-DD unless query already contains until:."
                            },
                            "start_time": {
                                "type": "string",
                                "description": "RFC3339 datetime (e.g. 2026-02-24T13:45:00Z) or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "end_time": {
                                "type": "string",
                                "description": "RFC3339 datetime (e.g. 2026-02-24T23:59:59Z) or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "exclude_replies": {
                                "type": "boolean",
                                "description": "Exclude reply tweets."
                            },
                            "exclude_retweets": {
                                "type": "boolean",
                                "description": "Exclude retweets."
                            },
                            "min_likes": {
                                "type": "integer",
                                "description": "Minimum likes (post-filter)."
                            },
                            "min_retweets": {
                                "type": "integer",
                                "description": "Minimum retweets (post-filter)."
                            },
                            "min_replies": {
                                "type": "integer",
                                "description": "Minimum replies (post-filter)."
                            },
                            "min_views": {
                                "type": "integer",
                                "description": "Minimum views (post-filter)."
                            },
                            "sort_by": {
                                "type": "string",
                                "description": "Sort: time|engagement.",
                                "enum": ["time","engagement"]
                            },
                            "order": {
                                "type": "string",
                                "description": "Sort order: asc|desc.",
                                "enum": ["asc","desc"]
                            }
                        },
                        "required": ["query"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_followers"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List followers of a user (paginated). Accepts a username (no @) or numeric user_id.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "username": {
                                "type": "string",
                                "description": "The user's username (no @) or numeric user_id."
                            },
                            "limit":{
                                "type": "integer",
                                "description": "Maximum number of followers to return"
                            },
                            "cursor":{
                                "type": "string",
                                "description": "Optional cursor for pagination"
                            }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_tweet"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a tweet by tweet_id. Use when you already have the ID (often from a URL).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "tweet_id":{
                                "type": "string",
                                "description": "The ID of the tweet"
                            }
                        },
                        "required": ["tweet_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_home_timeline"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get the authenticated user's home timeline (requires explicit user permission).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "count":{
                                "type": "integer",
                                "description": "Number of tweets to retrieve"
                            },
                            "exclude_replies":{
                                "type": "boolean",
                                "description": "Whether to exclude replies"
                            }
                        },
                        "required": ["count"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("fetch_tweets_and_replies"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a user's tweets + replies. Use when you want a user's recent activity feed.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "username":{
                                "type": "string",
                                "description": "The username for which to fetch tweets and replies"
                            },
                            "limit":{
                                "type": "integer",
                                "description": "Maximum number of tweets and replies to return"
                            },
                            "cursor":{
                                "type": "string",
                                "description": "Optional cursor for pagination"
                            }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_user_tweets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a user's tweets (no replies). Accepts username (no @) or numeric user_id. \
Use for a clean 'recent tweets' view.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "username":{
                                "type": "string",
                                "description": "The username (no @) or numeric user_id."
                            },
                            "limit":{
                                "type": "integer",
                                "description": "Maximum number of tweets to return"
                            },
                            "cursor":{
                                "type": "string",
                                "description": "Optional cursor for pagination"
                            },
                            "exclude_retweets":{
                                "type": "boolean",
                                "description": "Exclude retweets."
                            },
                            "start_time": {
                                "type": "string",
                                "description": "RFC3339 datetime or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "end_time": {
                                "type": "string",
                                "description": "RFC3339 datetime or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "order": {
                                "type": "string",
                                "description": "Sort order by time: asc|desc.",
                                "enum": ["asc","desc"]
                            }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_thread"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a tweet thread/conversation for a tweet_id (includes reply context when available).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "tweet_id":{
                                "type": "string",
                                "description": "The focal tweet ID."
                            },
                            "limit":{
                                "type": "integer",
                                "description": "Maximum number of tweets to return from the conversation."
                            },
                            "start_time": {
                                "type": "string",
                                "description": "RFC3339 datetime or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "end_time": {
                                "type": "string",
                                "description": "RFC3339 datetime or YYYY-MM-DD (interpreted as UTC midnight). Post-filtered locally."
                            },
                            "exclude_replies": {
                                "type": "boolean",
                                "description": "Exclude reply tweets (not typical for threads, but useful for cleanup)."
                            },
                            "exclude_retweets": {
                                "type": "boolean",
                                "description": "Exclude retweets."
                            },
                            "sort_by": {
                                "type": "string",
                                "description": "Sort: time|engagement.",
                                "enum": ["time","engagement"]
                            },
                            "order": {
                                "type": "string",
                                "description": "Sort order: asc|desc.",
                                "enum": ["asc","desc"]
                            }
                        },
                        "required": ["tweet_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_profiles"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search user profiles by keyword. Use when discovering accounts. Example: query=\"rust\" limit=10.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "query":{
                                "type": "string",
                                "description": "The search query for profiles"
                            },
                            "limit":{
                                "type": "integer",
                                "description": "Maximum number of profiles to return"
                            },
                            "cursor":{
                                "type": "string",
                                "description": "Optional cursor for pagination"
                            }
                        },
                        "required": ["query", "limit"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_direct_message_conversations"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List DM conversations for the authenticated user (requires explicit user permission).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "user_id":{
                                "type": "string",
                                "description": "The user ID"
                            },
                            "cursor":{
                                "type": "string",
                                "description": "Optional cursor for pagination"
                            }
                        },
                        "required": ["user_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_direct_message"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Send a DM to a conversation_id (requires explicit user permission). Use \
only when the user asked you to message someone.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties":{
                            "conversation_id":{
                                "type": "string",
                                "description": "The ID of the conversation"
                            },
                            "text":{
                                "type": "string",
                                "description": "The text of the message"
                            }
                        },
                        "required": ["conversation_id", "text"]
                    })
                    .as_object()
                    .expect("Schema object")
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
        let name: &str = &request.name;
        let args = request.arguments.unwrap_or_default();
        match name {
            "get_profile" => {
                let username = args.get("username").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' argument".to_string()),
                )?;
                // Strip "@" prefix if present
                let username = username.strip_prefix('@').unwrap_or(username);

                let profile: Profile = self
                    .scraper
                    .get_profile(username)
                    .await
                    .map_err(map_scraper_error)?;

                let text = serde_json::to_string(&profile)?;
                Ok(structured_result_with_text(&profile, Some(text))?)
            }
            "search_tweets" => {
                let query = args.get("query").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' argument".to_string()),
                )?;
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20) as i32;
                let cursor = args.get("cursor").and_then(Value::as_str).map(String::from);
                let mode = parse_search_mode(args.get("mode").and_then(Value::as_str))?;

                let since = parse_optional_date(&args, "since")?;
                let until = parse_optional_date(&args, "until")?;
                let mut start_time = parse_optional_time(&args, "start_time", false)?;
                let mut end_time = parse_optional_time(&args, "end_time", true)?;
                if start_time.is_none() {
                    if let Some(since) = since {
                        let naive = since.and_time(NaiveTime::MIN);
                        start_time = Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
                    }
                }
                if end_time.is_none() {
                    if let Some(until) = until {
                        let naive = until.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap());
                        end_time = Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
                    }
                }

                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    if start > end {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }

                let exclude_replies = args
                    .get("exclude_replies")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let exclude_retweets = args
                    .get("exclude_retweets")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                let min_likes = args.get("min_likes").and_then(Value::as_i64);
                let min_retweets = args.get("min_retweets").and_then(Value::as_i64);
                let min_replies = args.get("min_replies").and_then(Value::as_i64);
                let min_views = args.get("min_views").and_then(Value::as_i64);

                let sort_by = parse_sort_by(args.get("sort_by").and_then(Value::as_str))?;
                let order = parse_sort_order(args.get("order").and_then(Value::as_str))?;

                let query = normalize_query_with_date_filters(query, since, until);

                let tweets: QueryTweetsResponse = self
                    .scraper
                    .search_tweets(&query, limit, mode, cursor)
                    .await
                    .map_err(map_scraper_error)?;
                let QueryTweetsResponse {
                    tweets,
                    next,
                    previous,
                } = tweets;

                let filters = TweetFilters {
                    start_time,
                    end_time,
                    exclude_replies,
                    exclude_retweets,
                    min_likes,
                    min_retweets,
                    min_replies,
                    min_views,
                };
                let sort = TweetSort { by: sort_by, order };

                let filtered = filter_and_sort_tweets(tweets, filters, sort);

                let payload = json!({
                    "query": query,
                    "tweets": filtered,
                    "next_cursor": next,
                    "previous_cursor": previous,
                });
                let text = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(text))?)
            }
            "get_followers" => {
                let username = args.get("username").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' argument".to_string()),
                )?;
                let username = username.strip_prefix('@').unwrap_or(username);
                let user_id = self.resolve_user_id(username).await?;
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20) as i32;
                let cursor = args.get("cursor").and_then(Value::as_str).map(String::from);

                let (followers, next_cursor) = self
                    .scraper
                    .get_followers(&user_id, limit, cursor)
                    .await
                    .map_err(map_scraper_error)?;
                let payload = json!({
                    "followers": followers,
                    "next_cursor": next_cursor,
                });
                let text = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(text))?)
            }
            "get_tweet" => {
                let tweet_id = args.get("tweet_id").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'tweet_id' parameter".to_string()),
                )?;
                let tweet: Tweet = self
                    .scraper
                    .get_tweet(tweet_id)
                    .await
                    .map_err(map_scraper_error)?;
                let text = serde_json::to_string(&tweet)?;
                Ok(structured_result_with_text(&tweet, Some(text))?)
            }
            "get_home_timeline" => {
                let count = args.get("count").and_then(Value::as_i64).unwrap_or(20) as i32;
                let exclude_replies: Vec<String> =
                    match args.get("exclude_replies").and_then(Value::as_bool) {
                        Some(true) => vec!["rts".to_string(), "replies".to_string()],
                        _ => vec![],
                    };
                let tweets: Vec<Value> = self
                    .scraper
                    .get_home_timeline(count, exclude_replies)
                    .await
                    .map_err(map_scraper_error)?;
                let text = serde_json::to_string(&tweets)?;
                Ok(structured_result_with_text(&tweets, Some(text))?)
            }
            "fetch_tweets_and_replies" => {
                let username = args.get("username").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' argument".to_string()),
                )?;
                // Strip "@" prefix if present
                let username = username.strip_prefix('@').unwrap_or(username);
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20) as i32;
                let cursor = args.get("cursor").and_then(Value::as_str);
                let tweets: V2QueryTweetsResponse = self
                    .scraper
                    .fetch_tweets_and_replies(username, limit, cursor)
                    .await
                    .map_err(map_scraper_error)?;
                let text = serde_json::to_string(&tweets)?;
                Ok(structured_result_with_text(&tweets, Some(text))?)
            }
            "get_user_tweets" => {
                let username = args.get("username").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'username' argument".to_string()),
                )?;
                let username = username.strip_prefix('@').unwrap_or(username);
                let user_id = self.resolve_user_id(username).await?;
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(50) as i32;
                let cursor = args.get("cursor").and_then(Value::as_str).map(String::from);

                let start_time = parse_optional_time(&args, "start_time", false)?;
                let end_time = parse_optional_time(&args, "end_time", true)?;
                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    if start > end {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }
                let exclude_retweets = args
                    .get("exclude_retweets")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let order = parse_sort_order(args.get("order").and_then(Value::as_str))?;

                let mut resp = self
                    .scraper
                    .get_user_tweets(&user_id, limit, cursor)
                    .await
                    .map_err(map_scraper_error)?;

                let filtered = filter_and_sort_tweets(
                    resp.tweets,
                    TweetFilters {
                        start_time,
                        end_time,
                        exclude_replies: false,
                        exclude_retweets,
                        min_likes: None,
                        min_retweets: None,
                        min_replies: None,
                        min_views: None,
                    },
                    TweetSort {
                        by: SortBy::Time,
                        order,
                    },
                );

                resp.tweets = filtered;

                let payload = json!({
                    "tweets": resp.tweets,
                    "next_cursor": resp.next,
                    "previous_cursor": resp.previous,
                });
                let text = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(text))?)
            }
            "get_thread" => {
                let tweet_id = args.get("tweet_id").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'tweet_id' parameter".to_string()),
                )?;
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(200) as i32;

                let start_time = parse_optional_time(&args, "start_time", false)?;
                let end_time = parse_optional_time(&args, "end_time", true)?;
                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    if start > end {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }
                let exclude_replies = args
                    .get("exclude_replies")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let exclude_retweets = args
                    .get("exclude_retweets")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let sort_by = parse_sort_by(args.get("sort_by").and_then(Value::as_str))?;
                let order = parse_sort_order(args.get("order").and_then(Value::as_str))?;

                let tweets = self.fetch_thread_tweets(tweet_id).await?;
                let filtered = filter_and_sort_tweets(
                    tweets,
                    TweetFilters {
                        start_time,
                        end_time,
                        exclude_replies,
                        exclude_retweets,
                        min_likes: None,
                        min_retweets: None,
                        min_replies: None,
                        min_views: None,
                    },
                    TweetSort { by: sort_by, order },
                );

                let mut out = filtered;
                if limit > 0 && (out.len() as i32) > limit {
                    out.truncate(limit as usize);
                }

                let payload = json!({
                    "tweet_id": tweet_id,
                    "tweets": out,
                });
                let text = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(text))?)
            }
            "search_profiles" => {
                let query = args.get("query").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' argument".to_string()),
                )?;
                let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20) as i32;
                let cursor = args.get("cursor").and_then(Value::as_str).map(String::from);

                let profiles: QueryProfilesResponse = self
                    .scraper
                    .search_profiles(query, limit, cursor)
                    .await
                    .map_err(map_scraper_error)?;
                let payload = json!({
                    "query": query,
                    "profiles": profiles.profiles,
                    "next_cursor": profiles.next,
                    "previous_cursor": profiles.previous,
                });
                let text = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(text))?)
            }
            "get_direct_message_conversations" => {
                let user_id = args.get("user_id").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'user_id' argument".to_string()),
                )?;
                let cursor = args.get("cursor").and_then(Value::as_str);
                let conversations = self
                    .scraper
                    .get_direct_message_conversations(user_id, cursor)
                    .await
                    .map_err(map_scraper_error)?;
                let text = serde_json::to_string(&conversations)?;
                Ok(structured_result_with_text(&conversations, Some(text))?)
            }
            "send_direct_message" => {
                let conversation_id = args.get("conversation_id").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams(
                        "Missing 'conversation_id' parameter".to_string(),
                    ),
                )?;

                let text = args.get("text").and_then(Value::as_str).ok_or(
                    ConnectorError::InvalidParams("Missing 'text' parameter".to_string()),
                )?;
                self.scraper
                    .send_direct_message(conversation_id, text)
                    .await
                    .map_err(map_scraper_error)?;
                let payload = json!({
                    "status": "sent",
                    "message": "Direct message sent successfully.",
                });
                let serialized = serde_json::to_string(&payload)?;
                Ok(structured_result_with_text(&payload, Some(serialized))?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }
    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        let prompts = vec![Prompt {
            name: "summarize_user_tweets".to_string(),
            title: None,
            description: Some("Summarizes the recent tweets of a given user.".to_string()),
            arguments: Some(vec![PromptArgument {
                name: "username".to_string(),
                title: None,
                description: Some("Twitter username for which to summarize tweets".to_string()),
                required: Some(true),
            }]),
            icons: None,
        }];
        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        match name {
            "summarize_user_tweets" => {
                //Does not make sense to retrieve tweets here, we should probably inject them.
                let prompt = Prompt{
                    name: "summarize_user_tweets".to_string(),
                    title: None,
                    description: Some("Given the provided tweets, generate a concise summary highlighting the main topics, sentiments, and key information conveyed by the user.".to_string()),
                    arguments: Some(vec![
                        PromptArgument{
                            name: "username".to_string(),
                            title: None,
                            description: Some("Twitter username for which to summarize tweets".to_string()),
                            required: Some(true)
                        }
                    ]),
                    icons: None,
                };
                Ok(prompt)
            }
            _ => Err(ConnectorError::InvalidParams(format!(
                "Prompt with name {} not found",
                name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tweet_with(ts: i64, likes: i32, is_reply: bool, is_retweet: bool) -> Tweet {
        Tweet {
            ext_views: None,
            created_at: None,
            bookmark_count: None,
            conversation_id: None,
            hashtags: vec![],
            html: None,
            id: Some(format!("t{ts}")),
            in_reply_to_status: None,
            in_reply_to_status_id: None,
            is_quoted: None,
            is_pin: None,
            is_reply: Some(is_reply),
            is_retweet: Some(is_retweet),
            is_self_thread: None,
            likes: Some(likes),
            name: None,
            mentions: vec![],
            permanent_url: None,
            photos: vec![],
            place: None,
            quoted_status: None,
            quoted_status_id: None,
            replies: Some(0),
            retweets: Some(0),
            retweeted_status: None,
            retweeted_status_id: None,
            text: None,
            thread: vec![],
            time_parsed: None,
            timestamp: Some(ts),
            urls: vec![],
            user_id: None,
            username: None,
            videos: vec![],
            views: Some(0),
            sensitive_content: None,
            poll: None,
            quote_count: Some(0),
            reply_count: Some(0),
            retweet_count: Some(0),
            screen_name: None,
            thread_id: None,
        }
    }

    #[test]
    fn normalize_query_appends_since_until() {
        let q = normalize_query_with_date_filters(
            "rust lang:en",
            Some(NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date")),
            Some(NaiveDate::from_ymd_opt(2025, 1, 2).expect("valid date")),
        );
        assert!(q.contains("since:2025-01-01"));
        assert!(q.contains("until:2025-01-02"));
    }

    #[test]
    fn filter_and_sort_by_time_desc() {
        let tweets = vec![
            tweet_with(10, 1, false, false),
            tweet_with(30, 1, false, false),
            tweet_with(20, 1, false, false),
        ];
        let out = filter_and_sort_tweets(
            tweets,
            TweetFilters {
                start_time: None,
                end_time: None,
                exclude_replies: false,
                exclude_retweets: false,
                min_likes: None,
                min_retweets: None,
                min_replies: None,
                min_views: None,
            },
            TweetSort {
                by: SortBy::Time,
                order: SortOrder::Desc,
            },
        );
        let ids: Vec<String> = out.into_iter().map(|t| t.id.expect("id")).collect();
        assert_eq!(
            ids,
            vec!["t30".to_string(), "t20".to_string(), "t10".to_string()]
        );
    }

    #[test]
    fn filter_excludes_retweets_and_replies() {
        let tweets = vec![
            tweet_with(10, 1, false, false),
            tweet_with(20, 1, true, false),
            tweet_with(30, 1, false, true),
        ];
        let out = filter_and_sort_tweets(
            tweets,
            TweetFilters {
                start_time: None,
                end_time: None,
                exclude_replies: true,
                exclude_retweets: true,
                min_likes: None,
                min_retweets: None,
                min_replies: None,
                min_views: None,
            },
            TweetSort {
                by: SortBy::Time,
                order: SortOrder::Asc,
            },
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id.as_deref(), Some("t10"));
    }

    #[test]
    fn merge_cookie_headers_prefers_later_cookie_values() {
        let merged = merge_cookie_headers(&[
            "ct0=old; auth_token=from-x; guest_id=v1".to_string(),
            "auth_token=from-twitter; twid=u123".to_string(),
        ]);

        assert!(merged.contains("ct0=old"));
        assert!(merged.contains("auth_token=from-twitter"));
        assert!(merged.contains("guest_id=v1"));
        assert!(merged.contains("twid=u123"));
        assert!(!merged.contains("auth_token=from-x"));
    }

    #[test]
    fn has_required_x_session_cookies_requires_ct0_and_auth_token() {
        assert!(has_required_x_session_cookies("ct0=abc; auth_token=def"));
        assert!(!has_required_x_session_cookies("ct0=abc; guest_id=v1"));
        assert!(!has_required_x_session_cookies(""));
    }
}
