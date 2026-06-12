use crate::cli::Cli;
use crate::commands::tool_mappings::generic_get_tool_and_args;
use crate::commands::{copy_to_clipboard, CommandError, Result};
use crate::output::{format_output, format_pretty, OutputData};
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use rzn_tools_core::error::ConnectorError;
use rzn_tools_core::{CallToolRequestParam, CallToolResult, Connector, ProviderRegistry};
use serde_json::{json, Value};

pub async fn run(cli: &Cli, connector_name: &str, id: &str, field: Option<&str>) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Invalid progress template"),
    );
    spinner.set_message(format!("Fetching {} from {}...", id, connector_name));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let registry = create_registry(cli.auth_profile.as_deref()).await?;
    let provider = registry
        .get_provider(connector_name)
        .ok_or_else(|| CommandError::ConnectorNotFound(connector_name.to_string()))?
        .clone();

    let c = provider.lock().await;
    let response = if connector_name == "polymarket" && is_numeric_polymarket_id(id) {
        resolve_polymarket_numeric_get(&**c, id).await?
    } else if connector_name == "kalshi" && is_ambiguous_kalshi_id(id) {
        resolve_kalshi_ticker_get(&**c, id).await?
    } else {
        let (tool_name, arguments) = generic_get_tool_and_args(connector_name, id)?;
        let request = CallToolRequestParam {
            name: tool_name.into(),
            arguments: Some(arguments),
        };
        c.call_tool(request).await?
    };
    spinner.finish_and_clear();

    // Extract response data
    let data = if let Some(val) = &response.structured_content {
        val.clone()
    } else {
        json!({})
    };

    let data = if let Some(field) = field {
        select_top_level_field(&data, field)?
    } else {
        data
    };

    if field.is_some() && cli.output == crate::cli::OutputFormat::Text {
        print_field_text(&data)?;
    } else {
        let output_data = OutputData::ResourceData {
            connector: connector_name.to_string(),
            id: id.to_string(),
            data: data.clone(),
        };

        match cli.output {
            crate::cli::OutputFormat::Pretty if field.is_none() => {
                format_pretty_resource_data(connector_name, id, &data)?;
            }
            crate::cli::OutputFormat::Pretty => {
                println!("{}", format_pretty(&data));
            }
            _ => {
                format_output(&output_data, &cli.output)?;
            }
        }
    }

    // Copy to clipboard if requested
    if cli.copy {
        let text = match data.as_str() {
            Some(text) if field.is_some() && cli.output == crate::cli::OutputFormat::Text => {
                text.to_string()
            }
            _ => serde_json::to_string_pretty(&data)?,
        };
        copy_to_clipboard(&text)?;
    }

    Ok(())
}

fn select_top_level_field(data: &Value, field: &str) -> Result<Value> {
    let field = field.trim();
    if field.is_empty() {
        return Err(CommandError::InvalidInput(
            "--field requires a non-empty field name".to_string(),
        ));
    }

    data.get(field).cloned().ok_or_else(|| {
        CommandError::InvalidInput(format!("Field '{}' not found in resource payload", field))
    })
}

fn print_field_text(data: &Value) -> Result<()> {
    if let Some(text) = data.as_str() {
        println!("{}", text);
    } else {
        println!("{}", serde_json::to_string_pretty(data)?);
    }
    Ok(())
}

fn is_numeric_polymarket_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

fn is_ambiguous_kalshi_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with("http://")
        && !id.starts_with("https://")
        && !id.starts_with("kalshi:")
}

fn polymarket_numeric_requests(id: &str) -> Vec<CallToolRequestParam> {
    vec![
        CallToolRequestParam {
            name: "get".into(),
            arguments: Some(
                json!({ "id": id, "output_format": "display_v1" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
        CallToolRequestParam {
            name: "get_market".into(),
            arguments: Some(
                json!({ "id": id, "output_format": "display_v1" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
        CallToolRequestParam {
            name: "get_series".into(),
            arguments: Some(
                json!({ "id": id })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
    ]
}

async fn resolve_polymarket_numeric_get(
    connector: &dyn Connector,
    id: &str,
) -> Result<CallToolResult> {
    let mut successes = Vec::new();
    let mut first_non_not_found = None;

    for request in polymarket_numeric_requests(id) {
        match connector.call_tool(request).await {
            Ok(result) => successes.push(result),
            Err(ConnectorError::ResourceNotFound) => {}
            Err(err) => {
                if first_non_not_found.is_none() {
                    first_non_not_found = Some(err);
                }
            }
        }
    }

    match successes.len() {
        1 => Ok(successes.remove(0)),
        0 => {
            if let Some(err) = first_non_not_found {
                Err(CommandError::Core(err))
            } else {
                Err(CommandError::Core(ConnectorError::ResourceNotFound))
            }
        }
        _ => Err(CommandError::InvalidInput(format!(
            "Polymarket id '{}' is ambiguous. Use an explicit item_ref like polymarket:event:{0}, polymarket:market:{0}, or polymarket:series:{0}, or use `rzn-tools polymarket ...`.",
            id
        ))),
    }
}

fn kalshi_ticker_requests(id: &str) -> Vec<CallToolRequestParam> {
    vec![
        CallToolRequestParam {
            name: "get".into(),
            arguments: Some(
                json!({ "ticker": id, "output_format": "display_v1" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
        CallToolRequestParam {
            name: "get_market".into(),
            arguments: Some(
                json!({ "ticker": id, "output_format": "display_v1" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
        CallToolRequestParam {
            name: "get_series".into(),
            arguments: Some(
                json!({ "ticker": id, "output_format": "display_v1" })
                    .as_object()
                    .expect("json object")
                    .clone(),
            ),
        },
    ]
}

async fn resolve_kalshi_ticker_get(connector: &dyn Connector, id: &str) -> Result<CallToolResult> {
    let mut successes = Vec::new();
    let mut first_non_not_found = None;

    for request in kalshi_ticker_requests(id) {
        match connector.call_tool(request).await {
            Ok(result) => successes.push(result),
            Err(ConnectorError::ResourceNotFound) => {}
            Err(err) => {
                if first_non_not_found.is_none() {
                    first_non_not_found = Some(err);
                }
            }
        }
    }

    match successes.len() {
        1 => Ok(successes.remove(0)),
        0 => {
            if let Some(err) = first_non_not_found {
                Err(CommandError::Core(err))
            } else {
                Err(CommandError::Core(ConnectorError::ResourceNotFound))
            }
        }
        _ => Err(CommandError::InvalidInput(format!(
            "Kalshi ticker '{}' is ambiguous. Use an explicit item_ref like kalshi:event:{0}, kalshi:market:{0}, or kalshi:series:{0}, or use `rzn-tools kalshi ...`.",
            id
        ))),
    }
}

fn format_pretty_resource_data(connector: &str, id: &str, data: &Value) -> Result<()> {
    println!(
        "{} {} {}",
        "Resource:".bold().cyan(),
        id.yellow(),
        format!("({})", connector).dimmed()
    );
    println!();

    match connector {
        "youtube" => format_youtube_data(data)?,
        "reddit" => format_reddit_data(data)?,
        "hackernews" => format_hackernews_data(data)?,
        "wikipedia" => format_wikipedia_data(data)?,
        "arxiv" => format_arxiv_data(data)?,
        "pubmed" => format_pubmed_data(data)?,
        _ => {
            // Generic smart formatting for other connectors
            println!("{}", format_pretty(data));
        }
    }

    Ok(())
}

fn format_youtube_data(data: &Value) -> Result<()> {
    // Title as main heading
    if let Some(title) = data.get("title") {
        println!("# {}", title.as_str().unwrap_or("").bold());
        println!();
    }

    // Description as first paragraph
    if let Some(description) = data.get("description") {
        let desc = description.as_str().unwrap_or("");
        if !desc.is_empty() {
            println!("{}", desc);
            println!();
        }
    }

    // Full transcript if available
    // if let Some(transcript) = data.get("transcript") {
    //     if let Some(transcript_str) = transcript.as_str() {
    //         if !transcript_str.is_empty() {
    //             // println!("## {}", "Full Transcript".bold());
    //             println!("{}", transcript_str);
    //             println!();
    //         }
    //     }
    // }

    // Chapters with full content
    if let Some(chapters) = data.get("chapters").and_then(|c| c.as_array()) {
        if !chapters.is_empty() {
            println!("## {}", "Chapters".bold());
            println!();

            for chapter in chapters {
                if let (Some(heading), Some(start_time)) =
                    (chapter.get("heading"), chapter.get("start_time"))
                {
                    let time = start_time.as_i64().unwrap_or(0);
                    let mins = time / 60;
                    let secs = time % 60;

                    println!(
                        "### {} ({}:{:02})",
                        heading.as_str().unwrap_or("").bold(),
                        mins,
                        secs
                    );

                    if let Some(content) = chapter.get("content") {
                        let content_str = content.as_str().unwrap_or("");
                        if !content_str.is_empty() {
                            println!("{}", content_str);
                            println!();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_reddit_data(data: &Value) -> Result<()> {
    const MAX_COMMENT_PREVIEW: usize = 25;
    const MAX_POST_BODY_CHARS: usize = 2_000;
    const MAX_COMMENT_BODY_CHARS: usize = 600;
    const MAX_COMMENT_INDENT_LEVEL: usize = 6;

    let Some(post) = data.get("post") else {
        println!("{}", format_pretty(data));
        return Ok(());
    };

    let title = post
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !title.is_empty() {
        println!("# {}", title.bold());
        println!();
    }

    let subreddit = post
        .get("subreddit")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let author = post
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let score = post.get("score").and_then(Value::as_i64).unwrap_or(0);
    let num_comments = post
        .get("num_comments")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let mut meta = Vec::new();
    if !subreddit.is_empty() {
        meta.push(format!("r/{subreddit}"));
    }
    if !author.is_empty() {
        meta.push(format!("u/{author}"));
    }
    meta.push(format!("score {score}"));
    meta.push(format!("comments {num_comments}"));
    println!("{}", meta.join(" | ").dimmed());

    let permalink = post
        .get("permalink")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let canonical_url = if permalink.is_empty() {
        None
    } else if permalink.starts_with("http://") || permalink.starts_with("https://") {
        Some(permalink.to_string())
    } else {
        Some(format!("https://www.reddit.com{permalink}"))
    };

    if let Some(url) = canonical_url.as_deref() {
        println!("{}", url.blue());
    }

    let external_url = post.get("url").and_then(Value::as_str).unwrap_or("").trim();
    if !external_url.is_empty() && canonical_url.as_deref() != Some(external_url) {
        println!("{} {}", "external:".dimmed(), external_url.blue());
    }
    println!();

    let selftext = post
        .get("selftext")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !selftext.is_empty() {
        let clipped = truncate_for_cli(selftext, MAX_POST_BODY_CHARS);
        println!("{}", clipped);
        if clipped.len() < selftext.len() {
            println!(
                "{}",
                "Post body truncated for terminal readability.".dimmed()
            );
        }
        println!();
    }

    let Some(comments) = data.get("comments").and_then(Value::as_array) else {
        println!("{}", "No comments returned.".dimmed());
        return Ok(());
    };

    let flattened = flatten_reddit_comments(comments);
    if flattened.is_empty() {
        println!("{}", "No comments returned.".dimmed());
        return Ok(());
    }

    println!("## {}", "Comments".bold());
    println!();

    for (index, comment) in flattened.iter().take(MAX_COMMENT_PREVIEW).enumerate() {
        let author = comment
            .comment
            .get("author")
            .and_then(Value::as_str)
            .unwrap_or("[deleted]");
        let score = comment
            .comment
            .get("score")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = comment
            .comment
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("[deleted]")
            .trim();
        let body = if body.is_empty() { "[deleted]" } else { body };
        let clipped = truncate_for_cli(body, MAX_COMMENT_BODY_CHARS);
        let indent = "  ".repeat(comment.depth.min(MAX_COMMENT_INDENT_LEVEL));

        println!(
            "{}{}. {} {}",
            indent,
            index + 1,
            author.bold(),
            format!("(score {score})").dimmed()
        );
        println!("{}{}", indent, clipped);
        println!();
    }

    if flattened.len() > MAX_COMMENT_PREVIEW {
        println!(
            "{}",
            format!(
                "Showing {} of {} comments. Use `--output json` for the full payload.",
                MAX_COMMENT_PREVIEW,
                flattened.len()
            )
            .dimmed()
        );
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct RedditCommentView<'a> {
    comment: &'a Value,
    depth: usize,
}

fn flatten_reddit_comments<'a>(comments: &'a [Value]) -> Vec<RedditCommentView<'a>> {
    let mut flattened = Vec::new();
    collect_reddit_comments(comments, 0, &mut flattened);
    flattened
}

fn collect_reddit_comments<'a>(
    comments: &'a [Value],
    depth: usize,
    flattened: &mut Vec<RedditCommentView<'a>>,
) {
    for comment in comments {
        flattened.push(RedditCommentView { comment, depth });
        if let Some(replies) = comment.get("replies").and_then(Value::as_array) {
            collect_reddit_comments(replies, depth + 1, flattened);
        }
    }
}

fn truncate_for_cli(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if max_chars == 0 || trimmed.is_empty() {
        return String::new();
    }

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let keep = max_chars.saturating_sub(3).max(1);
    let mut shortened: String = trimmed.chars().take(keep).collect();
    while shortened.ends_with(char::is_whitespace) {
        shortened.pop();
    }
    format!("{shortened}...")
}

fn format_hackernews_data(data: &Value) -> Result<()> {
    println!("{}", format_pretty(data));
    Ok(())
}

fn format_wikipedia_data(data: &Value) -> Result<()> {
    println!("{}", format_pretty(data));
    Ok(())
}

fn format_arxiv_data(data: &Value) -> Result<()> {
    println!("{}", format_pretty(data));
    Ok(())
}

fn format_pubmed_data(data: &Value) -> Result<()> {
    println!("{}", format_pretty(data));
    Ok(())
}

async fn create_registry(auth_profile: Option<&str>) -> Result<ProviderRegistry> {
    // Reuse the registry creation logic from list.rs
    crate::commands::list::create_registry(auth_profile).await
}

#[cfg(test)]
mod tests {
    use super::{
        flatten_reddit_comments, kalshi_ticker_requests, polymarket_numeric_requests,
        truncate_for_cli,
    };
    use serde_json::json;

    #[test]
    fn truncate_for_cli_preserves_short_text() {
        assert_eq!(truncate_for_cli("hello world", 100), "hello world");
        assert_eq!(truncate_for_cli("  hello world  ", 100), "hello world");
    }

    #[test]
    fn truncate_for_cli_clips_and_adds_ellipsis() {
        assert_eq!(truncate_for_cli("1234567890", 6), "123...");
        assert_eq!(
            truncate_for_cli("abcdefghijklmnopqrstuvwxyz", 12),
            "abcdefghi..."
        );
    }

    #[test]
    fn flatten_reddit_comments_depth_first() {
        let comments = vec![
            json!({
                "id": "c1",
                "replies": [
                    { "id": "c1r1", "replies": [] },
                    {
                        "id": "c1r2",
                        "replies": [
                            { "id": "c1r2r1", "replies": [] }
                        ]
                    }
                ]
            }),
            json!({
                "id": "c2",
                "replies": []
            }),
        ];

        let flattened = flatten_reddit_comments(&comments);
        assert_eq!(flattened.len(), 5);

        assert_eq!(flattened[0].comment["id"].as_str(), Some("c1"));
        assert_eq!(flattened[0].depth, 0);
        assert_eq!(flattened[1].comment["id"].as_str(), Some("c1r1"));
        assert_eq!(flattened[1].depth, 1);
        assert_eq!(flattened[2].comment["id"].as_str(), Some("c1r2"));
        assert_eq!(flattened[2].depth, 1);
        assert_eq!(flattened[3].comment["id"].as_str(), Some("c1r2r1"));
        assert_eq!(flattened[3].depth, 2);
        assert_eq!(flattened[4].comment["id"].as_str(), Some("c2"));
        assert_eq!(flattened[4].depth, 0);
    }

    #[test]
    fn polymarket_numeric_requests_cover_all_numeric_namespaces() {
        let requests = polymarket_numeric_requests("123");
        let tool_names = requests
            .iter()
            .map(|request| request.name.to_string())
            .collect::<Vec<_>>();

        assert_eq!(tool_names, vec!["get", "get_market", "get_series"]);
    }

    #[test]
    fn kalshi_ticker_requests_cover_all_ticker_namespaces() {
        let requests = kalshi_ticker_requests("KXELONMARS-99");
        let tool_names = requests
            .iter()
            .map(|request| request.name.to_string())
            .collect::<Vec<_>>();

        assert_eq!(tool_names, vec!["get", "get_market", "get_series"]);
    }
}
