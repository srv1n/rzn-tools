use rzn_tools_core::{auth::AuthDetails, build_registry_enabled_only};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = build_registry_enabled_only().await;
    let names: Vec<_> = registry
        .list_providers()
        .into_iter()
        .map(|p| p.name)
        .collect();
    println!("providers: {:?}", names);

    if let Some(provider) = registry.get_provider("google-drive") {
        let mut c = provider.lock().await;
        let schema = c.config_schema();
        println!("google-drive fields: {}", schema.fields.len());
        let _ = c.set_auth_details(AuthDetails::new()).await; // no-op
        match c.test_auth().await {
            Ok(_) => println!("auth OK"),
            Err(e) => println!("auth not ready: {e}"),
        }
    }
    Ok(())
}
