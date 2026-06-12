use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

pub const DISPLAY_PAGE_V1_TYPE: &str = "rzn-tools.display_page.v1";
pub const DISPLAY_ITEM_V1_TYPE: &str = "rzn-tools.display_item.v1";

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct Partial {
    pub is_partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct Source {
    pub connector: String,
    pub tool: String,
    pub fetched_at: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionV1 {
    OpenUrl {
        label: String,
        url: String,
    },
    OpenUri {
        label: String,
        uri: String,
    },
    OpenPath {
        label: String,
        path: String,
    },
    CopyText {
        label: String,
        text: String,
    },
    RunTool {
        label: String,
        tool_id: String,
        #[serde(default, skip_serializing_if = "JsonValue::is_null")]
        args: JsonValue,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum DisplayMetaValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct DisplayItemSummaryV1 {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, DisplayMetaValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionV1>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyValueKindV1 {
    Text,
    Url,
    Date,
    Number,
    Badge,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct KeyValueItemV1 {
    pub key: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<KeyValueKindV1>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKindV1 {
    Image,
    Video,
    Audio,
    File,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum TableCellValueV1 {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct TableColumnV1 {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DisplayBlockV1 {
    Markdown {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        markdown: String,
    },
    Text {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        text: String,
    },
    KeyValue {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<KeyValueItemV1>,
    },
    List {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<DisplayItemSummaryV1>,
    },
    Table {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        columns: Vec<TableColumnV1>,
        rows: Vec<BTreeMap<String, TableCellValueV1>>,
    },
    Media {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        media_kind: MediaKindV1,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thumbnail_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    Code {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        code: String,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct DisplayPageV1 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<Partial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    pub items: Vec<DisplayItemSummaryV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<DisplayBlockV1>,
}

impl DisplayPageV1 {
    pub fn new(items: Vec<DisplayItemSummaryV1>) -> Self {
        Self {
            type_field: DISPLAY_PAGE_V1_TYPE.to_string(),
            title: None,
            subtitle: None,
            source: None,
            partial: None,
            diagnostics: Vec::new(),
            items,
            next_cursor: None,
            has_more: None,
            blocks: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
pub struct DisplayItemV1 {
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<Partial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    pub item: DisplayItemSummaryV1,
    pub blocks: Vec<DisplayBlockV1>,
}

impl DisplayItemV1 {
    pub fn new(item: DisplayItemSummaryV1, blocks: Vec<DisplayBlockV1>) -> Self {
        Self {
            type_field: DISPLAY_ITEM_V1_TYPE.to_string(),
            title: None,
            subtitle: None,
            source: None,
            partial: None,
            diagnostics: Vec::new(),
            item,
            blocks,
        }
    }
}
