#[cfg(all(test, feature = "all-connectors"))]
mod tests {
    use crate::commands::tool_mappings::{generic_get_tool_and_args, generic_search_tool_and_args};
    use crate::commands::Result;
    use std::collections::{BTreeSet, HashMap};
    use tokio::runtime::Runtime;

    fn extract_string_literal(s: &str) -> Option<(String, &str)> {
        // Returns (literal, rest_after_literal)
        let s = s.trim_start();
        if !s.starts_with('"') {
            return None;
        }
        let rest = &s[1..];
        let end = rest.find('"')?;
        let lit = rest[..end].to_string();
        Some((lit, &rest[end + 1..]))
    }

    fn parse_call_tool_raw(line: &str) -> Option<(String, String)> {
        let idx = line.find("call_tool_raw(")?;
        let mut rest = &line[idx + "call_tool_raw(".len()..];
        let (connector, r1) = extract_string_literal(rest)?;
        rest = r1.trim_start();
        rest = rest.strip_prefix(',')?.trim_start();
        let (tool, _r2) = extract_string_literal(rest)?;
        Some((connector, tool))
    }

    fn parse_call_tool(line: &str) -> Option<(String, Option<String>, bool)> {
        // Returns (connector, tool_if_literal, uses_tool_name_var)
        let idx = line.find("call_tool(cli,")?;
        let mut rest = &line[idx + "call_tool(cli,".len()..];
        rest = rest.trim_start();
        let (connector, r1) = extract_string_literal(rest)?;
        rest = r1.trim_start();
        rest = rest.strip_prefix(',')?.trim_start();

        if rest.starts_with("tool_name") {
            return Some((connector, None, true));
        }

        let (tool, _r2) = extract_string_literal(rest)?;
        Some((connector, Some(tool), false))
    }

    fn extract_tuple_tool_names(line: &str) -> Vec<String> {
        // Extracts the first string literal from patterns like ("tool", args)
        let mut out = Vec::new();
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("(\"") || trimmed.starts_with("return (\"")) {
            return out;
        }
        if !(line.contains(", args)") || line.contains(", tool_args)")) {
            return out;
        }
        let mut rest = line;
        while let Some(idx) = rest.find("(\"") {
            rest = &rest[idx + 1..]; // starts at '"'
            if let Some((tool, r1)) = extract_string_literal(rest) {
                out.push(tool);
                rest = r1;
            } else {
                break;
            }
        }
        out
    }

    fn parse_cli_tool_calls() -> HashMap<String, BTreeSet<String>> {
        let src = include_str!("connectors.rs");
        let mut out: HashMap<String, BTreeSet<String>> = HashMap::new();

        let mut current_fn: Option<String> = None;
        let mut pending_tools: BTreeSet<String> = BTreeSet::new();

        for line in src.lines() {
            if line.contains("pub async fn handle_") {
                current_fn = Some(line.to_string());
                pending_tools.clear();
                continue;
            }

            for tool in extract_tuple_tool_names(line) {
                pending_tools.insert(tool);
            }

            if let Some((connector, tool)) = parse_call_tool_raw(line) {
                out.entry(connector).or_default().insert(tool);
            }

            if let Some((connector, tool_opt, uses_tool_name_var)) = parse_call_tool(line) {
                if let Some(tool) = tool_opt {
                    out.entry(connector).or_default().insert(tool);
                } else if uses_tool_name_var && current_fn.is_some() {
                    out.entry(connector)
                        .or_default()
                        .extend(pending_tools.iter().cloned());
                    pending_tools.clear();
                    current_fn = None;
                }
            }
        }

        out
    }

    #[test]
    fn cli_tool_calls_match_core_tools() -> Result<()> {
        let rt = Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let registry = crate::commands::list::create_registry(None).await?;
            let cli_calls = parse_cli_tool_calls();

            for (connector, tools) in cli_calls {
                let Some(provider) = registry.get_provider(&connector) else {
                    return Err(crate::commands::CommandError::ConnectorNotFound(connector));
                };

                let c = provider.lock().await;
                let tools_response = c.list_tools(None).await?;
                let available: BTreeSet<String> = tools_response
                    .tools
                    .iter()
                    .map(|t| t.name.as_ref().to_string())
                    .collect();

                for tool in tools {
                    if !available.contains(&tool) {
                        return Err(crate::commands::CommandError::ToolNotFound(tool, connector));
                    }
                }
            }

            // Also validate the generic `get`/`search` command mappings (these do not go through
            // connectors.rs and shouldn't rely on heuristics).
            let generic_get_samples = vec![
                ("youtube", "dQw4w9WgXcQ"),
                (
                    "reddit",
                    "https://www.reddit.com/r/rust/comments/abc123/example/",
                ),
                ("hackernews", "38500000"),
                ("wikipedia", "Rust (programming language)"),
                ("arxiv", "2301.07041"),
                ("pubmed", "34762503"),
                ("polymarket", "will-bitcoin-hit-150k-in-2026"),
                ("semantic-scholar", "10.1038/nature12373"),
                ("github", "rust-lang/rust"),
            ];

            for (connector, id) in generic_get_samples {
                let (tool, _args) = generic_get_tool_and_args(connector, id)?;
                let provider = registry.get_provider(connector).ok_or_else(|| {
                    crate::commands::CommandError::ConnectorNotFound(connector.into())
                })?;
                let c = provider.lock().await;
                let tools_response = c.list_tools(None).await?;
                let exists = tools_response.tools.iter().any(|t| t.name.as_ref() == tool);
                if !exists {
                    return Err(crate::commands::CommandError::ToolNotFound(
                        tool.to_string(),
                        connector.to_string(),
                    ));
                }
            }

            let generic_search_samples = vec![
                ("youtube", "rust programming", 5),
                ("reddit", "tokio", 5),
                ("hackernews", "rust", 5),
                ("wikipedia", "quantum computing", 5),
                ("arxiv", "transformer", 5),
                ("pubmed", "CRISPR", 5),
                ("polymarket", "bitcoin", 5),
                ("semantic-scholar", "attention is all you need", 5),
                ("github", "language:rust stars:>5000", 5),
            ];

            for (connector, query, limit) in generic_search_samples {
                let (tool, _args) = generic_search_tool_and_args(connector, query, limit)?;
                let provider = registry.get_provider(connector).ok_or_else(|| {
                    crate::commands::CommandError::ConnectorNotFound(connector.into())
                })?;
                let c = provider.lock().await;
                let tools_response = c.list_tools(None).await?;
                let exists = tools_response.tools.iter().any(|t| t.name.as_ref() == tool);
                if !exists {
                    return Err(crate::commands::CommandError::ToolNotFound(
                        tool.to_string(),
                        connector.to_string(),
                    ));
                }
            }

            Ok(())
        })
    }
}
