use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "rzn-tools";
pub const ASSET_ENV_VAR: &str = "RZN_TOOLS_ASSET_DIR";

pub fn config_base_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|p| p.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_base_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|p| p.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    data_base_dir().join(APP_DIR_NAME)
}

pub fn managed_assets_dir() -> PathBuf {
    data_dir().join("assets")
}

pub fn installed_assets_dir_from_bin(bin_path: &Path) -> Option<PathBuf> {
    let prefix = bin_path.parent()?.parent()?;
    Some(prefix.join("share").join(APP_DIR_NAME))
}

pub fn is_asset_root(path: &Path) -> bool {
    path.join("resources").join("systems").is_dir() && path.join("examples").is_dir()
}

pub fn find_repo_asset_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| ancestor.join("Cargo.toml").is_file() && is_asset_root(ancestor))
        .map(Path::to_path_buf)
}

pub fn installed_assets_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| installed_assets_dir_from_bin(&exe))
}

pub fn resolve_asset_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(ASSET_ENV_VAR) {
        let path = PathBuf::from(path);
        if is_asset_root(&path) {
            return Some(path);
        }
    }

    let managed = managed_assets_dir();
    if is_asset_root(&managed) {
        return Some(managed);
    }

    if let Some(installed) = installed_assets_dir().filter(|path| is_asset_root(path)) {
        return Some(installed);
    }

    std::env::current_dir()
        .ok()
        .and_then(|cwd| find_repo_asset_root(&cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_assets_dir_uses_prefix_share_layout() {
        let bin = Path::new("/usr/local/bin/rzn-tools");
        let assets = installed_assets_dir_from_bin(bin).expect("install prefix");
        assert_eq!(assets, PathBuf::from("/usr/local/share/rzn-tools"));
    }

    #[test]
    fn repo_asset_root_requires_workspace_and_assets() {
        let base = std::env::temp_dir().join(format!("rzn-tools-paths-{}", std::process::id()));
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("resources").join("systems")).expect("systems dir");
        std::fs::create_dir_all(repo.join("examples")).expect("examples dir");
        std::fs::write(repo.join("Cargo.toml"), "[workspace]\n").expect("cargo toml");

        let nested = repo.join("rzn_tools_cli").join("src");
        std::fs::create_dir_all(&nested).expect("nested dir");

        let found = find_repo_asset_root(&nested).expect("repo root");
        assert_eq!(found, repo);

        std::fs::remove_dir_all(base).ok();
    }
}
