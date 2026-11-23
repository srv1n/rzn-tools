use crate::display::v1::{
    ActionV1, Diagnostic, DiagnosticLevel, DisplayBlockV1, DisplayItemSummaryV1, DisplayItemV1,
    DisplayMetaValue, DisplayPageV1, KeyValueItemV1, KeyValueKindV1,
};
use crate::error::ConnectorError;
use crate::ingest::parse_item_ref;
use crate::item::{At, Block, Item, ItemsPage, ITEMS_V2_TYPE, ITEM_V2_TYPE};
use rmcp::model::Meta;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

pub const META_ORIGINAL_STRUCTURED_CONTENT_KEY: &str = "rzn_tools_original_structured_content";
pub const META_ORIGINAL_OUTPUT_FORMAT_KEY: &str = "rzn_tools_original_output_format";
pub const META_DISPLAY_CONVERTED_KEY: &str = "rzn_tools_display_converted";

pub fn stash_original_structured_content_in_meta(
    meta: &mut Option<Meta>,
    original_structured: &JsonValue,
    original_output_format: &str,
) {
    let mut meta_obj = meta.take().unwrap_or_default();
    meta_obj
        .0
        .entry(META_ORIGINAL_STRUCTURED_CONTENT_KEY.to_string())
        .or_insert_with(|| original_structured.clone());
    meta_obj
        .0
        .entry(META_ORIGINAL_OUTPUT_FORMAT_KEY.to_string())
        .or_insert_with(|| JsonValue::String(original_output_format.to_string()));
    meta_obj
        .0
        .entry(META_DISPLAY_CONVERTED_KEY.to_string())
        .or_insert(JsonValue::Bool(true));
    *meta = Some(meta_obj);
}

pub fn display_page_from_items(page: &ItemsPage) -> DisplayPageV1 {
    let items = page
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| item_summary(item, Some(idx)))
        .collect();

    let mut display = DisplayPageV1::new(items);
    display.next_cursor = page.next_cursor.clone();
    display.has_more = Some(page.next_cursor.is_some());
    display
}

pub fn display_item_from_item(item: &Item) -> DisplayItemV1 {
    let summary = item_summary(item, None);
    let mut blocks = Vec::new();

    let mut kv_items = vec![KeyValueItemV1 {
        key: "kind".to_string(),
        value: item.kind.clone(),
        kind: Some(KeyValueKindV1::Text),
    }];
    if !item.url.is_empty() {
        kv_items.push(KeyValueItemV1 {
            key: "url".to_string(),
            value: item.url.clone(),
            kind: Some(KeyValueKindV1::Url),
        });
    }
    if let Some(published_at) = item.published_at.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "published_at".to_string(),
            value: published_at.clone(),
            kind: Some(KeyValueKindV1::Date),
        });
    }
    if let Some(author) = item.author.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "author".to_string(),
            value: author.name.clone(),
            kind: Some(KeyValueKindV1::Text),
        });
    }
    if let Some(place) = item.published_in.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "published_in".to_string(),
            value: place.name.clone(),
            kind: Some(KeyValueKindV1::Text),
        });
    }
    for (key, value) in item_meta(item) {
        if kv_items.iter().any(|kv| kv.key == key) {
            continue;
        }
        let (as_string, kind) = key_value_from_meta(&value);
        if let Some(as_string) = as_string {
            kv_items.push(KeyValueItemV1 {
                key,
                value: as_string,
                kind,
            });
        }
    }
    blocks.push(DisplayBlockV1::KeyValue {
        title: Some("Details".to_string()),
        items: kv_items,
    });

    blocks.extend(blocks_from_item(item));

    let mut display = DisplayItemV1::new(summary, blocks);
    if let Some(truncated) = item.truncated.as_ref() {
        display.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: format!(
                "Content truncated: {} (returned_blocks={}, total_blocks_hint={})",
                truncated.reason,
                truncated.returned_blocks,
                truncated
                    .total_blocks_hint
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            code: Some("TRUNCATED".to_string()),
        });
    }
    display
}

pub fn try_convert_item_structured_content_to_display_v1(
    value: &JsonValue,
) -> Result<Option<JsonValue>, ConnectorError> {
    match value.get("type").and_then(|v| v.as_str()) {
        Some(ITEMS_V2_TYPE) => {
            let page: ItemsPage =
                serde_json::from_value(value.clone()).map_err(ConnectorError::SerdeJson)?;
            let display = display_page_from_items(&page);
            Ok(Some(
                serde_json::to_value(display).map_err(ConnectorError::SerdeJson)?,
            ))
        }
        Some(ITEM_V2_TYPE) => {
            let item: Item =
                serde_json::from_value(value.clone()).map_err(ConnectorError::SerdeJson)?;
            let display = display_item_from_item(&item);
            Ok(Some(
                serde_json::to_value(display).map_err(ConnectorError::SerdeJson)?,
            ))
        }
        _ => Ok(None),
    }
}

fn item_summary(item: &Item, index: Option<usize>) -> DisplayItemSummaryV1 {
    let id = if item.key.is_empty() {
        format!("{}:unknown:{}", item.kind, index.unwrap_or(0))
    } else {
        item.key.clone()
    };
    let url = (!item.url.is_empty()).then(|| item.url.clone());
    let title = item
        .title
        .clone()
        .or_else(|| url.clone())
        .unwrap_or_else(|| id.clone());
    let mut badges = Vec::new();
    if let Some(parts) = parse_item_ref(&item.key) {
        badges.push(parts.connector);
    }
    let actions = url
        .iter()
        .map(|url| ActionV1::OpenUrl {
            label: "Open".to_string(),
            url: url.clone(),
        })
        .collect();

    DisplayItemSummaryV1 {
        id,
        kind: Some(item.kind.clone()),
        title,
        subtitle: build_item_subtitle(item),
        snippet: build_item_snippet(item),
        url,
        badges,
        meta: item_meta(item),
        actions,
    }
}

/// Scalar fields and counts, for display.
fn item_meta(item: &Item) -> BTreeMap<String, DisplayMetaValue> {
    let mut out = BTreeMap::new();
    for (key, value) in &item.fields {
        match value {
            JsonValue::String(s) => {
                out.insert(key.clone(), DisplayMetaValue::String(s.clone()));
            }
            JsonValue::Number(n) => {
                out.insert(key.clone(), DisplayMetaValue::Number(n.clone()));
            }
            JsonValue::Bool(b) => {
                out.insert(key.clone(), DisplayMetaValue::Bool(*b));
            }
            _ => {}
        }
    }
    for (key, value) in &item.counts {
        out.insert(key.clone(), DisplayMetaValue::Number(value.clone()));
    }
    out
}

fn key_value_from_meta(value: &DisplayMetaValue) -> (Option<String>, Option<KeyValueKindV1>) {
    match value {
        DisplayMetaValue::String(s) => (Some(s.clone()), Some(KeyValueKindV1::Text)),
        DisplayMetaValue::Number(n) => (Some(n.to_string()), Some(KeyValueKindV1::Number)),
        DisplayMetaValue::Bool(b) => (Some(b.to_string()), Some(KeyValueKindV1::Text)),
        DisplayMetaValue::Null => (None, None),
    }
}

fn blocks_from_item(item: &Item) -> Vec<DisplayBlockV1> {
    let mut out = Vec::new();

    let attachments: Vec<_> = item
        .blocks
        .iter()
        .flat_map(|b| b.attachments.iter())
        .collect();
    if !attachments.is_empty() {
        let items = attachments
            .into_iter()
            .map(|att| DisplayItemSummaryV1 {
                id: att
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("attachment:{}", att.kind)),
                kind: Some("attachment".to_string()),
                title: att
                    .title
                    .clone()
                    .or_else(|| att.alt.clone())
                    .unwrap_or_else(|| att.kind.clone()),
                subtitle: Some(att.kind.clone()),
                snippet: None,
                url: att.url.clone(),
                badges: Vec::new(),
                meta: BTreeMap::new(),
                actions: att
                    .url
                    .iter()
                    .map(|url| ActionV1::OpenUrl {
                        label: "Open".to_string(),
                        url: url.clone(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        out.push(DisplayBlockV1::List {
            title: Some("Attachments".to_string()),
            items,
        });
    }

    if item.blocks.is_empty() {
        return out;
    }

    if looks_like_thread(&item.blocks) {
        let items = item.blocks.iter().map(item_summary_from_block).collect();
        out.push(DisplayBlockV1::List {
            title: Some("Messages".to_string()),
            items,
        });
        return out;
    }

    let (segments, other_blocks): (Vec<&Block>, Vec<&Block>) = item
        .blocks
        .iter()
        .partition(|b| matches!(b.at, Some(At::Time { .. })));
    if segments.len() >= 2 {
        let markdown = markdown_from_blocks(&other_blocks);
        if !markdown.trim().is_empty() {
            out.push(DisplayBlockV1::Markdown {
                title: Some("Content".to_string()),
                markdown,
            });
        }
        let title = if segments.iter().any(|b| b.kind == "transcript") {
            "Transcript"
        } else {
            "Timeline"
        };
        out.push(DisplayBlockV1::List {
            title: Some(title.to_string()),
            items: segments
                .into_iter()
                .map(item_summary_from_time_block)
                .collect(),
        });
        return out;
    }

    let all: Vec<&Block> = item.blocks.iter().collect();
    let markdown = markdown_from_blocks(&all);
    if !markdown.trim().is_empty() {
        out.push(DisplayBlockV1::Markdown {
            title: Some("Content".to_string()),
            markdown,
        });
    }
    out
}

fn looks_like_thread(blocks: &[Block]) -> bool {
    blocks.len() >= 2 && blocks.iter().filter(|b| b.author.is_some()).count() >= 2
}

fn item_summary_from_block(block: &Block) -> DisplayItemSummaryV1 {
    let mut meta = BTreeMap::new();
    if let Some(parent) = block.parent.as_ref() {
        meta.insert(
            "reply_to".to_string(),
            DisplayMetaValue::String(parent.clone()),
        );
    }
    for (key, value) in &block.counts {
        meta.insert(key.clone(), DisplayMetaValue::Number(value.clone()));
    }
    if !block.attachments.is_empty() {
        meta.insert(
            "attachments".to_string(),
            DisplayMetaValue::Number(serde_json::Number::from(block.attachments.len())),
        );
    }
    DisplayItemSummaryV1 {
        id: block.id.clone(),
        kind: Some("message".to_string()),
        title: block
            .author
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        subtitle: block.published_at.clone(),
        snippet: normalize_snippet(&block.text, 280),
        url: block.url.clone(),
        badges: Vec::new(),
        meta,
        actions: Vec::new(),
    }
}

fn markdown_from_blocks(blocks: &[&Block]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        let mut header = String::new();
        if let Some(author) = block.author.as_ref() {
            header.push_str(&format!("**{}**", author.name));
        }
        if let Some(published_at) = block.published_at.as_ref() {
            if !header.is_empty() {
                header.push(' ');
            }
            header.push_str(&format!("({})", published_at));
        }
        if let Some(heading) = block.path.last() {
            if header.is_empty() {
                header = format!("### {heading}");
            }
        }
        let text = block.text.trim();
        if header.is_empty() {
            parts.push(text.to_string());
        } else if text.is_empty() {
            parts.push(header);
        } else {
            parts.push(format!("{}\n\n{}", header, text));
        }
    }
    parts.join("\n\n---\n\n")
}

fn item_summary_from_time_block(block: &Block) -> DisplayItemSummaryV1 {
    let (start_ms, end_ms) = match block.at {
        Some(At::Time { start_ms, end_ms }) => (start_ms, end_ms),
        _ => (0, 0),
    };
    let mut meta = BTreeMap::new();
    meta.insert(
        "start_ms".to_string(),
        DisplayMetaValue::Number(serde_json::Number::from(start_ms)),
    );
    meta.insert(
        "end_ms".to_string(),
        DisplayMetaValue::Number(serde_json::Number::from(end_ms)),
    );
    let subtitle = block.path.last().cloned();
    if let Some(chapter) = subtitle.as_ref() {
        meta.insert(
            "chapter".to_string(),
            DisplayMetaValue::String(chapter.clone()),
        );
    }
    let mut actions = Vec::new();
    if !block.text.trim().is_empty() {
        actions.push(ActionV1::CopyText {
            label: "Copy segment".to_string(),
            text: block.text.clone(),
        });
    }
    DisplayItemSummaryV1 {
        id: block.id.clone(),
        kind: Some("segment".to_string()),
        title: format_time_range(start_ms, end_ms),
        subtitle,
        snippet: normalize_snippet(&block.text, 280),
        url: block.url.clone(),
        badges: Vec::new(),
        meta,
        actions,
    }
}

fn format_time_range(start_ms: u64, end_ms: u64) -> String {
    if end_ms <= start_ms {
        format_time_ms(start_ms)
    } else {
        format!("{} – {}", format_time_ms(start_ms), format_time_ms(end_ms))
    }
}

fn format_time_ms(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

fn build_item_subtitle(item: &Item) -> Option<String> {
    let mut parts = Vec::new();
    if let Ok(parsed) = url::Url::parse(&item.url) {
        if let Some(host) = parsed.host_str() {
            parts.push(host.to_string());
        }
    }
    if let Some(author) = item.author.as_ref() {
        parts.push(author.name.clone());
    }
    if let Some(published_at) = item.published_at.as_ref() {
        parts.push(short_date(published_at));
    }
    (!parts.is_empty()).then(|| parts.join(" • "))
}

fn build_item_snippet(item: &Item) -> Option<String> {
    item.fields
        .get("description")
        .and_then(JsonValue::as_str)
        .into_iter()
        .chain(item.blocks.iter().map(|block| block.text.as_str()))
        .find_map(|text| normalize_snippet(text, 280))
}

fn short_date(rfc3339: &str) -> String {
    let s = rfc3339.trim();
    if s.len() >= 10 {
        s[0..10].to_string()
    } else {
        s.to_string()
    }
}

fn normalize_snippet(text: &str, max_chars: usize) -> Option<String> {
    let collapsed = text
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.chars().count() <= max_chars {
        return Some(collapsed);
    }
    let truncated = collapsed.chars().take(max_chars).collect::<String>();
    Some(format!("{}…", truncated.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Listing, Party};
    use serde_json::json;

    fn thread() -> Item {
        let mut item = Item::new("test:thread:1", "thread", "https://example.com/thread/1")
            .with_title("Hello")
            .with_author(Party::named("alice"))
            .with_published_at(Some("2025-01-01T00:00:00Z".into()))
            .with_published_in(Party::named("Example"))
            .with_fields(json!({"views": 123, "verified": true}));
        let mut first = Block::new("b1", "post", "First post");
        first.author = Some(Party::named("alice"));
        let mut second = Block::new("b2", "comment", "Reply");
        second.author = Some(Party::named("bob"));
        second.parent = Some("b1".into());
        item.blocks = vec![first, second];
        item
    }

    #[test]
    fn converts_page_to_display_page() {
        let page = ItemsPage::new(vec![thread()], Some("cursor".into()), Listing::Feed);
        let display = display_page_from_items(&page);
        assert_eq!(display.type_field, crate::display::v1::DISPLAY_PAGE_V1_TYPE);
        assert_eq!(display.items.len(), 1);
        assert_eq!(display.next_cursor.as_deref(), Some("cursor"));
        assert_eq!(display.has_more, Some(true));
        assert!(matches!(
            display.items[0].meta.get("views"),
            Some(DisplayMetaValue::Number(_))
        ));
        assert_eq!(
            display.items[0].meta.get("verified"),
            Some(&DisplayMetaValue::Bool(true))
        );
    }

    #[test]
    fn converts_thread_item_to_messages() {
        let display = display_item_from_item(&thread());
        assert_eq!(display.type_field, crate::display::v1::DISPLAY_ITEM_V1_TYPE);
        assert_eq!(display.item.title, "Hello");
        assert!(display.blocks.iter().any(|b| matches!(b, DisplayBlockV1::List { title, items } if title.as_deref() == Some("Messages") && items.len() == 2)));
    }

    #[test]
    fn time_range_blocks_render_as_transcript() {
        let mut item = thread();
        item.blocks = (0..2)
            .map(|i| {
                let mut block = Block::new(format!("t{i}"), "transcript", "part");
                block.at = Some(At::Time {
                    start_ms: i * 1000,
                    end_ms: i * 1000 + 1000,
                });
                block.path = vec!["Intro".into()];
                block
            })
            .collect();
        let display = display_item_from_item(&item);
        assert!(display.blocks.iter().any(|b| matches!(b, DisplayBlockV1::List { title, items } if title.as_deref() == Some("Transcript") && items.len() == 2)));
    }

    #[test]
    fn converts_structured_content_json() {
        let value = serde_json::to_value(thread()).unwrap();
        let converted = try_convert_item_structured_content_to_display_v1(&value)
            .expect("convert")
            .expect("some");
        assert_eq!(
            converted.get("type").and_then(|v| v.as_str()),
            Some(crate::display::v1::DISPLAY_ITEM_V1_TYPE)
        );
        assert!(
            try_convert_item_structured_content_to_display_v1(&json!({"x": 1}))
                .unwrap()
                .is_none()
        );
    }
}
