use async_trait::async_trait;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Relationship, Source,
};
use crate::utils::{html_to_text, structured_result, structured_result_with_text};
use crate::Connector;
use crate::{URLParamExtraction, URLPatternSpec};
use rmcp::model::*;
use urlencoding;

// Import the types module
mod types;
pub use types::{AlgoliaHit, HackerNewsItem, ItemType, SimpleItem};

const DEFAULT_STORY_FIELDS: &[&str] = &["title", "text"];
const DEFAULT_COMMENT_FIELDS: &[&str] = &["text"];

const STORY_FIELD_ORDER: &[&str] = &[
    "id",
    "title",
    "text",
    "url",
    "author",
    "created_at",
    "created_at_i",
    "type",
    "points",
    "parent_id",
    "story_id",
    "options",
];

const COMMENT_FIELD_ORDER: &[&str] = &[
    "id",
    "text",
    "author",
    "created_at",
    "created_at_i",
    "parent_id",
    "story_id",
    "points",
];

fn parse_field_sets(args: &serde_json::Map<String, Value>) -> (HashSet<String>, HashSet<String>) {
    (
        parse_field_list(args.get("storyFields"), DEFAULT_STORY_FIELDS),
        parse_field_list(args.get("commentFields"), DEFAULT_COMMENT_FIELDS),
    )
}

fn parse_field_list(value: Option<&Value>, defaults: &[&str]) -> HashSet<String> {
    let mut fields: HashSet<String> = defaults.iter().map(|s| s.to_string()).collect();

    if let Some(raw) = value {
        match raw {
            Value::Array(items) => {
                for item in items {
                    if let Some(text) = item.as_str() {
                        update_field_set(&mut fields, text);
                    }
                }
            }
            Value::String(text) => {
                for part in text.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        update_field_set(&mut fields, trimmed);
                    }
                }
            }
            _ => {}
        }
    }

    if fields.is_empty() {
        for default in defaults {
            fields.insert((*default).to_string());
        }
    }

    fields
}

fn update_field_set(fields: &mut HashSet<String>, raw: &str) {
    if raw.is_empty() {
        return;
    }

    let normalized = raw.trim();
    if normalized.starts_with('-') || normalized.starts_with('!') {
        let key = normalized
            .trim_start_matches('-')
            .trim_start_matches('!')
            .trim();
        if !key.is_empty() {
            fields.remove(key);
        }
    } else {
        fields.insert(normalized.to_string());
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HnPageCursor {
    page: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HnStoriesCursor {
    offset: usize,
    limit: usize,
    story_type: String,
}

fn parse_output_format(
    args: &serde_json::Map<String, Value>,
) -> Result<OutputFormat, ConnectorError> {
    ingest::output_format_from_args(args)
}

fn hn_item_ref(kind: &str, id: i64) -> String {
    format!("hackernews:{}:{}", kind, id)
}

fn extract_hn_id_from_url(url: &str) -> Option<i64> {
    let marker = "item?id=";
    let idx = url.find(marker)?;
    let rest = &url[idx + marker.len()..];
    let id_str = rest.split('&').next()?.trim();
    id_str.parse::<i64>().ok()
}

fn parse_i64_arg(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|num| i64::try_from(num).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
}

fn parse_usize_arg(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|num| usize::try_from(num).ok())
        .or_else(|| {
            value
                .as_i64()
                .filter(|num| *num >= 0)
                .and_then(|num| usize::try_from(num).ok())
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<usize>().ok())
        })
}

fn resolve_hn_id(args: &serde_json::Map<String, Value>) -> Result<i64, ConnectorError> {
    for key in ["id", "item_id", "story_id"] {
        if let Some(id) = args.get(key).and_then(parse_i64_arg) {
            return Ok(id);
        }
    }
    if let Some(item_ref) = args.get("item_ref").and_then(|v| v.as_str()) {
        if let Some((_kind, id)) = ingest::parse_item_ref_for_connector(item_ref, "hackernews") {
            if let Ok(id_num) = id.parse::<i64>() {
                return Ok(id_num);
            }
        }
    }
    if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
        if let Some(id) = extract_hn_id_from_url(url) {
            return Ok(id);
        }
    }
    Err(ConnectorError::InvalidParams(
        "Missing 'id'. Provide id, item_id, story_id, item_ref, or url.".to_string(),
    ))
}

fn parse_limit_alias(
    args: &serde_json::Map<String, Value>,
    keys: &[&str],
    default: usize,
    min: usize,
    max: usize,
) -> usize {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(parse_usize_arg))
        .unwrap_or(default)
        .clamp(min, max)
}

fn parse_hn_response_format(args: &serde_json::Map<String, Value>, default: &str) -> String {
    args.get("response_format")
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn plain_text(text: Option<&str>) -> String {
    html_to_text(text.unwrap_or("")).trim().to_string()
}

fn count_hn_comments(item: &HackerNewsItem) -> usize {
    item.children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .filter(|child| matches!(child.r#type, Some(ItemType::Comment)))
                .map(|child| 1 + count_hn_comments(child))
                .sum()
        })
        .unwrap_or(0)
}

fn compact_hit_payload(hit: &AlgoliaHit) -> Option<Value> {
    let id = hit.object_id.as_ref()?.parse::<i64>().ok()?;
    let title = hit
        .title
        .clone()
        .or(hit.story_title.clone())
        .unwrap_or_default();
    let text = plain_text(hit.comment_text.as_deref().or(hit.story_text.as_deref()));
    let mut map = serde_json::Map::new();

    map.insert(
        "kind".to_string(),
        json!(if hit.is_story() { "thread" } else { "comment" }),
    );
    map.insert("id".to_string(), json!(id));
    if !title.is_empty() {
        map.insert("title".to_string(), json!(title));
    }
    if !text.is_empty() {
        map.insert("text".to_string(), json!(text));
    }
    if let Some(url) = &hit.url {
        map.insert("url".to_string(), json!(url));
    }
    map.insert(
        "hn_url".to_string(),
        json!(format!("https://news.ycombinator.com/item?id={id}")),
    );
    if let Some(author) = &hit.author {
        map.insert("author".to_string(), json!(author));
    }
    if let Some(created_at) = hn_created_at_hit(hit) {
        map.insert("created_at".to_string(), json!(created_at));
    }
    if let Some(points) = hit.points {
        map.insert("points".to_string(), json!(points));
    }
    if let Some(story_id) = hit.story_id {
        map.insert("story_id".to_string(), json!(story_id));
    }
    if let Some(parent_id) = hit.parent_id {
        map.insert("parent_id".to_string(), json!(parent_id));
    }
    if let Some(children) = &hit.children {
        map.insert("comment_count".to_string(), json!(children.len()));
    }

    Some(Value::Object(map))
}

fn compact_story_payload(item: &HackerNewsItem) -> Value {
    let id = item.id.unwrap_or_default();
    let mut map = serde_json::Map::new();

    map.insert("id".to_string(), json!(id));
    map.insert(
        "title".to_string(),
        json!(item.title.clone().unwrap_or_default()),
    );
    let text = plain_text(item.text.as_deref());
    if !text.is_empty() {
        map.insert("text".to_string(), json!(text));
    }
    if let Some(url) = &item.url {
        map.insert("url".to_string(), json!(url));
    }
    if id > 0 {
        map.insert(
            "hn_url".to_string(),
            json!(format!("https://news.ycombinator.com/item?id={id}")),
        );
    }
    if let Some(author) = &item.author {
        map.insert("author".to_string(), json!(author));
    }
    if let Some(created_at) = hn_created_at_item(item) {
        map.insert("created_at".to_string(), json!(created_at));
    }
    if let Some(points) = item.points {
        map.insert("points".to_string(), json!(points));
    }
    map.insert("comment_count".to_string(), json!(count_hn_comments(item)));

    Value::Object(map)
}

fn append_compact_comments(
    item: &HackerNewsItem,
    depth: usize,
    remaining: &mut usize,
    out: &mut Vec<Value>,
) {
    if *remaining == 0 {
        return;
    }

    let Some(children) = item.children.as_ref() else {
        return;
    };

    for child in children {
        if *remaining == 0 {
            return;
        }
        if !matches!(child.r#type, Some(ItemType::Comment)) {
            continue;
        }

        let mut map = serde_json::Map::new();
        if let Some(id) = child.id {
            map.insert("id".to_string(), json!(id));
        }
        if let Some(parent_id) = child.parent_id {
            map.insert("parent_id".to_string(), json!(parent_id));
        }
        if let Some(author) = &child.author {
            map.insert("author".to_string(), json!(author));
        }
        if let Some(created_at) = hn_created_at_item(child) {
            map.insert("created_at".to_string(), json!(created_at));
        }
        if let Some(points) = child.points {
            map.insert("points".to_string(), json!(points));
        }
        map.insert("depth".to_string(), json!(depth));
        map.insert("text".to_string(), json!(plain_text(child.text.as_deref())));
        out.push(Value::Object(map));
        *remaining = remaining.saturating_sub(1);

        append_compact_comments(child, depth.saturating_add(1), remaining, out);
    }
}

fn compact_thread_payload(item: &HackerNewsItem, max_comments: usize) -> Value {
    let total_comments = count_hn_comments(item);
    let mut remaining = max_comments;
    let mut comments = Vec::new();
    append_compact_comments(item, 0, &mut remaining, &mut comments);

    let id = item.id.unwrap_or_default();
    let mut map = serde_json::Map::new();
    map.insert("id".to_string(), json!(id));
    map.insert(
        "title".to_string(),
        json!(item.title.clone().unwrap_or_default()),
    );
    let text = plain_text(item.text.as_deref());
    if !text.is_empty() {
        map.insert("text".to_string(), json!(text));
    }
    if let Some(url) = &item.url {
        map.insert("url".to_string(), json!(url));
    }
    if id > 0 {
        map.insert(
            "hn_url".to_string(),
            json!(format!("https://news.ycombinator.com/item?id={id}")),
        );
    }
    if let Some(author) = &item.author {
        map.insert("author".to_string(), json!(author));
    }
    if let Some(created_at) = hn_created_at_item(item) {
        map.insert("created_at".to_string(), json!(created_at));
    }
    if let Some(points) = item.points {
        map.insert("points".to_string(), json!(points));
    }
    map.insert("total_comments".to_string(), json!(total_comments));
    map.insert("returned_comments".to_string(), json!(comments.len()));
    map.insert(
        "truncated".to_string(),
        json!(comments.len() < total_comments),
    );
    map.insert("comments".to_string(), Value::Array(comments));

    Value::Object(map)
}

fn hn_author(name: Option<&String>) -> Vec<Author> {
    match name {
        Some(n) if !n.is_empty() => vec![Author {
            name: n.to_string(),
            id: None,
        }],
        _ => Vec::new(),
    }
}

fn hn_created_at_from_timestamp(ts: i64) -> Option<String> {
    if ts <= 0 {
        return None;
    }
    chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
}

fn hn_created_at_item(item: &HackerNewsItem) -> Option<String> {
    if let Some(created_at) = item.created_at.as_ref() {
        if !created_at.is_empty() {
            return Some(created_at.clone());
        }
    }
    item.created_at_i.and_then(hn_created_at_from_timestamp)
}

fn hn_created_at_hit(hit: &AlgoliaHit) -> Option<String> {
    if let Some(created_at) = hit.created_at.as_ref() {
        if !created_at.is_empty() {
            return Some(created_at.clone());
        }
    }
    hit.created_at_i.and_then(hn_created_at_from_timestamp)
}

fn append_hn_comments(
    item: &HackerNewsItem,
    story_id: i64,
    depth: i64,
    item_ref: &str,
    blocks: &mut Vec<ContentBlock>,
    relationships: &mut Vec<Relationship>,
) {
    let children = item.children.as_deref().unwrap_or(&[]);

    for child in children {
        if !matches!(child.r#type, Some(ItemType::Comment)) {
            continue;
        }
        let Some(id) = child.id else { continue };

        let block_ref = hn_item_ref("comment", id);
        let text_html = child.text.as_deref().unwrap_or("");
        let text = html_to_text(text_html);
        let reply_to = child.parent_id.and_then(|parent_id| {
            if parent_id == story_id {
                None
            } else {
                Some(hn_item_ref("comment", parent_id))
            }
        });
        let position = Some(serde_json::json!({ "kind": "thread_depth", "depth": depth }));

        blocks.push(ContentBlock {
            block_ref: block_ref.clone(),
            block_kind: "comment".to_string(),
            text,
            author: child.author.as_ref().map(|name| Author {
                name: name.to_string(),
                id: None,
            }),
            created_at: hn_created_at_item(child),
            reply_to: reply_to.clone(),
            position,
            score: child.points.map(|p| p as f64),
            attachments: Vec::new(),
            metadata: Some(serde_json::json!({
                "points": child.points,
                "parent_id": child.parent_id,
            })),
        });

        relationships.push(Relationship {
            rel: "has_block".to_string(),
            from: item_ref.to_string(),
            to: block_ref.clone(),
        });
        if let Some(parent_ref) = reply_to {
            relationships.push(Relationship {
                rel: "replies_to".to_string(),
                from: block_ref.clone(),
                to: parent_ref,
            });
        }

        append_hn_comments(
            child,
            story_id,
            depth.saturating_add(1),
            item_ref,
            blocks,
            relationships,
        );
    }
}

fn story_item_to_payload(
    item: &HackerNewsItem,
    story_fields: &HashSet<String>,
    comment_fields: &HashSet<String>,
) -> Value {
    let mut map = serde_json::Map::new();

    for field in STORY_FIELD_ORDER {
        if !story_fields.contains(*field) {
            continue;
        }

        match *field {
            "id" => {
                if let Some(id) = item.id {
                    map.insert("id".to_string(), json!(id));
                }
            }
            "title" => {
                let title = item.title.clone().unwrap_or_default();
                map.insert("title".to_string(), Value::String(title));
            }
            "text" => {
                let text = item.text.clone().unwrap_or_default();
                map.insert("text".to_string(), Value::String(text));
            }
            "url" => {
                if let Some(url) = &item.url {
                    map.insert("url".to_string(), json!(url));
                }
            }
            "author" => {
                if let Some(author) = &item.author {
                    map.insert("author".to_string(), json!(author));
                }
            }
            "created_at" => {
                if let Some(created_at) = &item.created_at {
                    map.insert("created_at".to_string(), json!(created_at));
                }
            }
            "created_at_i" => {
                if let Some(created_at_i) = item.created_at_i {
                    map.insert("created_at_i".to_string(), json!(created_at_i));
                }
            }
            "type" => {
                if let Some(item_type) = &item.r#type {
                    map.insert("type".to_string(), json!(item_type));
                }
            }
            "points" => {
                if let Some(points) = item.points {
                    map.insert("points".to_string(), json!(points));
                }
            }
            "parent_id" => {
                if let Some(parent_id) = item.parent_id {
                    map.insert("parent_id".to_string(), json!(parent_id));
                }
            }
            "story_id" => {
                if let Some(story_id) = item.story_id {
                    map.insert("story_id".to_string(), json!(story_id));
                }
            }
            "options" => {
                if let Some(options) = &item.options {
                    map.insert("options".to_string(), json!(options));
                }
            }
            _ => {}
        }
    }

    let comments = item
        .children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .filter(|child| matches!(child.r#type, Some(ItemType::Comment)))
                .map(|child| comment_item_to_payload(child, comment_fields, true))
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();

    map.insert("comments".to_string(), Value::Array(comments));

    Value::Object(map)
}

fn comment_item_to_payload(
    item: &HackerNewsItem,
    comment_fields: &HashSet<String>,
    include_children: bool,
) -> Value {
    let mut map = serde_json::Map::new();

    for field in COMMENT_FIELD_ORDER {
        if !comment_fields.contains(*field) {
            continue;
        }

        match *field {
            "id" => {
                if let Some(id) = item.id {
                    map.insert("id".to_string(), json!(id));
                }
            }
            "text" => {
                let text = item.text.clone().unwrap_or_default();
                map.insert("text".to_string(), Value::String(text));
            }
            "author" => {
                if let Some(author) = &item.author {
                    map.insert("author".to_string(), json!(author));
                }
            }
            "created_at" => {
                if let Some(created_at) = &item.created_at {
                    map.insert("created_at".to_string(), json!(created_at));
                }
            }
            "created_at_i" => {
                if let Some(created_at_i) = item.created_at_i {
                    map.insert("created_at_i".to_string(), json!(created_at_i));
                }
            }
            "parent_id" => {
                if let Some(parent_id) = item.parent_id {
                    map.insert("parent_id".to_string(), json!(parent_id));
                }
            }
            "story_id" => {
                if let Some(story_id) = item.story_id {
                    map.insert("story_id".to_string(), json!(story_id));
                }
            }
            "points" => {
                if let Some(points) = item.points {
                    map.insert("points".to_string(), json!(points));
                }
            }
            _ => {}
        }
    }

    let replies = if include_children {
        item.children
            .as_ref()
            .map(|children| {
                children
                    .iter()
                    .filter(|child| matches!(child.r#type, Some(ItemType::Comment)))
                    .map(|child| comment_item_to_payload(child, comment_fields, true))
                    .collect::<Vec<Value>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    map.insert("comments".to_string(), Value::Array(replies));

    Value::Object(map)
}

/// Response format - concise is the legacy token-saving shape.
const CONCISE_STORY_FIELDS: &[&str] = &["id", "title", "text"];
const CONCISE_COMMENT_FIELDS: &[&str] = &["text"];

/// Get field sets based on response_format
fn get_field_sets_for_format(
    args: &serde_json::Map<String, Value>,
    response_format: &str,
) -> (HashSet<String>, HashSet<String>) {
    if response_format == "concise" {
        (
            CONCISE_STORY_FIELDS.iter().map(|s| s.to_string()).collect(),
            CONCISE_COMMENT_FIELDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
    } else {
        parse_field_sets(args)
    }
}

/// Create concise story payload (minimal fields for token efficiency)
fn story_item_to_concise_payload(item: &HackerNewsItem) -> Value {
    let mut map = serde_json::Map::new();

    if let Some(id) = item.id {
        map.insert("id".to_string(), json!(id));
    }
    map.insert(
        "title".to_string(),
        json!(item.title.clone().unwrap_or_default()),
    );
    map.insert(
        "text".to_string(),
        json!(item.text.clone().unwrap_or_default()),
    );

    // Include comments but in concise form
    let comments = item
        .children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .filter(|child| matches!(child.r#type, Some(ItemType::Comment)))
                .map(comment_item_to_concise_payload)
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();

    map.insert("comments".to_string(), Value::Array(comments));
    Value::Object(map)
}

/// Create concise comment payload
fn comment_item_to_concise_payload(item: &HackerNewsItem) -> Value {
    let mut map = serde_json::Map::new();

    map.insert(
        "text".to_string(),
        json!(item.text.clone().unwrap_or_default()),
    );

    let replies = item
        .children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .filter(|child| matches!(child.r#type, Some(ItemType::Comment)))
                .map(comment_item_to_concise_payload)
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();

    map.insert("comments".to_string(), Value::Array(replies));
    Value::Object(map)
}

/// Flatten comments into concise format
fn flatten_comments_concise(item: &HackerNewsItem, out: &mut Vec<Value>) {
    if let Some(children) = &item.children {
        for child in children {
            if !matches!(child.r#type, Some(ItemType::Comment)) {
                continue;
            }
            out.push(json!({ "text": child.text.clone().unwrap_or_default() }));
            flatten_comments_concise(child, out);
        }
    }
}

fn story_as_comment_payload(item: &HackerNewsItem, comment_fields: &HashSet<String>) -> Value {
    let mut map = serde_json::Map::new();

    for field in COMMENT_FIELD_ORDER {
        if !comment_fields.contains(*field) {
            continue;
        }

        match *field {
            "id" => {
                if let Some(id) = item.id {
                    map.insert("id".to_string(), json!(id));
                }
            }
            "text" => {
                let combined = match (&item.title, &item.text) {
                    (Some(title), Some(text)) if !text.is_empty() => {
                        format!("{}\n\n{}", title, text)
                    }
                    (Some(title), _) => title.clone(),
                    (None, Some(text)) => text.clone(),
                    (None, None) => String::new(),
                };
                map.insert("text".to_string(), Value::String(combined));
            }
            "author" => {
                if let Some(author) = &item.author {
                    map.insert("author".to_string(), json!(author));
                }
            }
            "created_at" => {
                if let Some(created_at) = &item.created_at {
                    map.insert("created_at".to_string(), json!(created_at));
                }
            }
            "created_at_i" => {
                if let Some(created_at_i) = item.created_at_i {
                    map.insert("created_at_i".to_string(), json!(created_at_i));
                }
            }
            "story_id" => {
                if let Some(id) = item.id {
                    map.insert("story_id".to_string(), json!(id));
                }
            }
            "points" => {
                if let Some(points) = item.points {
                    map.insert("points".to_string(), json!(points));
                }
            }
            _ => {}
        }
    }

    map.insert("comments".to_string(), Value::Array(Vec::new()));

    Value::Object(map)
}

fn flatten_comment_values(
    item: &HackerNewsItem,
    comment_fields: &HashSet<String>,
    out: &mut Vec<Value>,
) {
    if let Some(children) = &item.children {
        for child in children {
            if !matches!(child.r#type, Some(ItemType::Comment)) {
                continue;
            }

            out.push(comment_item_to_payload(child, comment_fields, false));
            flatten_comment_values(child, comment_fields, out);
        }
    }
}

// Algolia search response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct AlgoliaSearchResponse {
    pub ab_test_id: Option<i64>,
    #[serde(rename = "abTestVariantID")]
    pub ab_test_variant_id: Option<i64>,
    #[serde(rename = "aroundLatLng")]
    pub around_lat_lng: Option<String>,
    #[serde(rename = "automaticRadius")]
    pub automatic_radius: Option<String>,
    pub exhaustive: Option<ExhaustiveInfo>,
    #[serde(rename = "appliedRules")]
    pub applied_rules: Option<Vec<HashMap<String, Value>>>,
    #[serde(rename = "exhaustiveFacetsCount")]
    pub exhaustive_facets_count: Option<bool>,
    #[serde(rename = "exhaustiveNbHits")]
    pub exhaustive_nb_hits: Option<bool>,
    #[serde(rename = "exhaustiveTypo")]
    pub exhaustive_typo: Option<bool>,
    pub facets: Option<HashMap<String, HashMap<String, i64>>>,
    #[serde(rename = "facets_stats")]
    pub facets_stats: Option<HashMap<String, FacetStats>>,
    pub index: Option<String>,
    #[serde(rename = "indexUsed")]
    pub index_used: Option<String>,
    pub message: Option<String>,
    #[serde(rename = "nbSortedHits")]
    pub nb_sorted_hits: Option<i64>,
    #[serde(rename = "parsedQuery")]
    pub parsed_query: Option<String>,
    #[serde(rename = "processingTimeMS")]
    pub processing_time_ms: Option<i64>,
    #[serde(rename = "processingTimingsMS")]
    pub processing_timings_ms: Option<HashMap<String, Value>>,
    #[serde(rename = "queryAfterRemoval")]
    pub query_after_removal: Option<String>,
    pub redirect: Option<RedirectInfo>,
    #[serde(rename = "renderingContent")]
    pub rendering_content: Option<RenderingContent>,
    #[serde(rename = "serverTimeMS")]
    pub server_time_ms: Option<i64>,
    #[serde(rename = "serverUsed")]
    pub server_used: Option<String>,
    #[serde(rename = "userData")]
    pub user_data: Option<HashMap<String, Value>>,
    #[serde(rename = "queryID")]
    pub query_id: Option<String>,
    #[serde(rename = "_automaticInsights")]
    pub automatic_insights: Option<bool>,
    pub page: Option<i64>,
    #[serde(rename = "nbHits")]
    pub nb_hits: Option<i64>,
    #[serde(rename = "nbPages")]
    pub nb_pages: Option<i64>,
    #[serde(rename = "hitsPerPage")]
    pub hits_per_page: Option<i64>,
    pub hits: Option<Vec<AlgoliaHit>>,
    pub query: Option<String>,
    pub params: Option<String>,
}

impl Default for AlgoliaSearchResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl AlgoliaSearchResponse {
    pub fn new() -> Self {
        AlgoliaSearchResponse {
            ab_test_id: None,
            ab_test_variant_id: None,
            around_lat_lng: None,
            automatic_radius: None,
            exhaustive: None,
            applied_rules: None,
            exhaustive_facets_count: None,
            exhaustive_nb_hits: None,
            exhaustive_typo: None,
            facets: None,
            facets_stats: None,
            index: None,
            index_used: None,
            message: None,
            nb_sorted_hits: None,
            parsed_query: None,
            processing_time_ms: None,
            processing_timings_ms: None,
            query_after_removal: None,
            redirect: None,
            rendering_content: None,
            server_time_ms: None,
            server_used: None,
            user_data: None,
            query_id: None,
            automatic_insights: None,
            page: None,
            nb_hits: None,
            nb_pages: None,
            hits_per_page: None,
            hits: None,
            query: None,
            params: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExhaustiveInfo {
    #[serde(rename = "facetsCount")]
    pub facets_count: Option<bool>,
    #[serde(rename = "facetValues")]
    pub facet_values: Option<bool>,
    #[serde(rename = "nbHits")]
    pub nb_hits: Option<bool>,
    #[serde(rename = "rulesMatch")]
    pub rules_match: Option<bool>,
    pub typo: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FacetStats {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub avg: Option<f64>,
    pub sum: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RedirectInfo {
    pub index: Option<Vec<RedirectIndexItem>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RedirectIndexItem {
    pub source: Option<String>,
    pub dest: Option<String>,
    pub reason: Option<String>,
    pub succeed: Option<bool>,
    pub data: Option<RedirectData>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RedirectData {
    #[serde(rename = "ruleObjectID")]
    pub rule_object_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenderingContent {
    #[serde(rename = "facetOrdering")]
    pub facet_ordering: Option<FacetOrdering>,
    pub redirect: Option<RenderingRedirect>,
    pub widgets: Option<Widgets>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FacetOrdering {
    pub facets: Option<FacetOrder>,
    pub values: Option<HashMap<String, FacetValueOrder>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FacetOrder {
    pub order: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FacetValueOrder {
    pub order: Option<Vec<String>>,
    #[serde(rename = "sortRemainingBy")]
    pub sort_remaining_by: Option<String>,
    pub hide: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenderingRedirect {
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Widgets {
    pub banners: Option<Vec<Banner>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Banner {
    pub image: Option<BannerImage>,
    pub link: Option<BannerLink>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BannerImage {
    pub urls: Option<Vec<UrlItem>>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlItem {
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BannerLink {
    pub url: Option<String>,
}

// Remove AlgoliaHit and related structs
#[derive(Clone)]
pub struct HackerNewsConnector {
    client: reqwest::Client,
}

impl Default for HackerNewsConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl HackerNewsConnector {
    pub fn new() -> Self {
        HackerNewsConnector {
            client: reqwest::Client::builder()
                .user_agent("rzn-tools/0.1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    fn client_from_proxy(proxy_url: Option<&str>) -> Result<reqwest::Client, ConnectorError> {
        let mut builder = reqwest::Client::builder().user_agent("rzn-tools/0.1.0");
        if let Some(url) = proxy_url {
            let proxy = reqwest::Proxy::all(url).map_err(|e| {
                ConnectorError::InvalidParams(format!("Invalid proxy_url '{}': {}", url, e))
            })?;
            builder = builder.proxy(proxy);
        }
        builder
            .build()
            .map_err(|e| ConnectorError::Other(format!("Failed to build HTTP client: {}", e)))
    }

    // Helper: fetch JSON from URL
    async fn fetch_json(&self, url: &str) -> Result<Value, ConnectorError> {
        let res = self
            .client
            .get(url)
            .header("User-Agent", "rzn-tools/0.1.0")
            .send()
            .await
            .map_err(|e| ConnectorError::Other(format!("Request error: {}", e)))?;
        let json = res
            .json::<Value>()
            .await
            .map_err(|e| ConnectorError::Other(format!("JSON parse error: {}", e)))?;
        Ok(json)
    }

    // Helper: fetch typed response from URL
    async fn fetch_typed<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
    ) -> Result<T, ConnectorError> {
        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ConnectorError::Other(format!("Request error: {}", e)))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(ConnectorError::Other(format!(
                "HTTP error {}: {}",
                status, body
            )));
        }

        let text = res
            .text()
            .await
            .map_err(|e| ConnectorError::Other(format!("Response error: {}", e)))?;

        serde_json::from_str(&text).map_err(|e| {
            ConnectorError::Other(format!(
                "JSON parse error: {} (url: {}, response: {}...)",
                e,
                url,
                &text[..100.min(text.len())]
            ))
        })
    }

    // Helper: fetch Algolia search response from URL
    async fn fetch_algolia_search(
        &self,
        url: &str,
    ) -> Result<AlgoliaSearchResponse, ConnectorError> {
        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ConnectorError::Other(format!("Request error: {}", e)))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(ConnectorError::Other(format!(
                "HTTP error {}: {}",
                status, body
            )));
        }

        let text = res
            .text()
            .await
            .map_err(|e| ConnectorError::Other(format!("Response error: {}", e)))?;

        serde_json::from_str(&text).map_err(|e| {
            ConnectorError::Other(format!(
                "JSON parse error: {} (response: {}...)",
                e,
                &text[..100.min(text.len())]
            ))
        })
    }

    // Helper: fetch a Hacker News item by ID using Algolia API
    async fn get_item(&self, item_id: i64) -> Result<HackerNewsItem, ConnectorError> {
        let url = format!("https://hn.algolia.com/api/v1/items/{}", item_id);
        self.fetch_typed::<HackerNewsItem>(&url).await
    }

    // Helper: build a stub story item with just an ID
    fn story_stub(id: i64) -> HackerNewsItem {
        HackerNewsItem {
            id: Some(id),
            author: None,
            created_at: None,
            created_at_i: None,
            r#type: Some(ItemType::Story),
            text: None,
            title: None,
            url: None,
            points: None,
            parent_id: None,
            story_id: None,
            options: None,
            children: None,
        }
    }

    // Helper: fetch top stories
    async fn get_top_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hacker-news.firebaseio.com/v0/topstories.json";
        let ids: Vec<i64> = self.fetch_typed(url).await?;
        Ok(ids.into_iter().map(Self::story_stub).collect())
    }

    // Helper: fetch new stories
    async fn get_new_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hn.algolia.com/api/v1/search_by_date?tags=story";
        let response = self.fetch_algolia_search(url).await?;
        Ok(self.hits_to_items(response.hits.unwrap_or_default()))
    }

    // Helper: fetch best stories (front page sorted by points)
    async fn get_best_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage=50";
        let response = self.fetch_algolia_search(url).await?;
        let mut items = self.hits_to_items(response.hits.unwrap_or_default());
        // Sort by points descending for "best"
        items.sort_by(|a, b| b.points.unwrap_or(0).cmp(&a.points.unwrap_or(0)));
        Ok(items)
    }

    // Helper: fetch ask stories
    async fn get_ask_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hn.algolia.com/api/v1/search_by_date?tags=ask_hn";
        let response = self.fetch_algolia_search(url).await?;
        Ok(self.hits_to_items(response.hits.unwrap_or_default()))
    }

    // Helper: fetch show stories
    async fn get_show_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hn.algolia.com/api/v1/search_by_date?tags=show_hn";
        let response = self.fetch_algolia_search(url).await?;
        Ok(self.hits_to_items(response.hits.unwrap_or_default()))
    }

    // Helper: fetch job stories
    async fn get_job_stories_list(&self) -> Result<Vec<HackerNewsItem>, ConnectorError> {
        let url = "https://hn.algolia.com/api/v1/search_by_date?tags=job";
        let response = self.fetch_algolia_search(url).await?;
        Ok(self.hits_to_items(response.hits.unwrap_or_default()))
    }

    // Helper: convert Algolia hits to HackerNewsItems
    fn hits_to_items(&self, hits: Vec<AlgoliaHit>) -> Vec<HackerNewsItem> {
        hits.into_iter()
            .filter_map(|hit| {
                if let Some(id) = hit.object_id.and_then(|id| id.parse().ok()) {
                    Some(HackerNewsItem {
                        id: Some(id),
                        author: hit.author,
                        created_at: hit.created_at,
                        created_at_i: hit.created_at_i,
                        r#type: Some(ItemType::Story),
                        text: hit.story_text.or(hit.comment_text),
                        title: hit.title,
                        url: hit.url,
                        points: hit.points,
                        parent_id: hit.parent_id,
                        story_id: hit.story_id,
                        options: None,
                        children: None,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

// Helper function to flatten comments recursively
#[async_trait]
impl Connector for HackerNewsConnector {
    fn name(&self) -> &'static str {
        "hackernews"
    }

    fn description(&self) -> &'static str {
        "A connector for interacting with Hacker News via Firebase and Algolia search API."
    }

    fn display_name(&self) -> &'static str {
        "Hacker News"
    }

    fn icon(&self) -> &'static str {
        "hackernews"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["news", "tech", "social"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?news\.ycombinator\.com/item\?id=(\d+)".to_string(),
            default_tool: "get".to_string(),
            description: "Fetch a Hacker News story by ID".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "id".to_string(),
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

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        // No auth required for public Hacker News APIs, but profiles may still want to
        // route traffic via a per-account proxy.
        let proxy_url = details.get("proxy_url").map(String::as_str);
        self.client = Self::client_from_proxy(proxy_url)?;
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test connectivity by fetching maxitem
        let _ = self
            .fetch_json("https://hacker-news.firebaseio.com/v0/maxitem.json")
            .await?;
        Ok(())
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
                "Canonical tools for LLMs: use 'get_thread' to fetch a thread, 'search' to search by relevance, 'search_recent' for chronological search, and 'list_threads' for top/new/best/ask/show/job feeds. 'get_thread' defaults to compact plain-text output with a bounded comment list. Legacy aliases ('get', 'get_post', 'search_stories', 'search_by_date', 'get_stories') remain available for compatibility.".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
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
                name: Cow::Borrowed("get_thread"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a Hacker News thread by ID or URL. Default output is compact, plain-text, and comment-limited for LLM use.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": { "type": "string", "description": "Normalized item_ref (for example hackernews:story:8863)." },
                        "url": { "type": "string", "description": "Hacker News item URL (for example https://news.ycombinator.com/item?id=8863)." },
                        "id": { "type": ["integer", "string"], "description": "Hacker News item ID. Numeric strings are accepted." },
                        "item_id": { "type": ["integer", "string"], "description": "Alias for id. Numeric strings are accepted." },
                        "max_comments": { "type": ["integer", "string"], "description": "Maximum number of comments to include in compact output. Numeric strings are accepted.", "default": 20, "minimum": 0, "maximum": 500 },
                        "flatten": { "type": "boolean", "description": "Flatten comments into a single ordered list. Compact output always uses a flat list.", "default": true },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "concise", "detailed"],
                            "description": "'compact' is the LLM-friendly default. 'concise' preserves the older minimal nested shape. 'detailed' includes metadata fields.",
                            "default": "compact"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        },
                        "storyFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional story fields for detailed output. Prefix with '-' to remove defaults."
                        },
                        "commentFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional comment fields for detailed output. Prefix with '-' to remove defaults."
                        }
                    },
                    "examples": [
                        {
                            "description": "Compact thread by URL",
                            "input": { "url": "https://news.ycombinator.com/item?id=8863" }
                        },
                        {
                            "description": "Compact thread by numeric string alias",
                            "input": { "item_id": "8863", "response_format": "compact" }
                        },
                        {
                            "description": "Compact thread with more comments",
                            "input": { "id": 8863, "max_comments": 50, "response_format": "compact" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["news", "tech", "social", "thread"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search Hacker News threads by relevance. Default output is compact and LLM-friendly.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query." },
                        "page": { "type": "integer", "description": "Result page number.", "default": 0 },
                        "limit": { "type": "integer", "description": "Maximum number of results.", "default": 10, "minimum": 1, "maximum": 100 },
                        "hitsPerPage": { "type": "integer", "description": "Legacy alias for limit." },
                        "tags": { "type": "string", "description": "Optional Algolia tags filter." },
                        "numericFilters": { "type": "string", "description": "Optional Algolia numeric filters." },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries. 'detailed' returns the full Algolia payload.",
                            "default": "compact"
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
                            "description": "Search for Rust discussions",
                            "input": { "query": "rust", "limit": 10 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["news", "tech", "social", "thread"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_recent"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search recent Hacker News threads in reverse chronological order. Default output is compact and LLM-friendly.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query." },
                        "page": { "type": "integer", "description": "Result page number.", "default": 0 },
                        "limit": { "type": "integer", "description": "Maximum number of results.", "default": 10, "minimum": 1, "maximum": 100 },
                        "hitsPerPage": { "type": "integer", "description": "Legacy alias for limit." },
                        "tags": { "type": "string", "description": "Optional Algolia tags filter." },
                        "numericFilters": { "type": "string", "description": "Optional Algolia numeric filters." },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries. 'detailed' returns the full Algolia payload.",
                            "default": "compact"
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
                            "description": "Recent AI threads",
                            "input": { "query": "ai agents", "limit": 10 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["news", "tech", "social", "thread"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_threads"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List top/new/best/ask/show/job threads. Default output is compact and LLM-friendly.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "feed": {
                            "type": "string",
                            "enum": ["top", "new", "best", "ask", "show", "job"],
                            "description": "Canonical feed name.",
                            "default": "top"
                        },
                        "story_type": {
                            "type": "string",
                            "enum": ["top", "new", "best", "ask", "show", "job"],
                            "description": "Legacy alias for feed."
                        },
                        "limit": { "type": "integer", "description": "Maximum number of threads to return.", "default": 10, "minimum": 1, "maximum": 100 },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "concise", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries.",
                            "default": "compact"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        },
                        "storyFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional story fields for detailed output. Prefix with '-' to remove defaults."
                        },
                        "commentFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional comment fields for detailed output. Prefix with '-' to remove defaults."
                        }
                    },
                    "examples": [
                        {
                            "description": "Top threads",
                            "input": { "feed": "top", "limit": 10 }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["news", "tech", "social", "thread"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_stories"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Legacy alias for 'search'. Search Hacker News via Algolia (relevance-ranked).",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query" },
                        "page": { "type": "integer", "description": "Page number", "default": 0 },
                        "hitsPerPage": { "type": "integer", "description": "Results per page", "default": 20 },
                        "limit": { "type": "integer", "description": "Alias for hitsPerPage." },
                        "tags": {
                            "type": "string",
                            "description": "Filter on specific tags (e.g., 'story', 'comment', 'poll', 'pollopt', 'show_hn', 'ask_hn', 'front_page', 'author_:USERNAME', 'story_:ID')"
                        },
                        "numericFilters": {
                            "type": "string",
                            "description": "Filter on numerical conditions (e.g., 'points>10', 'num_comments>5', 'created_at_i>1600000000')"
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries. 'detailed' returns the full Algolia payload.",
                            "default": "detailed"
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
                            "description": "Search AI discussions",
                            "input": { "query": "transformer architecture", "hitsPerPage": 10 }
                        },
                        {
                            "description": "Search recent Rust posts",
                            "input": { "query": "rust programming", "tags": "story", "page": 0 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["news", "tech", "social"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_by_date"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Legacy alias for 'search_recent'. Search recent Hacker News items in chronological order.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query" },
                        "page": { "type": "integer", "description": "Page number", "default": 0 },
                        "hitsPerPage": { "type": "integer", "description": "Results per page", "default": 20 },
                        "limit": { "type": "integer", "description": "Alias for hitsPerPage when using normalized output." },
                        "tags": {
                            "type": "string",
                            "description": "Filter on specific tags (e.g., 'story', 'comment', 'poll', 'pollopt', 'show_hn', 'ask_hn', 'front_page', 'author_:USERNAME', 'story_:ID')"
                        },
                        "numericFilters": {
                            "type": "string",
                            "description": "Filter on numerical conditions (e.g., 'points>10', 'num_comments>5', 'created_at_i>1600000000')"
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries. 'detailed' returns the full Algolia payload.",
                            "default": "detailed"
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
                            "description": "Search most recent posts",
                            "input": { "query": "open source", "hitsPerPage": 20 }
                        },
                        {
                            "description": "Recent Ask HN",
                            "input": { "query": "hiring", "tags": "ask_hn", "hitsPerPage": 10 }
                        }
                    ],
                    "_meta": {
                        "category": "search",
                        "tags": ["news", "tech", "social"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_stories"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Legacy alias for 'list_threads'. Top/new/best/ask/show/job stories by type.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "story_type": {
                            "type": "string",
                            "enum": ["top", "new", "best", "ask", "show", "job"],
                            "description": "Type of stories to fetch: 'top' (front page), 'new' (latest), 'best' (highest points), 'ask' (Ask HN), 'show' (Show HN), 'job' (job postings)",
                            "default": "top"
                        },
                        "limit": { "type": "integer", "description": "Maximum number of stories to return (default: 10)", "default": 10 },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "concise", "detailed"],
                            "description": "'compact' returns a clean list of thread summaries. 'concise' preserves the older minimal shape. 'detailed' includes metadata.",
                            "default": "concise"
                        },
                        "cursor": {
                            "type": ["string", "null"],
                            "description": "Opaque pagination cursor from a previous normalized response."
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        },
                        "storyFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional list of additional story fields to include. Prefix with '-' to remove defaults. Defaults: ['title','text']. Only used when response_format is 'detailed'."
                        },
                        "commentFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional list of additional comment fields to include. Prefix with '-' to remove defaults. Defaults: ['text']. Only used when response_format is 'detailed'."
                        }
                    },
                    "required": [],
                    "examples": [
                        {
                            "description": "Top stories",
                            "input": { "story_type": "top", "limit": 10 }
                        },
                        {
                            "description": "Latest Ask HN",
                            "input": { "story_type": "ask", "limit": 5 }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["news", "tech", "social"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Legacy alias for 'get_thread'. Story or comment by ID, with comments.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": { "type": "string", "description": "Normalized item_ref (e.g., hackernews:story:8863)." },
                        "url": { "type": "string", "description": "Canonical HN URL (e.g., https://news.ycombinator.com/item?id=8863)." },
                        "id": { "type": ["integer", "string"], "description": "The Hacker News item ID (e.g., 12345678) - numeric ID from the URL news.ycombinator.com/item?id=12345678. Numeric strings are accepted." },
                        "item_id": { "type": ["integer", "string"], "description": "Alias for id. Numeric strings are accepted." },
                        "max_comments": {
                            "type": ["integer", "string"],
                            "description": "Maximum number of comments to include in compact output. Numeric strings are accepted.",
                            "default": 20,
                            "minimum": 0,
                            "maximum": 500
                        },
                        "flatten": {
                            "type": "boolean",
                            "description": "Return comments as a flat array instead of nested tree structure",
                            "default": false
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["compact", "concise", "detailed"],
                            "description": "'compact' is the LLM-friendly default for get_thread. 'concise' preserves the older minimal shape. 'detailed' includes metadata.",
                            "default": "concise"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output.",
                            "default": "raw"
                        },
                        "storyFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional list of additional story fields to include. Prefix with '-' to remove defaults. Defaults: ['title','text']. Only used when response_format is 'detailed'."
                        },
                        "commentFields": {
                            "type": ["array", "string"],
                            "items": { "type": "string" },
                            "description": "Optional list of additional comment fields to include. Prefix with '-' to remove defaults. Defaults: ['text']. Only used when response_format is 'detailed'."
                        }
                    },
                    "examples": [
                        {
                            "description": "Get story by ID",
                            "input": { "id": 8863, "flatten": true }
                        },
                        {
                            "description": "Get story by numeric string alias",
                            "input": { "item_id": "8863", "response_format": "compact" }
                        },
                        {
                            "description": "Get story by URL",
                            "input": { "url": "https://news.ycombinator.com/item?id=8863" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["news", "tech", "social"],
                        "auth_required": false,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            //  Tool {
            //      name: Cow::Borrowed("get_user"),
            //      description: Some(Cow::Borrowed("Get Hacker News user details by username")),
            //      annotations: None,
            //      input_schema: Arc::new(json!({
            //          "type": "object",
            //          "properties": {
            //              "id": { "type": "string", "description": "The Hacker News username (case-sensitive)" }
            //          },
            //          "required": ["id"]
            //      }).as_object().expect("Schema object").clone()),
            //      output_schema: None,
            //  },
            //  Tool {
            //      name: Cow::Borrowed("get_max_item_id"),
            //      description: Some(Cow::Borrowed("Get the current largest item id on Hacker News")),
            //      annotations: None,
            //      input_schema: Arc::new(json!({
            //          "type": "object",
            //          "properties": {},
            //          "required": []
            //      }).as_object().expect("Schema object").clone()),
            //      output_schema: None,
            //  },
            //  Tool {
            //      name: Cow::Borrowed("get_updates"),
            //      description: Some(Cow::Borrowed("Get the latest item and profile changes on Hacker News")),
            //      annotations: None,
            //      input_schema: Arc::new(json!({
            //          "type": "object",
            //          "properties": {},
            //          "required": []
            //      }).as_object().expect("Schema object").clone()),
            //      output_schema: None,
            //  }
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
        match name {
            "search" | "search_stories" => {
                let output_format = parse_output_format(&args)?;
                let response_format = if name == "search" {
                    parse_hn_response_format(&args, "compact")
                } else {
                    parse_hn_response_format(&args, "detailed")
                };
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' parameter".to_string()),
                )?;
                let cursor = match args.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => {
                        Some(ingest::decode_cursor::<HnPageCursor>(s).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?)
                    }
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "cursor must be a string or null".to_string(),
                        ))
                    }
                };
                let page = cursor
                    .as_ref()
                    .map(|c| c.page)
                    .unwrap_or_else(|| args.get("page").and_then(parse_i64_arg).unwrap_or(0));
                let hits_per_page = args
                    .get("limit")
                    .and_then(parse_i64_arg)
                    .or_else(|| args.get("hitsPerPage").and_then(parse_i64_arg))
                    .unwrap_or(20)
                    .clamp(1, 100);

                // Build the base URL
                let mut url = format!(
                    "https://hn.algolia.com/api/v1/search?query={}&page={}&hitsPerPage={}",
                    urlencoding::encode(query),
                    page,
                    hits_per_page
                );

                // Add tags if provided
                if let Some(tags) = args.get("tags").and_then(|v| v.as_str()) {
                    url.push_str(&format!("&tags={}", urlencoding::encode(tags)));
                } else {
                    // Default to story tag if no tags specified
                    //   url.push_str("&tags=story");
                }
                //   if let Some(dateRange) = args.get("dateRange").and_then(|v| v.as_str()) {
                //       url.push_str(&format!("&dateRange={}", urlencoding::encode(dateRange)));
                //   } else {
                //       // Default to last 30 days if no date range specified
                //       url.push_str("&dateRange=all");
                //   }
                // Add numeric filters if provided
                if let Some(numeric_filters) = args.get("numericFilters").and_then(|v| v.as_str()) {
                    url.push_str(&format!(
                        "&numericFilters={}",
                        urlencoding::encode(numeric_filters)
                    ));
                }

                tracing::debug!(url = %url, "Executing Hacker News search");
                let result: AlgoliaSearchResponse = self.fetch_algolia_search(&url).await?;
                if output_format == OutputFormat::NormalizedV1 {
                    let hits = result.hits.clone().unwrap_or_default();
                    let items: Vec<ContentItem> = hits
                        .into_iter()
                        .filter_map(|hit| {
                            let id = hit.object_id.as_ref().and_then(|s| s.parse::<i64>().ok())?;
                            let is_story = hit.is_story();
                            let kind_label = if is_story { "story" } else { "comment" };
                            let kind = if is_story { "thread" } else { "comment" };
                            let canonical_url =
                                format!("https://news.ycombinator.com/item?id={}", id);
                            let tags = hit.tags.clone().unwrap_or_default();
                            Some(ContentItem {
                                item_ref: hn_item_ref(kind_label, id),
                                kind: kind.to_string(),
                                canonical_url: Some(canonical_url),
                                title: hit
                                    .title
                                    .clone()
                                    .or(hit.story_title.clone())
                                    .filter(|t| !t.is_empty()),
                                created_at: hn_created_at_hit(&hit),
                                source_updated_at: None,
                                authors: hn_author(hit.author.as_ref()),
                                tags,
                                metadata: Some(json!({
                                    "points": hit.points,
                                    "url": hit.url,
                                    "story_id": hit.story_id,
                                    "parent_id": hit.parent_id,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            })
                        })
                        .collect();

                    let has_more = result
                        .nb_pages
                        .zip(result.page)
                        .map(|(pages, page)| page + 1 < pages)
                        .unwrap_or(false);
                    let next_cursor = if has_more {
                        ingest::encode_cursor(&HnPageCursor { page: page + 1 }).ok()
                    } else {
                        None
                    };
                    let page_out = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(hits_per_page as u64))),
                        Source::new("hackernews", "search_stories"),
                    );
                    return structured_result(&page_out);
                }

                if response_format == "compact" {
                    let items = result
                        .hits
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|hit| compact_hit_payload(&hit))
                        .collect::<Vec<_>>();
                    let payload = json!({
                        "query": query,
                        "page": page,
                        "limit": hits_per_page,
                        "total": result.nb_hits.unwrap_or(items.len() as i64),
                        "has_more": result
                            .nb_pages
                            .zip(result.page)
                            .map(|(pages, current_page)| current_page + 1 < pages)
                            .unwrap_or(false),
                        "items": items
                    });
                    let text = serde_json::to_string(&payload)?;
                    return structured_result_with_text(&payload, Some(text));
                }

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            "search_recent" | "search_by_date" => {
                let output_format = parse_output_format(&args)?;
                let response_format = if name == "search_recent" {
                    parse_hn_response_format(&args, "compact")
                } else {
                    parse_hn_response_format(&args, "detailed")
                };
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("Missing 'query' parameter".to_string()),
                )?;
                let cursor = match args.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => {
                        Some(ingest::decode_cursor::<HnPageCursor>(s).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?)
                    }
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "cursor must be a string or null".to_string(),
                        ))
                    }
                };
                let page = cursor
                    .as_ref()
                    .map(|c| c.page)
                    .unwrap_or_else(|| args.get("page").and_then(parse_i64_arg).unwrap_or(0));
                let hits_per_page = args
                    .get("limit")
                    .and_then(parse_i64_arg)
                    .or_else(|| args.get("hitsPerPage").and_then(parse_i64_arg))
                    .unwrap_or(20)
                    .clamp(1, 100);

                // Build the base URL
                let mut url = format!(
                    "https://hn.algolia.com/api/v1/search_by_date?query={}&page={}&hitsPerPage={}",
                    urlencoding::encode(query),
                    page,
                    hits_per_page
                );

                // Add tags if provided
                if let Some(tags) = args.get("tags").and_then(|v| v.as_str()) {
                    url.push_str(&format!("&tags={}", urlencoding::encode(tags)));
                } else {
                    // Default to story tag if no tags specified
                    url.push_str("&tags=story");
                }

                // Add numeric filters if provided
                if let Some(numeric_filters) = args.get("numericFilters").and_then(|v| v.as_str()) {
                    url.push_str(&format!(
                        "&numericFilters={}",
                        urlencoding::encode(numeric_filters)
                    ));
                }

                let result: AlgoliaSearchResponse = self.fetch_algolia_search(&url).await?;
                if output_format == OutputFormat::NormalizedV1 {
                    let hits = result.hits.clone().unwrap_or_default();
                    let items: Vec<ContentItem> = hits
                        .into_iter()
                        .filter_map(|hit| {
                            let id = hit.object_id.as_ref().and_then(|s| s.parse::<i64>().ok())?;
                            let is_story = hit.is_story();
                            let kind_label = if is_story { "story" } else { "comment" };
                            let kind = if is_story { "thread" } else { "comment" };
                            let canonical_url =
                                format!("https://news.ycombinator.com/item?id={}", id);
                            let tags = hit.tags.clone().unwrap_or_default();
                            Some(ContentItem {
                                item_ref: hn_item_ref(kind_label, id),
                                kind: kind.to_string(),
                                canonical_url: Some(canonical_url),
                                title: hit
                                    .title
                                    .clone()
                                    .or(hit.story_title.clone())
                                    .filter(|t| !t.is_empty()),
                                created_at: hn_created_at_hit(&hit),
                                source_updated_at: None,
                                authors: hn_author(hit.author.as_ref()),
                                tags,
                                metadata: Some(json!({
                                    "points": hit.points,
                                    "url": hit.url,
                                    "story_id": hit.story_id,
                                    "parent_id": hit.parent_id,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            })
                        })
                        .collect();

                    let has_more = result
                        .nb_pages
                        .zip(result.page)
                        .map(|(pages, page)| page + 1 < pages)
                        .unwrap_or(false);
                    let next_cursor = if has_more {
                        ingest::encode_cursor(&HnPageCursor { page: page + 1 }).ok()
                    } else {
                        None
                    };
                    let page_out = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(hits_per_page as u64))),
                        Source::new("hackernews", "search_by_date"),
                    );
                    return structured_result(&page_out);
                }

                if response_format == "compact" {
                    let items = result
                        .hits
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|hit| compact_hit_payload(&hit))
                        .collect::<Vec<_>>();
                    let payload = json!({
                        "query": query,
                        "page": page,
                        "limit": hits_per_page,
                        "total": result.nb_hits.unwrap_or(items.len() as i64),
                        "has_more": result
                            .nb_pages
                            .zip(result.page)
                            .map(|(pages, current_page)| current_page + 1 < pages)
                            .unwrap_or(false),
                        "items": items
                    });
                    let text = serde_json::to_string(&payload)?;
                    return structured_result_with_text(&payload, Some(text));
                }

                let text = serde_json::to_string(&result)?;
                Ok(structured_result_with_text(&result, Some(text))?)
            }
            // Consolidated get_stories tool - handles all story types
            "list_threads" | "get_stories" => {
                let output_format = parse_output_format(&args)?;
                let story_type = args
                    .get("feed")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("story_type").and_then(|v| v.as_str()))
                    .unwrap_or("top");
                let limit = args
                    .get("limit")
                    .and_then(parse_usize_arg)
                    .unwrap_or(10)
                    .max(1);
                let cursor = match args.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => {
                        Some(ingest::decode_cursor::<HnStoriesCursor>(s).ok_or_else(|| {
                            ConnectorError::InvalidParams("Invalid cursor".to_string())
                        })?)
                    }
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "cursor must be a string or null".to_string(),
                        ))
                    }
                };
                let start_index = if let Some(c) = cursor {
                    if c.limit != limit {
                        return Err(ConnectorError::InvalidParams(
                            "cursor does not match limit".to_string(),
                        ));
                    }
                    if c.story_type != story_type {
                        return Err(ConnectorError::InvalidParams(
                            "cursor does not match story_type".to_string(),
                        ));
                    }
                    c.offset
                } else {
                    0
                };
                let response_format = if name == "list_threads" {
                    parse_hn_response_format(&args, "compact")
                } else {
                    parse_hn_response_format(&args, "concise")
                };

                // Get story IDs based on type
                let story_ids = match story_type {
                    "top" => self.get_top_stories_list().await?,
                    "new" => self.get_new_stories_list().await?,
                    "best" => self.get_best_stories_list().await?,
                    "ask" => self.get_ask_stories_list().await?,
                    "show" => self.get_show_stories_list().await?,
                    "job" => self.get_job_stories_list().await?,
                    _ => {
                        return Err(ConnectorError::InvalidParams(format!(
                            "Invalid story_type '{}'. Valid types: top, new, best, ask, show, job",
                            story_type
                        )));
                    }
                };

                // Fetch details for each story up to the limit
                let has_more = start_index + limit < story_ids.len();
                let next_cursor = if has_more {
                    ingest::encode_cursor(&HnStoriesCursor {
                        offset: start_index + limit,
                        limit,
                        story_type: story_type.to_string(),
                    })
                    .ok()
                } else {
                    None
                };

                if output_format == OutputFormat::NormalizedV1 {
                    let mut items = Vec::new();
                    for item in story_ids.iter().skip(start_index).take(limit) {
                        if let Some(id) = item.id {
                            let story = self.get_item(id).await?;
                            let canonical_url =
                                format!("https://news.ycombinator.com/item?id={}", id);
                            let content = ContentItem {
                                item_ref: hn_item_ref("story", id),
                                kind: "thread".to_string(),
                                canonical_url: Some(canonical_url),
                                title: story.title.clone().filter(|t| !t.is_empty()),
                                created_at: hn_created_at_item(&story),
                                source_updated_at: None,
                                authors: hn_author(story.author.as_ref()),
                                tags: Vec::new(),
                                metadata: Some(json!({
                                    "points": story.points,
                                    "url": story.url,
                                    "story_id": story.story_id,
                                })),
                                blocks: Vec::new(),
                                relationships: Vec::new(),
                                truncation: None,
                            };
                            items.push(content);
                        }
                    }

                    let page = NormalizedPageV1::new(
                        items,
                        next_cursor,
                        has_more,
                        Partial::complete(Some(ingest::limits_max_items(limit as u64))),
                        Source::new("hackernews", "get_stories"),
                    );
                    return structured_result(&page);
                }

                if response_format == "compact" {
                    let mut items = Vec::new();
                    for item in story_ids.iter().skip(start_index).take(limit) {
                        if let Some(id) = item.id {
                            let story = self.get_item(id).await?;
                            items.push(compact_story_payload(&story));
                        }
                    }

                    let payload = json!({
                        "feed": story_type,
                        "limit": limit,
                        "has_more": has_more,
                        "items": items
                    });
                    let text = serde_json::to_string(&payload)?;
                    return structured_result_with_text(&payload, Some(text));
                }

                let mut stories = Vec::new();
                for item in story_ids.iter().skip(start_index).take(limit) {
                    if let Some(id) = item.id {
                        let story = self.get_item(id).await?;
                        let payload = if response_format == "concise" {
                            story_item_to_concise_payload(&story)
                        } else {
                            let (story_fields, comment_fields) =
                                get_field_sets_for_format(&args, &response_format);
                            story_item_to_payload(&story, &story_fields, &comment_fields)
                        };
                        stories.push(payload);
                    }
                }

                let text = serde_json::to_string(&stories)?;
                Ok(structured_result_with_text(&stories, Some(text))?)
            }
            "get_thread" | "get" | "get_post" => {
                let id = resolve_hn_id(&args)?;
                let flatten = args
                    .get("flatten")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let response_format = if name == "get_thread" {
                    parse_hn_response_format(&args, "compact")
                } else {
                    parse_hn_response_format(&args, "concise")
                };
                let output_format = parse_output_format(&args)?;
                let max_comments = parse_limit_alias(&args, &["max_comments"], 20, 0, 500);

                // Use the Algolia items endpoint directly
                let url = format!("https://hn.algolia.com/api/v1/items/{}", id);
                let result = self.fetch_typed::<HackerNewsItem>(&url).await?;

                if output_format == OutputFormat::NormalizedV1 {
                    let story_id = result.id.unwrap_or(id);
                    let item_ref = hn_item_ref("story", story_id);
                    let canonical_url =
                        format!("https://news.ycombinator.com/item?id={}", story_id);
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    let mut relationships: Vec<Relationship> = Vec::new();

                    let body_text = result.text.as_deref().unwrap_or("");
                    let body_clean = html_to_text(body_text);
                    if !body_clean.is_empty() || result.title.as_deref().unwrap_or("").is_empty() {
                        let body_ref = format!("hackernews:story_body:{}", story_id);
                        blocks.push(ContentBlock {
                            block_ref: body_ref.clone(),
                            block_kind: "post_body".to_string(),
                            text: body_clean,
                            author: result.author.as_ref().map(|name| Author {
                                name: name.to_string(),
                                id: None,
                            }),
                            created_at: hn_created_at_item(&result),
                            reply_to: None,
                            position: None,
                            score: None,
                            attachments: Vec::new(),
                            metadata: None,
                        });
                        relationships.push(Relationship {
                            rel: "has_block".to_string(),
                            from: item_ref.clone(),
                            to: body_ref,
                        });
                    }

                    append_hn_comments(
                        &result,
                        story_id,
                        0,
                        &item_ref,
                        &mut blocks,
                        &mut relationships,
                    );

                    let item = ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "thread".to_string(),
                        canonical_url: Some(canonical_url),
                        title: result.title.clone().filter(|t| !t.is_empty()),
                        created_at: hn_created_at_item(&result),
                        source_updated_at: None,
                        authors: hn_author(result.author.as_ref()),
                        tags: Vec::new(),
                        metadata: Some(json!({
                            "points": result.points,
                            "url": result.url,
                            "story_id": result.story_id,
                        })),
                        blocks,
                        relationships,
                        truncation: None,
                    };

                    let normalized = NormalizedItemV1::new(
                        item,
                        Partial::complete(None),
                        Source::new("hackernews", "get"),
                    );
                    return structured_result(&normalized);
                }

                if response_format == "compact" {
                    let payload = compact_thread_payload(&result, max_comments);
                    let text = serde_json::to_string(&payload)?;
                    return structured_result_with_text(&payload, Some(text));
                }

                if response_format == "concise" {
                    if flatten {
                        // Concise + flatten: just text content as flat list
                        let mut flattened = vec![json!({
                            "title": result.title.clone().unwrap_or_default(),
                            "text": result.text.clone().unwrap_or_default()
                        })];
                        flatten_comments_concise(&result, &mut flattened);
                        let text = serde_json::to_string(&flattened)?;
                        Ok(structured_result_with_text(&flattened, Some(text))?)
                    } else {
                        // Concise nested
                        let payload = story_item_to_concise_payload(&result);
                        let text = serde_json::to_string(&payload)?;
                        Ok(structured_result_with_text(&payload, Some(text))?)
                    }
                } else {
                    // Detailed format (original behavior)
                    let (story_fields, comment_fields) =
                        get_field_sets_for_format(&args, &response_format);
                    if flatten {
                        let mut flattened_payload = Vec::new();
                        flattened_payload.push(story_as_comment_payload(&result, &comment_fields));
                        flatten_comment_values(&result, &comment_fields, &mut flattened_payload);
                        let text = serde_json::to_string(&flattened_payload)?;
                        Ok(structured_result_with_text(&flattened_payload, Some(text))?)
                    } else {
                        let payload =
                            story_item_to_payload(&result, &story_fields, &comment_fields);
                        let text = serde_json::to_string(&payload)?;
                        Ok(structured_result_with_text(&payload, Some(text))?)
                    }
                }
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompts not supported".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn sample_thread() -> HackerNewsItem {
        HackerNewsItem {
            id: Some(100),
            author: Some("pg".to_string()),
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            created_at_i: None,
            r#type: Some(ItemType::Story),
            text: Some("<p>Hello <i>HN</i></p>".to_string()),
            title: Some("Test thread".to_string()),
            url: Some("https://example.com".to_string()),
            points: Some(42),
            parent_id: None,
            story_id: None,
            options: None,
            children: Some(vec![
                HackerNewsItem {
                    id: Some(101),
                    author: Some("alice".to_string()),
                    created_at: Some("2026-01-01T00:01:00Z".to_string()),
                    created_at_i: None,
                    r#type: Some(ItemType::Comment),
                    text: Some("<p>First</p>".to_string()),
                    title: None,
                    url: None,
                    points: Some(3),
                    parent_id: Some(100),
                    story_id: Some(100),
                    options: None,
                    children: None,
                },
                HackerNewsItem {
                    id: Some(102),
                    author: Some("bob".to_string()),
                    created_at: Some("2026-01-01T00:02:00Z".to_string()),
                    created_at_i: None,
                    r#type: Some(ItemType::Comment),
                    text: Some("<p>Second</p>".to_string()),
                    title: None,
                    url: None,
                    points: Some(4),
                    parent_id: Some(100),
                    story_id: Some(100),
                    options: None,
                    children: Some(vec![HackerNewsItem {
                        id: Some(103),
                        author: Some("carol".to_string()),
                        created_at: Some("2026-01-01T00:03:00Z".to_string()),
                        created_at_i: None,
                        r#type: Some(ItemType::Comment),
                        text: Some("<p>Reply</p>".to_string()),
                        title: None,
                        url: None,
                        points: Some(1),
                        parent_id: Some(102),
                        story_id: Some(100),
                        options: None,
                        children: None,
                    }]),
                },
            ]),
        }
    }

    #[tokio::test]
    async fn rejects_invalid_proxy_url() {
        let mut connector = HackerNewsConnector::new();
        let mut details = AuthDetails::new();
        details.insert("proxy_url".to_string(), "not a url".to_string());

        let err =
            <HackerNewsConnector as crate::Connector>::set_auth_details(&mut connector, details)
                .await
                .unwrap_err();
        match err {
            ConnectorError::InvalidParams(_) => {}
            other => panic!("expected InvalidParams, got: {other:?}"),
        }
    }

    #[test]
    fn compact_thread_payload_limits_and_cleans_comments() {
        let payload = compact_thread_payload(&sample_thread(), 2);
        let obj = payload.as_object().expect("object");

        assert_eq!(obj.get("total_comments").and_then(Value::as_u64), Some(3));
        assert_eq!(
            obj.get("returned_comments").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(obj.get("truncated").and_then(Value::as_bool), Some(true));
        assert_eq!(obj.get("text").and_then(Value::as_str), Some("Hello HN"));

        let comments = obj
            .get("comments")
            .and_then(Value::as_array)
            .expect("comments array");
        assert_eq!(comments.len(), 2);
        assert_eq!(
            comments[0].get("text").and_then(Value::as_str),
            Some("First")
        );
        assert_eq!(
            comments[1].get("text").and_then(Value::as_str),
            Some("Second")
        );
    }

    #[test]
    fn resolve_hn_id_accepts_numeric_strings_and_aliases() {
        let args = json!({ "id": "47712656" })
            .as_object()
            .expect("args object")
            .clone();
        assert_eq!(resolve_hn_id(&args).expect("string id"), 47_712_656);

        let alias_args = json!({ "item_id": "47712656" })
            .as_object()
            .expect("args object")
            .clone();
        assert_eq!(
            resolve_hn_id(&alias_args).expect("item_id alias"),
            47_712_656
        );
    }

    #[test]
    fn parse_limit_alias_accepts_numeric_strings() {
        let args = json!({ "max_comments": "25" })
            .as_object()
            .expect("args object")
            .clone();
        assert_eq!(parse_limit_alias(&args, &["max_comments"], 20, 0, 500), 25);
    }

    #[tokio::test]
    async fn tool_schema_advertises_string_friendly_id_inputs() {
        let connector = HackerNewsConnector::new();
        let tools = connector.list_tools(None).await.expect("list tools").tools;

        for tool_name in ["get_thread", "get"] {
            let tool = tools
                .iter()
                .find(|tool| tool.name.as_ref() == tool_name)
                .unwrap_or_else(|| panic!("missing tool {tool_name}"));
            let props = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .expect("schema properties");

            let id_types = props
                .get("id")
                .and_then(|value| value.get("type"))
                .expect("id type");
            let item_id_types = props
                .get("item_id")
                .and_then(|value| value.get("type"))
                .expect("item_id type");

            let has_string_type = |value: &Value| {
                value
                    .as_array()
                    .is_some_and(|types| types.iter().any(|entry| entry.as_str() == Some("string")))
            };

            assert!(
                has_string_type(id_types),
                "{tool_name} id schema should accept strings"
            );
            assert!(
                has_string_type(item_id_types),
                "{tool_name} item_id schema should accept strings"
            );
        }
    }
}
