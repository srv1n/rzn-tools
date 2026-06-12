use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::{
    Cli, OutputFormat, SkillAction, SkillArgs, SkillClient, SkillInstallArgs, SkillRemoveArgs,
    SkillScope, SkillSource,
};
use crate::commands::{CommandError, Result};

const SKILL_NAME: &str = "rzn-tools";
const RELEASE_VERSION: &str = env!("CARGO_PKG_VERSION");

struct EmbeddedFile {
    path: &'static str,
    content: &'static str,
    executable: bool,
}

const EMBEDDED_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        path: "SKILL.md",
        content: include_str!("../../../.agents/skills/rzn-tools/SKILL.md"),
        executable: false,
    },
    EmbeddedFile {
        path: "agents/openai.yaml",
        content: include_str!("../../../.agents/skills/rzn-tools/agents/openai.yaml"),
        executable: false,
    },
    EmbeddedFile {
        path: "references/cli-mcp.md",
        content: include_str!("../../../.agents/skills/rzn-tools/references/cli-mcp.md"),
        executable: false,
    },
    EmbeddedFile {
        path: "references/connector-development.md",
        content: include_str!(
            "../../../.agents/skills/rzn-tools/references/connector-development.md"
        ),
        executable: false,
    },
    EmbeddedFile {
        path: "references/normalized-output.md",
        content: include_str!("../../../.agents/skills/rzn-tools/references/normalized-output.md"),
        executable: false,
    },
    EmbeddedFile {
        path: "references/plugin-release.md",
        content: include_str!("../../../.agents/skills/rzn-tools/references/plugin-release.md"),
        executable: false,
    },
    EmbeddedFile {
        path: "scripts/validate.sh",
        content: include_str!("../../../.agents/skills/rzn-tools/scripts/validate.sh"),
        executable: true,
    },
];

#[derive(Debug, Serialize)]
struct SkillReport {
    action: String,
    skill: String,
    scope: String,
    release_version: String,
    source: SourceReport,
    targets: Vec<TargetReport>,
}

#[derive(Debug, Serialize)]
struct SourceReport {
    kind: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct TargetReport {
    client: String,
    path: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_target: Option<String>,
}

#[derive(Debug, Clone)]
struct Target {
    client: String,
    path: PathBuf,
}

#[derive(Debug)]
struct SkillSourcePath {
    kind: &'static str,
    path: PathBuf,
}

pub async fn run(cli: &Cli, action: SkillAction) -> Result<()> {
    let report = match action {
        SkillAction::Status(args) => status(args)?,
        SkillAction::Install(args) => install("install", args)?,
        SkillAction::Update(args) => install("update", args)?,
        SkillAction::Remove(args) => remove(args)?,
    };

    print_report(cli, &report)
}

fn status(args: SkillArgs) -> Result<SkillReport> {
    let source = resolve_source(SkillSource::Auto, false)?;
    let targets = target_paths(args.scope, &args.clients)?
        .into_iter()
        .map(|target| inspect_target(target, Some(&source.path)))
        .collect();

    Ok(SkillReport {
        action: "status".to_string(),
        skill: SKILL_NAME.to_string(),
        scope: scope_label(args.scope).to_string(),
        release_version: RELEASE_VERSION.to_string(),
        source: source_report(source),
        targets,
    })
}

fn install(action: &str, args: SkillInstallArgs) -> Result<SkillReport> {
    let source = resolve_source(args.source, true)?;
    let targets = target_paths(args.scope, &args.clients)?
        .into_iter()
        .map(|target| install_target(target, &source.path, args.force))
        .collect::<Result<Vec<_>>>()?;

    Ok(SkillReport {
        action: action.to_string(),
        skill: SKILL_NAME.to_string(),
        scope: scope_label(args.scope).to_string(),
        release_version: RELEASE_VERSION.to_string(),
        source: source_report(source),
        targets,
    })
}

fn remove(args: SkillRemoveArgs) -> Result<SkillReport> {
    let source = resolve_source(SkillSource::Auto, false)?;
    let targets = target_paths(args.scope, &args.clients)?
        .into_iter()
        .map(remove_target)
        .collect::<Result<Vec<_>>>()?;

    if args.delete_source {
        let managed = managed_skill_dir();
        if managed.exists() {
            fs::remove_dir_all(managed)?;
        }
    }

    Ok(SkillReport {
        action: "remove".to_string(),
        skill: SKILL_NAME.to_string(),
        scope: scope_label(args.scope).to_string(),
        release_version: RELEASE_VERSION.to_string(),
        source: source_report(source),
        targets,
    })
}

fn resolve_source(preference: SkillSource, materialize: bool) -> Result<SkillSourcePath> {
    match preference {
        SkillSource::Repo => repo_skill_dir()
            .map(|path| SkillSourcePath { kind: "repo", path })
            .ok_or_else(|| {
                CommandError::Other(
                    "No repo skill found at .agents/skills/rzn-tools from the current directory"
                        .to_string(),
                )
            }),
        SkillSource::Embedded => {
            let path = managed_skill_dir();
            if materialize {
                materialize_embedded_skill(&path)?;
            }
            Ok(SkillSourcePath {
                kind: "embedded",
                path,
            })
        }
        SkillSource::Auto => {
            if let Some(path) = repo_skill_dir() {
                Ok(SkillSourcePath { kind: "repo", path })
            } else {
                let path = managed_skill_dir();
                if materialize {
                    materialize_embedded_skill(&path)?;
                }
                Ok(SkillSourcePath {
                    kind: "embedded",
                    path,
                })
            }
        }
    }
}

fn materialize_embedded_skill(destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| CommandError::Other("Managed skill path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;

    let staging = parent.join(format!(".{SKILL_NAME}-stage-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    for file in EMBEDDED_FILES {
        let path = staging.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, file.content)?;
        if file.executable {
            make_executable(&path)?;
        }
    }

    let manifest = serde_json::json!({
        "skill": SKILL_NAME,
        "source": "embedded",
        "version": RELEASE_VERSION,
    });
    fs::write(
        staging.join("rzn-tools-skill-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::rename(staging, destination)?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn install_target(target: Target, source: &Path, force: bool) -> Result<TargetReport> {
    if same_existing_path(&target.path, source) {
        return Ok(target_report(target, "source", None));
    }

    if target.path.symlink_metadata().is_ok() {
        if symlink_points_to(&target.path, source) {
            return Ok(target_report(
                target,
                "linked",
                Some(source.display().to_string()),
            ));
        }

        if !force {
            return Err(CommandError::Other(format!(
                "{} already exists. Pass --force to replace it.",
                target.path.display()
            )));
        }

        remove_existing_path(&target.path)?;
    }

    let parent = target
        .path
        .parent()
        .ok_or_else(|| CommandError::Other("Skill target path has no parent".to_string()))?;
    fs::create_dir_all(parent)?;
    symlink_dir(source, &target.path)?;

    Ok(target_report(
        target,
        "linked",
        Some(source.display().to_string()),
    ))
}

fn remove_target(target: Target) -> Result<TargetReport> {
    match target.path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(&target.path)?;
            Ok(target_report(target, "removed", None))
        }
        Ok(_) => Ok(target_report(target, "not-managed", None)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(target_report(target, "missing", None))
        }
        Err(err) => Err(err.into()),
    }
}

fn inspect_target(target: Target, expected_source: Option<&Path>) -> TargetReport {
    if same_existing_path(&target.path, expected_source.unwrap_or(&target.path)) {
        return target_report(target, "source", None);
    }

    match fs::read_link(&target.path) {
        Ok(link) => {
            let status = match expected_source {
                Some(source) if paths_equivalent(&link, source) => "linked",
                Some(_) => "linked-other",
                None => "linked",
            };
            target_report(target, status, Some(link.display().to_string()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            target_report(target, "missing", None)
        }
        Err(_) if target.path.exists() => target_report(target, "not-managed", None),
        Err(_) => target_report(target, "missing", None),
    }
}

fn remove_existing_path(path: &Path) -> Result<()> {
    let metadata = path.symlink_metadata()?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)?;
    } else {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn symlink_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

fn target_paths(scope: SkillScope, clients: &[SkillClient]) -> Result<Vec<Target>> {
    let mut targets: BTreeMap<String, Target> = BTreeMap::new();
    for client in expand_clients(clients) {
        let base = client_skill_dir(scope, client)?;
        let path = base.join(SKILL_NAME);
        let key = path.to_string_lossy().to_string();
        if let Some(existing) = targets.get_mut(&key) {
            existing.client.push(',');
            existing.client.push_str(client_label(client));
        } else {
            targets.insert(
                key,
                Target {
                    client: client_label(client).to_string(),
                    path,
                },
            );
        }
    }
    Ok(targets.into_values().collect())
}

fn expand_clients(clients: &[SkillClient]) -> Vec<SkillClient> {
    if clients.is_empty() || clients.contains(&SkillClient::All) {
        vec![
            SkillClient::Claude,
            SkillClient::Gemini,
            SkillClient::Agent,
            SkillClient::Codex,
        ]
    } else {
        clients.to_vec()
    }
}

fn client_skill_dir(scope: SkillScope, client: SkillClient) -> Result<PathBuf> {
    match scope {
        SkillScope::Global => match client {
            SkillClient::Claude => Ok(home_dir()?.join(".claude").join("skills")),
            SkillClient::Gemini => Ok(home_dir()?.join(".gemini").join("skills")),
            SkillClient::Agent => Ok(home_dir()?.join(".agents").join("skills")),
            SkillClient::Codex => Ok(codex_home_dir().join("skills")),
            SkillClient::All => unreachable!("all must be expanded before path resolution"),
        },
        SkillScope::Project => {
            let cwd = std::env::current_dir()?;
            match client {
                SkillClient::Claude => Ok(cwd.join(".claude").join("skills")),
                SkillClient::Gemini => Ok(cwd.join(".gemini").join("skills")),
                SkillClient::Agent | SkillClient::Codex => Ok(cwd.join(".agents").join("skills")),
                SkillClient::All => unreachable!("all must be expanded before path resolution"),
            }
        }
    }
}

fn repo_skill_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    cwd.ancestors()
        .map(|ancestor| ancestor.join(".agents").join("skills").join(SKILL_NAME))
        .find(|path| path.join("SKILL.md").is_file())
}

fn managed_skill_dir() -> PathBuf {
    rzn_tools_core::paths::data_dir()
        .join("skills")
        .join(SKILL_NAME)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| CommandError::Other("Could not resolve home directory".to_string()))
}

fn codex_home_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".codex")
        })
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    left.exists() && paths_equivalent(left, right)
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn symlink_points_to(path: &Path, source: &Path) -> bool {
    fs::read_link(path)
        .map(|link| paths_equivalent(&link, source))
        .unwrap_or(false)
}

fn source_report(source: SkillSourcePath) -> SourceReport {
    SourceReport {
        kind: source.kind.to_string(),
        path: source.path.display().to_string(),
    }
}

fn target_report(target: Target, status: &str, link_target: Option<String>) -> TargetReport {
    TargetReport {
        client: target.client,
        path: target.path.display().to_string(),
        status: status.to_string(),
        link_target,
    }
}

fn scope_label(scope: SkillScope) -> &'static str {
    match scope {
        SkillScope::Global => "global",
        SkillScope::Project => "project",
    }
}

fn client_label(client: SkillClient) -> &'static str {
    match client {
        SkillClient::All => "all",
        SkillClient::Claude => "claude",
        SkillClient::Gemini => "gemini",
        SkillClient::Agent => "agent",
        SkillClient::Codex => "codex",
    }
}

fn print_report(cli: &Cli, report: &SkillReport) -> Result<()> {
    match cli.output {
        OutputFormat::Pretty => print_pretty(report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(report)?),
        OutputFormat::Yaml => println!("{}", serde_yaml::to_string(report)?),
        OutputFormat::Text | OutputFormat::Markdown => {
            println!("{}", serde_json::to_string_pretty(report)?)
        }
    }
    Ok(())
}

fn print_pretty(report: &SkillReport) {
    println!("{}", "Agent Skill Installer".cyan().bold());
    println!();
    println!("{} {}", "Action:".dimmed(), report.action.green());
    println!("{} {}", "Skill:".dimmed(), report.skill.bold());
    println!("{} {}", "Scope:".dimmed(), report.scope.green());
    println!("{} {}", "Version:".dimmed(), report.release_version.green());
    println!(
        "{} {} ({})",
        "Source:".dimmed(),
        report.source.path.green(),
        report.source.kind
    );
    println!();
    for target in &report.targets {
        let status = match target.status.as_str() {
            "linked" | "source" | "removed" => target.status.green().to_string(),
            "missing" => target.status.yellow().to_string(),
            _ => target.status.red().to_string(),
        };
        println!(
            "  {:<7} {:<12} {}",
            target.client.bold(),
            status,
            target.path
        );
        if let Some(link_target) = &target.link_target {
            println!("          {} {}", "->".dimmed(), link_target.dimmed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_expands_to_supported_clients() {
        assert_eq!(
            expand_clients(&[SkillClient::All]),
            vec![
                SkillClient::Claude,
                SkillClient::Gemini,
                SkillClient::Agent,
                SkillClient::Codex,
            ]
        );
    }

    #[test]
    fn explicit_clients_are_preserved() {
        assert_eq!(
            expand_clients(&[SkillClient::Claude, SkillClient::Codex]),
            vec![SkillClient::Claude, SkillClient::Codex]
        );
    }
}
