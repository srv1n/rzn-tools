use rmcp::model::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example of how to properly access text content from rmcp Content

    // Create a Content object with text
    let content = Content::text("Hello, World!");

    // Access the text content
    match &*content {
        // Dereference to get RawContent directly due to Deref impl
        RawContent::Text(text_content) => {
            println!("Text content: {}", text_content.text);
        }
        _ => {
            println!("Not text content");
        }
    }

    // Alternative approach using the as_text method
    if let Some(text_content) = content.as_text() {
        println!("Text content (alternative): {}", text_content.text);
    }

    // Example of creating a CallToolResult
    let result = CallToolResult::success(vec![Content::text("Response from tool")]);

    // Access the first content item
    if let Some(first_content) = result.content.first() {
        if let Some(text_content) = first_content.as_text() {
            println!("Tool response text: {}", text_content.text);
        }
    }

    Ok(())
}
