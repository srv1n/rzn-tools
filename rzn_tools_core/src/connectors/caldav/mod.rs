use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{Client, Method, RequestBuilder};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use url::Url;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::ingest::{ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat};
use crate::utils::{structured_result, structured_result_with_text};
use crate::Connector;

const CALDAV_NS: &str = "urn:ietf:params:xml:ns:caldav";
const DAV_NS: &str = "DAV:";
const CALENDARS_DISCOVERY_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/">
  <d:prop>
    <d:displayname />
    <d:resourcetype />
    <cs:getctag />
    <d:sync-token />
  </d:prop>
</d:propfind>"#;
const HOME_DISCOVERY_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:current-user-principal />
    <c:calendar-home-set />
  </d:prop>
</d:propfind>"#;

#[derive(Debug, Clone)]
struct CaldavConfig {
    base_url: String,
    username: Option<String>,
    password: Option<String>,
    bearer_token: Option<String>,
    calendar_url: Option<String>,
}

#[derive(Debug, Default)]
struct DavResponse {
    href: Option<String>,
    display_name: Option<String>,
    getetag: Option<String>,
    getctag: Option<String>,
    sync_token: Option<String>,
    calendar_data: Option<String>,
    resource_types: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
struct CalendarSummary {
    name: String,
    url: String,
    ctag: Option<String>,
    sync_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CaldavEvent {
    item_ref: String,
    url: String,
    uid: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    status: Option<String>,
    start: Option<String>,
    end: Option<String>,
    organizer: Option<String>,
    last_modified: Option<String>,
    etag: Option<String>,
    #[serde(skip_serializing)]
    raw_ical: String,
}

#[derive(Debug, Default)]
struct ParsedIcalEvent {
    uid: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    status: Option<String>,
    organizer: Option<String>,
    start: Option<String>,
    end: Option<String>,
    created: Option<String>,
    last_modified: Option<String>,
    dtstamp: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListCalendarsArgs {
    #[serde(default)]
    response_format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListEventsArgs {
    #[serde(default)]
    calendar_url: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    time_min: Option<String>,
    #[serde(default)]
    time_max: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize)]
struct GetEventArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    event_url: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize)]
struct CreateEventArgs {
    #[serde(default)]
    calendar_url: Option<String>,
    #[serde(default)]
    event_path: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    event_url: Option<String>,
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    organizer: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    raw_ical: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize)]
struct UpdateEventArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    event_url: Option<String>,
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    organizer: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    raw_ical: Option<String>,
    #[serde(default)]
    if_match: Option<String>,
    #[serde(default)]
    output_format: OutputFormat,
    #[serde(default = "default_response_format")]
    response_format: String,
}

#[derive(Debug, Deserialize)]
struct DeleteEventArgs {
    #[serde(default)]
    item_ref: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    event_url: Option<String>,
    #[serde(default)]
    if_match: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CaldavCursor {
    calendar_url: String,
    offset: usize,
    time_min: String,
    time_max: String,
}

fn default_response_format() -> String {
    "concise".to_string()
}

fn validate_response_format(value: &str) -> Result<String, ConnectorError> {
    let normalized = value.to_ascii_lowercase();
    if normalized == "concise" || normalized == "detailed" {
        Ok(normalized)
    } else {
        Err(ConnectorError::InvalidParams(
            "response_format must be either 'concise' or 'detailed'".to_string(),
        ))
    }
}

fn default_limit() -> usize {
    25
}

pub struct CaldavConnector {
    client: Client,
    config: Option<CaldavConfig>,
}

impl CaldavConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools-caldav-connector/0.1.0")
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .map_err(ConnectorError::HttpRequest)?;

        let mut connector = Self {
            client,
            config: None,
        };

        if !auth.is_empty() {
            connector.set_auth_details(auth).await?;
        }

        Ok(connector)
    }

    fn ensure_config(&self) -> Result<&CaldavConfig, ConnectorError> {
        self.config.as_ref().ok_or_else(|| {
            ConnectorError::Authentication(
                "CalDAV credentials are not configured. Run `rzn-tools setup caldav`.".to_string(),
            )
        })
    }

    fn apply_auth(&self, request: RequestBuilder, config: &CaldavConfig) -> RequestBuilder {
        if let Some(token) = config
            .bearer_token
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            request.bearer_auth(token)
        } else if let (Some(username), Some(password)) = (
            config.username.as_ref().filter(|value| !value.is_empty()),
            config.password.as_ref().filter(|value| !value.is_empty()),
        ) {
            request.basic_auth(username, Some(password))
        } else {
            request
        }
    }

    async fn send_webdav(
        &self,
        config: &CaldavConfig,
        method: Method,
        url: &str,
        depth: Option<&str>,
        body: Option<&str>,
    ) -> Result<String, ConnectorError> {
        let mut request = self
            .client
            .request(method, url)
            .header(ACCEPT, "application/xml");

        if let Some(depth_header) = depth {
            request = request.header("Depth", depth_header);
        }

        if let Some(xml_body) = body {
            request = request
                .header(CONTENT_TYPE, "application/xml; charset=utf-8")
                .body(xml_body.to_string());
        }

        let response = self.apply_auth(request, config).send().await?;
        let status = response.status();
        let payload = response.text().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "CalDAV request failed with status {}: {}",
                status, payload
            )));
        }

        Ok(payload)
    }

    async fn send_event_get(
        &self,
        config: &CaldavConfig,
        event_url: &str,
    ) -> Result<String, ConnectorError> {
        let request = self
            .client
            .get(event_url)
            .header(ACCEPT, "text/calendar, application/calendar+json, */*");

        let response = self.apply_auth(request, config).send().await?;
        let status = response.status();
        let payload = response.text().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Fetching event failed with status {}: {}",
                status, payload
            )));
        }

        Ok(payload)
    }

    async fn send_event_put(
        &self,
        config: &CaldavConfig,
        event_url: &str,
        raw_ical: &str,
        if_match: Option<&str>,
        if_none_match_any: bool,
    ) -> Result<Option<String>, ConnectorError> {
        let mut request = self
            .client
            .put(event_url)
            .header(CONTENT_TYPE, "text/calendar; charset=utf-8")
            .header(ACCEPT, "application/xml, text/plain, */*")
            .body(raw_ical.to_string());

        if let Some(etag) = if_match.filter(|value| !value.trim().is_empty()) {
            request = request.header("If-Match", etag.trim());
        }
        if if_none_match_any {
            request = request.header("If-None-Match", "*");
        }

        let response = self.apply_auth(request, config).send().await?;
        let status = response.status();
        let etag = response
            .headers()
            .get("ETag")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());
        let payload = response.text().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Writing event failed with status {}: {}",
                status, payload
            )));
        }

        Ok(etag)
    }

    async fn send_event_delete(
        &self,
        config: &CaldavConfig,
        event_url: &str,
        if_match: Option<&str>,
    ) -> Result<(), ConnectorError> {
        let mut request = self
            .client
            .delete(event_url)
            .header(ACCEPT, "application/xml, text/plain, */*");

        if let Some(etag) = if_match.filter(|value| !value.trim().is_empty()) {
            request = request.header("If-Match", etag.trim());
        }

        let response = self.apply_auth(request, config).send().await?;
        let status = response.status();
        let payload = response.text().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Deleting event failed with status {}: {}",
                status, payload
            )));
        }
        Ok(())
    }

    async fn discover_home_url(&self, config: &CaldavConfig) -> Result<String, ConnectorError> {
        let base_url = config.base_url.clone();
        let base_probe = self
            .send_webdav(
                config,
                Method::from_bytes(b"PROPFIND").expect("valid PROPFIND method"),
                &base_url,
                Some("0"),
                Some(HOME_DISCOVERY_BODY),
            )
            .await?;

        if let Some(home_href) = extract_href_property(&base_probe, "calendar-home-set") {
            return Ok(resolve_href(&base_url, &home_href));
        }

        if let Some(principal_href) = extract_href_property(&base_probe, "current-user-principal") {
            let principal_url = resolve_href(&base_url, &principal_href);
            let principal_probe = self
                .send_webdav(
                    config,
                    Method::from_bytes(b"PROPFIND").expect("valid PROPFIND method"),
                    &principal_url,
                    Some("0"),
                    Some(HOME_DISCOVERY_BODY),
                )
                .await?;
            if let Some(home_href) = extract_href_property(&principal_probe, "calendar-home-set") {
                return Ok(resolve_href(&base_url, &home_href));
            }
        }

        Ok(base_url)
    }

    async fn list_calendars_internal(
        &self,
        config: &CaldavConfig,
    ) -> Result<Vec<CalendarSummary>, ConnectorError> {
        let root_url = self.discover_home_url(config).await?;
        let body = self
            .send_webdav(
                config,
                Method::from_bytes(b"PROPFIND").expect("valid PROPFIND method"),
                &root_url,
                Some("1"),
                Some(CALENDARS_DISCOVERY_BODY),
            )
            .await?;

        let parsed = parse_multistatus(&body)?;
        let mut seen = HashSet::new();
        let mut calendars = Vec::new();

        for entry in parsed {
            if !entry
                .resource_types
                .iter()
                .any(|resource_type| resource_type == "calendar")
            {
                continue;
            }

            let href = entry.href.unwrap_or_default();
            if href.is_empty() {
                continue;
            }

            let calendar_url = resolve_href(&root_url, &href);
            if !seen.insert(calendar_url.clone()) {
                continue;
            }

            let name = entry
                .display_name
                .and_then(|value| trim_or_none(&value))
                .unwrap_or_else(|| calendar_name_from_url(&calendar_url));

            calendars.push(CalendarSummary {
                name,
                url: calendar_url,
                ctag: entry.getctag.and_then(|value| trim_or_none(&value)),
                sync_token: entry.sync_token.and_then(|value| trim_or_none(&value)),
            });
        }

        if calendars.is_empty() {
            if let Some(explicit_calendar_url) = config.calendar_url.as_ref() {
                calendars.push(CalendarSummary {
                    name: calendar_name_from_url(explicit_calendar_url),
                    url: explicit_calendar_url.to_string(),
                    ctag: None,
                    sync_token: None,
                });
            }
        }

        calendars.sort_by_key(|calendar| calendar.name.to_lowercase());
        Ok(calendars)
    }

    async fn resolve_calendar_url(
        &self,
        config: &CaldavConfig,
        requested_calendar_url: Option<&str>,
    ) -> Result<String, ConnectorError> {
        if let Some(url) = requested_calendar_url {
            return Ok(url.to_string());
        }

        if let Some(url) = config
            .calendar_url
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            return Ok(url.clone());
        }

        let calendars = self.list_calendars_internal(config).await?;
        calendars
            .first()
            .map(|calendar| calendar.url.clone())
            .ok_or_else(|| {
                ConnectorError::Other(
                    "No calendars discovered. Set `calendar_url` in CalDAV credentials."
                        .to_string(),
                )
            })
    }

    fn resolve_event_url_from_inputs(
        &self,
        item_ref: Option<String>,
        url: Option<String>,
        event_url: Option<String>,
    ) -> Result<String, ConnectorError> {
        if let Some(value) = url.or(event_url) {
            return Ok(value);
        }

        if let Some(reference) = item_ref {
            let (kind, encoded_id) = crate::ingest::parse_item_ref_for_connector(
                &reference, "caldav",
            )
            .ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "Invalid item_ref for CalDAV. Expected format caldav:event:<id>.".to_string(),
                )
            })?;
            if kind != "event" {
                return Err(ConnectorError::InvalidParams(format!(
                    "Unsupported CalDAV item_ref kind '{}', expected 'event'",
                    kind
                )));
            }
            return decode_event_item_id(&encoded_id).ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "Invalid CalDAV event ID in item_ref; expected base64url string.".to_string(),
                )
            });
        }

        Err(ConnectorError::InvalidParams(
            "Provide one of: item_ref, url, or event_url.".to_string(),
        ))
    }

    async fn query_events(
        &self,
        config: &CaldavConfig,
        calendar_url: &str,
        time_min: &str,
        time_max: &str,
    ) -> Result<Vec<CaldavEvent>, ConnectorError> {
        let time_min_caldav = to_caldav_timestamp(time_min)?;
        let time_max_caldav = to_caldav_timestamp(time_max)?;

        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="{dav}" xmlns:c="{caldav}">
  <d:prop>
    <d:getetag />
    <c:calendar-data />
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start}" end="{end}" />
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#,
            dav = DAV_NS,
            caldav = CALDAV_NS,
            start = time_min_caldav,
            end = time_max_caldav
        );

        let payload = self
            .send_webdav(
                config,
                Method::from_bytes(b"REPORT").expect("valid REPORT method"),
                calendar_url,
                Some("1"),
                Some(&body),
            )
            .await?;

        let responses = parse_multistatus(&payload)?;
        let mut events = Vec::new();

        for response in responses {
            let Some(calendar_data) = response.calendar_data else {
                continue;
            };

            let href = response.href.unwrap_or_default();
            if href.is_empty() {
                continue;
            }

            let event_url = resolve_href(calendar_url, &href);
            let parsed_event = parse_ical_event(&calendar_data);
            let item_ref = event_item_ref_from_url(&event_url);

            let mut event = CaldavEvent {
                item_ref,
                url: event_url.clone(),
                uid: parsed_event.uid.clone(),
                summary: parsed_event.summary.clone(),
                description: parsed_event.description.clone(),
                location: parsed_event.location.clone(),
                status: parsed_event.status.clone(),
                start: parsed_event.start.clone(),
                end: parsed_event.end.clone(),
                organizer: parsed_event.organizer.clone(),
                last_modified: parsed_event
                    .last_modified
                    .clone()
                    .or(parsed_event.dtstamp.clone()),
                etag: response.getetag.and_then(|value| trim_or_none(&value)),
                raw_ical: calendar_data,
            };

            if event.uid.is_none() {
                event.uid = Some(event_url.clone());
            }
            if event.summary.is_none() {
                event.summary = Some(event_title_from_url(&event_url));
            }
            if event.url.is_empty() {
                event.url = parsed_event.url.unwrap_or(event_url);
            }

            events.push(event);
        }

        events.sort_by(|left, right| {
            left.start
                .as_deref()
                .unwrap_or("")
                .cmp(right.start.as_deref().unwrap_or(""))
                .then_with(|| {
                    left.summary
                        .as_deref()
                        .unwrap_or("")
                        .cmp(right.summary.as_deref().unwrap_or(""))
                })
        });

        Ok(events)
    }

    fn event_to_item(event: &CaldavEvent) -> ContentItem {
        let block_text = event
            .description
            .as_ref()
            .or(event.summary.as_ref())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "(no description)".to_string());

        let block = ContentBlock {
            block_ref: format!("caldav:event_body:{}", event_item_id_from_url(&event.url)),
            block_kind: "description".to_string(),
            text: block_text,
            author: event.organizer.as_ref().map(|name| crate::ingest::Author {
                name: name.to_string(),
                id: None,
            }),
            created_at: event.start.clone(),
            reply_to: None,
            position: None,
            score: None,
            attachments: Vec::new(),
            metadata: None,
        };

        let metadata = json!({
            "uid": event.uid,
            "etag": event.etag,
            "location": event.location,
            "status": event.status,
            "start": event.start,
            "end": event.end,
            "calendar_url": calendar_root_from_event_url(&event.url),
            "event_url": event.url
        });

        ContentItem {
            item_ref: event.item_ref.clone(),
            kind: "event".to_string(),
            canonical_url: Some(event.url.clone()),
            title: event.summary.clone(),
            created_at: event.start.clone(),
            source_updated_at: event.last_modified.clone(),
            authors: event
                .organizer
                .as_ref()
                .map(|name| {
                    vec![crate::ingest::Author {
                        name: name.to_string(),
                        id: None,
                    }]
                })
                .unwrap_or_default(),
            tags: Vec::new(),
            metadata: Some(metadata),
            blocks: vec![block],
            relationships: Vec::new(),
            truncation: None,
        }
    }

    fn event_to_raw_value(event: &CaldavEvent, detailed: bool) -> Value {
        if detailed {
            json!({
                "item_ref": event.item_ref,
                "url": event.url,
                "uid": event.uid,
                "summary": event.summary,
                "description": event.description,
                "location": event.location,
                "status": event.status,
                "start": event.start,
                "end": event.end,
                "organizer": event.organizer,
                "last_modified": event.last_modified,
                "etag": event.etag,
                "raw_ical": event.raw_ical
            })
        } else {
            json!({
                "item_ref": event.item_ref,
                "url": event.url,
                "uid": event.uid,
                "summary": event.summary,
                "start": event.start,
                "end": event.end,
                "etag": event.etag
            })
        }
    }

    async fn handle_list_calendars(
        &self,
        args: ListCalendarsArgs,
    ) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let detailed = args
            .response_format
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("detailed"));

        let calendars = self.list_calendars_internal(config).await?;
        let data = if detailed {
            json!({ "count": calendars.len(), "calendars": calendars })
        } else {
            let concise: Vec<Value> = calendars
                .into_iter()
                .map(|calendar| json!({ "name": calendar.name, "url": calendar.url }))
                .collect();
            json!({ "count": concise.len(), "calendars": concise })
        };
        structured_result_with_text(&data, None)
    }

    async fn handle_list_events(
        &self,
        args: ListEventsArgs,
    ) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let response_format = validate_response_format(&args.response_format)?;

        let limit = args.limit.clamp(1, 500);
        let calendar_url = self
            .resolve_calendar_url(config, args.calendar_url.as_deref())
            .await?;

        let default_time_min = (Utc::now() - Duration::days(30)).to_rfc3339();
        let default_time_max = (Utc::now() + Duration::days(365)).to_rfc3339();
        let effective_time_min = args.time_min.unwrap_or(default_time_min);
        let effective_time_max = args.time_max.unwrap_or(default_time_max);

        let min_dt = DateTime::parse_from_rfc3339(&effective_time_min).map_err(|error| {
            ConnectorError::InvalidParams(format!("Invalid time_min: {}", error))
        })?;
        let max_dt = DateTime::parse_from_rfc3339(&effective_time_max).map_err(|error| {
            ConnectorError::InvalidParams(format!("Invalid time_max: {}", error))
        })?;

        if min_dt >= max_dt {
            return Err(ConnectorError::InvalidParams(
                "time_min must be earlier than time_max".to_string(),
            ));
        }

        let offset = if let Some(cursor) = args.cursor.as_deref() {
            let decoded: CaldavCursor = crate::ingest::decode_cursor(cursor).ok_or_else(|| {
                ConnectorError::InvalidParams("Invalid cursor for caldav/list".to_string())
            })?;

            if decoded.calendar_url != calendar_url
                || decoded.time_min != effective_time_min
                || decoded.time_max != effective_time_max
            {
                return Err(ConnectorError::InvalidParams(
                    "Cursor does not match requested calendar/time range".to_string(),
                ));
            }
            decoded.offset
        } else {
            0
        };

        let events = self
            .query_events(
                config,
                &calendar_url,
                &effective_time_min,
                &effective_time_max,
            )
            .await?;
        let total = events.len();

        let page_events: Vec<CaldavEvent> = events.into_iter().skip(offset).take(limit).collect();
        let consumed = offset.saturating_add(page_events.len());
        let has_more = consumed < total;

        let next_cursor = if has_more {
            Some(crate::ingest::encode_cursor(&CaldavCursor {
                calendar_url: calendar_url.clone(),
                offset: consumed,
                time_min: effective_time_min.clone(),
                time_max: effective_time_max.clone(),
            })?)
        } else {
            None
        };

        if args.output_format == OutputFormat::NormalizedV1 {
            let items: Vec<ContentItem> = page_events.iter().map(Self::event_to_item).collect();
            let normalized = NormalizedPageV1::new(
                items,
                next_cursor,
                has_more,
                crate::ingest::Partial::complete(Some(json!({
                    "max_items": limit,
                    "offset": offset,
                    "total_hint": total
                }))),
                crate::ingest::Source::new(self.name(), "list"),
            );
            return structured_result(&normalized);
        }

        let detailed = response_format == "detailed";
        let events_json: Vec<Value> = page_events
            .iter()
            .map(|event| Self::event_to_raw_value(event, detailed))
            .collect();

        let raw_payload = json!({
            "calendar_url": calendar_url,
            "time_min": effective_time_min,
            "time_max": effective_time_max,
            "count": events_json.len(),
            "total_hint": total,
            "offset": offset,
            "next_cursor": next_cursor,
            "has_more": has_more,
            "events": events_json
        });
        structured_result_with_text(&raw_payload, None)
    }

    async fn handle_get_event(&self, args: GetEventArgs) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let response_format = validate_response_format(&args.response_format)?;
        let event_url =
            self.resolve_event_url_from_inputs(args.item_ref, args.url, args.event_url)?;

        let raw_ical = self.send_event_get(config, &event_url).await?;
        let event = build_event_from_raw(event_url, raw_ical, None);

        if args.output_format == OutputFormat::NormalizedV1 {
            let normalized = NormalizedItemV1::complete(
                Self::event_to_item(&event),
                crate::ingest::Source::new(self.name(), "get"),
            );
            return structured_result(&normalized);
        }

        let detailed = response_format == "detailed";
        let payload = json!({
            "event": Self::event_to_raw_value(&event, detailed)
        });
        structured_result_with_text(&payload, None)
    }

    async fn handle_create_event(
        &self,
        args: CreateEventArgs,
    ) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let response_format = validate_response_format(&args.response_format)?;

        let parsed_from_raw = args.raw_ical.as_deref().map(parse_ical_event);
        let uid = args
            .uid
            .as_deref()
            .and_then(trim_or_none)
            .or_else(|| parsed_from_raw.as_ref().and_then(|value| value.uid.clone()))
            .unwrap_or_else(generate_uid);

        let event_url = if let Some(url) = args.url.or(args.event_url) {
            url
        } else {
            let calendar_url = self
                .resolve_calendar_url(config, args.calendar_url.as_deref())
                .await?;
            build_event_url(&calendar_url, args.event_path.as_deref(), &uid)?
        };

        let raw_ical = if let Some(raw) = args.raw_ical {
            if raw.trim().is_empty() {
                return Err(ConnectorError::InvalidParams(
                    "raw_ical cannot be empty".to_string(),
                ));
            }
            raw
        } else {
            let summary = args
                .summary
                .as_deref()
                .and_then(trim_or_none)
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(
                        "summary is required when raw_ical is not provided".to_string(),
                    )
                })?;
            let start = args.start.as_deref().ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "start is required when raw_ical is not provided".to_string(),
                )
            })?;
            let end = args.end.as_deref().ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "end is required when raw_ical is not provided".to_string(),
                )
            })?;
            build_ical_event(
                &uid,
                Some(summary.as_str()),
                args.description.as_deref(),
                args.location.as_deref(),
                args.status.as_deref(),
                args.organizer.as_deref(),
                Some(start),
                Some(end),
                None,
                Some(&event_url),
            )?
        };

        let etag = self
            .send_event_put(config, &event_url, &raw_ical, None, true)
            .await?;

        let fetched_raw = self
            .send_event_get(config, &event_url)
            .await
            .unwrap_or(raw_ical);
        let event = build_event_from_raw(event_url.clone(), fetched_raw, etag);

        if args.output_format == OutputFormat::NormalizedV1 {
            let normalized = NormalizedItemV1::complete(
                Self::event_to_item(&event),
                crate::ingest::Source::new(self.name(), "create"),
            );
            return structured_result(&normalized);
        }

        let detailed = response_format == "detailed";
        let payload = json!({
            "status": "created",
            "event": Self::event_to_raw_value(&event, detailed)
        });
        structured_result_with_text(&payload, None)
    }

    async fn handle_update_event(
        &self,
        args: UpdateEventArgs,
    ) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let response_format = validate_response_format(&args.response_format)?;
        let event_url =
            self.resolve_event_url_from_inputs(args.item_ref, args.url, args.event_url)?;

        let raw_ical = if let Some(raw) = args.raw_ical {
            if raw.trim().is_empty() {
                return Err(ConnectorError::InvalidParams(
                    "raw_ical cannot be empty".to_string(),
                ));
            }
            raw
        } else {
            let existing_raw = self.send_event_get(config, &event_url).await?;
            let existing = parse_ical_event(&existing_raw);

            let has_changes = args.summary.is_some()
                || args.description.is_some()
                || args.location.is_some()
                || args.status.is_some()
                || args.organizer.is_some()
                || args.start.is_some()
                || args.end.is_some()
                || args.uid.is_some();
            if !has_changes {
                return Err(ConnectorError::InvalidParams(
                    "No fields provided to update. Provide raw_ical or one of summary/description/location/status/organizer/start/end/uid."
                        .to_string(),
                ));
            }

            let uid = args
                .uid
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.uid)
                .unwrap_or_else(generate_uid);
            let summary = args
                .summary
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.summary.as_deref().and_then(trim_or_none));
            let description = args
                .description
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.description.as_deref().and_then(trim_or_none));
            let location = args
                .location
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.location.as_deref().and_then(trim_or_none));
            let status = args
                .status
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.status.as_deref().and_then(trim_or_none));
            let organizer = args
                .organizer
                .as_deref()
                .and_then(trim_or_none)
                .or(existing.organizer.as_deref().and_then(trim_or_none));
            let start = args
                .start
                .as_deref()
                .or(existing.start.as_deref())
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(
                        "Unable to determine event start time for update.".to_string(),
                    )
                })?;
            let end = args.end.as_deref().or(existing.end.as_deref());

            build_ical_event(
                &uid,
                summary.as_deref(),
                description.as_deref(),
                location.as_deref(),
                status.as_deref(),
                organizer.as_deref(),
                Some(start),
                end,
                existing.created.as_deref(),
                Some(&event_url),
            )?
        };

        let etag = self
            .send_event_put(
                config,
                &event_url,
                &raw_ical,
                args.if_match.as_deref(),
                false,
            )
            .await?;

        let fetched_raw = self
            .send_event_get(config, &event_url)
            .await
            .unwrap_or(raw_ical);
        let event = build_event_from_raw(event_url, fetched_raw, etag);

        if args.output_format == OutputFormat::NormalizedV1 {
            let normalized = NormalizedItemV1::complete(
                Self::event_to_item(&event),
                crate::ingest::Source::new(self.name(), "update"),
            );
            return structured_result(&normalized);
        }

        let detailed = response_format == "detailed";
        let payload = json!({
            "status": "updated",
            "event": Self::event_to_raw_value(&event, detailed)
        });
        structured_result_with_text(&payload, None)
    }

    async fn handle_delete_event(
        &self,
        args: DeleteEventArgs,
    ) -> Result<CallToolResult, ConnectorError> {
        let config = self.ensure_config()?;
        let event_url =
            self.resolve_event_url_from_inputs(args.item_ref, args.url, args.event_url)?;
        self.send_event_delete(config, &event_url, args.if_match.as_deref())
            .await?;
        let item_ref = event_item_ref_from_url(&event_url);

        let payload = json!({
            "status": "deleted",
            "event_url": event_url,
            "item_ref": item_ref
        });
        structured_result_with_text(&payload, None)
    }
}

#[async_trait]
impl Connector for CaldavConnector {
    fn name(&self) -> &'static str {
        "caldav"
    }

    fn description(&self) -> &'static str {
        "CalDAV calendar connector for listing calendars and reading/updating events from servers like iCloud, Fastmail, Nextcloud, and Radicale."
    }

    fn display_name(&self) -> &'static str {
        "CalDAV"
    }

    fn icon(&self) -> &'static str {
        "calendar"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "calendar"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(Default::default()),
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
                title: Some("CalDAV".to_string()),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Configure base_url + authentication (username/password or bearer token). \
Use app-specific passwords where your provider requires them."
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
                name: Cow::Borrowed("list_calendars"),
                title: Some("List Calendars".into()),
                description: Some(Cow::Borrowed(
                    "Discover calendars on the configured CalDAV account.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "response_format": {
                                "type": "string",
                                "enum": ["concise", "detailed"],
                                "default": "concise"
                            }
                        }
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
                name: Cow::Borrowed("list"),
                title: Some("List Events".into()),
                description: Some(Cow::Borrowed(
                    "List events from a calendar. Supports pagination with opaque cursors and normalized output.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "calendar_url": { "type": "string", "description": "Optional calendar collection URL. Uses configured/default calendar when omitted." },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 25 },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous response." },
                            "time_min": { "type": "string", "description": "RFC3339 start time. Defaults to now-30 days." },
                            "time_max": { "type": "string", "description": "RFC3339 end time. Defaults to now+365 days." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        }
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
                name: Cow::Borrowed("get"),
                title: Some("Get Event".into()),
                description: Some(Cow::Borrowed(
                    "Fetch a single calendar event by item_ref or event URL.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Preferred identifier, e.g. caldav:event:<base64url>" },
                            "url": { "type": "string", "description": "Event URL (.ics or CalDAV resource URL)." },
                            "event_url": { "type": "string", "description": "Alias for url." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        }
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
                name: Cow::Borrowed("create"),
                title: Some("Create Event".into()),
                description: Some(Cow::Borrowed(
                    "Create a new calendar event via PUT. You can provide structured fields or raw_ical.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "calendar_url": { "type": "string", "description": "Optional calendar collection URL override." },
                            "event_path": { "type": "string", "description": "Optional resource path/name (e.g., standup-2026-02-20.ics)." },
                            "url": { "type": "string", "description": "Optional full event URL (overrides calendar_url/event_path)." },
                            "event_url": { "type": "string", "description": "Alias for url." },
                            "uid": { "type": "string", "description": "Optional event UID. Generated when omitted." },
                            "summary": { "type": "string" },
                            "description": { "type": "string" },
                            "location": { "type": "string" },
                            "status": { "type": "string", "description": "e.g., CONFIRMED, TENTATIVE, CANCELLED" },
                            "organizer": { "type": "string", "description": "email or mailto: value" },
                            "start": { "type": "string", "description": "RFC3339 or iCal datetime/date format" },
                            "end": { "type": "string", "description": "RFC3339 or iCal datetime/date format" },
                            "raw_ical": { "type": "string", "description": "Raw VCALENDAR payload. When provided, structured fields are optional." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        }
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
                name: Cow::Borrowed("update"),
                title: Some("Update Event".into()),
                description: Some(Cow::Borrowed(
                    "Update an existing event by item_ref/url. Supports conditional writes with if_match.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Preferred reference, e.g. caldav:event:<base64url>" },
                            "url": { "type": "string", "description": "Event URL" },
                            "event_url": { "type": "string", "description": "Alias for url" },
                            "if_match": { "type": "string", "description": "Optional ETag for optimistic concurrency control." },
                            "uid": { "type": "string" },
                            "summary": { "type": "string" },
                            "description": { "type": "string" },
                            "location": { "type": "string" },
                            "status": { "type": "string" },
                            "organizer": { "type": "string" },
                            "start": { "type": "string", "description": "RFC3339 or iCal datetime/date format" },
                            "end": { "type": "string", "description": "RFC3339 or iCal datetime/date format" },
                            "raw_ical": { "type": "string", "description": "Raw VCALENDAR payload to fully replace the resource." },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        }
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
                name: Cow::Borrowed("delete"),
                title: Some("Delete Event".into()),
                description: Some(Cow::Borrowed(
                    "Delete an existing event by item_ref/url. Supports optional if_match ETag checks.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "item_ref": { "type": "string", "description": "Preferred reference, e.g. caldav:event:<base64url>" },
                            "url": { "type": "string", "description": "Event URL" },
                            "event_url": { "type": "string", "description": "Alias for url" },
                            "if_match": { "type": "string", "description": "Optional ETag for optimistic concurrency control." }
                        }
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
        let args_map = request.arguments.unwrap_or_default();
        match request.name.as_ref() {
            "list_calendars" => {
                let args: ListCalendarsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_list_calendars(args).await
            }
            "list" => {
                let args: ListEventsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_list_events(args).await
            }
            "get" => {
                let args: GetEventArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_get_event(args).await
            }
            "create" => {
                let args: CreateEventArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_create_event(args).await
            }
            "update" => {
                let args: UpdateEventArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_update_event(args).await
            }
            "delete" => {
                let args: DeleteEventArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|error| ConnectorError::InvalidParams(error.to_string()))?;
                self.handle_delete_event(args).await
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

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut details = AuthDetails::new();
        if let Some(config) = self.config.as_ref() {
            details.insert("base_url".to_string(), config.base_url.clone());
            if let Some(username) = config.username.as_ref() {
                details.insert("username".to_string(), username.clone());
            }
            if let Some(password) = config.password.as_ref() {
                details.insert("password".to_string(), password.clone());
            }
            if let Some(token) = config.bearer_token.as_ref() {
                details.insert("bearer_token".to_string(), token.clone());
            }
            if let Some(calendar_url) = config.calendar_url.as_ref() {
                details.insert("calendar_url".to_string(), calendar_url.clone());
            }
        }
        Ok(details)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let base_url = value_from_details_or_env(&details, "base_url", "CALDAV_BASE_URL")
            .ok_or_else(|| {
                ConnectorError::InvalidInput(
                    "CalDAV base_url is required (or set CALDAV_BASE_URL).".to_string(),
                )
            })?;
        let username = value_from_details_or_env(&details, "username", "CALDAV_USERNAME");
        let password = value_from_details_or_env(&details, "password", "CALDAV_PASSWORD");
        let bearer_token =
            value_from_details_or_env(&details, "bearer_token", "CALDAV_BEARER_TOKEN");
        let calendar_url =
            value_from_details_or_env(&details, "calendar_url", "CALDAV_CALENDAR_URL");

        let has_basic_auth = username.as_ref().is_some_and(|value| !value.is_empty())
            && password.as_ref().is_some_and(|value| !value.is_empty());
        let has_bearer_auth = bearer_token.as_ref().is_some_and(|value| !value.is_empty());

        if !has_basic_auth && !has_bearer_auth {
            return Err(ConnectorError::InvalidInput(
                "Provide either username+password or bearer_token for CalDAV.".to_string(),
            ));
        }

        self.config = Some(CaldavConfig {
            base_url,
            username,
            password,
            bearer_token,
            calendar_url,
        });

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let config = self.ensure_config()?;
        let _ = self.list_calendars_internal(config).await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "base_url".to_string(),
                    label: "CalDAV Base URL".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    description: Some(
                        "CalDAV server URL or principal URL (e.g., iCloud/Fastmail/Nextcloud CalDAV endpoint)."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Username/email for Basic auth (often required with app-specific password)."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Password or app-specific password for Basic auth.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "bearer_token".to_string(),
                    label: "Bearer Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional OAuth bearer token (alternative to username/password)."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "calendar_url".to_string(),
                    label: "Default Calendar URL".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Optional default calendar collection URL. If omitted, connector auto-discovers calendars."
                            .to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }
}

fn value_from_details_or_env(details: &AuthDetails, key: &str, env_var: &str) -> Option<String> {
    details
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var(env_var)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn xml_local_name(name: &[u8]) -> String {
    std::str::from_utf8(name)
        .ok()
        .and_then(|value| value.rsplit(':').next())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn parse_multistatus(xml: &str) -> Result<Vec<DavResponse>, ConnectorError> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(false);

    let mut buffer = Vec::new();
    let mut responses = Vec::new();
    let mut current_response: Option<DavResponse> = None;
    let mut current_field: Option<String> = None;
    let mut in_resourcetype = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let tag = xml_local_name(event.name().as_ref());
                match tag.as_str() {
                    "response" => current_response = Some(DavResponse::default()),
                    "href" | "displayname" | "getetag" | "getctag" | "sync-token"
                    | "calendar-data" => current_field = Some(tag),
                    "resourcetype" => in_resourcetype = true,
                    _ => {
                        if in_resourcetype {
                            if let Some(response) = current_response.as_mut() {
                                response.resource_types.push(tag);
                            }
                        }
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let tag = xml_local_name(event.name().as_ref());
                if in_resourcetype {
                    if let Some(response) = current_response.as_mut() {
                        response.resource_types.push(tag);
                    }
                }
            }
            Ok(Event::Text(event)) => {
                if let (Some(response), Some(field)) =
                    (current_response.as_mut(), current_field.as_ref())
                {
                    let text = event.unescape().unwrap_or(Cow::Borrowed("")).to_string();
                    set_response_field(response, field, text);
                }
            }
            Ok(Event::CData(event)) => {
                if let (Some(response), Some(field)) =
                    (current_response.as_mut(), current_field.as_ref())
                {
                    let text = String::from_utf8_lossy(event.as_ref()).to_string();
                    set_response_field(response, field, text);
                }
            }
            Ok(Event::End(event)) => {
                let tag = xml_local_name(event.name().as_ref());
                if tag == "response" {
                    if let Some(response) = current_response.take() {
                        responses.push(response);
                    }
                } else if tag == "resourcetype" {
                    in_resourcetype = false;
                }

                if current_field.as_deref() == Some(tag.as_str()) {
                    current_field = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ConnectorError::Other(format!(
                    "Failed to parse WebDAV XML response: {}",
                    error
                )))
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(responses)
}

fn set_response_field(response: &mut DavResponse, field: &str, text: String) {
    match field {
        "href" => append_text_field(&mut response.href, text, false),
        "displayname" => append_text_field(&mut response.display_name, text, false),
        "getetag" => append_text_field(&mut response.getetag, text, false),
        "getctag" => append_text_field(&mut response.getctag, text, false),
        "sync-token" => append_text_field(&mut response.sync_token, text, false),
        "calendar-data" => append_text_field(&mut response.calendar_data, text, true),
        _ => {}
    }
}

fn append_text_field(target: &mut Option<String>, text: String, preserve_whitespace: bool) {
    if preserve_whitespace {
        if let Some(existing) = target.as_mut() {
            existing.push_str(&text);
        } else {
            *target = Some(text);
        }
        return;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    if let Some(existing) = target.as_mut() {
        if !existing.is_empty() {
            existing.push(' ');
        }
        existing.push_str(trimmed);
    } else {
        *target = Some(trimmed.to_string());
    }
}

fn extract_href_property(xml: &str, property_name: &str) -> Option<String> {
    let property = property_name.to_ascii_lowercase();
    let mut reader = Reader::from_str(xml);
    reader.trim_text(false);

    let mut buffer = Vec::new();
    let mut inside_property = false;
    let mut capture_href = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let tag = xml_local_name(event.name().as_ref());
                if tag == property {
                    inside_property = true;
                } else if inside_property && tag == "href" {
                    capture_href = true;
                }
            }
            Ok(Event::Text(event)) if capture_href => {
                let text = event.unescape().unwrap_or(Cow::Borrowed("")).to_string();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            Ok(Event::End(event)) => {
                let tag = xml_local_name(event.name().as_ref());
                if capture_href && tag == "href" {
                    capture_href = false;
                }
                if inside_property && tag == property {
                    inside_property = false;
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buffer.clear();
    }

    None
}

fn resolve_href(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    Url::parse(base_url)
        .ok()
        .and_then(|base| base.join(href).ok())
        .map(|url| url.to_string())
        .unwrap_or_else(|| href.to_string())
}

fn trim_or_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn calendar_name_from_url(calendar_url: &str) -> String {
    Url::parse(calendar_url)
        .ok()
        .and_then(|url| {
            url.path_segments().and_then(|mut segments| {
                segments
                    .rfind(|segment| !segment.is_empty())
                    .map(ToString::to_string)
            })
        })
        .unwrap_or_else(|| "calendar".to_string())
}

fn event_title_from_url(event_url: &str) -> String {
    Url::parse(event_url)
        .ok()
        .and_then(|url| {
            url.path_segments().and_then(|mut segments| {
                segments
                    .rfind(|segment| !segment.is_empty())
                    .map(|segment| segment.trim_end_matches(".ics").to_string())
            })
        })
        .unwrap_or_else(|| "Untitled Event".to_string())
}

fn calendar_root_from_event_url(event_url: &str) -> Option<String> {
    let mut parsed = Url::parse(event_url).ok()?;
    let mut segments: Vec<String> = parsed.path_segments()?.map(ToString::to_string).collect();
    segments.pop()?;
    let new_path = format!("/{}", segments.join("/"));
    parsed.set_path(&new_path);
    Some(parsed.to_string())
}

fn event_item_id_from_url(event_url: &str) -> String {
    URL_SAFE_NO_PAD.encode(event_url.as_bytes())
}

fn event_item_ref_from_url(event_url: &str) -> String {
    format!("caldav:event:{}", event_item_id_from_url(event_url))
}

fn decode_event_item_id(encoded: &str) -> Option<String> {
    let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(decoded).ok()
}

fn build_event_from_raw(event_url: String, raw_ical: String, etag: Option<String>) -> CaldavEvent {
    let parsed = parse_ical_event(&raw_ical);
    CaldavEvent {
        item_ref: event_item_ref_from_url(&event_url),
        url: event_url.clone(),
        uid: parsed.uid.or_else(|| Some(event_url.clone())),
        summary: parsed
            .summary
            .or_else(|| Some(event_title_from_url(&event_url))),
        description: parsed.description,
        location: parsed.location,
        status: parsed.status,
        start: parsed.start,
        end: parsed.end,
        organizer: parsed.organizer,
        last_modified: parsed.last_modified.or(parsed.dtstamp),
        etag,
        raw_ical,
    }
}

fn generate_uid() -> String {
    format!(
        "rzn-tools-{}@local",
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000)
    )
}

fn build_event_url(
    calendar_url: &str,
    event_path: Option<&str>,
    uid: &str,
) -> Result<String, ConnectorError> {
    let mut base = Url::parse(calendar_url).map_err(|error| {
        ConnectorError::InvalidParams(format!(
            "Invalid calendar_url '{}': {}",
            calendar_url, error
        ))
    })?;
    if !base.path().ends_with('/') {
        let mut path = base.path().to_string();
        path.push('/');
        base.set_path(&path);
    }

    let mut path = event_path
        .and_then(trim_or_none)
        .unwrap_or_else(|| sanitize_path_segment(uid));
    if path.contains("..") {
        return Err(ConnectorError::InvalidParams(
            "event_path must not contain '..' segments".to_string(),
        ));
    }
    if !path.ends_with(".ics") {
        path.push_str(".ics");
    }

    base.join(path.trim_start_matches('/'))
        .map(|url| url.to_string())
        .map_err(|error| ConnectorError::InvalidParams(format!("Invalid event_path: {}", error)))
}

fn sanitize_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '@') {
            output.push(character);
        } else {
            output.push('_');
        }
    }
    let cleaned = output.trim_matches('_');
    if cleaned.is_empty() {
        "event".to_string()
    } else {
        cleaned.to_string()
    }
}

fn build_ical_event(
    uid: &str,
    summary: Option<&str>,
    description: Option<&str>,
    location: Option<&str>,
    status: Option<&str>,
    organizer: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    created: Option<&str>,
    event_url: Option<&str>,
) -> Result<String, ConnectorError> {
    let dtstamp = now_ical_datetime_utc();
    let start_value = start.ok_or_else(|| {
        ConnectorError::InvalidParams("start is required for calendar events".to_string())
    })?;

    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//rzn-tools//CalDAV Connector//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", escape_ical_text(uid)),
        format!("DTSTAMP:{}", dtstamp),
        format!("LAST-MODIFIED:{}", dtstamp),
    ];

    if let Some(created_value) = created.and_then(trim_or_none) {
        lines.push(format!(
            "CREATED:{}",
            to_ical_datetime_input(&created_value)?
        ));
    }
    if let Some(summary_value) = summary.and_then(trim_or_none) {
        lines.push(format!("SUMMARY:{}", escape_ical_text(&summary_value)));
    }
    if let Some(description_value) = description.and_then(trim_or_none) {
        lines.push(format!(
            "DESCRIPTION:{}",
            escape_ical_text(&description_value)
        ));
    }
    if let Some(location_value) = location.and_then(trim_or_none) {
        lines.push(format!("LOCATION:{}", escape_ical_text(&location_value)));
    }
    if let Some(status_value) = status.and_then(trim_or_none) {
        lines.push(format!("STATUS:{}", status_value.to_ascii_uppercase()));
    }
    if let Some(organizer_value) = organizer.and_then(trim_or_none) {
        if organizer_value.to_ascii_lowercase().starts_with("mailto:") {
            lines.push(format!("ORGANIZER:{}", organizer_value));
        } else {
            lines.push(format!("ORGANIZER:mailto:{}", organizer_value));
        }
    }

    lines.push(format!("DTSTART:{}", to_ical_datetime_input(start_value)?));
    if let Some(end_value) = end.and_then(trim_or_none) {
        lines.push(format!("DTEND:{}", to_ical_datetime_input(&end_value)?));
    }
    if let Some(url_value) = event_url.and_then(trim_or_none) {
        lines.push(format!("URL:{}", escape_ical_text(&url_value)));
    }

    lines.push("END:VEVENT".to_string());
    lines.push("END:VCALENDAR".to_string());

    Ok(lines.join("\r\n") + "\r\n")
}

fn now_ical_datetime_utc() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn to_ical_datetime_input(value: &str) -> Result<String, ConnectorError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConnectorError::InvalidParams(
            "datetime value cannot be empty".to_string(),
        ));
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(parsed
            .with_timezone(&Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string());
    }
    if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%SZ") {
        return Ok(Utc
            .from_utc_datetime(&parsed)
            .format("%Y%m%dT%H%M%SZ")
            .to_string());
    }
    if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S") {
        return Ok(Utc
            .from_utc_datetime(&parsed)
            .format("%Y%m%dT%H%M%SZ")
            .to_string());
    }
    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y%m%d") {
        return Ok(parsed.format("%Y%m%d").to_string());
    }

    Err(ConnectorError::InvalidParams(format!(
        "Unsupported datetime format '{}'. Use RFC3339, YYYYMMDDTHHMMSSZ, or YYYYMMDD.",
        trimmed
    )))
}

fn parse_ical_event(raw_ical: &str) -> ParsedIcalEvent {
    let lines = unfold_ical_lines(raw_ical);
    let mut parsed = ParsedIcalEvent::default();
    let mut in_event = false;

    for line in lines {
        let normalized = line.trim();
        if normalized.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_event = true;
            continue;
        }
        if normalized.eq_ignore_ascii_case("END:VEVENT") {
            break;
        }
        if !in_event {
            continue;
        }

        let Some((raw_key, raw_value)) = normalized.split_once(':') else {
            continue;
        };
        let property_name = raw_key
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        let value = unescape_ical_text(raw_value);
        let date_value = parse_ical_datetime(&value);

        match property_name.as_str() {
            "UID" => parsed.uid = trim_or_none(&value),
            "SUMMARY" => parsed.summary = trim_or_none(&value),
            "DESCRIPTION" => parsed.description = trim_or_none(&value),
            "LOCATION" => parsed.location = trim_or_none(&value),
            "STATUS" => parsed.status = trim_or_none(&value),
            "ORGANIZER" => {
                let cleaned = value.trim().trim_start_matches("mailto:");
                parsed.organizer = trim_or_none(cleaned);
            }
            "DTSTART" => parsed.start = date_value,
            "DTEND" => parsed.end = date_value,
            "CREATED" => parsed.created = date_value,
            "LAST-MODIFIED" => parsed.last_modified = date_value,
            "DTSTAMP" => parsed.dtstamp = date_value,
            "URL" => parsed.url = trim_or_none(&value),
            _ => {}
        }
    }

    parsed
}

fn unfold_ical_lines(raw_ical: &str) -> Vec<String> {
    let normalized = raw_ical.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for line in normalized.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(previous) = lines.last_mut() {
                previous.push_str(line.trim_start());
            }
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

fn unescape_ical_text(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn escape_ical_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\r', "")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

fn parse_ical_datetime(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%SZ") {
        return Some(Utc.from_utc_datetime(&parsed).to_rfc3339());
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S") {
        return Some(Utc.from_utc_datetime(&parsed).to_rfc3339());
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y%m%d") {
        if let Some(datetime) = parsed.and_hms_opt(0, 0, 0) {
            return Some(Utc.from_utc_datetime(&datetime).to_rfc3339());
        }
    }

    Some(trimmed.to_string())
}

fn to_caldav_timestamp(rfc3339: &str) -> Result<String, ConnectorError> {
    let parsed = DateTime::parse_from_rfc3339(rfc3339).map_err(|error| {
        ConnectorError::InvalidParams(format!("Invalid RFC3339 timestamp: {}", error))
    })?;
    Ok(parsed
        .with_timezone(&Utc)
        .format("%Y%m%dT%H%M%SZ")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_webdav_multistatus() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/dav/calendars/home/default/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Default Calendar</d:displayname>
        <d:resourcetype>
          <d:collection/>
          <c:calendar/>
        </d:resourcetype>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let parsed = parse_multistatus(xml).expect("parsed");
        assert_eq!(parsed.len(), 1);
        let first = &parsed[0];
        assert_eq!(first.display_name.as_deref(), Some("Default Calendar"));
        assert_eq!(first.href.as_deref(), Some("/dav/calendars/home/default/"));
        assert!(first.resource_types.iter().any(|value| value == "calendar"));
    }

    #[test]
    fn parses_ical_event_with_folded_description() {
        let raw_ical = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:test-123\r\nSUMMARY:Team Standup\r\nDESCRIPTION:Line one\\n line two\r\nDTSTART:20260221T150000Z\r\nDTEND:20260221T153000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ical_event(raw_ical);
        assert_eq!(event.uid.as_deref(), Some("test-123"));
        assert_eq!(event.summary.as_deref(), Some("Team Standup"));
        assert_eq!(event.start.as_deref(), Some("2026-02-21T15:00:00+00:00"));
        assert!(event
            .description
            .as_deref()
            .is_some_and(|value| value.contains("Line one")));
    }

    #[test]
    fn item_ref_roundtrip() {
        let url = "https://example.com/caldav/calendars/default/event-1.ics";
        let reference = event_item_ref_from_url(url);
        let (_, encoded) =
            crate::ingest::parse_item_ref_for_connector(&reference, "caldav").expect("item_ref");
        assert_eq!(decode_event_item_id(&encoded).as_deref(), Some(url));
    }

    #[test]
    fn builds_event_url_from_calendar_url() {
        let calendar_url = "https://example.com/caldav/calendars/work/";
        let url = build_event_url(calendar_url, Some("team-standup"), "uid-123").expect("url");
        assert_eq!(
            url,
            "https://example.com/caldav/calendars/work/team-standup.ics"
        );
    }

    #[test]
    fn builds_ical_event_and_parses_back() {
        let raw = build_ical_event(
            "uid-123",
            Some("Planning"),
            Some("Roadmap review"),
            Some("Conference Room"),
            Some("CONFIRMED"),
            Some("owner@example.com"),
            Some("2026-02-21T15:00:00Z"),
            Some("2026-02-21T16:00:00Z"),
            None,
            Some("https://example.com/events/uid-123.ics"),
        )
        .expect("ical");

        let parsed = parse_ical_event(&raw);
        assert_eq!(parsed.uid.as_deref(), Some("uid-123"));
        assert_eq!(parsed.summary.as_deref(), Some("Planning"));
        assert_eq!(parsed.location.as_deref(), Some("Conference Room"));
        assert_eq!(parsed.start.as_deref(), Some("2026-02-21T15:00:00+00:00"));
        assert_eq!(parsed.end.as_deref(), Some("2026-02-21T16:00:00+00:00"));
    }
}
