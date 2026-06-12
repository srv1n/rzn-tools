//! DOCX document extractor
//!
//! Provides text extraction and heading-based navigation for Microsoft Word documents.

use super::Extractor;
use crate::connectors::localfs::types::*;
use crate::error::ConnectorError;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub struct DocxExtractor;

#[derive(Debug, Clone)]
struct DocxParagraph {
    text: String,
    style: Option<String>,
}

impl DocxExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Open DOCX archive
    fn open_archive(&self, path: &Path) -> Result<ZipArchive<File>, ConnectorError> {
        let file = File::open(path).map_err(ConnectorError::Io)?;
        ZipArchive::new(file)
            .map_err(|e| ConnectorError::Other(format!("Failed to open DOCX: {}", e)))
    }

    /// Read a file from the archive
    fn read_archive_file(
        &self,
        archive: &mut ZipArchive<File>,
        name: &str,
    ) -> Result<String, ConnectorError> {
        let mut file = archive.by_name(name).map_err(|e| {
            ConnectorError::Other(format!("File not found in DOCX: {} - {}", name, e))
        })?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| ConnectorError::Other(format!("Failed to read {}: {}", name, e)))?;
        Ok(content)
    }

    /// Extract metadata from docProps/core.xml
    fn extract_metadata(&self, archive: &mut ZipArchive<File>) -> (Option<String>, Option<String>) {
        let core_xml = match self.read_archive_file(archive, "docProps/core.xml") {
            Ok(content) => content,
            Err(_) => return (None, None),
        };

        let mut reader = Reader::from_str(&core_xml);
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut title = None;
        let mut author = None;
        let mut in_title = false;
        let mut in_creator = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name.contains("title") {
                        in_title = true;
                    } else if name.contains("creator") {
                        in_creator = true;
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if in_title {
                        title = Some(text);
                        in_title = false;
                    } else if in_creator {
                        author = Some(text);
                        in_creator = false;
                    }
                }
                Ok(Event::End(_)) => {
                    in_title = false;
                    in_creator = false;
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        (title, author)
    }

    /// Parse document.xml and extract paragraphs with styles
    fn parse_document(
        &self,
        archive: &mut ZipArchive<File>,
    ) -> Result<Vec<DocxParagraph>, ConnectorError> {
        let doc_xml = self.read_archive_file(archive, "word/document.xml")?;

        let mut reader = Reader::from_str(&doc_xml);
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut paragraphs = Vec::new();
        let mut current_para_text = String::new();
        let mut current_style: Option<String> = None;
        let mut in_paragraph = false;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name();
                    let local_name = std::str::from_utf8(name.as_ref())
                        .unwrap_or("")
                        .split(':')
                        .next_back()
                        .unwrap_or("");

                    match local_name {
                        "p" => {
                            in_paragraph = true;
                            current_para_text.clear();
                            current_style = None;
                        }
                        "pStyle" => {
                            // Extract paragraph style (e.g., Heading1, Heading2)
                            for attr in e.attributes().filter_map(|a| a.ok()) {
                                if attr.key.as_ref() == b"w:val"
                                    || attr.key.local_name().as_ref() == b"val"
                                {
                                    current_style =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    let local_name = std::str::from_utf8(name.as_ref())
                        .unwrap_or("")
                        .split(':')
                        .next_back()
                        .unwrap_or("");

                    if local_name == "pStyle" {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"w:val"
                                || attr.key.local_name().as_ref() == b"val"
                            {
                                current_style =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if in_paragraph {
                        let text = e.unescape().unwrap_or_default().to_string();
                        current_para_text.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name();
                    let local_name = std::str::from_utf8(name.as_ref())
                        .unwrap_or("")
                        .split(':')
                        .next_back()
                        .unwrap_or("");

                    if local_name == "p" && in_paragraph {
                        if !current_para_text.trim().is_empty() {
                            paragraphs.push(DocxParagraph {
                                text: current_para_text.trim().to_string(),
                                style: current_style.clone(),
                            });
                        }
                        in_paragraph = false;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(ConnectorError::Other(format!("XML parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(paragraphs)
    }

    /// Check if a style name indicates a heading
    fn is_heading_style(style: &str) -> Option<usize> {
        let style_lower = style.to_lowercase();

        // Check for common heading style patterns
        if style_lower.starts_with("heading") || style_lower.starts_with("titre") {
            // Try to extract the heading level
            let digits: String = style_lower.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(level) = digits.parse::<usize>() {
                return Some(level);
            }
            return Some(1); // Default to level 1
        }

        // Check for numbered patterns like "Heading1", "Title"
        if style_lower == "title" {
            return Some(1);
        }

        None
    }

    /// Build sections from headings with previews
    fn build_sections(&self, paragraphs: &[DocxParagraph]) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut heading_indices: Vec<(usize, usize)> = Vec::new(); // (para_index, heading_level)

        // First pass: find all headings
        for (para_idx, para) in paragraphs.iter().enumerate() {
            if let Some(ref style) = para.style {
                if let Some(level) = Self::is_heading_style(style) {
                    heading_indices.push((para_idx, level));
                }
            }
        }

        // Second pass: build sections with previews
        for (heading_idx, (para_idx, level)) in heading_indices.iter().enumerate() {
            let para = &paragraphs[*para_idx];

            // Find end of section (next heading or end of document)
            let end_para_idx = heading_indices
                .get(heading_idx + 1)
                .map(|(idx, _)| *idx)
                .unwrap_or(paragraphs.len());

            // Extract preview from content paragraphs after the heading
            let preview = if *para_idx + 1 < end_para_idx {
                let content_text: String = paragraphs
                    [*para_idx + 1..end_para_idx.min(*para_idx + 5)]
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let trimmed = content_text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    let preview_len = 150.min(trimmed.len());
                    let mut preview: String = trimmed.chars().take(preview_len).collect();
                    if preview_len < trimmed.len() {
                        if let Some(last_space) = preview.rfind(' ') {
                            if last_space > 80 {
                                preview.truncate(last_space);
                            }
                        }
                        preview.push_str("...");
                    }
                    Some(preview)
                }
            } else {
                None
            };

            sections.push(Section {
                id: format!("heading:{}", heading_idx),
                index: heading_idx,
                title: para.text.clone(),
                depth: level.saturating_sub(1),
                start_page: None,
                end_page: None,
                preview,
            });
        }

        // If no headings found, create a single section
        if sections.is_empty() {
            let preview = paragraphs.first().map(|p| {
                let trimmed = p.text.trim();
                let preview_len = 150.min(trimmed.len());
                let mut preview: String = trimmed.chars().take(preview_len).collect();
                if preview_len < trimmed.len() {
                    preview.push_str("...");
                }
                preview
            });

            sections.push(Section {
                id: "heading:0".to_string(),
                index: 0,
                title: "Document".to_string(),
                depth: 0,
                start_page: None,
                end_page: None,
                preview,
            });
        }

        sections
    }
}

impl Extractor for DocxExtractor {
    fn extensions(&self) -> &[&str] {
        &["docx", "doc"]
    }

    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let mut archive = self.open_archive(path)?;
        let paragraphs = self.parse_document(&mut archive)?;

        let content: String = paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

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

    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError> {
        let mut archive = self.open_archive(path)?;
        let (title, author) = self.extract_metadata(&mut archive);
        let paragraphs = self.parse_document(&mut archive)?;
        let sections = self.build_sections(&paragraphs);

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Docx,
            title: title.or_else(|| path.file_name().and_then(|n| n.to_str()).map(String::from)),
            author,
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
                ConnectorError::InvalidParams(format!("Invalid heading number: {}", section_id))
            })?
        } else {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID for DOCX: {}. Expected: N or heading:N",
                section_id
            )));
        };

        let mut archive = self.open_archive(path)?;
        let paragraphs = self.parse_document(&mut archive)?;

        // Find heading paragraph indices
        let mut heading_para_indices: Vec<usize> = Vec::new();
        for (idx, para) in paragraphs.iter().enumerate() {
            if let Some(ref style) = para.style {
                if Self::is_heading_style(style).is_some() {
                    heading_para_indices.push(idx);
                }
            }
        }

        if heading_idx >= heading_para_indices.len() {
            return Err(ConnectorError::InvalidParams(format!(
                "Heading {} not found (document has {} headings)",
                heading_idx,
                heading_para_indices.len()
            )));
        }

        // Get content from this heading to the next (or end)
        let start_para = heading_para_indices[heading_idx];
        let end_para = heading_para_indices
            .get(heading_idx + 1)
            .copied()
            .unwrap_or(paragraphs.len());

        let title = paragraphs.get(start_para).map(|p| p.text.clone());
        let content: String = paragraphs[start_para..end_para]
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let word_count = content.split_whitespace().count();

        let prev = if heading_idx > 0 {
            Some(format!("heading:{}", heading_idx - 1))
        } else {
            None
        };

        let next = if heading_idx + 1 < heading_para_indices.len() {
            Some(format!("heading:{}", heading_idx + 1))
        } else {
            None
        };

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title,
            content,
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
        let mut archive = self.open_archive(path)?;
        let paragraphs = self.parse_document(&mut archive)?;

        let query_lower = query.to_lowercase();
        let mut matches = Vec::new();

        // Build heading map for section context
        let mut current_heading_idx = 0;
        let mut para_to_heading: Vec<usize> = Vec::new();

        for para in paragraphs.iter() {
            if let Some(ref style) = para.style {
                if Self::is_heading_style(style).is_some() {
                    current_heading_idx = para_to_heading.len();
                }
            }
            para_to_heading.push(current_heading_idx);
        }

        for (para_idx, para) in paragraphs.iter().enumerate() {
            if let Some(col) = para.text.to_lowercase().find(&query_lower) {
                // Get context paragraphs
                let start = para_idx.saturating_sub(context_lines);
                let end = (para_idx + context_lines + 1).min(paragraphs.len());
                let context = paragraphs[start..end]
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                let section_id = para_to_heading
                    .get(para_idx)
                    .map(|idx| format!("heading:{}", idx));

                matches.push(SearchMatch {
                    line_number: para_idx + 1,
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
