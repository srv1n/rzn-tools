//! Output formats and paging helpers shared by connectors. The item shape itself is in
//! [`crate::item`].

use crate::error::ConnectorError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

/// The `output_format` argument values every ingest tool accepts.
pub const OUTPUT_FORMATS: [&str; 3] = ["raw", "item_v2", "display_v1"];

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Raw,
    /// `rzn.item/v2` for a get, `rzn.items/v2` for a list or search.
    ItemV2,
    DisplayV1,
}

impl OutputFormat {
    pub fn is_item(self) -> bool {
        matches!(self, OutputFormat::ItemV2)
    }

    pub fn is_display(self) -> bool {
        matches!(self, OutputFormat::DisplayV1)
    }
}

pub fn output_format_from_args(
    args: &JsonMap<String, JsonValue>,
) -> Result<OutputFormat, ConnectorError> {
    match args.get("output_format") {
        None => Ok(OutputFormat::Raw),
        Some(value) => match value.as_str() {
            Some("raw") => Ok(OutputFormat::Raw),
            Some("item_v2") => Ok(OutputFormat::ItemV2),
            Some("display_v1") => Ok(OutputFormat::DisplayV1),
            Some(other) => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'output_format': '{}'. Expected 'raw', 'item_v2', or 'display_v1'.",
                other
            ))),
            None => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'output_format': expected a string ('raw', 'item_v2', or 'display_v1'), got {}",
                value
            ))),
        },
    }
}

/// The JSON Schema property every ingest tool declares for `output_format`.
pub fn output_format_property() -> JsonValue {
    json!({
        "type": "string",
        "enum": OUTPUT_FORMATS,
        "default": "raw",
        "description": "Default raw. Use item_v2 for ingestion (rzn.item/v2 for a get, rzn.items/v2 for a list). Use display_v1 for UI-friendly output."
    })
}

pub fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ConnectorError> {
    let bytes = serde_json::to_vec(cursor).map_err(|e| ConnectorError::Other(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Option<T> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRefParts {
    pub connector: String,
    pub kind: String,
    pub id: String,
}

/// Parse an item key of the form "{connector}:{kind}:{id}".
/// Uses splitn(3) so that the id segment may contain ':' characters.
pub fn parse_item_ref(item_ref: &str) -> Option<ItemRefParts> {
    let mut parts = item_ref.splitn(3, ':');
    let connector = parts.next()?.trim();
    let kind = parts.next()?.trim();
    let id = parts.next()?.trim();
    if connector.is_empty() || kind.is_empty() || id.is_empty() {
        return None;
    }
    Some(ItemRefParts {
        connector: connector.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
    })
}

/// Parse an item key and ensure it matches the expected connector.
pub fn parse_item_ref_for_connector(item_ref: &str, connector: &str) -> Option<(String, String)> {
    let parts = parse_item_ref(item_ref)?;
    if parts.connector != connector {
        return None;
    }
    Some((parts.kind, parts.id))
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_parses() {
        let mut args = JsonMap::new();
        assert_eq!(output_format_from_args(&args).unwrap(), OutputFormat::Raw);
        args.insert("output_format".to_string(), json!("item_v2"));
        assert_eq!(
            output_format_from_args(&args).unwrap(),
            OutputFormat::ItemV2
        );
        args.insert("output_format".to_string(), json!("display_v1"));
        assert_eq!(
            output_format_from_args(&args).unwrap(),
            OutputFormat::DisplayV1
        );
        args.insert("output_format".to_string(), json!("normalized_v1"));
        assert!(output_format_from_args(&args).is_err());
    }

    #[test]
    fn output_format_rejects_non_string() {
        let mut args = JsonMap::new();
        args.insert("output_format".to_string(), json!({ "value": "item_v2" }));
        let err = output_format_from_args(&args).unwrap_err();
        assert!(matches!(err, ConnectorError::InvalidParams(_)));
    }

    #[test]
    fn cursor_round_trip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Cursor {
            after: String,
            count: usize,
        }

        let cursor = Cursor {
            after: "t3_abc".to_string(),
            count: 42,
        };
        let encoded = encode_cursor(&cursor).expect("encode");
        let decoded: Cursor = decode_cursor(&encoded).expect("decode");
        assert_eq!(cursor, decoded);
    }
}
