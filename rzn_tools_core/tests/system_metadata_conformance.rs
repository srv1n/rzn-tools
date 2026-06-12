use std::collections::BTreeSet;
use std::path::Path;

use rzn_tools_core::system_metadata::{
    collect_bundled_system_validation_errors, discover_bundled_system_specs,
};
use serde_json::Value;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
}

#[test]
fn bundled_system_metadata_contract_is_valid() {
    let specs = discover_bundled_system_specs(repo_root()).expect("load bundled system metadata");
    let errors = collect_bundled_system_validation_errors(&specs);
    assert!(
        errors.is_empty(),
        "bundled system metadata validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn bundled_core_systems_are_present() {
    let specs = discover_bundled_system_specs(repo_root()).expect("load bundled system metadata");
    let actual: BTreeSet<String> = specs
        .iter()
        .map(|spec| spec.metadata.system.id.clone())
        .collect();

    let expected = [
        "wikipedia",
        "youtube_transcripts",
        "pubmed",
        "reddit",
        "web_search",
    ];

    for system_id in expected {
        assert!(
            actual.contains(system_id),
            "expected bundled system '{}' to be present; found {:?}",
            system_id,
            actual
        );
    }
}

#[test]
fn bundled_connector_icons_manifest_is_valid() {
    let manifest_path = repo_root()
        .join("resources")
        .join("icons")
        .join("connectors")
        .join("manifest.json");
    let raw = std::fs::read_to_string(&manifest_path).expect("read icon manifest");
    let manifest: Value = serde_json::from_str(&raw).expect("parse icon manifest");

    let icons = manifest
        .get("icons")
        .and_then(Value::as_object)
        .expect("icons object");
    assert!(!icons.is_empty(), "icon manifest should not be empty");

    for (icon_id, entry) in icons {
        let rel_path = entry
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("icon '{icon_id}' missing path"));
        assert!(
            repo_root().join(rel_path).is_file(),
            "icon '{icon_id}' points to missing file '{}'",
            rel_path
        );
    }

    let connectors = manifest
        .get("connectors")
        .and_then(Value::as_object)
        .expect("connectors object");
    assert!(
        !connectors.is_empty(),
        "connector icon manifest should include connector mappings"
    );

    for (connector_id, entry) in connectors {
        let icon_id = entry
            .get("icon")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("connector '{connector_id}' missing icon"));
        assert!(
            icons.contains_key(icon_id),
            "connector '{connector_id}' references unknown icon '{icon_id}'"
        );
    }
}
