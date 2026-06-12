//! EPUB document extractor
//!
//! Provides text extraction, chapter navigation, and TOC extraction for EPUB files.

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub struct EpubExtractor;

#[derive(Debug)]
#[allow(dead_code)]
struct EpubMetadata {
    title: Option<String>,
    author: Option<String>,
    opf_path: String,
    opf_dir: String,
}

#[derive(Debug)]
#[allow(dead_code)]
struct SpineItem {
    id: String,
    href: String,
    index: usize,
}

impl EpubExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Deduplicate title patterns like "Title TitleMore text" -> "Title"
    /// Common in EPUB where heading text is repeated as body text
    fn deduplicate_title(text: &str) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 2 {
            return text.to_string();
        }

        // Try to find where the title repeats
        // Start with longer prefixes and work down
        for prefix_len in (1..=words.len() / 2).rev() {
            let prefix = words[..prefix_len].join(" ");
            let rest = words[prefix_len..].join(" ");

            // Check if the rest starts with the same prefix (case-insensitive)
            if rest.to_lowercase().starts_with(&prefix.to_lowercase()) {
                return prefix;
            }

            // Also check if first word of rest matches first word of prefix
            // (handles "Title TitleMore" where TitleMore is one word)
            if prefix_len == 1 {
                if let Some(first_rest_word) = words.get(prefix_len) {
                    let first_prefix_word = words[0];
                    if first_rest_word
                        .to_lowercase()
                        .starts_with(&first_prefix_word.to_lowercase())
                        && first_rest_word.len() > first_prefix_word.len()
                    {
                        return first_prefix_word.to_string();
                    }
                }
            }
        }

        // No duplication found - just truncate if too long
        if text.len() > 50 {
            let short: String = text.chars().take(50).collect();
            short
                .rfind(' ')
                .map(|pos| text[..pos].to_string())
                .unwrap_or(short)
        } else {
            text.to_string()
        }
    }

    /// Open EPUB archive
    fn open_archive(&self, path: &Path) -> Result<ZipArchive<File>, ConnectorError> {
        let file = File::open(path).map_err(ConnectorError::Io)?;
        ZipArchive::new(file)
            .map_err(|e| ConnectorError::Other(format!("Failed to open EPUB: {}", e)))
    }

    /// Read a file from the archive
    fn read_archive_file(
        &self,
        archive: &mut ZipArchive<File>,
        name: &str,
    ) -> Result<String, ConnectorError> {
        let mut file = archive.by_name(name).map_err(|e| {
            ConnectorError::Other(format!("File not found in EPUB: {} - {}", name, e))
        })?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| ConnectorError::Other(format!("Failed to read {}: {}", name, e)))?;
        Ok(content)
    }

    /// Find OPF file path from container.xml
    fn find_opf_path(&self, archive: &mut ZipArchive<File>) -> Result<String, ConnectorError> {
        let container = self.read_archive_file(archive, "META-INF/container.xml")?;

        let mut reader = Reader::from_str(&container);
        reader.trim_text(true);

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                    if e.name().as_ref() == b"rootfile" {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.as_ref() == b"full-path" {
                                return String::from_utf8(attr.value.to_vec()).map_err(|e| {
                                    ConnectorError::Other(format!("Invalid OPF path: {}", e))
                                });
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(ConnectorError::Other(format!("XML parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Err(ConnectorError::Other(
            "OPF file not found in container.xml".to_string(),
        ))
    }

    /// Parse OPF file and extract metadata, manifest, and spine
    fn parse_opf(
        &self,
        archive: &mut ZipArchive<File>,
        opf_path: &str,
    ) -> Result<(EpubMetadata, Vec<SpineItem>, HashMap<String, String>), ConnectorError> {
        let opf_content = self.read_archive_file(archive, opf_path)?;

        let opf_dir = Path::new(opf_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut reader = Reader::from_str(&opf_content);
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut title = None;
        let mut author = None;
        let mut manifest: HashMap<String, String> = HashMap::new(); // id -> href
        let mut spine_ids: Vec<String> = Vec::new();
        let mut in_metadata = false;
        let mut in_title = false;
        let mut in_creator = false;

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
                        "metadata" => in_metadata = true,
                        "title" if in_metadata => in_title = true,
                        "creator" if in_metadata => in_creator = true,
                        "item" => {
                            let mut id = String::new();
                            let mut href = String::new();
                            for attr in e.attributes().filter_map(|a| a.ok()) {
                                match attr.key.as_ref() {
                                    b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                    b"href" => {
                                        href = String::from_utf8_lossy(&attr.value).to_string()
                                    }
                                    _ => {}
                                }
                            }
                            if !id.is_empty() && !href.is_empty() {
                                manifest.insert(id, href);
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

                    match local_name {
                        "item" => {
                            let mut id = String::new();
                            let mut href = String::new();
                            for attr in e.attributes().filter_map(|a| a.ok()) {
                                match attr.key.as_ref() {
                                    b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                    b"href" => {
                                        href = String::from_utf8_lossy(&attr.value).to_string()
                                    }
                                    _ => {}
                                }
                            }
                            if !id.is_empty() && !href.is_empty() {
                                manifest.insert(id, href);
                            }
                        }
                        "itemref" => {
                            for attr in e.attributes().filter_map(|a| a.ok()) {
                                if attr.key.as_ref() == b"idref" {
                                    spine_ids
                                        .push(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if in_title && title.is_none() {
                        title = Some(text);
                    } else if in_creator && author.is_none() {
                        author = Some(text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name();
                    let local_name = std::str::from_utf8(name.as_ref())
                        .unwrap_or("")
                        .split(':')
                        .next_back()
                        .unwrap_or("");

                    match local_name {
                        "metadata" => in_metadata = false,
                        "title" => in_title = false,
                        "creator" => in_creator = false,
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(ConnectorError::Other(format!("OPF parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        // Build spine items
        let spine: Vec<SpineItem> = spine_ids
            .iter()
            .enumerate()
            .filter_map(|(idx, id)| {
                manifest.get(id).map(|href| SpineItem {
                    id: id.clone(),
                    href: href.clone(),
                    index: idx,
                })
            })
            .collect();

        Ok((
            EpubMetadata {
                title,
                author,
                opf_path: opf_path.to_string(),
                opf_dir,
            },
            spine,
            manifest,
        ))
    }

    /// Strip HTML tags from content
    fn strip_html(&self, html: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        let mut tag_buffer = String::new();
        let mut in_script = false;
        let mut in_style = false;

        for c in html.chars() {
            match c {
                '<' => {
                    in_tag = true;
                    tag_buffer.clear();
                }
                '>' => {
                    if in_tag {
                        let tag_lower = tag_buffer.to_lowercase();
                        if tag_lower.starts_with("script") {
                            in_script = true;
                        } else if tag_lower.starts_with("/script") {
                            in_script = false;
                        } else if tag_lower.starts_with("style") {
                            in_style = true;
                        } else if tag_lower.starts_with("/style") {
                            in_style = false;
                        }
                    }
                    in_tag = false;
                    tag_buffer.clear();
                }
                _ if in_tag => {
                    tag_buffer.push(c);
                }
                _ if !in_script && !in_style => {
                    result.push(c);
                }
                _ => {}
            }
        }

        // Clean up whitespace
        result.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Extract text from a spine item
    fn extract_spine_item_text(
        &self,
        archive: &mut ZipArchive<File>,
        opf_dir: &str,
        href: &str,
    ) -> Result<String, ConnectorError> {
        let full_path = if opf_dir.is_empty() {
            href.to_string()
        } else {
            format!("{}/{}", opf_dir, href)
        };

        let content = self.read_archive_file(archive, &full_path)?;
        Ok(self.strip_html(&content))
    }
}

impl Extractor for EpubExtractor {
    fn extensions(&self) -> &[&str] {
        &["epub"]
    }

    fn extract_text(&self, path: &Path) -> Result<TextContent, ConnectorError> {
        let mut archive = self.open_archive(path)?;
        let opf_path = self.find_opf_path(&mut archive)?;
        let (metadata, spine, _) = self.parse_opf(&mut archive, &opf_path)?;

        let mut all_text = String::new();

        for item in &spine {
            if let Ok(text) =
                self.extract_spine_item_text(&mut archive, &metadata.opf_dir, &item.href)
            {
                if !all_text.is_empty() {
                    all_text.push_str("\n\n");
                }
                all_text.push_str(&text);
            }
        }

        let word_count = all_text.split_whitespace().count();
        let char_count = all_text.chars().count();

        Ok(TextContent {
            path: path.to_string_lossy().to_string(),
            content: all_text,
            format: "plain".to_string(),
            word_count,
            char_count,
            truncated: false,
            original_char_count: None,
        })
    }

    fn get_structure(&self, path: &Path) -> Result<DocumentStructure, ConnectorError> {
        let mut archive = self.open_archive(path)?;
        let opf_path = self.find_opf_path(&mut archive)?;
        let (metadata, spine, _) = self.parse_opf(&mut archive, &opf_path)?;

        // Build sections from spine with content previews
        let mut sections: Vec<Section> = Vec::with_capacity(spine.len());

        for item in &spine {
            // Extract content to get both title and preview
            let content_text = self
                .extract_spine_item_text(&mut archive, &metadata.opf_dir, &item.href)
                .ok();

            // Try to extract a meaningful title from the content
            let title = content_text
                .as_ref()
                .and_then(|text| {
                    let trimmed = text.trim();
                    // Get first line as title candidate
                    let first_line = trimmed.lines().next().unwrap_or("").trim();

                    if first_line.is_empty() {
                        return None;
                    }

                    // Clean up common duplicate patterns in EPUB headings
                    let title_text = Self::deduplicate_title(first_line);

                    if title_text.is_empty() {
                        None
                    } else {
                        Some(title_text)
                    }
                })
                .unwrap_or_else(|| {
                    // Fallback to filename-based title
                    item.href
                        .split('/')
                        .next_back()
                        .unwrap_or(&item.href)
                        .trim_end_matches(".xhtml")
                        .trim_end_matches(".html")
                        .replace(['_', '-'], " ")
                });

            // Extract preview (content after the title, ~150 chars)
            let preview = content_text.as_ref().and_then(|text| {
                let trimmed = text.trim();

                // Find where the title ends and get the rest as preview
                // Look for the title and skip past it (and any repeats)
                let rest = if let Some(pos) = trimmed.find(&title) {
                    let mut after_pos = pos + title.len();

                    // Skip any whitespace/newlines
                    while after_pos < trimmed.len() {
                        let c = trimmed.chars().nth(after_pos).unwrap_or(' ');
                        if c.is_whitespace() {
                            after_pos += c.len_utf8();
                        } else {
                            break;
                        }
                    }

                    // Check if the title repeats (common in EPUB)
                    let remainder = &trimmed[after_pos..];
                    if remainder.starts_with(&title) {
                        let skip_again = after_pos + title.len();
                        // Skip whitespace after second title
                        let mut final_pos = skip_again;
                        while final_pos < trimmed.len() {
                            let c = trimmed.chars().nth(final_pos).unwrap_or(' ');
                            if c.is_whitespace() {
                                final_pos += c.len_utf8();
                            } else {
                                break;
                            }
                        }
                        &trimmed[final_pos..]
                    } else {
                        remainder
                    }
                } else {
                    // Fallback: use content after first 50 chars
                    let skip_len = 50.min(trimmed.len());
                    let remainder = &trimmed[skip_len..];
                    // Find next word boundary
                    remainder
                        .find(char::is_whitespace)
                        .map(|pos| remainder[pos..].trim_start())
                        .unwrap_or("")
                };

                if rest.is_empty() || rest.len() < 10 {
                    None
                } else {
                    let preview_len = 150.min(rest.len());
                    let mut preview: String = rest.chars().take(preview_len).collect();
                    if preview_len < rest.len() {
                        if let Some(last_space) = preview.rfind(' ') {
                            if last_space > 80 {
                                preview.truncate(last_space);
                            }
                        }
                        preview.push_str("...");
                    }
                    Some(preview)
                }
            });

            sections.push(Section {
                id: format!("{}", item.index), // Simple integer ID
                index: item.index,
                title,
                depth: 0,
                start_page: None,
                end_page: None,
                preview,
            });
        }

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Epub,
            title: metadata.title,
            author: metadata.author,
            sections,
            total_pages: None,
            total_chapters: Some(spine.len()),
        })
    }

    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError> {
        // Parse section ID - supports:
        // - N - chapter by index (0-indexed)
        // - chapter:N - legacy format (still supported)
        let chapter_idx: usize = if let Ok(idx) = section_id.parse::<usize>() {
            idx
        } else if let Some(chapter_str) = section_id.strip_prefix("chapter:") {
            // Legacy format support
            chapter_str.parse().map_err(|_| {
                ConnectorError::InvalidParams(format!("Invalid chapter number: {}", section_id))
            })?
        } else {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID for EPUB: {}. Expected: N (0-indexed chapter number)",
                section_id
            )));
        };

        let mut archive = self.open_archive(path)?;
        let opf_path = self.find_opf_path(&mut archive)?;
        let (metadata, spine, _) = self.parse_opf(&mut archive, &opf_path)?;

        let item = spine.get(chapter_idx).ok_or_else(|| {
            ConnectorError::InvalidParams(format!(
                "Chapter {} not found (EPUB has {} chapters)",
                chapter_idx,
                spine.len()
            ))
        })?;

        let content = self.extract_spine_item_text(&mut archive, &metadata.opf_dir, &item.href)?;
        let word_count = content.split_whitespace().count();

        // Extract title from content (same logic as get_structure)
        let title = {
            let first_line = content.trim().lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                format!("Chapter {}", chapter_idx + 1)
            } else {
                Self::deduplicate_title(first_line)
            }
        };

        // Use simple integer IDs for prev/next
        let prev = if chapter_idx > 0 {
            Some(format!("{}", chapter_idx - 1))
        } else {
            None
        };

        let next = if chapter_idx + 1 < spine.len() {
            Some(format!("{}", chapter_idx + 1))
        } else {
            None
        };

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title: Some(title),
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
        let opf_path = self.find_opf_path(&mut archive)?;
        let (metadata, spine, _) = self.parse_opf(&mut archive, &opf_path)?;

        let query_lower = query.to_lowercase();
        let mut matches = Vec::new();

        for item in &spine {
            if let Ok(content) =
                self.extract_spine_item_text(&mut archive, &metadata.opf_dir, &item.href)
            {
                let lines: Vec<&str> = content.lines().collect();

                for (line_idx, line) in lines.iter().enumerate() {
                    if let Some(col) = line.to_lowercase().find(&query_lower) {
                        let start = line_idx.saturating_sub(context_lines);
                        let end = (line_idx + context_lines + 1).min(lines.len());
                        let context = lines[start..end].join("\n");

                        matches.push(SearchMatch {
                            line_number: line_idx + 1,
                            column: col + 1,
                            context,
                            section_id: Some(format!("{}", item.index)), // Simple integer ID
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
}
