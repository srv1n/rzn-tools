use rmcp::model::*;
use serde_json::json;

fn main() {
    // Attempt to use structured result API; compile-time check only.
    let _r: CallToolResult = CallToolResult::structured(json!({"ok": true}));
    let _e: CallToolResult = CallToolResult::structured_error(json!({"message": "oops"}));
    println!("structured API available");
}
