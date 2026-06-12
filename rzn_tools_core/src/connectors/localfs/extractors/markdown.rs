//! Markdown file extractor

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use regex::Regex;
use std::fs;
use std::path::Path;

pub struct MarkdownExtractor;

#[derive(Debug)]
struct HeadingInfo {
    level: usize,
    title: String,
    line_number: usize,
}

impl MarkdownExtractor {
    pub fn new() -> Self {
        MarkdownExtractor
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

    fn extract_headings(&self, content: &str) -> Vec<HeadingInfo> {
        let heading_regex = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();
        let mut headings = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            if let Some(caps) = heading_regex.captures(line) {
                let level = caps.get(1).unwrap().as_str().len();
                let title = caps.get(2).unwrap().as_str().trim().to_string();
                headings.push(HeadingInfo {
                    level,
                    title,
                    line_number: line_num,
                });
            }
        }

        headings
    }

    fn strip_markdown(&self, text: &str) -> String {
        // Simple markdown stripping - remove common markdown syntax
        let mut result = text.to_string();

        // Remove code blocks
        let code_block_regex = Regex::new(r"```[\s\S]*?```").unwrap();
        result = code_block_regex.replace_all(&result, "").to_string();

        // Remove inline code
        let inline_code_regex = Regex::new(r"`[^`]+`").unwrap();
        result = inline_code_regex.replace_all(&result, "").to_string();

        // Remove links but keep text [text](url) -> text
        let link_regex = Regex::new(r"\[([^\]]+)\]\([^\)]+\)").unwrap();
        result = link_regex.replace_all(&result, "$1").to_string();

        // Remove images
        let image_regex = Regex::new(r"!\[([^\]]*)\]\([^\)]+\)").unwrap();
        result = image_regex.replace_all(&result, "").to_string();

        // Remove bold (** and __)
        let bold_asterisk_regex = Regex::new(r"\*\*([^*]+)\*\*").unwrap();
        result = bold_asterisk_regex.replace_all(&result, "$1").to_string();
        let bold_underscore_regex = Regex::new(r"__([^_]+)__").unwrap();
        result = bold_underscore_regex.replace_all(&result, "$1").to_string();

        // Remove italic (* and _)
        let italic_asterisk_regex = Regex::new(r"\*([^*]+)\*").unwrap();
        result = italic_asterisk_regex.replace_all(&result, "$1").to_string();
        let italic_underscore_regex = Regex::new(r"_([^_]+)_").unwrap();
        result = italic_underscore_regex
            .replace_all(&result, "$1")
            .to_string();

        // Remove headings markers
        let heading_regex = Regex::new(r"^#{1,6}\s+").unwrap();
        result = heading_regex.replace_all(&result, "").to_string();

        result
    }
}

impl Extractor for MarkdownExtractor {
    fn extensions(&self) -> &[&str] {
        &["md", "markdown", "mdown", "mkd"]
    }

    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let content = self.read_file(path)?;

        // Strip markdown for plain format
        let plain_text = self.strip_markdown(&content);
        let word_count = plain_text.split_whitespace().count();
        let char_count = plain_text.chars().count();

        Ok(TextContent {
            path: path.to_string_lossy().to_string(),
            content: plain_text,
            format: "plain".to_string(),
            word_count,
            char_count,
            truncated: false,
            original_char_count: None,
        })
    }

    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError> {
        let content = self.read_file(path)?;
        let headings = self.extract_headings(&content);
        let lines: Vec<&str> = content.lines().collect();

        let mut sections: Vec<Section> = Vec::with_capacity(headings.len());

        for (i, h) in headings.iter().enumerate() {
            // Find end of this section (next heading of same or higher level, or end of file)
            let start_line = h.line_number;
            let end_line = headings
                .iter()
                .skip(i + 1)
                .find(|next| next.level <= h.level)
                .map(|next| next.line_number)
                .unwrap_or(lines.len());

            // Extract preview (first ~150 chars of content after heading)
            let preview = if start_line + 1 < end_line {
                let content_lines: String = lines[start_line + 1..end_line.min(start_line + 10)]
                    .join(" ")
                    .trim()
                    .to_string();
                let stripped = self.strip_markdown(&content_lines);
                let trimmed = stripped.trim();
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
                id: format!("heading:{}", i),
                index: i,
                title: h.title.clone(),
                depth: h.level - 1, // Convert to 0-indexed depth
                start_page: None,
                end_page: None,
                preview,
            });
        }

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Markdown,
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
                "Invalid section ID for markdown: {}. Expected: N or heading:N",
                section_id
            )));
        };

        let content = self.read_file(path)?;
        let headings = self.extract_headings(&content);

        if heading_idx >= headings.len() {
            return Err(ConnectorError::InvalidParams(format!(
                "Heading index {} out of range (total headings: {})",
                heading_idx,
                headings.len()
            )));
        }

        let lines: Vec<&str> = content.lines().collect();
        let current_heading = &headings[heading_idx];

        // Find the content between this heading and the next heading of same or higher level
        let start_line = current_heading.line_number;
        let mut end_line = lines.len();

        for next_heading in headings.iter().skip(heading_idx + 1) {
            if next_heading.level <= current_heading.level {
                end_line = next_heading.line_number;
                break;
            }
        }

        let section_content = lines[start_line..end_line].join("\n");
        let word_count = section_content.split_whitespace().count();

        // Calculate prev/next sections
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
            title: Some(current_heading.title.clone()),
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
        let headings = self.extract_headings(&content);

        let mut matches = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            if let Some(col) = line.to_lowercase().find(&query_lower) {
                // Get context lines
                let start = line_num.saturating_sub(context_lines);
                let end = (line_num + context_lines + 1).min(lines.len());
                let context = lines[start..end].join("\n");

                // Find which heading this line belongs to
                let section_id = headings
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, h)| h.line_number <= line_num)
                    .map(|(idx, _)| format!("heading:{}", idx));

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
