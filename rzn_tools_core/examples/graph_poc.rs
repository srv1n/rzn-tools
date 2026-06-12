use rzn_tools_core::{auth::AuthDetails, build_registry_enabled_only};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal smoke: build registry with feature enabled and list providers
    let registry = build_registry_enabled_only().await;
    let names: Vec<_> = registry
        .list_providers()
        .into_iter()
        .map(|p| p.name)
        .collect();
    println!("providers: {:?}", names);

    if let Some(provider) = registry.get_provider("microsoft-graph") {
        let mut c = provider.lock().await;
        // Show config fields and test auth (expected to fail until configured)
        let schema = c.config_schema();
        println!("microsoft-graph fields: {}", schema.fields.len());
        let _ = c.set_auth_details(AuthDetails::new()).await; // no-op
        match c.test_auth().await {
            Ok(_) => println!("auth OK"),
            Err(e) => println!("auth not ready: {e}"),
        }
    }
    Ok(())
}
