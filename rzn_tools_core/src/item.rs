//! `rzn.item/v2`: the one shape every non-file source delivers to an index.
//!
//! An item is one video, post thread, web page, paper or message thread: who made it, when, where it
//! came out, the facts the source knows about it, the numbers that keep changing, and one flat
//! ordered list of blocks (or, for document-like sources, a pointer to bytes). The design is
//! `docs/plans/any-source/design.md` in the backend repository.
//!
//! MCP carries this as `structuredContent`; called as a library the same structs are handed over.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value as JsonValue};
use std::collections::{BTreeMap, HashSet};

pub const ITEM_V2_TYPE: &str = "rzn.item/v2";
pub const ITEMS_V2_TYPE: &str = "rzn.items/v2";
pub const DESCRIBE_V2_TYPE: &str = "rzn.describe/v2";

/// Facts the source knows about an item or a block. A value is a string, number, boolean, date
/// (an RFC 3339 string) or a list of them.
pub type Fields = BTreeMap<String, JsonValue>;

/// Numbers that keep changing (views, likes, score). Kept outside the content hash.
pub type Counts = BTreeMap<String, Number>;

/// A person, account or place: an author, a channel, a subreddit, a journal, a website.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Party {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Party {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            url: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// Where a block sits inside its item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum At {
    /// A time range in milliseconds (transcripts, media).
    Time { start_ms: u64, end_ms: u64 },
    /// A character span in the item's text.
    Chars { start: u64, end: u64 },
    /// Position only.
    Order(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Attachment {
    /// image, video, audio, document, link.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// One unit of an item's text. Items hold a flat, ordered list; a reply names what it answers in
/// `parent`, headings above the block go in `path`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Block {
    pub id: String,
    /// transcript, paragraph, post, comment, quoted, description, entry, ...
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Set only when it differs from the item's author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Party>,
    /// Set only when it differs from the item's date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: Fields,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counts: Counts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

impl Block {
    pub fn new(id: impl Into<String>, kind: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            text: text.into(),
            ..Self::default()
        }
    }
}

/// Bytes for a document-like source; they go through the file reader for `media_type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentRef {
    pub uri: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Set when the connector returned fewer blocks than the item has. An index refuses such an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Truncated {
    pub reason: String,
    pub returned_blocks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_blocks_hint: Option<u64>,
}

fn item_type() -> String {
    ITEM_V2_TYPE.to_string()
}

fn items_type() -> String {
    ITEMS_V2_TYPE.to_string()
}

fn describe_type() -> String {
    DESCRIBE_V2_TYPE.to_string()
}

/// `rzn.item/v2`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    #[serde(rename = "type", default = "item_type")]
    #[schemars(extend("const" = "rzn.item/v2"))]
    pub type_field: String,
    /// Identity, one rule per item kind: `youtube:video:<id>`, `x:post:<first post id>`, a URL.
    pub key: String,
    /// video, page, post, thread, paper, entry, ...
    pub kind: String,
    /// Canonical URL; what a citation opens.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Required on a full item; a listing entry may leave it for `get` to fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Party>,
    /// RFC 3339. Required on a full item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// The place the item came out in: the channel, subreddit, journal or website. Required on a
    /// full item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_in: Option<Party>,
    /// Cheap change signal from the listing (an etag, an edit count, an update time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: Fields,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counts: Counts,
    /// Empty in a listing entry; `get` fills it. Never set together with `document`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<Truncated>,
}

impl Item {
    /// A listing entry or the head of a full item. Full items also need an author, a date and a
    /// place of publication.
    pub fn new(key: impl Into<String>, kind: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            type_field: item_type(),
            key: key.into(),
            kind: kind.into(),
            url: url.into(),
            title: None,
            author: None,
            published_at: None,
            published_in: None,
            marker: None,
            fields: Fields::new(),
            counts: Counts::new(),
            blocks: Vec::new(),
            document: None,
            truncated: None,
        }
    }

    pub fn with_author(mut self, author: Party) -> Self {
        if !author.name.trim().is_empty() {
            self.author = Some(author);
        }
        self
    }

    pub fn with_published_at(mut self, published_at: Option<String>) -> Self {
        self.published_at = published_at.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn with_published_in(mut self, published_in: Party) -> Self {
        if !published_in.name.trim().is_empty() {
            self.published_in = Some(published_in);
        }
        self
    }

    pub fn with_marker(mut self, marker: Option<String>) -> Self {
        self.marker = marker.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        let title = title.into();
        if !title.trim().is_empty() {
            self.title = Some(title);
        }
        self
    }

    /// Merge object entries into `fields`, dropping nulls and empty strings.
    pub fn with_fields(mut self, fields: JsonValue) -> Self {
        merge_fields(&mut self.fields, fields);
        self
    }

    /// Merge object entries into `counts`, keeping only numbers.
    pub fn with_counts(mut self, counts: JsonValue) -> Self {
        merge_counts(&mut self.counts, counts);
        self
    }

    /// Checks a full item (from `get` or a push): the listing checks plus a body, unique block ids,
    /// replies that point backwards, and a time range on every transcript block.
    pub fn check(&self) -> Result<(), String> {
        self.check_listed()?;
        match &self.author {
            Some(author) if !author.name.trim().is_empty() => {}
            _ => return Err("author is missing".into()),
        }
        match &self.published_in {
            Some(place) if !place.name.trim().is_empty() => {}
            _ => return Err("published_in is missing".into()),
        }
        if self.published_at.is_none() {
            return Err("published_at is missing".into());
        }
        match (self.blocks.is_empty(), self.document.is_some()) {
            (false, true) => return Err("an item has blocks or a document, not both".into()),
            (true, false) => return Err("an item needs blocks or a document".into()),
            _ => {}
        }
        if let Some(truncated) = &self.truncated {
            return Err(format!("item is truncated: {}", truncated.reason));
        }
        let mut seen = HashSet::new();
        for block in &self.blocks {
            if block.id.trim().is_empty() {
                return Err("a block has an empty id".into());
            }
            if block.kind.trim().is_empty() {
                return Err(format!("block {} has no kind", block.id));
            }
            if let Some(parent) = &block.parent {
                if !seen.contains(parent.as_str()) {
                    return Err(format!(
                        "block {} names parent {parent}, which is not an earlier block",
                        block.id
                    ));
                }
            }
            if !seen.insert(block.id.as_str()) {
                return Err(format!("block id {} repeats", block.id));
            }
            match block.at {
                Some(At::Time { start_ms, end_ms }) if end_ms < start_ms => {
                    return Err(format!("block {} ends before it starts", block.id));
                }
                Some(At::Chars { start, end }) if end < start => {
                    return Err(format!("block {} ends before it starts", block.id));
                }
                Some(At::Time { .. }) => {}
                _ if block.kind == "transcript" => {
                    return Err(format!("transcript block {} has no time range", block.id));
                }
                _ => {}
            }
            if let Some(date) = &block.published_at {
                check_date(date).map_err(|error| format!("block {}: {error}", block.id))?;
            }
            check_fields(&block.fields).map_err(|error| format!("block {}: {error}", block.id))?;
        }
        Ok(())
    }

    /// Checks what a listing entry must carry: the type and the identity; and any date it gives.
    pub fn check_listed(&self) -> Result<(), String> {
        if self.type_field != ITEM_V2_TYPE {
            return Err(format!(
                "type is {}, expected {ITEM_V2_TYPE}",
                self.type_field
            ));
        }
        for (name, value) in [("key", &self.key), ("kind", &self.kind), ("url", &self.url)] {
            if value.trim().is_empty() {
                return Err(format!("{name} is empty"));
            }
        }
        if let Some(date) = &self.published_at {
            check_date(date)?;
        }
        check_fields(&self.fields)
    }
}

/// A page of items from a list call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ItemsPage {
    #[serde(rename = "type", default = "items_type")]
    #[schemars(extend("const" = "rzn.items/v2"))]
    pub type_field: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    pub listing: Listing,
}

impl ItemsPage {
    pub fn new(items: Vec<Item>, next_cursor: Option<String>, listing: Listing) -> Self {
        Self {
            type_field: items_type(),
            items,
            next_cursor,
            listing,
        }
    }
}

/// `full`: the pages name everything in scope, so a key missing from a completed listing was
/// deleted. `feed`: they do not (a search, a front page), so absence means nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Listing {
    Full,
    Feed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldOf {
    Item,
    Passage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Number,
    Date,
    Boolean,
}

/// One field a connector exposes. `of` is a fact about the source; `changes: true` marks a count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FieldDescription {
    pub name: String,
    pub of: FieldOf,
    #[serde(rename = "type")]
    pub value_type: FieldType,
    #[serde(default)]
    pub list: bool,
    #[serde(default)]
    pub changes: bool,
}

impl FieldDescription {
    pub fn item(name: &str, value_type: FieldType) -> Self {
        Self {
            name: name.to_string(),
            of: FieldOf::Item,
            value_type,
            list: false,
            changes: false,
        }
    }

    pub fn passage(name: &str, value_type: FieldType) -> Self {
        Self {
            of: FieldOf::Passage,
            ..Self::item(name, value_type)
        }
    }

    pub fn list(mut self) -> Self {
        self.list = true;
        self
    }

    pub fn count(mut self) -> Self {
        self.changes = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KindDescription {
    pub kind: String,
    /// The identity rule, for example `youtube:video:<video id>`.
    pub identity: String,
    pub listing: Listing,
    /// Scope arguments the list tool accepts (channel, playlist, subreddit, ...).
    pub scopes: Vec<String>,
    /// The tool that lists items and the tool that fetches one.
    pub list_tool: String,
    pub get_tool: String,
    pub fields: Vec<FieldDescription>,
}

/// What a connector exposes, returned once by its `describe` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Description {
    #[serde(rename = "type", default = "describe_type")]
    #[schemars(extend("const" = "rzn.describe/v2"))]
    pub type_field: String,
    pub connector: String,
    pub kinds: Vec<KindDescription>,
}

impl Description {
    pub fn new(connector: &str, kinds: Vec<KindDescription>) -> Self {
        Self {
            type_field: describe_type(),
            connector: connector.to_string(),
            kinds,
        }
    }
}

/// The fields every item carries, which a description lists first.
pub fn shared_item_fields() -> Vec<FieldDescription> {
    vec![
        FieldDescription::item("title", FieldType::String),
        FieldDescription::item("author", FieldType::String),
        FieldDescription::item("published_at", FieldType::Date),
        FieldDescription::item("published_in", FieldType::String),
        FieldDescription::item("kind", FieldType::String),
    ]
}

/// The schema of a full item: a listing entry's schema with author, date and place required.
pub fn item_schema() -> JsonValue {
    let mut schema = serde_json::to_value(schemars::schema_for!(Item)).unwrap_or(JsonValue::Null);
    if let Some(required) = schema.get_mut("required").and_then(JsonValue::as_array_mut) {
        for name in ["author", "published_at", "published_in"] {
            if !required.iter().any(|value| value == name) {
                required.push(JsonValue::String(name.to_string()));
            }
        }
    }
    schema
}

pub fn items_page_schema() -> JsonValue {
    serde_json::to_value(schemars::schema_for!(ItemsPage)).unwrap_or(JsonValue::Null)
}

pub fn description_schema() -> JsonValue {
    serde_json::to_value(schemars::schema_for!(Description)).unwrap_or(JsonValue::Null)
}

pub fn merge_fields(fields: &mut Fields, value: JsonValue) {
    let JsonValue::Object(map) = value else {
        return;
    };
    for (key, value) in map {
        match value {
            JsonValue::Null => {}
            JsonValue::String(s) if s.trim().is_empty() => {}
            JsonValue::Array(values) => {
                let values: Vec<JsonValue> = values
                    .into_iter()
                    .filter(|v| {
                        matches!(
                            v,
                            JsonValue::String(_) | JsonValue::Number(_) | JsonValue::Bool(_)
                        )
                    })
                    .collect();
                if !values.is_empty() {
                    fields.insert(key, JsonValue::Array(values));
                }
            }
            JsonValue::Object(_) => {}
            other => {
                fields.insert(key, other);
            }
        }
    }
}

pub fn merge_counts(counts: &mut Counts, value: JsonValue) {
    let JsonValue::Object(map) = value else {
        return;
    };
    for (key, value) in map {
        if let JsonValue::Number(n) = value {
            counts.insert(key, n);
        }
    }
}

/// Fields from a JSON object literal, dropping nulls, empty strings and nested objects.
pub fn fields(value: JsonValue) -> Fields {
    let mut out = Fields::new();
    merge_fields(&mut out, value);
    out
}

/// Counts from a JSON object literal, keeping only numbers.
pub fn counts(value: JsonValue) -> Counts {
    let mut out = Counts::new();
    merge_counts(&mut out, value);
    out
}

/// A source date as RFC 3339: accepts RFC 3339, RFC 2822, `YYYY-MM-DD` and `YYYY-MM-DD HH:MM:SS`
/// (taken as UTC), and Unix seconds.
pub fn rfc3339(value: &str) -> Option<String> {
    use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(value) {
        return Some(dt.to_rfc3339());
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(value, format) {
            return Some(Utc.from_utc_datetime(&dt).to_rfc3339());
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|dt| Utc.from_utc_datetime(&dt).to_rfc3339());
    }
    if let Ok(seconds) = value.parse::<i64>() {
        return Utc
            .timestamp_opt(seconds, 0)
            .single()
            .map(|dt| dt.to_rfc3339());
    }
    None
}

fn check_date(value: &str) -> Result<(), String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| format!("published_at {value:?} is not an RFC 3339 date"))
}

fn check_fields(fields: &Fields) -> Result<(), String> {
    for (name, value) in fields {
        let ok = match value {
            JsonValue::String(_) | JsonValue::Number(_) | JsonValue::Bool(_) => true,
            JsonValue::Array(values) => values.iter().all(|v| {
                matches!(
                    v,
                    JsonValue::String(_) | JsonValue::Number(_) | JsonValue::Bool(_)
                )
            }),
            _ => false,
        };
        if !ok {
            return Err(format!(
                "field {name} is not a string, number, boolean or list of them"
            ));
        }
    }
    Ok(())
}

/// Read `structuredContent` as an item, refusing anything that is not a full `rzn.item/v2`.
pub fn item_from_value(value: JsonValue) -> Result<Item, String> {
    let item: Item = serde_json::from_value(value).map_err(|e| e.to_string())?;
    item.check()?;
    Ok(item)
}

/// Read `structuredContent` as a page, checking each listing entry.
pub fn items_page_from_value(value: JsonValue) -> Result<ItemsPage, String> {
    let page: ItemsPage = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if page.type_field != ITEMS_V2_TYPE {
        return Err(format!(
            "type is {}, expected {ITEMS_V2_TYPE}",
            page.type_field
        ));
    }
    for item in &page.items {
        item.check_listed()
            .map_err(|e| format!("{}: {e}", item.key))?;
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn video() -> Item {
        let mut item = Item::new(
            "youtube:video:abc",
            "video",
            "https://www.youtube.com/watch?v=abc",
        )
        .with_author(Party::named("Channel").with_id("UC1"))
        .with_published_at(Some("2026-03-01T10:00:00Z".into()))
        .with_published_in(Party::named("Channel").with_id("UC1"))
        .with_title("A video")
        .with_fields(json!({"duration_s": 90, "tags": ["sleep"], "empty": "", "none": null}))
        .with_counts(json!({"views": 12, "label": "x"}));
        let mut block = Block::new("t-0", "transcript", "hello");
        block.path = vec!["Intro".into()];
        block.at = Some(At::Time {
            start_ms: 0,
            end_ms: 1000,
        });
        item.blocks.push(block);
        item
    }

    #[test]
    fn item_serializes_to_the_documented_shape() {
        let value = serde_json::to_value(video()).unwrap();
        assert_eq!(value["type"], ITEM_V2_TYPE);
        assert_eq!(
            value["blocks"][0]["at"],
            json!({"time": {"start_ms": 0, "end_ms": 1000}})
        );
        assert_eq!(
            value["fields"],
            json!({"duration_s": 90, "tags": ["sleep"]})
        );
        assert_eq!(value["counts"], json!({"views": 12}));
        assert!(item_from_value(value).is_ok());
    }

    #[test]
    fn transcript_block_without_time_is_refused() {
        let mut item = video();
        item.blocks[0].at = Some(At::Order(0));
        assert!(item.check().unwrap_err().contains("no time range"));
    }

    #[test]
    fn blocks_and_document_are_exclusive() {
        let mut item = video();
        item.document = Some(DocumentRef {
            uri: "https://example.com".into(),
            media_type: "text/html".into(),
            sha256: None,
        });
        assert!(item.check().is_err());
        item.blocks.clear();
        assert!(item.check().is_ok());
        item.document = None;
        assert!(item.check().is_err());
    }

    #[test]
    fn reply_must_point_backwards() {
        let mut item = video();
        let mut reply = Block::new("c-1", "comment", "hi");
        reply.parent = Some("c-9".into());
        item.blocks.push(reply);
        assert!(item.check().unwrap_err().contains("not an earlier block"));
    }

    #[test]
    fn listing_entry_needs_shared_fields() {
        let mut item = video();
        item.blocks.clear();
        item.author = None;
        assert!(item.check_listed().is_ok());
        item.blocks.push(Block::new("p", "paragraph", "x"));
        assert!(item.check().unwrap_err().contains("author"));
        item.published_at = Some("yesterday".into());
        assert!(item.check_listed().is_err());
    }

    #[test]
    fn page_and_description_round_trip() {
        let mut entry = video();
        entry.blocks.clear();
        let page = ItemsPage::new(vec![entry], Some("c2".into()), Listing::Full);
        let value = serde_json::to_value(&page).unwrap();
        assert_eq!(value["listing"], "full");
        assert!(items_page_from_value(value).is_ok());

        let description = Description::new(
            "youtube",
            vec![KindDescription {
                kind: "video".into(),
                identity: "youtube:video:<video id>".into(),
                listing: Listing::Full,
                scopes: vec!["playlist".into()],
                list_tool: "list".into(),
                get_tool: "get".into(),
                fields: vec![FieldDescription::item("views", FieldType::Number).count()],
            }],
        );
        let value = serde_json::to_value(&description).unwrap();
        assert_eq!(
            value["kinds"][0]["fields"][0],
            json!({"name": "views", "of": "item", "type": "number", "list": false, "changes": true})
        );
    }

    #[test]
    fn dates_normalize_to_rfc3339() {
        assert_eq!(
            rfc3339("2026-01-02").as_deref(),
            Some("2026-01-02T00:00:00+00:00")
        );
        assert_eq!(
            rfc3339("Tue, 1 Jul 2003 10:52:37 +0200").as_deref(),
            Some("2003-07-01T10:52:37+02:00")
        );
        assert_eq!(
            rfc3339("1700000000").as_deref(),
            Some("2023-11-14T22:13:20+00:00")
        );
        assert!(rfc3339("yesterday").is_none());
    }

    #[test]
    fn schemas_name_the_required_fields() {
        let schema = item_schema();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for name in [
            "key",
            "kind",
            "url",
            "author",
            "published_at",
            "published_in",
        ] {
            assert!(required.contains(&name), "{name} not required");
        }
        assert!(items_page_schema()["required"].as_array().is_some());
        assert!(description_schema()["required"].as_array().is_some());
    }
}
