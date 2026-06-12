use serde::{Deserialize, Serialize};

const MAX_NOTE_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowFailureReportDraft {
    pub schema_version: u8,
    pub submission_mode: String,
    pub source: String,
    pub product: String,
    pub flow_kind: String,
    pub surface: String,
    pub flow: String,
    pub flow_version: String,
    pub failed_stage: String,
    pub error: String,
    pub app_version: String,
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolFailureClass {
    pub failed_stage: &'static str,
    pub error: &'static str,
}

pub fn build_tool_flow_failure_draft(
    surface: &str,
    tool: &str,
    flow_version: &str,
    raw_error: &str,
    app_version: &str,
    note: Option<&str>,
) -> FlowFailureReportDraft {
    let surface = safe_flow_segment(surface);
    let tool = safe_flow_segment(tool);
    let class = classify_tool_failure(raw_error);

    FlowFailureReportDraft {
        schema_version: 1,
        submission_mode: "host_auto".to_string(),
        source: "rzn-tools".to_string(),
        product: "rzn-tools".to_string(),
        flow_kind: "tool".to_string(),
        flow: format!("{surface}/{tool}-v1"),
        surface,
        flow_version: safe_version(flow_version),
        failed_stage: class.failed_stage.to_string(),
        error: class.error.to_string(),
        app_version: safe_version(app_version),
        platform: platform_family().to_string(),
        note: note.and_then(trim_note),
    }
}

pub fn classify_tool_failure(raw_error: &str) -> ToolFailureClass {
    let raw = raw_error.to_ascii_lowercase();

    if contains_any(
        &raw,
        &[
            "invalid input",
            "invalid params",
            "missing required",
            "bad request",
        ],
    ) {
        class("validate_args", "invalid_args")
    } else if contains_any(
        &raw,
        &[
            "auth",
            "token",
            "credential",
            "unauthorized",
            "login",
            "sign in",
        ],
    ) {
        class("auth_check", "auth_required")
    } else if contains_any(
        &raw,
        &[
            "permission",
            "forbidden",
            "access denied",
            "not allowed",
            "operation not permitted",
        ],
    ) {
        class("permission_check", "permission_denied")
    } else if contains_any(
        &raw,
        &["rate limit", "rate-limit", "too many requests", "429"],
    ) {
        class("api_call", "rate_limited")
    } else if contains_any(&raw, &["timeout", "timed out", "deadline elapsed"]) {
        class("api_call", "timeout")
    } else if contains_any(&raw, &["file not found", "no such file", "not a file"]) {
        class("api_call", "file_not_found")
    } else if contains_any(
        &raw,
        &[
            "write failed",
            "failed to write",
            "read-only",
            "readonly",
            "broken pipe",
        ],
    ) {
        class("write_result", "write_failed")
    } else if contains_any(
        &raw,
        &[
            "parse",
            "deserialize",
            "invalid json",
            "schema",
            "invalid response",
        ],
    ) {
        class("parse_response", "invalid_response")
    } else if contains_any(
        &raw,
        &[
            "dns",
            "tls",
            "connection",
            "connect",
            "http",
            "upstream",
            "provider unavailable",
            "503",
            "502",
        ],
    ) {
        class("api_call", "provider_unavailable")
    } else if contains_any(&raw, &["api error", "provider error", "status code"]) {
        class("api_call", "api_error")
    } else {
        class("api_call", "unknown_failure")
    }
}

pub fn platform_family() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn class(failed_stage: &'static str, error: &'static str) -> ToolFailureClass {
    ToolFailureClass {
        failed_stage,
        error,
    }
}

fn contains_any(raw: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| raw.contains(needle))
}

fn safe_flow_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_sep = false;

    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_sep = false;
        } else if matches!(ch, '-' | '_') && !last_sep {
            out.push(ch);
            last_sep = true;
        } else if !last_sep {
            out.push('-');
            last_sep = true;
        }
    }

    let trimmed = out.trim_matches('-').trim_matches('_').to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

fn safe_version(value: &str) -> String {
    let trimmed = value.trim();
    let safe: String = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+'))
        .take(80)
        .collect();
    if safe.is_empty() {
        "unknown".to_string()
    } else {
        safe
    }
}

fn trim_note(note: &str) -> Option<String> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(MAX_NOTE_CHARS).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const FORBIDDEN_KEYS: &[&str] = &[
        "args",
        "arguments",
        "request",
        "response",
        "payload",
        "logs",
        "log",
        "run_id",
        "trace_id",
        "provider",
        "provider_id",
        "stdout",
        "stderr",
        "token",
        "path",
        "filename",
    ];

    #[test]
    fn representative_payloads_match_canonical_shape() {
        let auth = build_tool_flow_failure_draft(
            "gmail",
            "send-email",
            "0.2.17",
            "Unauthorized token for alice@example.com",
            "0.2.17",
            None,
        );
        assert_eq!(auth.failed_stage, "auth_check");
        assert_eq!(auth.error, "auth_required");

        let api = build_tool_flow_failure_draft(
            "slack",
            "post-message",
            "0.2.17",
            "429 too many requests for channel C123",
            "0.2.17",
            None,
        );
        assert_eq!(api.failed_stage, "api_call");
        assert_eq!(api.error, "rate_limited");

        let parse = build_tool_flow_failure_draft(
            "web",
            "scrape-url",
            "0.2.17",
            "invalid JSON response body: {\"secret\":\"value\"}",
            "0.2.17",
            None,
        );
        assert_eq!(parse.failed_stage, "parse_response");
        assert_eq!(parse.error, "invalid_response");

        let file = build_tool_flow_failure_draft(
            "filesystem",
            "read-file",
            "0.2.17",
            "file not found: /Users/sarav/private.txt",
            "0.2.17",
            None,
        );
        assert_eq!(
            serde_json::to_value(file).unwrap(),
            json!({
                "schema_version": 1,
                "submission_mode": "host_auto",
                "source": "rzn-tools",
                "product": "rzn-tools",
                "flow_kind": "tool",
                "surface": "filesystem",
                "flow": "filesystem/read-file-v1",
                "flow_version": "0.2.17",
                "failed_stage": "api_call",
                "error": "file_not_found",
                "app_version": "0.2.17",
                "platform": platform_family()
            })
        );
    }

    #[test]
    fn forbidden_keys_are_absent_from_payload() {
        let draft = build_tool_flow_failure_draft(
            "slack",
            "post-message",
            "0.2.17",
            "Slack API response {\"channel\":\"C123\",\"text\":\"secret\"}",
            "0.2.17",
            Some("user says selector changed"),
        );
        let value = serde_json::to_value(draft).unwrap();
        let object = value.as_object().unwrap();

        for key in FORBIDDEN_KEYS {
            assert!(!object.contains_key(*key), "forbidden key leaked: {key}");
        }
    }

    #[test]
    fn private_provider_errors_are_normalized() {
        let raw = "Slack API error: channel C123 user U456 message text 'private' response {\"ok\":false}";
        let draft =
            build_tool_flow_failure_draft("slack", "post-message", "0.2.17", raw, "0.2.17", None);
        let serialized = serde_json::to_string(&draft).unwrap();

        assert_eq!(draft.failed_stage, "api_call");
        assert_eq!(draft.error, "api_error");
        assert!(!serialized.contains("C123"));
        assert!(!serialized.contains("U456"));
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("response"));
    }

    #[test]
    fn serialized_payload_has_only_canonical_fields() {
        let draft = build_tool_flow_failure_draft(
            "google-gmail",
            "send-email",
            "0.2.17",
            "SMTP response included recipient bob@example.com",
            "0.2.17",
            None,
        );
        let value = serde_json::to_value(draft).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(
            keys,
            vec![
                "app_version",
                "error",
                "failed_stage",
                "flow",
                "flow_kind",
                "flow_version",
                "platform",
                "product",
                "schema_version",
                "source",
                "submission_mode",
                "surface",
            ]
        );
        assert_no_raw_leak(&value, "bob@example.com");
    }

    fn assert_no_raw_leak(value: &Value, needle: &str) {
        assert!(!serde_json::to_string(value).unwrap().contains(needle));
    }
}
