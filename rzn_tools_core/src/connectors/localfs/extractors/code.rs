//! Code file extractor
//!
//! Extracts code structure using regex patterns for multiple languages.

use super::super::types::*;
use super::Extractor;
use crate::error::ConnectorError;
use regex::Regex;
use std::fs;
use std::path::Path;

pub struct CodeExtractor;

#[derive(Debug, Clone)]
struct CodeElement {
    kind: String, // "fn", "class", "struct", "impl", "mod", etc.
    name: String,
    line_number: usize,
}

impl CodeExtractor {
    pub fn new() -> Self {
        CodeExtractor
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

    fn detect_language(&self, path: &Path) -> Option<&str> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "rs" => Some("rust"),
            "py" => Some("python"),
            "js" | "jsx" => Some("javascript"),
            "ts" | "tsx" => Some("typescript"),
            "go" => Some("go"),
            "java" => Some("java"),
            "cpp" | "cc" | "cxx" => Some("cpp"),
            "c" | "h" => Some("c"),
            "rb" => Some("ruby"),
            "swift" => Some("swift"),
            "kt" => Some("kotlin"),
            "scala" => Some("scala"),
            "php" => Some("php"),
            _ => None,
        }
    }

    fn extract_code_elements(&self, content: &str, language: Option<&str>) -> Vec<CodeElement> {
        let mut elements = Vec::new();

        match language {
            Some("rust") => self.extract_rust_elements(content, &mut elements),
            Some("python") => self.extract_python_elements(content, &mut elements),
            Some("javascript") | Some("typescript") => {
                self.extract_js_ts_elements(content, &mut elements)
            }
            Some("go") => self.extract_go_elements(content, &mut elements),
            Some("java") | Some("kotlin") | Some("scala") => {
                self.extract_java_like_elements(content, &mut elements)
            }
            _ => {}
        }

        elements.sort_by_key(|e| e.line_number);
        elements
    }

    fn extract_rust_elements(&self, content: &str, elements: &mut Vec<CodeElement>) {
        let patterns = vec![
            (r"(?m)^[\s]*(?:pub\s+)?fn\s+(\w+)", "fn"),
            (r"(?m)^[\s]*(?:pub\s+)?struct\s+(\w+)", "struct"),
            (r"(?m)^[\s]*(?:pub\s+)?enum\s+(\w+)", "enum"),
            (r"(?m)^[\s]*impl(?:\s+<[^>]+>)?\s+(\w+)", "impl"),
            (r"(?m)^[\s]*(?:pub\s+)?mod\s+(\w+)", "mod"),
            (r"(?m)^[\s]*(?:pub\s+)?trait\s+(\w+)", "trait"),
        ];
        self.apply_patterns(content, patterns, elements);
    }

    fn extract_python_elements(&self, content: &str, elements: &mut Vec<CodeElement>) {
        let patterns = vec![
            (r"(?m)^[\s]*(?:async\s+)?def\s+(\w+)", "fn"),
            (r"(?m)^[\s]*class\s+(\w+)", "class"),
        ];
        self.apply_patterns(content, patterns, elements);
    }

    fn extract_js_ts_elements(&self, content: &str, elements: &mut Vec<CodeElement>) {
        let patterns = vec![
            (
                r"(?m)^[\s]*(?:export\s+)?(?:async\s+)?function\s+(\w+)",
                "fn",
            ),
            (r"(?m)^[\s]*(?:export\s+)?class\s+(\w+)", "class"),
            (
                r"(?m)^[\s]*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?\(",
                "fn",
            ),
            (r"(?m)^[\s]*(?:export\s+)?interface\s+(\w+)", "interface"),
            (r"(?m)^[\s]*(?:export\s+)?type\s+(\w+)\s*=", "type"),
        ];
        self.apply_patterns(content, patterns, elements);
    }

    fn extract_go_elements(&self, content: &str, elements: &mut Vec<CodeElement>) {
        let patterns = vec![
            (r"(?m)^[\s]*func\s+(\w+)", "fn"),
            (r"(?m)^[\s]*func\s+\(\w+\s+\*?\w+\)\s+(\w+)", "fn"),
            (r"(?m)^[\s]*type\s+(\w+)\s+struct", "struct"),
            (r"(?m)^[\s]*type\s+(\w+)\s+interface", "interface"),
        ];
        self.apply_patterns(content, patterns, elements);
    }

    fn extract_java_like_elements(&self, content: &str, elements: &mut Vec<CodeElement>) {
        let patterns = vec![
            (
                r"(?m)^[\s]*(?:public|private|protected)?\s*class\s+(\w+)",
                "class",
            ),
            (
                r"(?m)^[\s]*(?:public|private|protected)?\s*interface\s+(\w+)",
                "interface",
            ),
        ];
        self.apply_patterns(content, patterns, elements);
    }

    fn apply_patterns(
        &self,
        content: &str,
        patterns: Vec<(&str, &str)>,
        elements: &mut Vec<CodeElement>,
    ) {
        for (pattern_str, kind) in patterns {
            if let Ok(regex) = Regex::new(pattern_str) {
                for caps in regex.captures_iter(content) {
                    if let Some(name_match) = caps.get(1) {
                        let name = name_match.as_str().to_string();
                        let line_content = &content[..name_match.start()];
                        let line_number = line_content.lines().count();

                        elements.push(CodeElement {
                            kind: kind.to_string(),
                            name,
                            line_number,
                        });
                    }
                }
            }
        }
    }
}

impl Extractor for CodeExtractor {
    fn extensions(&self) -> &[&str] {
        &[
            "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "cpp", "c", "h", "hpp", "rb",
            "swift", "kt", "scala", "sh", "bash", "zsh", "fish", "ps1", "yaml", "yml", "json",
            "toml", "xml", "css", "scss", "less", "sql", "graphql", "proto", "lua", "r", "php",
            "pl", "ex", "exs", "erl", "hrl", "hs", "ml", "mli", "fs", "fsi", "clj", "cljs", "cljc",
            "vim", "el", "lisp", "scm", "rkt", "zig", "nim", "v", "d", "dart", "vue", "svelte",
        ]
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
        let language = self.detect_language(path);
        let elements = self.extract_code_elements(&content, language);

        let lines: Vec<&str> = content.lines().collect();

        let sections: Vec<Section> = if !elements.is_empty() {
            // Use code elements as sections with previews
            let mut sections = Vec::with_capacity(elements.len());
            for (i, elem) in elements.iter().enumerate() {
                let start_line = elem.line_number;
                let end_line = elements
                    .get(i + 1)
                    .map(|next| next.line_number)
                    .unwrap_or(lines.len());

                // Extract preview (first ~150 chars of the function/class)
                let preview = if start_line < lines.len() {
                    let preview_lines: String = lines[start_line..end_line.min(start_line + 5)]
                        .join(" ")
                        .trim()
                        .to_string();
                    let trimmed = preview_lines.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        let preview_len = 150.min(trimmed.len());
                        let mut preview: String = trimmed.chars().take(preview_len).collect();
                        if preview_len < trimmed.len() {
                            preview.push_str("...");
                        }
                        Some(preview)
                    }
                } else {
                    None
                };

                sections.push(Section {
                    id: format!("{}:{}", elem.kind, elem.name),
                    index: i,
                    title: format!("{} {}", elem.kind, elem.name),
                    depth: 0,
                    start_page: None,
                    end_page: None,
                    preview,
                });
            }
            sections
        } else {
            // Fallback to line-based sections
            let total_lines = lines.len();
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
            sections
        };

        Ok(DocumentStructure {
            path: path.to_string_lossy().to_string(),
            file_type: FileType::Code,
            title: path.file_name().and_then(|n| n.to_str()).map(String::from),
            author: None,
            sections,
            total_pages: None,
            total_chapters: None,
        })
    }

    fn get_section(&self, path: &Path, section_id: &str) -> Result<SectionContent, ConnectorError> {
        let content = self.read_file(path)?;
        let lines: Vec<&str> = content.lines().collect();

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

        // Handle line-based section IDs like "lines:10-50"
        if let Some(range_str) = section_id.strip_prefix("lines:") {
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

            let total_lines = lines.len();
            let start_idx = start.saturating_sub(1);
            let end_idx = end.min(total_lines);

            let section_content = lines[start_idx..end_idx].join("\n");
            let word_count = section_content.split_whitespace().count();

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

            return Ok(SectionContent {
                path: path.to_string_lossy().to_string(),
                section_id: section_id.to_string(),
                title: Some(format!("Lines {}-{}", start, end)),
                content: section_content,
                word_count,
                prev_section: prev,
                next_section: next,
                truncated: false,
                original_char_count: None,
            });
        }

        // Handle code element section IDs like "fn:function_name" or "class:ClassName"
        let parts: Vec<&str> = section_id.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid section ID: {}. Expected format: KIND:NAME or lines:START-END",
                section_id
            )));
        }

        let kind = parts[0];
        let name = parts[1];

        let language = self.detect_language(path);
        let elements = self.extract_code_elements(&content, language);

        // Find the matching element
        let elem = elements
            .iter()
            .find(|e| e.kind == kind && e.name == name)
            .ok_or_else(|| ConnectorError::ResourceNotFound)?;

        // Find the end of this element (naive: until next element or EOF)
        let start_line = elem.line_number;
        let mut end_line = lines.len();

        for next_elem in elements.iter() {
            if next_elem.line_number > elem.line_number {
                end_line = next_elem.line_number;
                break;
            }
        }

        let section_content = lines[start_line..end_line].join("\n");
        let word_count = section_content.split_whitespace().count();

        Ok(SectionContent {
            path: path.to_string_lossy().to_string(),
            section_id: section_id.to_string(),
            title: Some(format!("{} {}", elem.kind, elem.name)),
            content: section_content,
            word_count,
            prev_section: None,
            next_section: None,
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

        let language = self.detect_language(path);
        let elements = self.extract_code_elements(&content, language);

        let mut matches = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            if let Some(col) = line.to_lowercase().find(&query_lower) {
                let start = line_num.saturating_sub(context_lines);
                let end = (line_num + context_lines + 1).min(lines.len());
                let context = lines[start..end].join("\n");

                // Find which code element this line belongs to
                let section_id = if !elements.is_empty() {
                    elements
                        .iter()
                        .rev()
                        .find(|e| e.line_number <= line_num)
                        .map(|e| format!("{}:{}", e.kind, e.name))
                } else {
                    Some(format!(
                        "lines:{}-{}",
                        (line_num / 100) * 100 + 1,
                        ((line_num / 100) + 1) * 100
                    ))
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
