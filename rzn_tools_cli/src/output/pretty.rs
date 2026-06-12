//! Pretty formatter for terminal output.
//!
//! Design principles (from clig.dev and best CLI tools):
//! - Whitespace & breathing room between logical groups
//! - Visual hierarchy: bold titles, dimmed metadata, colored links
//! - Card-like grouping for results
//! - Important info at the end (eye settles there)
//! - Truncate long text, don't wrap endlessly

use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, ContentArrangement, Table};
use owo_colors::OwoColorize;
use serde_json::Value;
use std::collections::HashSet;

/// Terminal width for formatting (default fallback)
const DEFAULT_WIDTH: usize = 80;

/// Indent for card content (after number)
const CARD_INDENT: usize = 6;

/// Known keys that typically contain result lists
const LIST_KEYS: &[&str] = &[
    "results",
    "articles",
    "items",
    "entries",
    "documents",
    "records",
    "posts",
    "stories",
    "videos",
    "papers",
    "messages",
    "hits",
    "search_results",
    "data",
    "content",
    "files",
    "repositories",
    "issues",
    "comments",
    "users",
    "channels",
    "events",
    "sections", // Document structure sections
];

/// Keys to show as primary (title-like)
const TITLE_KEYS: &[&str] = &["title", "name", "subject", "headline", "query"];

/// Keys to show as links
const URL_KEYS: &[&str] = &["url", "link", "href", "uri", "permalink"];

/// Keys to show as descriptions/snippets
const SNIPPET_KEYS: &[&str] = &[
    "snippet",
    "description",
    "summary",
    "highlights", // Exa search highlights (array of strings)
    "abstract",
    "excerpt",
    "text",
    "body",
    "content",
    "preview", // Document section previews
];

/// Keys for metadata (shown dimmed) - ordered by importance
const META_KEYS: &[&str] = &[
    // Identity fields first (critical for follow-up actions)
    "uid", // IMAP message UID - critical for get_message
    "id",  // Generic ID field
    // Sender/recipient info
    "from",
    "to",
    "author",
    "authors",
    "user",
    "username",
    "channel_name",
    "by",
    // Date/time fields
    "date",
    "internal_date", // IMAP internal date
    "published",
    "publishedDate", // Exa format
    "crawlDate",     // Exa format
    "created_at",
    "updated_at",
    "uploaded_at",
    "time",
    "timestamp",
    // Stats and metadata
    "size", // Message size
    "views",
    "score",
    "points",
    "comment_count",
    "total_comments",
    "returned_comments",
    "rating",
    "count",
    "domain", // Exa domain field
    "source",
    "message_id", // Email Message-ID header
];

// ============================================================================
// Public API
// ============================================================================

/// Format JSON value with card-like readable output
pub fn format_pretty(value: &Value) -> String {
    let mut output = String::new();
    let width = terminal_width();
    format_value(value, &mut output, width, 0);
    output
}

/// Format a list of items as cards (for search results, etc.)
pub fn format_cards(items: &[Value], source_label: Option<&str>) -> String {
    let mut output = String::new();
    let width = terminal_width();

    if let Some(source) = source_label {
        output.push_str(&format_section_header(source, Some(items.len()), width));
        output.push('\n');
    }

    for (i, item) in items.iter().enumerate() {
        output.push_str(&format_card(item, i + 1, width));
        if i < items.len() - 1 {
            output.push('\n');
        }
    }

    output
}

// ============================================================================
// Core Formatting
// ============================================================================

fn format_value(value: &Value, output: &mut String, width: usize, depth: usize) {
    match value {
        Value::Array(arr) if !arr.is_empty() => {
            // Check if array contains objects (structured data)
            if arr.iter().any(|v| v.is_object()) {
                output.push_str(&format_cards(arr, None));
            } else {
                // Simple array - show as bullet list
                for item in arr {
                    output.push_str(&format!("  {} {}\n", "•".dimmed(), format_scalar(item)));
                }
            }
        }
        Value::Object(obj) => {
            // Check if there's a list key we should extract
            if let Some((list_key, list_value)) = find_list_in_object(obj) {
                // Show top-level metadata first (skip redundant fields)
                let file_type = obj.get("file_type").and_then(|v| v.as_str());
                let skip_keys: &[&str] = match (list_key, file_type) {
                    ("sections", Some("epub")) => &["total_chapters", "total_pages"],
                    ("sections", Some("pdf")) => &["total_chapters", "total_pages"],
                    _ => &[],
                };

                let metadata: Vec<_> = obj
                    .iter()
                    .filter(|(k, v)| {
                        *k != list_key
                            && !v.is_array()
                            && !v.is_object()
                            && !skip_keys.contains(&k.as_str())
                    })
                    .collect();

                if !metadata.is_empty() {
                    for (key, val) in &metadata {
                        output.push_str(&format!("{}: {}\n", key.dimmed(), format_scalar(val)));
                    }
                    output.push('\n');
                }

                // Use context-aware label for document structures
                let label = match (list_key, file_type) {
                    ("sections", Some("epub")) => "chapters",
                    ("sections", Some("pdf")) => "pages",
                    ("sections", Some("markdown" | "docx" | "html")) => "headings",
                    ("sections", Some("code")) => "definitions",
                    _ => list_key,
                };

                // Show the list as cards
                output.push_str(&format_cards(list_value, Some(label)));
            } else {
                // No list found - format as key-value pairs with hierarchy
                format_object_hierarchical(obj, output, width, depth);
            }
        }
        _ => {
            // Scalar value
            output.push_str(&format_scalar(value));
        }
    }
}

fn format_object_hierarchical(
    obj: &serde_json::Map<String, Value>,
    output: &mut String,
    width: usize,
    depth: usize,
) {
    let indent = "  ".repeat(depth);

    // Separate into categories
    let mut scalars: Vec<(&String, &Value)> = Vec::new();
    let mut arrays: Vec<(&String, &Value)> = Vec::new();
    let mut objects: Vec<(&String, &Value)> = Vec::new();

    for (key, value) in obj {
        match value {
            Value::Array(_) => arrays.push((key, value)),
            Value::Object(_) => objects.push((key, value)),
            _ => scalars.push((key, value)),
        }
    }

    // Show scalars as key-value pairs
    for (key, value) in &scalars {
        let formatted_key = if TITLE_KEYS.contains(&key.as_str()) {
            key.bold().to_string()
        } else {
            key.dimmed().to_string()
        };

        let formatted_val = if URL_KEYS.contains(&key.as_str()) {
            format_scalar(value).blue().to_string()
        } else {
            format_scalar(value)
        };

        output.push_str(&format!("{}{}: {}\n", indent, formatted_key, formatted_val));
    }

    // Show arrays
    for (key, value) in &arrays {
        if let Value::Array(arr) = value {
            output.push('\n');
            output.push_str(&format!(
                "{}{} ({} items):\n",
                indent,
                key.cyan().bold(),
                arr.len()
            ));
            for item in arr {
                if item.is_object() {
                    output.push_str(&format_card(item, 0, width));
                } else {
                    output.push_str(&format!(
                        "{}  {} {}\n",
                        indent,
                        "•".dimmed(),
                        format_scalar(item)
                    ));
                }
            }
        }
    }

    // Show nested objects
    for (key, value) in &objects {
        if let Value::Object(nested) = value {
            output.push('\n');
            output.push_str(&format!("{}{}:\n", indent, key.cyan().bold()));
            format_object_hierarchical(nested, output, width, depth + 1);
        }
    }
}

// ============================================================================
// Card Formatting (the main visual pattern)
// ============================================================================

fn format_card(item: &Value, index: usize, width: usize) -> String {
    let mut output = String::new();

    // Calculate content width for wrapping (not truncation)
    let content_width = width.saturating_sub(CARD_INDENT + 2);
    let _ = content_width; // Used for future text wrapping

    let obj = match item.as_object() {
        Some(o) => o,
        None => {
            // Not an object, just format as scalar
            if index > 0 {
                output.push_str(&format!(
                    " {:>3}. {}\n",
                    index.to_string().cyan().bold(),
                    format_scalar(item)
                ));
            } else {
                output.push_str(&format!("   {} {}\n", "•".dimmed(), format_scalar(item)));
            }
            return output;
        }
    };

    // Handle tagged enum pattern: {"type": "...", "data": {...}}
    // Unwrap the inner data object if present
    let obj = if obj.contains_key("type") && obj.contains_key("data") {
        if let Some(Value::Object(inner)) = obj.get("data") {
            inner
        } else {
            obj
        }
    } else {
        obj
    };

    // Extract key fields
    let title = find_field(obj, TITLE_KEYS);
    let url = find_field(obj, URL_KEYS);
    let snippet = find_field(obj, SNIPPET_KEYS);
    let meta_fields = extract_meta_fields(obj);

    // Line 1: Index + Title
    let index_str = if index > 0 {
        format!(" {:>3}. ", index).cyan().bold().to_string()
    } else {
        "      ".to_string()
    };

    let rendered_title = if let Some(t) = &title {
        output.push_str(&format!("{}{}\n", index_str, t.bold()));
        Some(t.clone())
    } else {
        // No title - show first available string field
        if let Some(first_str) = obj.values().find_map(|v| v.as_str()) {
            output.push_str(&format!("{}{}\n", index_str, first_str.bold()));
            Some(first_str.to_string())
        } else {
            output.push_str(&format!("{}(no title)\n", index_str));
            None
        }
    };

    // URL: full clickable hyperlink
    if let Some(u) = &url {
        let hyperlink = format_hyperlink(u, u);
        output.push_str(&format!("      {}\n", hyperlink.blue()));
    }

    // Snippet/description: show full content
    if let Some(s) = &snippet {
        let clean = clean_snippet(s);
        if !clean.is_empty() && rendered_title.as_deref() != Some(clean.as_str()) {
            output.push_str(&format!("      {}\n", clean.dimmed()));
        }
    }

    // Metadata: show each field on its own line for readability
    if !meta_fields.is_empty() {
        for (key, value) in meta_fields.iter().take(6) {
            output.push_str(&format!("      {}: {}\n", key.dimmed(), value.dimmed()));
        }
    }

    output
}

fn find_field(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(val) = obj.get(*key) {
            // Handle string values
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
            // Handle array values (e.g., Exa's highlights field)
            if let Some(arr) = val.as_array() {
                let strings: Vec<&str> = arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !strings.is_empty() {
                    // Join multiple highlights with separator
                    return Some(strings.join(" ... "));
                }
            }
        }
    }
    None
}

fn extract_meta_fields(obj: &serde_json::Map<String, Value>) -> Vec<(String, String)> {
    let mut meta = Vec::new();

    for key in META_KEYS {
        if let Some(val) = obj.get(*key) {
            let formatted = match val {
                Value::String(s) if !s.is_empty() => {
                    // For ISO dates, show just the date part (YYYY-MM-DD)
                    if s.len() > 10 && s.contains('T') {
                        s.split('T').next().unwrap_or(s).to_string()
                    } else {
                        s.clone()
                    }
                }
                Value::Number(n) => n.to_string(),
                Value::Array(arr) => {
                    // For arrays like "authors", join all items
                    let items: Vec<_> = arr.iter().filter_map(|v| v.as_str()).collect();
                    items.join(", ")
                }
                _ => continue,
            };

            if !formatted.is_empty() {
                meta.push((key.to_string(), formatted));
            }
        }
    }

    meta
}

// ============================================================================
// Section Headers
// ============================================================================

fn format_section_header(label: &str, count: Option<usize>, width: usize) -> String {
    let count_str = match count {
        Some(n) => format!(" ({} results)", n),
        None => String::new(),
    };

    let header_text = format!("{}{}", label, count_str);
    let line_len = (width.saturating_sub(header_text.len() + 4)).min(60);
    let line = "─".repeat(line_len);

    format!(
        "{} {} {}",
        "──".cyan(),
        header_text.green().bold(),
        line.cyan()
    )
}

// ============================================================================
// Table Formatting (for structured columnar data)
// ============================================================================

/// Format as a table (when data is highly structured and columnar)
#[allow(dead_code)]
pub fn format_as_table(items: &[Value], columns: Option<&[&str]>) -> String {
    if items.is_empty() {
        return "(empty)\n".dimmed().to_string();
    }

    let objects: Vec<&serde_json::Map<String, Value>> =
        items.iter().filter_map(|v| v.as_object()).collect();

    if objects.is_empty() {
        // Not objects - fall back to card format
        return format_cards(items, None);
    }

    // Determine columns
    let all_keys: HashSet<&str> = objects
        .iter()
        .flat_map(|obj| obj.keys().map(|k| k.as_str()))
        .collect();

    let selected_columns: Vec<&str> = if let Some(cols) = columns {
        cols.iter()
            .filter(|c| all_keys.contains(*c))
            .copied()
            .collect()
    } else {
        // Auto-select: prioritize title/name, then other useful fields
        let mut cols = Vec::new();
        for key in TITLE_KEYS
            .iter()
            .chain(URL_KEYS.iter())
            .chain(META_KEYS.iter())
        {
            if all_keys.contains(key) && cols.len() < 5 {
                cols.push(*key);
            }
        }
        cols
    };

    if selected_columns.is_empty() {
        return format_cards(items, None);
    }

    // Build table
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Header
    let headers: Vec<Cell> = selected_columns
        .iter()
        .map(|col| Cell::new(col.cyan().bold().to_string()))
        .collect();
    table.set_header(headers);

    // Rows
    for obj in objects.iter().take(50) {
        let cells: Vec<Cell> = selected_columns
            .iter()
            .map(|col| {
                let val = obj.get(*col).unwrap_or(&Value::Null);
                Cell::new(format_cell_value(val))
            })
            .collect();
        table.add_row(cells);
    }

    let mut output = table.to_string();
    output.push('\n');

    if objects.len() > 50 {
        output.push_str(
            &format!("... and {} more\n", objects.len() - 50)
                .dimmed()
                .to_string(),
        );
    }

    output
}

#[allow(dead_code)]
fn format_cell_value(value: &Value) -> String {
    match value {
        Value::Null => "-".dimmed().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => truncate_str(s, 45),
        Value::Array(arr) if arr.is_empty() => "[]".to_string(),
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(obj) if obj.is_empty() => "{}".to_string(),
        Value::Object(obj) => format!("{{{}...}}", obj.len()),
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

fn find_list_in_object(obj: &serde_json::Map<String, Value>) -> Option<(&str, &Vec<Value>)> {
    // Check priority list keys first
    for key in LIST_KEYS {
        if let Some(Value::Array(arr)) = obj.get(*key) {
            if !arr.is_empty() {
                return Some((key, arr));
            }
        }
    }

    // Fallback: any non-empty array
    for (key, value) in obj {
        if let Value::Array(arr) = value {
            if !arr.is_empty() && arr.iter().any(|v| v.is_object()) {
                return Some((key.as_str(), arr));
            }
        }
    }

    None
}

fn format_scalar(value: &Value) -> String {
    match value {
        Value::Null => "-".dimmed().to_string(),
        Value::Bool(b) => {
            if *b {
                "true".green().to_string()
            } else {
                "false".red().to_string()
            }
        }
        Value::Number(n) => n.yellow().to_string(),
        Value::String(s) => s.to_string(),
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(obj) => format!("{{{}...}}", obj.len()),
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    // Take first line only
    let first_line = s.lines().next().unwrap_or(s);

    if first_line.chars().count() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

fn clean_snippet(s: &str) -> String {
    // Remove HTML entities, extra whitespace, newlines
    s.replace("\\n", " ")
        .replace('\n', " ")
        .replace('\r', "")
        .replace("  ", " ")
        .trim()
        .to_string()
}

fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(DEFAULT_WIDTH)
}

/// Format a URL as a clickable hyperlink using OSC 8 escape sequences.
/// Supported by most modern terminals (iTerm2, Hyper, Windows Terminal, GNOME Terminal, etc.)
fn format_hyperlink(url: &str, display_text: &str) -> String {
    // OSC 8 format: \x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\
    // Using \x07 (BEL) as terminator for broader compatibility
    format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, display_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_card() {
        let item = json!({
            "title": "Test Article",
            "url": "https://example.com",
            "description": "This is a test description"
        });
        let output = format_card(&item, 1, 80);
        assert!(output.contains("Test Article"));
        assert!(output.contains("example.com"));
    }

    #[test]
    fn test_truncate_str() {
        let long = "This is a very long string that should be truncated";
        let truncated = truncate_str(long, 20);
        assert!(truncated.ends_with("..."));
        assert!(truncated.chars().count() <= 20);
    }

    #[test]
    fn test_format_section_header() {
        let header = format_section_header("arxiv", Some(10), 80);
        assert!(header.contains("arxiv"));
        assert!(header.contains("10"));
    }

    #[test]
    fn test_format_card_does_not_duplicate_snippet_when_title_falls_back_to_text() {
        let item = json!({
            "text": "Same text"
        });
        let output = format_card(&item, 1, 80);
        assert_eq!(output.matches("Same text").count(), 1);
    }
}
