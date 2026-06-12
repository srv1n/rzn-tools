use crate::display::v1::{
    ActionV1, Diagnostic, DiagnosticLevel, DisplayBlockV1, DisplayItemSummaryV1, DisplayItemV1,
    DisplayMetaValue, DisplayPageV1, KeyValueItemV1, KeyValueKindV1, Partial as DisplayPartial,
    Source as DisplaySource,
};
use crate::error::ConnectorError;
use crate::ingest::{
    parse_item_ref, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1,
};
use rmcp::model::Meta;
use serde_json::{json, Value as JsonValue};
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

pub fn display_page_from_normalized_v1(page: &NormalizedPageV1) -> DisplayPageV1 {
    let items = page
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| item_summary_from_content_item(item, Some(idx)))
        .collect();

    let mut display = DisplayPageV1::new(items);
    display.source = Some(source_from_ingest(&page.source));
    display.partial = Some(partial_from_ingest(&page.partial));
    display.next_cursor = page.next_cursor.clone();
    display.has_more = Some(page.has_more);
    display.diagnostics = diagnostics_from_partial(&page.partial);
    display
}

pub fn display_item_from_normalized_v1(item: &NormalizedItemV1) -> DisplayItemV1 {
    let summary = item_summary_from_content_item(&item.item, None);
    let mut blocks = Vec::new();

    let mut kv_items = Vec::new();
    kv_items.push(KeyValueItemV1 {
        key: "kind".to_string(),
        value: item.item.kind.clone(),
        kind: Some(KeyValueKindV1::Text),
    });

    if let Some(url) = item.item.canonical_url.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "url".to_string(),
            value: url.clone(),
            kind: Some(KeyValueKindV1::Url),
        });
    }

    if let Some(created_at) = item.item.created_at.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "created_at".to_string(),
            value: created_at.clone(),
            kind: Some(KeyValueKindV1::Date),
        });
    }

    if let Some(updated_at) = item.item.source_updated_at.as_ref() {
        kv_items.push(KeyValueItemV1 {
            key: "source_updated_at".to_string(),
            value: updated_at.clone(),
            kind: Some(KeyValueKindV1::Date),
        });
    }

    if !item.item.authors.is_empty() {
        let authors = item
            .item
            .authors
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        kv_items.push(KeyValueItemV1 {
            key: "authors".to_string(),
            value: authors,
            kind: Some(KeyValueKindV1::Text),
        });
    }

    if !item.item.tags.is_empty() {
        kv_items.push(KeyValueItemV1 {
            key: "tags".to_string(),
            value: item.item.tags.join(", "),
            kind: Some(KeyValueKindV1::Badge),
        });
    }

    if let Some(meta) = item_meta_from_ingest_metadata(item.item.metadata.as_ref()) {
        for (key, value) in &meta {
            if kv_items.iter().any(|kv| kv.key == *key) {
                continue;
            }
            let (as_string, kind) = key_value_from_meta(value);
            if let Some(as_string) = as_string {
                kv_items.push(KeyValueItemV1 {
                    key: key.clone(),
                    value: as_string,
                    kind,
                });
            }
        }
    }

    if !kv_items.is_empty() {
        blocks.push(DisplayBlockV1::KeyValue {
            title: Some("Details".to_string()),
            items: kv_items,
        });
    }

    blocks.extend(blocks_from_content_item(&item.item));

    let mut display = DisplayItemV1::new(summary, blocks);
    display.source = Some(source_from_ingest(&item.source));
    display.partial = Some(partial_from_ingest(&item.partial));
    display.diagnostics = diagnostics_from_partial(&item.partial);
    if let Some(truncation) = item.item.truncation.as_ref() {
        display.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: format!(
                "Content truncated: {} (returned_blocks={}, total_blocks_hint={})",
                truncation.reason,
                truncation.returned_blocks,
                truncation
                    .total_blocks_hint
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            code: Some("TRUNCATED".to_string()),
        });
    }
    display
}

pub fn try_convert_normalized_structured_content_to_display_v1(
    value: &JsonValue,
) -> Result<Option<JsonValue>, ConnectorError> {
    let type_field = value.get("type").and_then(|v| v.as_str());
    match type_field {
        Some(crate::ingest::NORMALIZED_PAGE_V1_TYPE) => {
            let page: NormalizedPageV1 =
                serde_json::from_value(value.clone()).map_err(ConnectorError::SerdeJson)?;
            let display = display_page_from_normalized_v1(&page);
            Ok(Some(
                serde_json::to_value(display).map_err(ConnectorError::SerdeJson)?,
            ))
        }
        Some(crate::ingest::NORMALIZED_ITEM_V1_TYPE) => {
            let item: NormalizedItemV1 =
                serde_json::from_value(value.clone()).map_err(ConnectorError::SerdeJson)?;
            let display = display_item_from_normalized_v1(&item);
            Ok(Some(
                serde_json::to_value(display).map_err(ConnectorError::SerdeJson)?,
            ))
        }
        _ => Ok(None),
    }
}

fn diagnostics_from_partial(partial: &crate::ingest::Partial) -> Vec<Diagnostic> {
    if !partial.is_partial {
        return Vec::new();
    }
    let mut message = "Result is partial.".to_string();
    if let Some(reason) = partial.reason.as_ref() {
        message = format!("Result is partial: {}", reason);
    }
    vec![Diagnostic {
        level: DiagnosticLevel::Warning,
        message,
        code: Some("PARTIAL".to_string()),
    }]
}

fn source_from_ingest(source: &crate::ingest::Source) -> DisplaySource {
    DisplaySource {
        connector: source.connector.clone(),
        tool: source.tool.clone(),
        fetched_at: source.fetched_at.clone(),
    }
}

fn partial_from_ingest(partial: &crate::ingest::Partial) -> DisplayPartial {
    DisplayPartial {
        is_partial: partial.is_partial,
        reason: partial.reason.clone(),
        limits: partial.limits.clone(),
    }
}

fn item_summary_from_content_item(
    item: &ContentItem,
    index: Option<usize>,
) -> DisplayItemSummaryV1 {
    let id = item_id_from_content_item(item, index);

    let title = item
        .title
        .clone()
        .or_else(|| item.canonical_url.clone())
        .unwrap_or_else(|| id.clone());

    let subtitle = build_item_subtitle(item);
    let snippet = build_item_snippet(item);

    let url = item.canonical_url.clone();
    let mut badges = Vec::new();
    if let Some(parts) = parse_item_ref(&item.item_ref) {
        if !parts.connector.is_empty() {
            badges.push(parts.connector);
        }
    }

    let mut actions = Vec::new();
    if let Some(url) = url.as_ref() {
        actions.push(ActionV1::OpenUrl {
            label: "Open".to_string(),
            url: url.clone(),
        });
    }

    DisplayItemSummaryV1 {
        id,
        kind: Some(item.kind.clone()),
        title,
        subtitle,
        snippet,
        url,
        badges,
        meta: item_meta_from_ingest_metadata(item.metadata.as_ref()).unwrap_or_default(),
        actions,
    }
}

fn item_id_from_content_item(item: &ContentItem, index: Option<usize>) -> String {
    if !item.item_ref.is_empty() {
        return item.item_ref.clone();
    }
    if let Some(url) = item.canonical_url.as_ref() {
        return url.clone();
    }

    let mut parts = Vec::new();
    if let Some(title) = item.title.as_ref() {
        parts.push(format!("title={}", title));
    }
    if let Some(created_at) = item.created_at.as_ref() {
        parts.push(format!("created_at={}", created_at));
    }
    if let Some(updated_at) = item.source_updated_at.as_ref() {
        parts.push(format!("updated_at={}", updated_at));
    }
    if !item.authors.is_empty() {
        let names = item
            .authors
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("authors={}", names));
    }

    if parts.is_empty() {
        if let Some(idx) = index {
            return format!("{}:unknown:{}", item.kind, idx);
        }
        return format!("{}:unknown", item.kind);
    }

    let stable_key = parts.join("|");
    format!("{}:{:016x}", item.kind, fnv1a_64(&stable_key))
}

fn fnv1a_64(s: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn item_meta_from_ingest_metadata(
    metadata: Option<&JsonValue>,
) -> Option<BTreeMap<String, DisplayMetaValue>> {
    let JsonValue::Object(map) = metadata? else {
        return None;
    };

    let mut out = BTreeMap::new();
    for (key, value) in map {
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
            JsonValue::Null => {
                out.insert(key.clone(), DisplayMetaValue::Null);
            }
            JsonValue::Array(_) | JsonValue::Object(_) => {}
        }
    }
    Some(out)
}

fn key_value_from_meta(value: &DisplayMetaValue) -> (Option<String>, Option<KeyValueKindV1>) {
    match value {
        DisplayMetaValue::String(s) => (Some(s.clone()), Some(KeyValueKindV1::Text)),
        DisplayMetaValue::Number(n) => (Some(n.to_string()), Some(KeyValueKindV1::Number)),
        DisplayMetaValue::Bool(b) => (Some(b.to_string()), Some(KeyValueKindV1::Text)),
        DisplayMetaValue::Null => (None, None),
    }
}

fn blocks_from_content_item(item: &ContentItem) -> Vec<DisplayBlockV1> {
    let mut out = Vec::new();

    let attachments = collect_attachments(item);
    if !attachments.is_empty() {
        let items = attachments
            .into_iter()
            .map(|att| DisplayItemSummaryV1 {
                id: att
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("attachment:{}", att.kind)),
                kind: Some("attachment".to_string()),
                title: att.title.unwrap_or_else(|| att.kind.clone()),
                subtitle: Some(att.kind),
                snippet: None,
                url: att.url.clone(),
                badges: Vec::new(),
                meta: BTreeMap::new(),
                actions: att
                    .url
                    .map(|url| {
                        vec![ActionV1::OpenUrl {
                            label: "Open".to_string(),
                            url,
                        }]
                    })
                    .unwrap_or_default(),
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
        let items = item
            .blocks
            .iter()
            .map(item_summary_from_content_block)
            .collect::<Vec<_>>();
        out.push(DisplayBlockV1::List {
            title: Some("Messages".to_string()),
            items,
        });
        return out;
    }

    let (segments, other_blocks): (Vec<&ContentBlock>, Vec<&ContentBlock>) = item
        .blocks
        .iter()
        .partition(|b| parse_time_range_position(b.position.as_ref()).is_some());
    if segments.len() >= 2 {
        let markdown = markdown_from_block_refs(&other_blocks);
        if !markdown.trim().is_empty() {
            out.push(DisplayBlockV1::Markdown {
                title: Some("Content".to_string()),
                markdown,
            });
        }

        let title = if segments
            .iter()
            .any(|b| b.block_kind.to_lowercase().contains("transcript"))
        {
            "Transcript"
        } else {
            "Timeline"
        };
        out.push(DisplayBlockV1::List {
            title: Some(title.to_string()),
            items: segments
                .into_iter()
                .map(item_summary_from_time_range_block)
                .collect(),
        });
        return out;
    }

    let markdown = markdown_from_blocks(&item.blocks);
    if !markdown.trim().is_empty() {
        out.push(DisplayBlockV1::Markdown {
            title: Some("Content".to_string()),
            markdown,
        });
    }

    out
}

fn looks_like_thread(blocks: &[ContentBlock]) -> bool {
    if blocks.len() < 2 {
        return false;
    }
    let with_authors = blocks.iter().filter(|b| b.author.is_some()).count();
    with_authors >= 2
}

fn item_summary_from_content_block(block: &ContentBlock) -> DisplayItemSummaryV1 {
    let mut meta = BTreeMap::new();
    if let Some(reply_to) = block.reply_to.as_ref() {
        meta.insert(
            "reply_to".to_string(),
            DisplayMetaValue::String(reply_to.clone()),
        );
    }
    if let Some(score) = block.score {
        if let Some(num) = serde_json::Number::from_f64(score) {
            meta.insert("score".to_string(), DisplayMetaValue::Number(num));
        }
    }
    if !block.attachments.is_empty() {
        meta.insert(
            "attachments".to_string(),
            DisplayMetaValue::Number(serde_json::Number::from(block.attachments.len())),
        );
    }

    let title = block
        .author
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let subtitle = block.created_at.clone();
    let snippet = normalize_snippet(&block.text, 280);

    DisplayItemSummaryV1 {
        id: block.block_ref.clone(),
        kind: Some("message".to_string()),
        title,
        subtitle,
        snippet,
        url: None,
        badges: Vec::new(),
        meta,
        actions: Vec::new(),
    }
}

fn markdown_from_blocks(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        let mut header = String::new();
        if let Some(author) = block.author.as_ref() {
            header.push_str(&format!("**{}**", author.name));
        }
        if let Some(created_at) = block.created_at.as_ref() {
            if !header.is_empty() {
                header.push(' ');
            }
            header.push_str(&format!("({})", created_at));
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

fn markdown_from_block_refs(blocks: &[&ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        let mut header = String::new();
        if let Some(author) = block.author.as_ref() {
            header.push_str(&format!("**{}**", author.name));
        }
        if let Some(created_at) = block.created_at.as_ref() {
            if !header.is_empty() {
                header.push(' ');
            }
            header.push_str(&format!("({})", created_at));
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

fn parse_time_range_position(position: Option<&JsonValue>) -> Option<(u64, u64)> {
    let JsonValue::Object(obj) = position? else {
        return None;
    };
    if obj.get("kind").and_then(|v| v.as_str()) != Some("time_range") {
        return None;
    }
    let start_ms = obj.get("start_ms")?.as_u64()?;
    let end_ms = obj.get("end_ms")?.as_u64()?;
    Some((start_ms, end_ms))
}

fn item_summary_from_time_range_block(block: &ContentBlock) -> DisplayItemSummaryV1 {
    let (start_ms, end_ms) = parse_time_range_position(block.position.as_ref()).unwrap_or((0, 0));
    let mut meta = BTreeMap::new();
    meta.insert(
        "start_ms".to_string(),
        DisplayMetaValue::Number(serde_json::Number::from(start_ms)),
    );
    meta.insert(
        "end_ms".to_string(),
        DisplayMetaValue::Number(serde_json::Number::from(end_ms)),
    );

    let mut subtitle = None;
    if let Some(JsonValue::Object(m)) = block.metadata.as_ref() {
        if let Some(chapter) = m.get("chapter").and_then(|v| v.as_str()) {
            subtitle = Some(chapter.to_string());
            meta.insert(
                "chapter".to_string(),
                DisplayMetaValue::String(chapter.to_string()),
            );
        }
    }

    let title = format_time_range(start_ms, end_ms);
    let snippet = normalize_snippet(&block.text, 280);

    let mut actions = Vec::new();
    if !block.text.trim().is_empty() {
        actions.push(ActionV1::CopyText {
            label: "Copy segment".to_string(),
            text: block.text.clone(),
        });
    }

    DisplayItemSummaryV1 {
        id: block.block_ref.clone(),
        kind: Some("segment".to_string()),
        title,
        subtitle,
        snippet,
        url: None,
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

fn collect_attachments(item: &ContentItem) -> Vec<crate::ingest::Attachment> {
    let mut out = Vec::new();
    for block in &item.blocks {
        for att in &block.attachments {
            out.push(att.clone());
        }
    }
    out
}

fn build_item_subtitle(item: &ContentItem) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(url) = item.canonical_url.as_ref() {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                parts.push(host.to_string());
            }
        }
    }

    if !item.authors.is_empty() {
        let author = item.authors[0].name.clone();
        if item.authors.len() > 1 {
            parts.push(format!("{} et al.", author));
        } else {
            parts.push(author);
        }
    }

    if let Some(created_at) = item.created_at.as_ref() {
        parts.push(short_date(created_at));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

fn build_item_snippet(item: &ContentItem) -> Option<String> {
    for block in &item.blocks {
        if let Some(snippet) = normalize_snippet(&block.text, 280) {
            if !snippet.trim().is_empty() {
                return Some(snippet);
            }
        }
    }
    None
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

#[allow(dead_code)]
fn raw_json_block(title: &str, value: &JsonValue) -> DisplayBlockV1 {
    let code = serde_json::to_string_pretty(value).unwrap_or_else(|_| json!({}).to_string());
    DisplayBlockV1::Code {
        title: Some(title.to_string()),
        language: Some("json".to_string()),
        code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1};

    #[test]
    fn converts_normalized_page_to_display_page() {
        let item = ContentItem {
            item_ref: "test:thread:1".to_string(),
            kind: "thread".to_string(),
            canonical_url: Some("https://example.com/thread/1".to_string()),
            title: Some("Hello".to_string()),
            created_at: Some("2025-01-01T00:00:00Z".to_string()),
            source_updated_at: None,
            authors: vec![Author {
                name: "alice".to_string(),
                id: None,
            }],
            tags: vec!["tag1".to_string()],
            metadata: None,
            blocks: vec![ContentBlock {
                block_ref: "test:block:1".to_string(),
                block_kind: "text".to_string(),
                text: "First post".to_string(),
                author: None,
                created_at: None,
                reply_to: None,
                position: None,
                score: None,
                attachments: Vec::new(),
                metadata: None,
            }],
            relationships: Vec::new(),
            truncation: None,
        };
        let page = NormalizedPageV1::new(
            vec![item],
            Some("cursor".to_string()),
            true,
            crate::ingest::Partial::complete(None),
            crate::ingest::Source::new("test", "search"),
        );
        let display = display_page_from_normalized_v1(&page);
        assert_eq!(display.type_field, crate::display::v1::DISPLAY_PAGE_V1_TYPE);
        assert_eq!(display.items.len(), 1);
        assert_eq!(display.next_cursor.as_deref(), Some("cursor"));
        assert_eq!(display.has_more, Some(true));
    }

    #[test]
    fn converts_normalized_item_to_display_item() {
        let item = ContentItem {
            item_ref: "test:thread:1".to_string(),
            kind: "thread".to_string(),
            canonical_url: None,
            title: Some("Hello".to_string()),
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: Vec::new(),
            metadata: None,
            blocks: vec![ContentBlock {
                block_ref: "test:block:1".to_string(),
                block_kind: "text".to_string(),
                text: "First post".to_string(),
                author: Some(Author {
                    name: "alice".to_string(),
                    id: None,
                }),
                created_at: Some("2025-01-01T00:00:00Z".to_string()),
                reply_to: None,
                position: None,
                score: Some(12.0),
                attachments: Vec::new(),
                metadata: None,
            }],
            relationships: Vec::new(),
            truncation: None,
        };
        let normalized =
            NormalizedItemV1::complete(item, crate::ingest::Source::new("test", "get"));
        let display = display_item_from_normalized_v1(&normalized);
        assert_eq!(display.type_field, crate::display::v1::DISPLAY_ITEM_V1_TYPE);
        assert_eq!(display.item.title, "Hello");
        assert!(!display.blocks.is_empty());
    }

    #[test]
    fn includes_ingest_metadata_in_summary_meta() {
        let item = ContentItem {
            item_ref: "test:video:1".to_string(),
            kind: "video".to_string(),
            canonical_url: Some("https://example.com/v/1".to_string()),
            title: Some("Hello".to_string()),
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: Vec::new(),
            metadata: Some(json!({
                "views": 123,
                "verified": true,
                "nested": { "x": 1 },
                "arr": [1, 2, 3],
                "nullish": null
            })),
            blocks: Vec::new(),
            relationships: Vec::new(),
            truncation: None,
        };
        let page = NormalizedPageV1::new(
            vec![item],
            None,
            false,
            crate::ingest::Partial::complete(None),
            crate::ingest::Source::new("test", "search"),
        );
        let display = display_page_from_normalized_v1(&page);
        let meta = &display.items[0].meta;
        assert!(matches!(
            meta.get("views"),
            Some(DisplayMetaValue::Number(_))
        ));
        assert_eq!(meta.get("verified"), Some(&DisplayMetaValue::Bool(true)));
        assert!(meta.contains_key("nullish"));
        assert!(!meta.contains_key("nested"));
        assert!(!meta.contains_key("arr"));
    }

    #[test]
    fn fallback_ids_do_not_collide_in_pages() {
        let item1 = ContentItem {
            item_ref: String::new(),
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
        let item2 = ContentItem {
            item_ref: String::new(),
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
            vec![item1, item2],
            None,
            false,
            crate::ingest::Partial::complete(None),
            crate::ingest::Source::new("test", "search"),
        );
        let display = display_page_from_normalized_v1(&page);
        assert_ne!(display.items[0].id, display.items[1].id);
    }

    #[test]
    fn time_range_blocks_render_as_list() {
        let item = ContentItem {
            item_ref: "test:video:1".to_string(),
            kind: "video".to_string(),
            canonical_url: None,
            title: Some("Hello".to_string()),
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: Vec::new(),
            metadata: None,
            blocks: vec![
                ContentBlock {
                    block_ref: "seg1".to_string(),
                    block_kind: "transcript_segment".to_string(),
                    text: "First part".to_string(),
                    author: None,
                    created_at: None,
                    reply_to: None,
                    position: Some(json!({
                        "kind": "time_range",
                        "start_ms": 0,
                        "end_ms": 1000
                    })),
                    score: None,
                    attachments: Vec::new(),
                    metadata: Some(json!({"chapter":"Intro"})),
                },
                ContentBlock {
                    block_ref: "seg2".to_string(),
                    block_kind: "transcript_segment".to_string(),
                    text: "Second part".to_string(),
                    author: None,
                    created_at: None,
                    reply_to: None,
                    position: Some(json!({
                        "kind": "time_range",
                        "start_ms": 1000,
                        "end_ms": 2000
                    })),
                    score: None,
                    attachments: Vec::new(),
                    metadata: None,
                },
            ],
            relationships: Vec::new(),
            truncation: None,
        };
        let normalized =
            NormalizedItemV1::complete(item, crate::ingest::Source::new("test", "get"));
        let display = display_item_from_normalized_v1(&normalized);
        assert!(display.blocks.iter().any(|b| matches!(b, DisplayBlockV1::List { title, items } if title.as_deref() == Some("Transcript") && items.len() == 2)));
    }

    #[test]
    fn converts_normalized_structured_content_json() {
        let item = ContentItem {
            item_ref: "test:thread:1".to_string(),
            kind: "thread".to_string(),
            canonical_url: None,
            title: Some("Hello".to_string()),
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: Vec::new(),
            metadata: None,
            blocks: Vec::new(),
            relationships: Vec::new(),
            truncation: None,
        };
        let normalized =
            NormalizedItemV1::complete(item, crate::ingest::Source::new("test", "get"));
        let value = serde_json::to_value(normalized).expect("serialize");
        let converted = try_convert_normalized_structured_content_to_display_v1(&value)
            .expect("convert")
            .expect("some");
        assert_eq!(
            converted.get("type").and_then(|v| v.as_str()),
            Some(crate::display::v1::DISPLAY_ITEM_V1_TYPE)
        );
    }
}
