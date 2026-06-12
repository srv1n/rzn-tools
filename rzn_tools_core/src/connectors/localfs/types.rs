use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size_bytes: u64,
    pub modified: Option<String>, // ISO 8601 timestamp
    pub file_type: FileType,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Pdf,
    Epub,
    Docx,
    Html,
    Markdown,
    Code,
    Text,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    pub path: String,
    pub content: String,
    pub format: String, // "plain" or "markdown"
    pub word_count: usize,
    pub char_count: usize,
    #[serde(default)]
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_char_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentStructure {
    pub path: String,
    pub file_type: FileType,
    pub title: Option<String>,
    pub author: Option<String>,
    pub sections: Vec<Section>,
    pub total_pages: Option<usize>,    // For PDFs
    pub total_chapters: Option<usize>, // For EPUBs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: String, // Identifier like "page:1", "chapter:3", "heading:2"
    pub index: usize,
    pub title: String,
    pub depth: usize, // Nesting level (0 = top level)
    pub start_page: Option<usize>,
    pub end_page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>, // First few lines of content for LLM context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionContent {
    pub path: String,
    pub section_id: String,
    pub title: Option<String>,
    pub content: String,
    pub word_count: usize,
    pub prev_section: Option<String>,
    pub next_section: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_char_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub line_number: usize,
    pub column: usize,
    pub context: String,            // The matched line with surrounding context
    pub section_id: Option<String>, // Which section this is in
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListResult {
    pub directory: String,
    pub files: Vec<FileInfo>,
    pub total_count: usize,
    pub truncated: bool,
}
