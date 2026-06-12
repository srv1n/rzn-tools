//! HTML file extractor

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use std::fs;
use std::path::Path;

use scraper::{Html, Selector};

pub struct HtmlExtractor;

#[derive(Debug)]
struct HeadingInfo {
    level: usize,
    title: String,
    text_offset: usize, // Approximate position in extracted text
}

impl HtmlExtractor {
    pub fn new() -> Self {
        HtmlExtractor
    }

    fn read_file(&self, path: &Path) -> Result<String, ConnectorError> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(content),
            Err(_) => {
                let bytes = fs::read(path).map_err(ConnectorError::Io)?;
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
        }
    }

    fn extract_text_and_headings(
        &self,
        html: &str,
    ) -> Result<(String, Vec<HeadingInfo>), ConnectorError> {
        let document = Html::parse_document(html);

        // Remove script and style tags
        let body_selector = Selector::parse("body").unwrap();

        let mut text_parts = Vec::new();
        let mut headings = Vec::new();
        let current_offset = 0;

        // Try to get body, fallback to whole document
        let root = document
            .select(&body_selector)
            .next()
            .map(|e| e.html())
            .unwrap_or_else(|| html.to_string());

        let body_doc = Html::parse_fragment(&root);

        // Extract headings
        for level in 1..=6 {
            let selector = Selector::parse(&format!("h{}", level)).unwrap();
            for element in body_doc.select(&selector) {
                let title = element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                if !title.is_empty() {
                    headings.push(HeadingInfo {
                        level,
                        title,
                        text_offset: current_offset,
                    });
                }
            }
        }

        // Sort headings by their appearance in the document
        headings.sort_by_key(|h| h.text_offset);

        // Extract all text, removing scripts and styles
        let text_selector = Selector::parse("*").unwrap();
        for element in body_doc.select(&text_selector) {
            // Skip script and style elements
            if element.value().name() == "script" || element.value().name() == "style" {
                continue;
            }

            let text = element.text().collect::<Vec<_>>().join(" ");
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                text_parts.push(trimmed.to_string());
            }
        }

        let full_text = text_parts.join("\n");
        Ok((full_text, headings))
    }
}

impl Extractor for HtmlExtractor {
    fn extensions(&self) -> &[&str] {
        &["html", "htm", "xhtml"]
    }

    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let html = self.read_file(path)?;
        let (text, _) = self.extract_text_and_headings(&html)?;

        let word_count = text.split_whitespace().count();
        let char_count = text.chars().count();

        Ok(TextContent {
            path: path.to_string_lossy().to_string(),
            content: text,
            format: "plain".to_string(),
            word_count,
            char_count,
            truncated: false,
            original_char_count: None,
        })
    }

    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError> {
        let html = self.read_file(path)?;
        let (_, headings) = self.extract_text_and_headings(&html)?;

        let sections: Vec<Section> = headings
            .iter()
            .enumerate()
            .map(|(i, h)| {
                Section {
                    id: format!("heading:{}", i),
                    index: i,
                    title: h.title.clone(),
                    depth: h.level - 1, // Convert to 0-indexed depth
                    start_page: None,
                    end_page: None,
                    preview: None, // HTML structure doesn't track content positions reliably
                }
            })
            .collect();

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Html,
            title: path.file_name().and_then(|n| n.to_str()).map(String::from),
            author: None,
            sections,
            total_pages: None,
            total_chapters: None,
        })
    }

    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError> {
        // Parse section ID - supports:
        // - N - heading by index (integer shorthand)
        // - heading:N - heading by index
        let heading_idx: usize = if let Ok(idx) = section_id.parse::<usize>() {
            idx
        } else if let Some(heading_str) = section_id.strip_prefix("heading:") {
            heading_str.parse().map_err(|_| {
                ConnectorError::InvalidParams(format!("Invalid heading index: {}", section_id))
            })?
        } else {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID for HTML: {}. Expected: N or heading:N",
                section_id
            )));
        };

        let html = self.read_file(path)?;
        let (full_text, headings) = self.extract_text_and_headings(&html)?;

        if heading_idx >= headings.len() {
            return Err(ConnectorError::InvalidParams(format!(
                "Heading index {} out of range (total headings: {})",
                heading_idx,
                headings.len()
            )));
        }

        // For simplicity, return the full text as we don't have precise boundaries
        // In a more sophisticated implementation, we would track text positions
        let word_count = full_text.split_whitespace().count();

        let prev = if heading_idx > 0 {
            Some(format!("heading:{}", heading_idx - 1))
        } else {
            None
        };

        let next = if heading_idx + 1 < headings.len() {
            Some(format!("heading:{}", heading_idx + 1))
        } else {
            None
        };

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title: Some(headings[heading_idx].title.clone()),
            content: full_text,
            word_count,
            prev_section: prev,
            next_section: next,
            truncated: false,
            original_char_count: None,
        })
    }

    fn search(
        &self,
        path: &Path,
        query: &str,
        context_lines: usize,
    ) -> Result<SearchResult, ConnectorError> {
        let html = self.read_file(path)?;
        let (text, headings) = self.extract_text_and_headings(&html)?;

        let lines: Vec<&str> = text.lines().collect();
        let query_lower = query.to_lowercase();

        let mut matches = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            if let Some(col) = line.to_lowercase().find(&query_lower) {
                // Get context lines
                let start = line_num.saturating_sub(context_lines);
                let end = (line_num + context_lines + 1).min(lines.len());
                let context = lines[start..end].join("\n");

                // Approximate heading association
                let section_id = if !headings.is_empty() {
                    let char_pos = lines[..line_num].iter().map(|l| l.len() + 1).sum::<usize>();
                    headings
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, h)| h.text_offset <= char_pos)
                        .map(|(idx, _)| format!("heading:{}", idx))
                } else {
                    None
                };

                matches.push(SearchMatch {
                    line_number: line_num + 1,
                    column: col + 1,
                    context,
                    section_id,
                });
            }
        }

        Ok(SearchResult {
            path: path.to_string_lossy().to_string(),
            query: query.to_string(),
            total_matches: matches.len(),
            matches,
        })
    }
}
