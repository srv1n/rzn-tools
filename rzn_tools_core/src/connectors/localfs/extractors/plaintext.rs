//! Plain text file extractor

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use std::fs;
use std::path::Path;

pub struct PlainTextExtractor;

impl PlainTextExtractor {
    pub fn new() -> Self {
        PlainTextExtractor
    }

    fn read_file(&self, path: &Path) -> Result<String, ConnectorError> {
        // Try UTF-8 first, fall back to lossy conversion
        match fs::read_to_string(path) {
            Ok(content) => Ok(content),
            Err(_) => {
                let bytes = fs::read(path).map_err(ConnectorError::Io)?;
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
        }
    }
}

impl Extractor for PlainTextExtractor {
    fn extensions(&self) -> &[&str] {
        &["txt", "text", "log"]
    }

    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let content = self.read_file(path)?;
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
        let content = self.read_file(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Create sections every 100 lines
        let mut sections = Vec::new();
        let chunk_size = 100;

        for (i, chunk_start) in (0..total_lines).step_by(chunk_size).enumerate() {
            let chunk_end = (chunk_start + chunk_size).min(total_lines);
            // Preview: first line of the chunk
            let preview = lines.get(chunk_start).map(|l| {
                let trimmed = l.trim();
                if trimmed.len() > 80 {
                    format!("{}...", &trimmed[..77])
                } else {
                    trimmed.to_string()
                }
            });
            sections.push(Section {
                id: format!("lines:{}-{}", chunk_start + 1, chunk_end),
                index: i,
                title: format!("Lines {}-{}", chunk_start + 1, chunk_end),
                depth: 0,
                start_page: None,
                end_page: None,
                preview,
            });
        }

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Text,
            title: path.file_name().and_then(|n| n.to_str()).map(String::from),
            author: None,
            sections,
            total_pages: None,
            total_chapters: None,
        })
    }

    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError> {
        // Handle integer shorthand - maps to section by index
        if let Ok(idx) = section_id.parse::<usize>() {
            let structure = self.get_structure(path)?;
            let section = structure.sections.get(idx).ok_or_else(|| {
                ConnectorError::InvalidParams(format!(
                    "Section index {} out of range (total: {})",
                    idx,
                    structure.sections.len()
                ))
            })?;
            // Recursively call with the actual section ID
            return self.get_section(path, &section.id);
        }

        // Parse section_id like "lines:10-50"
        if !section_id.starts_with("lines:") {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID for text: {}. Expected: N or lines:START-END",
                section_id
            )));
        }

        let range_str = &section_id[6..]; // Skip "lines:"
        let parts: Vec<&str> = range_str.split('-').collect();
        if parts.len() != 2 {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid line range: {}. Expected format: lines:START-END",
                section_id
            )));
        }

        let start: usize = parts[0].parse().map_err(|_| {
            ConnectorError::InvalidParams(format!("Invalid start line: {}", parts[0]))
        })?;
        let end: usize = parts[1].parse().map_err(|_| {
            ConnectorError::InvalidParams(format!("Invalid end line: {}", parts[1]))
        })?;

        let content = self.read_file(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Convert to 0-indexed
        let start_idx = start.saturating_sub(1);
        let end_idx = end.min(total_lines);

        let section_content = lines[start_idx..end_idx].join("\n");
        let word_count = section_content.split_whitespace().count();

        // Calculate prev/next sections
        let prev = if start_idx > 0 {
            let prev_start = start_idx.saturating_sub(100) + 1;
            Some(format!("lines:{}-{}", prev_start, start_idx))
        } else {
            None
        };

        let next = if end_idx < total_lines {
            let next_end = (end_idx + 100).min(total_lines);
            Some(format!("lines:{}-{}", end_idx + 1, next_end))
        } else {
            None
        };

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title: Some(format!("Lines {}-{}", start, end)),
            content: section_content,
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
        let content = self.read_file(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let query_lower = query.to_lowercase();

        let mut matches = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            if let Some(col) = line.to_lowercase().find(&query_lower) {
                // Get context lines
                let start = line_num.saturating_sub(context_lines);
                let end = (line_num + context_lines + 1).min(lines.len());
                let context = lines[start..end].join("\n");

                matches.push(SearchMatch {
                    line_number: line_num + 1,
                    column: col + 1,
                    context,
                    section_id: Some(format!(
                        "lines:{}-{}",
                        (line_num / 100) * 100 + 1,
                        ((line_num / 100) + 1) * 100
                    )),
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
