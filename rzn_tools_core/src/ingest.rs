use crate::error::ConnectorError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

pub const NORMALIZED_PAGE_V1_TYPE: &str = "rzn-tools.normalized_page.v1";
pub const NORMALIZED_ITEM_V1_TYPE: &str = "rzn-tools.normalized_item.v1";

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Raw,
    NormalizedV1,
    DisplayV1,
}

impl OutputFormat {
    pub fn is_normalized(self) -> bool {
        matches!(self, OutputFormat::NormalizedV1)
    }

    pub fn is_display(self) -> bool {
        matches!(self, OutputFormat::DisplayV1)
    }
}

pub fn output_format_from_args(
    args: &JsonMap<String, JsonValue>,
) -> Result<OutputFormat, ConnectorError> {
    match args.get("output_format") {
        None => Ok(OutputFormat::Raw),
        Some(value) => match value.as_str() {
            Some("raw") => Ok(OutputFormat::Raw),
            Some("normalized_v1") => Ok(OutputFormat::NormalizedV1),
            Some("display_v1") => Ok(OutputFormat::DisplayV1),
            Some(other) => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'output_format': '{}'. Expected 'raw', 'normalized_v1', or 'display_v1'.",
                other
            ))),
            None => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'output_format': expected a string ('raw', 'normalized_v1', or 'display_v1'), got {}",
                value
            ))),
        },
    }
}

pub fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ConnectorError> {
    let bytes = serde_json::to_vec(cursor).map_err(|e| ConnectorError::Other(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Option<T> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRefParts {
    pub connector: String,
    pub kind: String,
    pub id: String,
}

/// Parse an item_ref of the form "{connector}:{kind}:{id}".
/// Uses splitn(3) so that the id segment may contain ':' characters.
pub fn parse_item_ref(item_ref: &str) -> Option<ItemRefParts> {
    let mut parts = item_ref.splitn(3, ':');
    let connector = parts.next()?.trim();
    let kind = parts.next()?.trim();
    let id = parts.next()?.trim();
    if connector.is_empty() || kind.is_empty() || id.is_empty() {
        return None;
    }
    Some(ItemRefParts {
        connector: connector.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
    })
}

/// Parse an item_ref and ensure it matches the expected connector.
pub fn parse_item_ref_for_connector(item_ref: &str, connector: &str) -> Option<(String, String)> {
    let parts = parse_item_ref(item_ref)?;
    if parts.connector != connector {
        return None;
    }
    Some((parts.kind, parts.id))
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Partial {
    pub is_partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<JsonValue>,
}

impl Partial {
    pub fn complete(limits: Option<JsonValue>) -> Self {
        Self {
            is_partial: false,
            reason: None,
            limits,
        }
    }

    pub fn truncated(reason: impl Into<String>, limits: Option<JsonValue>) -> Self {
        Self {
            is_partial: true,
            reason: Some(reason.into()),
            limits,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Source {
    pub connector: String,
    pub tool: String,
    pub fetched_at: String,
}

impl Source {
    pub fn new(connector: impl Into<String>, tool: impl Into<String>) -> Self {
        Self {
            connector: connector.into(),
            tool: tool.into(),
            fetched_at: now_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attachment {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Truncation {
    pub is_truncated: bool,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_blocks_hint: Option<u64>,
    pub returned_blocks: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Relationship {
    pub rel: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContentBlock {
    pub block_ref: String,
    pub block_kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContentItem {
    pub item_ref: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    pub blocks: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalizedPageV1 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub items: Vec<ContentItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub partial: Partial,
    pub source: Source,
}

impl NormalizedPageV1 {
    pub fn new(
        items: Vec<ContentItem>,
        next_cursor: Option<String>,
        has_more: bool,
        partial: Partial,
        source: Source,
    ) -> Self {
        Self {
            type_field: NORMALIZED_PAGE_V1_TYPE.to_string(),
            items,
            next_cursor,
            has_more,
            partial,
            source,
        }
    }

    pub fn with_limits(self, limits: JsonValue) -> Self {
        Self {
            partial: Partial::complete(Some(limits)),
            ..self
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalizedItemV1 {
    #[serde(rename = "type")]
    pub type_field: String,
    pub item: ContentItem,
    pub partial: Partial,
    pub source: Source,
}

impl NormalizedItemV1 {
    pub fn new(item: ContentItem, partial: Partial, source: Source) -> Self {
        Self {
            type_field: NORMALIZED_ITEM_V1_TYPE.to_string(),
            item,
            partial,
            source,
        }
    }

    pub fn complete(item: ContentItem, source: Source) -> Self {
        Self::new(item, Partial::complete(None), source)
    }

    pub fn truncated(
        item: ContentItem,
        reason: impl Into<String>,
        limits: Option<JsonValue>,
        source: Source,
    ) -> Self {
        Self::new(item, Partial::truncated(reason, limits), source)
    }
}

pub fn limits_max_items(max_items: u64) -> JsonValue {
    json!({ "max_items": max_items })
}

pub fn limits_max_blocks(max_blocks: u64) -> JsonValue {
    json!({ "max_blocks_per_item": max_blocks })
}

pub fn limits_window_size(window_size: u64) -> JsonValue {
    json!({ "window_size": window_size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_parses() {
        let mut args = JsonMap::new();
        assert_eq!(output_format_from_args(&args).unwrap(), OutputFormat::Raw);
        args.insert(
            "output_format".to_string(),
            JsonValue::String("normalized_v1".to_string()),
        );
        assert_eq!(
            output_format_from_args(&args).unwrap(),
            OutputFormat::NormalizedV1
        );
        args.insert(
            "output_format".to_string(),
            JsonValue::String("display_v1".to_string()),
        );
        assert_eq!(
            output_format_from_args(&args).unwrap(),
            OutputFormat::DisplayV1
        );
    }

    #[test]
    fn output_format_rejects_non_string() {
        let mut args = JsonMap::new();
        args.insert(
            "output_format".to_string(),
            json!({ "value": "normalized_v1" }),
        );
        let err = output_format_from_args(&args).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidParams(_)));
    }

    #[test]
    fn cursor_round_trip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Cursor {
            after: String,
            count: usize,
        }

        let cursor = Cursor {
            after: "t3_abc".to_string(),
            count: 42,
        };
        let encoded = encode_cursor(&cursor).expect("encode");
        let decoded: Cursor = decode_cursor(&encoded).expect("decode");
        assert_eq!(cursor, decoded);
    }

    #[test]
    fn normalized_page_serializes() {
        let item = ContentItem {
            item_ref: "x:item:1".to_string(),
            kind: "thread".to_string(),
            canonical_url: None,
            title: None,
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: Vec::new(),
            metadata: None,
            blocks: Vec::new(),
            relationships: Vec::new(),
            truncation: None,
        };
        let page = NormalizedPageV1::new(
            vec![item],
            None,
            false,
            Partial::complete(None),
            Source::new("test", "list"),
        );
        let value = serde_json::to_value(page).expect("serialize");
        assert_eq!(
            value.get("type").and_then(|v| v.as_str()),
            Some(NORMALIZED_PAGE_V1_TYPE)
        );
    }
}
