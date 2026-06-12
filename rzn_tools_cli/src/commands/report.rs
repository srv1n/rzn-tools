use rzn_tools_core::flow_failure::{
    build_tool_flow_failure_draft, classify_tool_failure, FlowFailureReportDraft,
};

use crate::cli::{ReportAction, ToolBrokenReportArgs};
use crate::commands::Result;

pub async fn run(action: ReportAction) -> Result<()> {
    match action {
        ReportAction::ToolBroken(args) => run_tool_broken(args).await,
    }
}

async fn run_tool_broken(args: ToolBrokenReportArgs) -> Result<()> {
    let draft = build_tool_failure_draft(
        &args.connector,
        &args.tool,
        &args.error,
        &args.flow_version,
        args.note.as_deref(),
    );

    println!("{}", serde_json::to_string_pretty(&draft)?);
    Ok(())
}

pub fn build_tool_failure_draft(
    connector: &str,
    tool: &str,
    raw_error: &str,
    flow_version: &str,
    note: Option<&str>,
) -> FlowFailureReportDraft {
    build_tool_flow_failure_draft(
        connector,
        tool,
        flow_version,
        raw_error,
        env!("CARGO_PKG_VERSION"),
        note,
    )
}

pub fn render_tool_failure_report_block(connector: &str, tool: &str, raw_error: &str) -> String {
    let draft =
        build_tool_failure_draft(connector, tool, raw_error, env!("CARGO_PKG_VERSION"), None);
    let draft_json = serde_json::to_string_pretty(&draft).unwrap_or_else(|_| "{}".to_string());
    let class = classify_tool_failure(raw_error);

    [
        format!("Tool failed: {}/{}", draft.surface, tool),
        format!("Classified as: {} / {}", class.failed_stage, class.error),
        String::new(),
        "Flow failure draft for the host to submit:".to_string(),
        draft_json,
        String::new(),
        "The draft excludes args, payloads, outputs, logs, provider ids, paths, and tokens."
            .to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manual_command_body_is_canonical_draft() {
        let draft = build_tool_failure_draft(
            "youtube",
            "search",
            "selector missing at https://example.com/search?q=private",
            "0.2.17",
            Some("  Search box disappeared.  "),
        );

        assert_eq!(
            serde_json::to_value(draft).unwrap(),
            json!({
                "schema_version": 1,
                "submission_mode": "host_auto",
                "source": "rzn-tools",
                "product": "rzn-tools",
                "flow_kind": "tool",
                "surface": "youtube",
                "flow": "youtube/search-v1",
                "flow_version": "0.2.17",
                "failed_stage": "api_call",
                "error": "provider_unavailable",
                "app_version": env!("CARGO_PKG_VERSION"),
                "platform": rzn_tools_core::flow_failure::platform_family(),
                "note": "Search box disappeared."
            })
        );
    }

    #[test]
    fn rendered_block_does_not_include_raw_private_error() {
        let block = render_tool_failure_report_block(
            "web",
            "scrape",
            "selector missing at https://example.com/search?q=private",
        );

        assert!(block.contains("Flow failure draft"));
        assert!(block.contains("\"source\": \"rzn-tools\""));
        assert!(block.contains("\"flow\": \"web/scrape-v1\""));
        assert!(!block.contains("https://example.com"));
        assert!(!block.contains("private"));
        assert!(!block.contains("selector missing"));
    }
}
