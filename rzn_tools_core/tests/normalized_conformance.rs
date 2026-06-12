use rmcp::model::PaginatedRequestParam;
use rzn_tools_core::build_registry_enabled_only;
use rzn_tools_core::ingest::{NORMALIZED_ITEM_V1_TYPE, NORMALIZED_PAGE_V1_TYPE};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[tokio::test]
async fn tool_schema_conformance() {
    let registry = build_registry_enabled_only().await;
    let providers = registry.list_providers();

    for provider in providers {
        let connector = registry
            .get_provider(&provider.name)
            .expect("provider exists");
        let c = connector.lock().await;
        let tools = c
            .list_tools(Some(PaginatedRequestParam { cursor: None }))
            .await
            .expect("list_tools");

        for tool in tools.tools {
            let tool_id = format!("{}/{}", provider.name, tool.name);
            let meta = tool.input_schema.get("_meta").and_then(|v| v.as_object());
            let supports_output = meta
                .and_then(|m| m.get("supports_output_format"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !supports_output {
                continue;
            }

            let props = tool
                .input_schema
                .get("properties")
                .and_then(|v| v.as_object());
            let props = match props {
                Some(map) => map,
                None => {
                    panic!("tool {} missing properties in schema", tool_id);
                }
            };

            let output_format = props
                .get("output_format")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| panic!("tool {} missing output_format schema", tool_id));
            let enum_vals = output_format
                .get("enum")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let has_raw = enum_vals.iter().any(|v| v == "raw");
            let has_normalized = enum_vals.iter().any(|v| v == "normalized_v1");
            assert!(
                has_raw && has_normalized,
                "tool {} missing output_format enum values",
                tool_id
            );
            assert!(
                output_format.get("default").and_then(|v| v.as_str()) == Some("raw"),
                "tool {} output_format default must be raw",
                tool_id
            );

            let examples = tool.input_schema.get("examples").and_then(|v| v.as_array());
            assert!(
                examples.is_some() && !examples.unwrap().is_empty(),
                "tool {} must include examples when supports_output_format=true",
                tool_id
            );

            let category = meta
                .and_then(|m| m.get("category"))
                .and_then(|v| v.as_str());
            assert!(
                category.is_some(),
                "tool {} must include _meta.category when supports_output_format=true",
                tool_id
            );

            let supports_cursor = meta
                .and_then(|m| m.get("supports_cursor"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if supports_cursor {
                assert!(
                    props.contains_key("cursor"),
                    "tool {} supports_cursor=true but cursor is missing in schema",
                    tool_id
                );
                assert!(
                    props.contains_key("limit"),
                    "tool {} supports_cursor=true but limit is missing in schema",
                    tool_id
                );
            }
        }
    }
}

#[test]
fn normalized_fixtures_conform() {
    let fixture_dir = Path::new("tests/fixtures/normalized");
    let entries = fs::read_dir(fixture_dir).expect("fixtures directory exists");
    for entry in entries {
        let entry = entry.expect("fixture entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let data = fs::read_to_string(&path).expect("read fixture");
        let value: Value = serde_json::from_str(&data).expect("parse fixture json");
        validate_normalized_payload(&value).unwrap_or_else(|err| {
            panic!("fixture {} failed validation: {}", path.display(), err);
        });
    }
}

fn validate_normalized_payload(value: &Value) -> Result<(), String> {
    let obj = value.as_object().ok_or("payload must be object")?;
    let type_field = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or("payload missing type")?;

    match type_field {
        NORMALIZED_PAGE_V1_TYPE => {
            let items = obj
                .get("items")
                .and_then(|v| v.as_array())
                .ok_or("page missing items")?;
            for item in items {
                validate_item(item)?;
            }
            let has_more = obj
                .get("has_more")
                .and_then(|v| v.as_bool())
                .ok_or("page missing has_more")?;
            let next_cursor = obj.get("next_cursor").and_then(|v| v.as_str());
            if has_more && next_cursor.is_none() {
                return Err("has_more=true but next_cursor missing".to_string());
            }
            if !has_more && next_cursor.is_some() {
                return Err("has_more=false but next_cursor present".to_string());
            }
        }
        NORMALIZED_ITEM_V1_TYPE => {
            let item = obj.get("item").ok_or("item payload missing item")?;
            validate_item(item)?;
        }
        other => return Err(format!("unsupported normalized type: {}", other)),
    }

    let source = obj.get("source").and_then(|v| v.as_object());
    if let Some(source) = source {
        let connector = source
            .get("connector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tool = source.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        if connector.is_empty() || tool.is_empty() {
            return Err("source.connector and source.tool must be non-empty".to_string());
        }
    }

    Ok(())
}

fn validate_item(item: &Value) -> Result<(), String> {
    let obj = item.as_object().ok_or("item must be object")?;
    let item_ref = obj.get("item_ref").and_then(|v| v.as_str()).unwrap_or("");
    if item_ref.is_empty() {
        return Err("item_ref must be non-empty".to_string());
    }
    let kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind.is_empty() {
        return Err("kind must be non-empty".to_string());
    }
    let blocks = obj
        .get("blocks")
        .and_then(|v| v.as_array())
        .ok_or("blocks must be array")?;
    for block in blocks {
        let block_obj = block.as_object().ok_or("block must be object")?;
        let block_ref = block_obj
            .get("block_ref")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if block_ref.is_empty() {
            return Err("block_ref must be non-empty".to_string());
        }
        let text = block_obj.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if text.is_empty() {
            return Err("block text must be non-empty".to_string());
        }
    }
    Ok(())
}
