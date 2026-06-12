use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use owo_colors::OwoColorize;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use tar::Archive;

use crate::cli::{Cli, OutputFormat, WorkflowAction};
use crate::commands::{CommandError, Result};
use rzn_tools_core::paths::{
    find_repo_asset_root, installed_assets_dir, is_asset_root, managed_assets_dir,
    resolve_asset_root, ASSET_ENV_VAR,
};
use rzn_tools_core::system_metadata::discover_bundled_system_specs;

const DEFAULT_RELEASE_REPO: &str = "srv1n/rzn-tools";

#[derive(Debug, Serialize)]
struct WorkflowSummary {
    id: String,
    display_name: String,
    quickstarts: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkflowManifest {
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowListReport {
    active_root: Option<String>,
    bundled_root: Option<String>,
    managed_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    managed_manifest: Option<WorkflowManifest>,
    systems: Vec<WorkflowSummary>,
}

#[derive(Debug, Serialize)]
struct WorkflowSyncReport {
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    destination: String,
    systems: usize,
    quickstarts: usize,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn run(cli: &Cli, action: WorkflowAction) -> Result<()> {
    match action {
        WorkflowAction::List => list(cli),
        WorkflowAction::Sync { remote, version } => {
            sync(cli, remote || version.is_some(), version.as_deref()).await
        }
    }
}

fn list(cli: &Cli) -> Result<()> {
    let active_root = resolve_asset_root();
    let bundled_root = resolve_bundled_source_root();
    let managed_root = managed_assets_dir();
    let managed_manifest = read_manifest(&managed_root);

    let systems = match active_root.as_ref() {
        Some(root) => summarize_systems(root)?,
        None => Vec::new(),
    };

    let report = WorkflowListReport {
        active_root: active_root.map(display_path),
        bundled_root: bundled_root.map(display_path),
        managed_root: display_path(managed_root),
        managed_manifest,
        systems,
    };

    match cli.output {
        OutputFormat::Pretty => print_list_pretty(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&report)?),
        OutputFormat::Text | OutputFormat::Markdown => {
            println!("{}", serde_json::to_string_pretty(&report)?)
        }
    }

    Ok(())
}

async fn sync(cli: &Cli, remote: bool, version: Option<&str>) -> Result<()> {
    let report = if remote {
        sync_from_release(version).await?
    } else {
        sync_from_bundled()?
    };

    match cli.output {
        OutputFormat::Pretty => print_sync_pretty(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&report)?),
        OutputFormat::Text | OutputFormat::Markdown => {
            println!("{}", serde_json::to_string_pretty(&report)?)
        }
    }

    Ok(())
}

fn sync_from_bundled() -> Result<WorkflowSyncReport> {
    let Some(source_root) = resolve_bundled_source_root() else {
        return Err(CommandError::Other(
            "No bundled workflow asset root found. Install the share payload or run from the repo root."
                .to_string(),
        ));
    };

    install_asset_tree(
        &source_root,
        WorkflowManifest {
            source: "bundled".to_string(),
            version: None,
        },
    )
}

async fn sync_from_release(version: Option<&str>) -> Result<WorkflowSyncReport> {
    let repo = DEFAULT_RELEASE_REPO;
    let release = fetch_release(repo, version).await?;
    let asset_name = format!("rzn-tools-workflows-{}.tar.gz", release.tag_name);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| {
            CommandError::Other(format!(
                "Release {} is missing workflow bundle {}",
                release.tag_name, asset_name
            ))
        })?;

    let client = github_client()?;
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|err| CommandError::Other(format!("Failed to download workflow bundle: {err}")))?
        .error_for_status()
        .map_err(|err| CommandError::Other(format!("Workflow bundle download failed: {err}")))?
        .bytes()
        .await
        .map_err(|err| {
            CommandError::Other(format!("Failed to read workflow bundle download: {err}"))
        })?;

    let scratch_root = temp_path("workflows-download");
    if scratch_root.exists() {
        fs::remove_dir_all(&scratch_root)?;
    }
    fs::create_dir_all(&scratch_root)?;

    let archive = GzDecoder::new(Cursor::new(bytes));
    let mut tar = Archive::new(archive);
    tar.unpack(&scratch_root)
        .map_err(|err| CommandError::Other(format!("Failed to unpack workflow bundle: {err}")))?;

    let source_root = scratch_root.join("share").join("rzn-tools");
    if !is_asset_root(&source_root) {
        fs::remove_dir_all(&scratch_root).ok();
        return Err(CommandError::Other(format!(
            "Workflow bundle did not contain share/rzn-tools assets at {}",
            source_root.display()
        )));
    }

    let report = install_asset_tree(
        &source_root,
        WorkflowManifest {
            source: "github-release".to_string(),
            version: Some(release.tag_name),
        },
    )?;

    fs::remove_dir_all(&scratch_root).ok();
    Ok(report)
}

fn install_asset_tree(
    source_root: &Path,
    manifest: WorkflowManifest,
) -> Result<WorkflowSyncReport> {
    if !is_asset_root(source_root) {
        return Err(CommandError::Other(format!(
            "Invalid workflow asset root: {}",
            source_root.display()
        )));
    }

    let destination = managed_assets_dir();
    let parent = destination
        .parent()
        .ok_or_else(|| CommandError::Other("Managed asset path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;

    let staging = parent.join(format!(
        ".assets-stage-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    copy_dir(source_root, &staging)?;
    fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    fs::rename(&staging, &destination)?;

    let specs = discover_bundled_system_specs(&destination).map_err(map_system_metadata_error)?;
    Ok(WorkflowSyncReport {
        source: manifest.source,
        version: manifest.version,
        destination: display_path(destination),
        systems: specs.len(),
        quickstarts: specs.iter().map(|spec| spec.quickstarts.len()).sum(),
    })
}

fn summarize_systems(root: &Path) -> Result<Vec<WorkflowSummary>> {
    let specs = discover_bundled_system_specs(root).map_err(map_system_metadata_error)?;
    Ok(specs
        .into_iter()
        .map(|spec| WorkflowSummary {
            id: spec.metadata.system.id,
            display_name: spec.metadata.system.display_name,
            quickstarts: spec.quickstarts.len(),
        })
        .collect())
}

fn read_manifest(root: &Path) -> Option<WorkflowManifest> {
    let path = root.join("manifest.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn resolve_bundled_source_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(ASSET_ENV_VAR) {
        let path = PathBuf::from(path);
        if is_asset_root(&path) {
            return Some(path);
        }
    }

    if let Some(installed) = installed_assets_dir().filter(|path| is_asset_root(path)) {
        return Some(installed);
    }

    std::env::current_dir()
        .ok()
        .and_then(|cwd| find_repo_asset_root(&cwd))
}

fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_dir(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn print_list_pretty(report: &WorkflowListReport) {
    println!("{}", "Workflow Assets".cyan().bold());
    println!();
    println!(
        "{} {}",
        "Active root:".dimmed(),
        report
            .active_root
            .as_deref()
            .unwrap_or("(not found)")
            .green()
    );
    println!(
        "{} {}",
        "Bundled root:".dimmed(),
        report
            .bundled_root
            .as_deref()
            .unwrap_or("(not found)")
            .green()
    );
    println!(
        "{} {}",
        "Managed root:".dimmed(),
        report.managed_root.green()
    );
    if let Some(manifest) = &report.managed_manifest {
        println!(
            "{} {}{}",
            "Managed source:".dimmed(),
            manifest.source.green(),
            manifest
                .version
                .as_deref()
                .map(|version| format!(" ({version})"))
                .unwrap_or_default()
        );
    }
    println!();

    if report.systems.is_empty() {
        println!("{}", "No bundled workflow assets discovered.".yellow());
        println!(
            "{}",
            "Run `rzn-tools workflows sync` after install, or `rzn-tools workflows sync --remote` to pull the latest bundle."
                .dimmed()
        );
        return;
    }

    println!(
        "{} {}",
        "Systems:".dimmed(),
        report.systems.len().to_string().cyan().bold()
    );
    for system in &report.systems {
        println!(
            "  {}  {}  {} quickstarts",
            system.id.bold(),
            system.display_name,
            system.quickstarts
        );
    }
}

fn print_sync_pretty(report: &WorkflowSyncReport) {
    println!("{}", "Workflow Sync Complete".cyan().bold());
    println!();
    println!("{} {}", "Source:".dimmed(), report.source.green());
    if let Some(version) = &report.version {
        println!("{} {}", "Version:".dimmed(), version.green());
    }
    println!("{} {}", "Destination:".dimmed(), report.destination.green());
    println!(
        "{} {}",
        "Systems:".dimmed(),
        report.systems.to_string().cyan()
    );
    println!(
        "{} {}",
        "Quickstarts:".dimmed(),
        report.quickstarts.to_string().cyan()
    );
}

fn github_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .build()
        .map_err(|err| CommandError::Other(format!("Failed to build HTTP client: {err}")))
}

async fn fetch_release(repo: &str, version: Option<&str>) -> Result<GithubRelease> {
    let endpoint = match version {
        Some(version) => format!(
            "https://api.github.com/repos/{repo}/releases/tags/{}",
            normalize_tag(version)
        ),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };

    github_client()?
        .get(endpoint)
        .header(USER_AGENT, "rzn-tools-cli")
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| CommandError::Other(format!("Failed to query GitHub releases: {err}")))?
        .error_for_status()
        .map_err(|err| CommandError::Other(format!("GitHub release lookup failed: {err}")))?
        .json::<GithubRelease>()
        .await
        .map_err(|err| CommandError::Other(format!("Failed to decode GitHub release JSON: {err}")))
}

fn normalize_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

fn temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rzn-tools-{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ))
}

fn map_system_metadata_error(
    err: rzn_tools_core::system_metadata::SystemMetadataError,
) -> CommandError {
    CommandError::Other(format!("Workflow metadata error: {err}"))
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}
