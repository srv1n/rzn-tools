mod code;
mod docx;
mod epub;
mod html;
mod markdown;
mod pdf;
mod plaintext;

pub use code::CodeExtractor;
pub use docx::DocxExtractor;
pub use epub::EpubExtractor;
pub use html::HtmlExtractor;
pub use markdown::MarkdownExtractor;
pub use pdf::PdfExtractor;
pub use plaintext::PlainTextExtractor;

use super::types::*;
use crate::error::ConnectorError;
use std::path::Path;

/// Trait for document extractors
pub trait Extractor: Send + Sync {
    /// File extensions this extractor handles
    fn extensions(&self) -> &[&str];

    /// Extract all text from the file
    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError>;

    /// Get document structure (TOC, headings, etc.)
    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError>;

    /// Get a specific section by ID
    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError>;

    /// Search content
    fn search(
        &self,
        path: &Path,
        query: &str,
        context_lines: usize,
    ) -> Result<SearchResult, ConnectorError>;
}

/// Get the appropriate extractor for a file
pub fn get_extractor_for_path(path: &Path) -> Option<Box<dyn Extractor>> {
    let ext = path.extension()?.to_str()?.to_lowercase();

    match ext.as_str() {
        "pdf" => Some(Box::new(PdfExtractor::new())),
        "epub" => Some(Box::new(EpubExtractor::new())),
        "docx" | "doc" => Some(Box::new(DocxExtractor::new())),
        "html" | "htm" | "xhtml" => Some(Box::new(HtmlExtractor::new())),
        "md" | "markdown" | "mdown" | "mkd" => Some(Box::new(MarkdownExtractor::new())),
        "txt" | "text" | "log" => Some(Box::new(PlainTextExtractor::new())),
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "cpp" | "c" | "h" | "hpp"
        | "rb" | "swift" | "kt" | "scala" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "yaml"
        | "yml" | "json" | "toml" | "xml" | "css" | "scss" | "less" | "sql" | "graphql"
        | "proto" | "lua" | "r" | "php" | "pl" | "ex" | "exs" | "erl" | "hrl" | "hs" | "ml"
        | "mli" | "fs" | "fsi" | "clj" | "cljs" | "cljc" | "vim" | "el" | "lisp" | "scm"
        | "rkt" | "zig" | "nim" | "v" | "d" | "dart" | "vue" | "svelte" => {
            Some(Box::new(CodeExtractor::new()))
        }
        _ => None,
    }
}

/// Detect file type from path
pub fn detect_file_type(path: &Path) -> FileType {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return FileType::Unknown,
    };

    match ext.as_str() {
        "pdf" => FileType::Pdf,
        "epub" => FileType::Epub,
        "docx" | "doc" => FileType::Docx,
        "html" | "htm" | "xhtml" => FileType::Html,
        "md" | "markdown" | "mdown" | "mkd" => FileType::Markdown,
        "txt" | "text" | "log" => FileType::Text,
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "cpp" | "c" | "h" | "hpp"
        | "rb" | "swift" | "kt" | "scala" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "yaml"
        | "yml" | "json" | "toml" | "xml" | "css" | "scss" | "less" | "sql" | "graphql"
        | "proto" | "lua" | "r" | "php" | "pl" | "ex" | "exs" | "erl" | "hrl" | "hs" | "ml"
        | "mli" | "fs" | "fsi" | "clj" | "cljs" | "cljc" | "vim" | "el" | "lisp" | "scm"
        | "rkt" | "zig" | "nim" | "v" | "d" | "dart" | "vue" | "svelte" => FileType::Code,
        _ => FileType::Unknown,
    }
}
