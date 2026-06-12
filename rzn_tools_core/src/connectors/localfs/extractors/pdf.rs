//! PDF document extractor
//!
//! Provides text extraction, page-based navigation, and TOC/bookmark extraction for PDFs.

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use std::path::Path;

#[cfg(feature = "localfs")]
use lopdf::Document;

pub struct PdfExtractor;

impl PdfExtractor {
    pub fn new() -> Self {
        PdfExtractor
    }

    #[cfg(feature = "localfs")]
    /// Load a PDF document
    fn load_document(&self, path: &Path) -> Result<Document, ConnectorError> {
        Document::load(path)
            .map_err(|e| ConnectorError::Other(format!("Failed to load PDF: {}", e)))
    }

    #[cfg(feature = "localfs")]
    /// Get total page count
    fn get_page_count(&self, doc: &Document) -> usize {
        doc.get_pages().len()
    }

    #[cfg(feature = "localfs")]
    /// Extract text from a specific page (1-indexed)
    fn extract_page_text(&self, doc: &Document, page_num: u32) -> Result<String, ConnectorError> {
        let pages = doc.get_pages();
        let _page_id = pages
            .get(&page_num)
            .ok_or_else(|| ConnectorError::InvalidParams(format!("Page {} not found", page_num)))?;

        // Use lopdf's text extraction
        doc.extract_text(&[page_num]).map_err(|e| {
            ConnectorError::Other(format!(
                "Failed to extract text from page {}: {}",
                page_num, e
            ))
        })
    }

    #[cfg(feature = "localfs")]
    /// Extract text from all pages
    fn extract_all_text(&self, doc: &Document) -> Result<String, ConnectorError> {
        let page_count = self.get_page_count(doc);
        let page_nums: Vec<u32> = (1..=page_count as u32).collect();

        doc.extract_text(&page_nums)
            .map_err(|e| ConnectorError::Other(format!("Failed to extract text: {}", e)))
    }

    #[cfg(feature = "localfs")]
    #[allow(dead_code)]
    /// Extract bookmarks/outline as sections (without previews)
    fn extract_bookmarks_basic(&self, doc: &Document) -> Vec<Section> {
        // Try to get the document outline (bookmarks)
        let mut sections = Vec::new();

        // lopdf provides access to the outline through the catalog
        // This is a simplified implementation - real bookmarks are more complex
        if let Ok(catalog) = doc.catalog() {
            if let Ok(_outlines) = catalog.get(b"Outlines") {
                // Parse the outline tree...
                // For now, fall back to page-based sections
            }
        }

        // If no bookmarks, create page-based sections
        if sections.is_empty() {
            let page_count = self.get_page_count(doc);
            for page in 1..=page_count {
                sections.push(Section {
                    id: format!("page:{}", page),
                    index: page - 1,
                    title: format!("Page {}", page),
                    depth: 0,
                    start_page: Some(page),
                    end_page: Some(page),
                    preview: None,
                });
            }
        }

        sections
    }

    #[cfg(feature = "localfs")]
    /// Extract bookmarks/outline as sections with content previews
    fn extract_bookmarks_with_previews(&self, doc: &Document) -> Vec<Section> {
        let page_count = self.get_page_count(doc);
        let mut sections = Vec::with_capacity(page_count);

        for page in 1..=page_count {
            // Extract preview from page content (first ~150 chars of actual text)
            let preview = self
                .extract_page_text(doc, page as u32)
                .ok()
                .and_then(|text| {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        // Take first 150 chars, break at word boundary
                        let preview_len = 150.min(trimmed.len());
                        let mut preview: String = trimmed.chars().take(preview_len).collect();
                        // Try to break at last space to avoid cutting words
                        if preview_len < trimmed.len() {
                            if let Some(last_space) = preview.rfind(' ') {
                                if last_space > 80 {
                                    // Keep at least 80 chars
                                    preview.truncate(last_space);
                                }
                            }
                            preview.push_str("...");
                        }
                        Some(preview)
                    }
                });

            sections.push(Section {
                id: format!("{}", page), // Simple integer ID (1-indexed for pages)
                index: page - 1,
                title: format!("Page {}", page),
                depth: 0,
                start_page: Some(page),
                end_page: Some(page),
                preview,
            });
        }

        sections
    }
}

impl Extractor for PdfExtractor {
    fn extensions(&self) -> &[&str] {
        &["pdf"]
    }

    #[cfg(feature = "localfs")]
    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let doc = self.load_document(path)?;
        let content = self.extract_all_text(&doc)?;
        let word_count = content.split_whitespace().count();
        let char_count = content.chars().count();

        Ok(TextContent {
            path: path.to_string_lossy().to_string(),
            content,
            format: "plain".to_string(),
            word_count,
            char_count,
            truncated: false,
            original_char_count: None,
        })
    }

    #[cfg(not(feature = "localfs"))]
    fn extract_text(&self, _path: &Path) -> Result<TextContent, ConnectorError> {
        Err(ConnectorError::Other(
            "localfs feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "localfs")]
    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError> {
        let doc = self.load_document(path)?;
        let page_count = self.get_page_count(&doc);
        let sections = self.extract_bookmarks_with_previews(&doc);

        // Try to extract title from PDF metadata
        // lopdf doesn't have a direct get_info() method, we'll use the filename as title
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.trim_end_matches(".pdf"))
            .map(String::from);

        let author: Option<String> = None;

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Pdf,
            title,
            author,
            sections,
            total_pages: Some(page_count),
            total_chapters: None,
        })
    }

    #[cfg(not(feature = "localfs"))]
    fn get_structure(&self, _path: &Path) -> Result<DocumentStructure, ConnectorError> {
        Err(ConnectorError::Other(
            "localfs feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "localfs")]
    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError> {
        let doc = self.load_document(path)?;
        let page_count = self.get_page_count(&doc);

        // Parse section ID - supports:
        // - N - single page (integer shorthand, 1-indexed)
        // - page:N - single page
        // - pages:START-END - page range
        let (start_page, end_page) = if let Ok(page) = section_id.parse::<u32>() {
            // Integer shorthand: just the page number (1-indexed)
            (page, page)
        } else if let Some(page_str) = section_id.strip_prefix("page:") {
            let page: u32 = page_str.parse().map_err(|_| {
                ConnectorError::InvalidParams(format!("Invalid page number: {}", section_id))
            })?;
            (page, page)
        } else if let Some(range_str) = section_id.strip_prefix("pages:") {
            let parts: Vec<&str> = range_str.split('-').collect();
            if parts.len() != 2 {
                return Err(ConnectorError::InvalidParams(format!(
                    "Invalid page range: {}. Expected format: pages:START-END",
                    section_id
                )));
            }
            let start: u32 = parts[0].parse().map_err(|_| {
                ConnectorError::InvalidParams(format!("Invalid start page: {}", parts[0]))
            })?;
            let end: u32 = parts[1].parse().map_err(|_| {
                ConnectorError::InvalidParams(format!("Invalid end page: {}", parts[1]))
            })?;
            (start, end)
        } else {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID for PDF: {}. Expected: N, page:N, or pages:START-END",
                section_id
            )));
        };

        // Validate page range
        if start_page < 1 || end_page > page_count as u32 || start_page > end_page {
            return Err(ConnectorError::InvalidParams(format!(
                "Page range {}-{} is out of bounds (document has {} pages)",
                start_page, end_page, page_count
            )));
        }

        // Extract text from the page range
        let page_nums: Vec<u32> = (start_page..=end_page).collect();
        let content = doc
            .extract_text(&page_nums)
            .map_err(|e| ConnectorError::Other(format!("Failed to extract text: {}", e)))?;

        let word_count = content.split_whitespace().count();

        // Calculate prev/next
        let prev = if start_page > 1 {
            Some(format!("page:{}", start_page - 1))
        } else {
            None
        };

        let next = if end_page < page_count as u32 {
            Some(format!("page:{}", end_page + 1))
        } else {
            None
        };

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title: Some(if start_page == end_page {
                format!("Page {}", start_page)
            } else {
                format!("Pages {}-{}", start_page, end_page)
            }),
            content,
            word_count,
            prev_section: prev,
            next_section: next,
            truncated: false,
            original_char_count: None,
        })
    }

    #[cfg(not(feature = "localfs"))]
    fn get_section(
        &self,
        _path: &Path,
        _section_id: &str,
    ) -> Result<SectionContent, ConnectorError> {
        Err(ConnectorError::Other(
            "localfs feature not enabled".to_string(),
        ))
    }

    #[cfg(feature = "localfs")]
    fn search(
        &self,
        path: &Path,
        query: &str,
        context_lines: usize,
    ) -> Result<SearchResult, ConnectorError> {
        let doc = self.load_document(path)?;
        let page_count = self.get_page_count(&doc);
        let query_lower = query.to_lowercase();

        let mut matches = Vec::new();

        // Search each page
        for page_num in 1..=page_count as u32 {
            if let Ok(page_text) = self.extract_page_text(&doc, page_num) {
                let lines: Vec<&str> = page_text.lines().collect();

                for (line_idx, line) in lines.iter().enumerate() {
                    if let Some(col) = line.to_lowercase().find(&query_lower) {
                        // Get context
                        let start = line_idx.saturating_sub(context_lines);
                        let end = (line_idx + context_lines + 1).min(lines.len());
                        let context = lines[start..end].join("\n");

                        matches.push(SearchMatch {
                            line_number: line_idx + 1,
                            column: col + 1,
                            context,
                            section_id: Some(format!("page:{}", page_num)),
                        });
                    }
                }
            }
        }

        Ok(SearchResult {
            path: path.to_string_lossy().to_string(),
            query: query.to_string(),
            total_matches: matches.len(),
            matches,
        })
    }

    #[cfg(not(feature = "localfs"))]
    fn search(
        &self,
        _path: &Path,
        _query: &str,
        _context_lines: usize,
    ) -> Result<SearchResult, ConnectorError> {
        Err(ConnectorError::Other(
            "localfs feature not enabled".to_string(),
        ))
    }
}
