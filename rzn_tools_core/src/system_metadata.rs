use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SYSTEM_METADATA_FILE_NAME: &str = "system.metadata.yaml";
pub const QUICKSTARTS_FILE_NAME: &str = "quickstarts.json";

#[derive(Debug, Error)]
pub enum SystemMetadataError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse YAML {path}: {source}")]
    ParseYaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to parse JSON {path}: {source}")]
    ParseJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("metadata root not found: {0}")]
    MissingMetadataRoot(PathBuf),
}

#[derive(Debug, Clone, Serialize)]
pub struct BundledSystemSpec {
    pub system_dir: PathBuf,
    pub metadata_path: PathBuf,
    pub metadata: SystemMetadata,
    pub quickstarts_path: PathBuf,
    pub quickstarts: Vec<QuickstartExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetadata {
    pub version: u32,
    pub system: SystemDescriptor,
    #[serde(default)]
    pub capability_groups: Vec<CapabilityGroup>,
    #[serde(default)]
    pub setup_steps: Vec<SetupStep>,
    #[serde(default)]
    pub context_parameters: Vec<ContextParameter>,
    #[serde(default)]
    pub quick_starts: Vec<SystemQuickStart>,
    pub result_handling: ResultHandling,
    #[serde(default)]
    pub x_runtime: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDescriptor {
    pub id: String,
    pub display_name: String,
    pub what_it_does: String,
    #[serde(default)]
    pub use_cases: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGroup {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    #[serde(default)]
    pub tool_patterns: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStep {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextParameter {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemQuickStart {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tool_id: String,
    pub args: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultHandling {
    pub tools: BTreeMap<String, ToolResultHandling>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultHandling {
    #[serde(default)]
    pub human_view: Option<Value>,
    #[serde(default)]
    pub llm_view: Option<Value>,
    #[serde(default)]
    pub index_view: Option<IndexView>,
    #[serde(default)]
    pub debug_view: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexView {
    pub mode: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickstartExample {
    pub id: String,
    pub title: String,
    pub system_id: String,
    pub runtime_tool: String,
    pub args: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn discover_bundled_system_specs(
    repo_root: impl AsRef<Path>,
) -> Result<Vec<BundledSystemSpec>, SystemMetadataError> {
    let repo_root = repo_root.as_ref();
    let systems_root = repo_root.join("resources").join("systems");
    if !systems_root.exists() {
        return Err(SystemMetadataError::MissingMetadataRoot(systems_root));
    }

    let mut system_dirs = Vec::new();
    for entry in fs::read_dir(&systems_root).map_err(|source| SystemMetadataError::Read {
        path: systems_root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| SystemMetadataError::Read {
            path: systems_root.clone(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            system_dirs.push(path);
        }
    }

    system_dirs.sort();

    let mut bundled_specs = Vec::with_capacity(system_dirs.len());
    for system_dir in system_dirs {
        let metadata_path = system_dir.join(SYSTEM_METADATA_FILE_NAME);
        let metadata = load_system_metadata(&metadata_path)?;
        let quickstarts_path = repo_root
            .join("examples")
            .join(&metadata.system.id)
            .join(QUICKSTARTS_FILE_NAME);
        let quickstarts = load_quickstart_examples(&quickstarts_path)?;

        bundled_specs.push(BundledSystemSpec {
            system_dir,
            metadata_path,
            metadata,
            quickstarts_path,
            quickstarts,
        });
    }

    Ok(bundled_specs)
}

pub fn load_system_metadata(path: impl AsRef<Path>) -> Result<SystemMetadata, SystemMetadataError> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path).map_err(|source| SystemMetadataError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&raw).map_err(|source| SystemMetadataError::ParseYaml {
        path: path.to_path_buf(),
        source,
    })
}

pub fn load_quickstart_examples(
    path: impl AsRef<Path>,
) -> Result<Vec<QuickstartExample>, SystemMetadataError> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path).map_err(|source| SystemMetadataError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| SystemMetadataError::ParseJson {
        path: path.to_path_buf(),
        source,
    })
}

pub fn collect_bundled_system_validation_errors(specs: &[BundledSystemSpec]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen_system_ids = BTreeSet::new();

    for spec in specs {
        let system_id = &spec.metadata.system.id;
        if !seen_system_ids.insert(system_id.clone()) {
            errors.push(format!("duplicate system.id detected: {system_id}"));
        }

        let dir_name = spec
            .system_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if dir_name != system_id {
            errors.push(format!(
                "system directory '{}' does not match system.id '{}'",
                spec.system_dir.display(),
                system_id
            ));
        }

        validate_metadata(spec, &mut errors);
        validate_quickstarts(spec, &mut errors);
    }

    errors
}

fn validate_metadata(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    let metadata = &spec.metadata;
    let system = &metadata.system;

    if metadata.version != 1 {
        errors.push(format!(
            "{}: version must be 1, found {}",
            spec.metadata_path.display(),
            metadata.version
        ));
    }

    require_non_blank(&system.id, &spec.metadata_path, "system.id", errors);
    require_non_blank(
        &system.display_name,
        &spec.metadata_path,
        "system.display_name",
        errors,
    );
    require_non_blank(
        &system.what_it_does,
        &spec.metadata_path,
        "system.what_it_does",
        errors,
    );

    if system.use_cases.is_empty() {
        errors.push(format!(
            "{}: system.use_cases must contain at least one entry",
            spec.metadata_path.display()
        ));
    }
    for (index, use_case) in system.use_cases.iter().enumerate() {
        require_non_blank(
            use_case,
            &spec.metadata_path,
            &format!("system.use_cases[{index}]"),
            errors,
        );
    }

    if metadata.capability_groups.is_empty() {
        errors.push(format!(
            "{}: capability_groups must contain at least one entry",
            spec.metadata_path.display()
        ));
    }
    validate_capability_groups(spec, errors);

    if metadata.setup_steps.is_empty() {
        errors.push(format!(
            "{}: setup_steps must contain at least one entry",
            spec.metadata_path.display()
        ));
    }
    validate_setup_steps(spec, errors);
    validate_context_parameters(spec, errors);

    if metadata.quick_starts.is_empty() {
        errors.push(format!(
            "{}: quick_starts must contain at least one entry",
            spec.metadata_path.display()
        ));
    }
    validate_metadata_quickstarts(spec, errors);
    validate_result_handling(spec, errors);
}

fn validate_capability_groups(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    let mut seen_ids = BTreeSet::new();
    for (index, group) in spec.metadata.capability_groups.iter().enumerate() {
        let base = format!("capability_groups[{index}]");
        require_non_blank(
            &group.id,
            &spec.metadata_path,
            &format!("{base}.id"),
            errors,
        );
        require_non_blank(
            &group.name,
            &spec.metadata_path,
            &format!("{base}.name"),
            errors,
        );
        require_non_blank(
            &group.description,
            &spec.metadata_path,
            &format!("{base}.description"),
            errors,
        );
        if !group.id.is_empty() && !seen_ids.insert(group.id.clone()) {
            errors.push(format!(
                "{}: duplicate capability_groups id '{}'",
                spec.metadata_path.display(),
                group.id
            ));
        }
        if group.tool_ids.is_empty() && group.tool_patterns.is_empty() {
            errors.push(format!(
                "{}: {base} must define tool_ids and/or tool_patterns",
                spec.metadata_path.display()
            ));
        }
    }
}

fn validate_setup_steps(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    let mut seen_ids = BTreeSet::new();
    for (index, step) in spec.metadata.setup_steps.iter().enumerate() {
        let base = format!("setup_steps[{index}]");
        require_non_blank(&step.id, &spec.metadata_path, &format!("{base}.id"), errors);
        require_non_blank(
            &step.kind,
            &spec.metadata_path,
            &format!("{base}.kind"),
            errors,
        );
        require_non_blank(
            &step.label,
            &spec.metadata_path,
            &format!("{base}.label"),
            errors,
        );
        require_non_blank(
            &step.description,
            &spec.metadata_path,
            &format!("{base}.description"),
            errors,
        );
        if !step.id.is_empty() && !seen_ids.insert(step.id.clone()) {
            errors.push(format!(
                "{}: duplicate setup_steps id '{}'",
                spec.metadata_path.display(),
                step.id
            ));
        }
    }
}

fn validate_context_parameters(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    let mut seen_ids = BTreeSet::new();
    for (index, parameter) in spec.metadata.context_parameters.iter().enumerate() {
        let base = format!("context_parameters[{index}]");
        require_non_blank(
            &parameter.id,
            &spec.metadata_path,
            &format!("{base}.id"),
            errors,
        );
        require_non_blank(
            &parameter.label,
            &spec.metadata_path,
            &format!("{base}.label"),
            errors,
        );
        require_non_blank(
            &parameter.kind,
            &spec.metadata_path,
            &format!("{base}.kind"),
            errors,
        );
        if !parameter.id.is_empty() && !seen_ids.insert(parameter.id.clone()) {
            errors.push(format!(
                "{}: duplicate context_parameters id '{}'",
                spec.metadata_path.display(),
                parameter.id
            ));
        }
    }
}

fn validate_metadata_quickstarts(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    let mut seen_ids = BTreeSet::new();
    let available_tool_views: BTreeSet<&str> = spec
        .metadata
        .result_handling
        .tools
        .keys()
        .map(String::as_str)
        .collect();

    for (index, quickstart) in spec.metadata.quick_starts.iter().enumerate() {
        let base = format!("quick_starts[{index}]");
        require_non_blank(
            &quickstart.id,
            &spec.metadata_path,
            &format!("{base}.id"),
            errors,
        );
        require_non_blank(
            &quickstart.title,
            &spec.metadata_path,
            &format!("{base}.title"),
            errors,
        );
        require_non_blank(
            &quickstart.description,
            &spec.metadata_path,
            &format!("{base}.description"),
            errors,
        );
        require_non_blank(
            &quickstart.tool_id,
            &spec.metadata_path,
            &format!("{base}.tool_id"),
            errors,
        );

        if !quickstart.id.is_empty() && !seen_ids.insert(quickstart.id.clone()) {
            errors.push(format!(
                "{}: duplicate quick_starts id '{}'",
                spec.metadata_path.display(),
                quickstart.id
            ));
        }

        if !quickstart.tool_id.is_empty()
            && !available_tool_views.contains(quickstart.tool_id.as_str())
        {
            errors.push(format!(
                "{}: quick start '{}' references tool '{}' but result_handling.tools has no matching entry",
                spec.metadata_path.display(),
                quickstart.id,
                quickstart.tool_id
            ));
        }
    }
}

fn validate_result_handling(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    if spec.metadata.result_handling.tools.is_empty() {
        errors.push(format!(
            "{}: result_handling.tools must contain at least one entry",
            spec.metadata_path.display()
        ));
        return;
    }

    for (tool_name, handling) in &spec.metadata.result_handling.tools {
        if is_blank(tool_name) {
            errors.push(format!(
                "{}: result_handling.tools contains a blank tool id",
                spec.metadata_path.display()
            ));
        }
        if handling.human_view.is_none() {
            errors.push(format!(
                "{}: result_handling.tools.{tool_name}.human_view is required",
                spec.metadata_path.display()
            ));
        }
        if handling.llm_view.is_none() {
            errors.push(format!(
                "{}: result_handling.tools.{tool_name}.llm_view is required",
                spec.metadata_path.display()
            ));
        }
        if handling.debug_view.is_none() {
            errors.push(format!(
                "{}: result_handling.tools.{tool_name}.debug_view is required",
                spec.metadata_path.display()
            ));
        }
        match &handling.index_view {
            Some(index_view) => {
                if is_blank(&index_view.mode) {
                    errors.push(format!(
                        "{}: result_handling.tools.{tool_name}.index_view.mode is required",
                        spec.metadata_path.display()
                    ));
                }
            }
            None => errors.push(format!(
                "{}: result_handling.tools.{tool_name}.index_view is required",
                spec.metadata_path.display()
            )),
        }
    }
}

fn validate_quickstarts(spec: &BundledSystemSpec, errors: &mut Vec<String>) {
    if spec.quickstarts.is_empty() {
        errors.push(format!(
            "{}: quickstarts.json must contain at least one entry",
            spec.quickstarts_path.display()
        ));
        return;
    }

    let metadata_quickstarts: BTreeMap<&str, &SystemQuickStart> = spec
        .metadata
        .quick_starts
        .iter()
        .map(|quickstart| (quickstart.id.as_str(), quickstart))
        .collect();

    let mut seen_ids = BTreeSet::new();
    for (index, quickstart) in spec.quickstarts.iter().enumerate() {
        let base = format!("quickstarts[{index}]");
        require_non_blank(
            &quickstart.id,
            &spec.quickstarts_path,
            &format!("{base}.id"),
            errors,
        );
        require_non_blank(
            &quickstart.title,
            &spec.quickstarts_path,
            &format!("{base}.title"),
            errors,
        );
        require_non_blank(
            &quickstart.system_id,
            &spec.quickstarts_path,
            &format!("{base}.system_id"),
            errors,
        );
        require_non_blank(
            &quickstart.runtime_tool,
            &spec.quickstarts_path,
            &format!("{base}.runtime_tool"),
            errors,
        );

        if !quickstart.id.is_empty() && !seen_ids.insert(quickstart.id.clone()) {
            errors.push(format!(
                "{}: duplicate quickstarts id '{}'",
                spec.quickstarts_path.display(),
                quickstart.id
            ));
        }

        if quickstart.system_id != spec.metadata.system.id {
            errors.push(format!(
                "{}: quickstart '{}' has system_id '{}' but metadata declares '{}'",
                spec.quickstarts_path.display(),
                quickstart.id,
                quickstart.system_id,
                spec.metadata.system.id
            ));
        }

        match metadata_quickstarts.get(quickstart.id.as_str()) {
            Some(metadata_quickstart) => {
                if quickstart.title != metadata_quickstart.title {
                    errors.push(format!(
                        "{}: quickstart '{}' title differs from metadata quick_starts entry",
                        spec.quickstarts_path.display(),
                        quickstart.id
                    ));
                }
                if quickstart.args != metadata_quickstart.args {
                    errors.push(format!(
                        "{}: quickstart '{}' args differ from metadata quick_starts entry",
                        spec.quickstarts_path.display(),
                        quickstart.id
                    ));
                }
            }
            None => errors.push(format!(
                "{}: quickstart '{}' does not exist in system.metadata.yaml",
                spec.quickstarts_path.display(),
                quickstart.id
            )),
        }
    }

    let example_ids: BTreeSet<&str> = spec
        .quickstarts
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    for metadata_quickstart in &spec.metadata.quick_starts {
        if !example_ids.contains(metadata_quickstart.id.as_str()) {
            errors.push(format!(
                "{}: metadata quick start '{}' is missing from {}",
                spec.metadata_path.display(),
                metadata_quickstart.id,
                spec.quickstarts_path.display()
            ));
        }
    }
}

fn require_non_blank(value: &str, path: &Path, field: &str, errors: &mut Vec<String>) {
    if is_blank(value) {
        errors.push(format!("{}: {field} must be non-empty", path.display()));
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}
