use rmcp::model::PaginatedRequestParam;
use rzn_tools_core::build_registry_enabled_only;
use rzn_tools_core::item::{
    item_from_value, item_schema, items_page_from_value, items_page_schema, ITEMS_V2_TYPE,
    ITEM_V2_TYPE,
};
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
            let has_item = enum_vals.iter().any(|v| v == "item_v2");
            assert!(
                has_raw && has_item,
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
fn item_fixtures_conform_to_the_contract_and_the_published_schema() {
    let item_schema =
        jsonschema::JSONSchema::compile(&item_schema()).expect("item schema compiles");
    let page_schema =
        jsonschema::JSONSchema::compile(&items_page_schema()).expect("page schema compiles");
    let fixture_dir = Path::new("tests/fixtures/items");
    let mut checked = 0;
    for entry in fs::read_dir(fixture_dir).expect("fixtures directory exists") {
        let path = entry.expect("fixture entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let value: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        let (schema, checked_value) = match value["type"].as_str() {
            Some(ITEM_V2_TYPE) => (&item_schema, item_from_value(value.clone()).map(|_| ())),
            Some(ITEMS_V2_TYPE) => (
                &page_schema,
                items_page_from_value(value.clone()).map(|_| ()),
            ),
            other => panic!("fixture {} has type {other:?}", path.display()),
        };
        checked_value
            .unwrap_or_else(|err| panic!("fixture {} fails the contract: {err}", path.display()));
        if let Err(errors) = schema.validate(&value) {
            let errors: Vec<String> = errors.map(|e| e.to_string()).collect();
            panic!("fixture {} fails the schema: {errors:?}", path.display());
        }
        checked += 1;
    }
    assert!(checked >= 10, "expected the fixtures, checked {checked}");
}
