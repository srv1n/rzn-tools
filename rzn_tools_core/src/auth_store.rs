use crate::auth::AuthDetails;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store unavailable: {0}")]
    Unavailable(String),
    #[error("persist error: {0}")]
    Persist(String),
}

pub trait AuthStore: Send + Sync {
    fn load(&self, provider: &str) -> Option<AuthDetails>;
    fn save(&self, provider: &str, auth: &AuthDetails) -> Result<(), StoreError>;
}

/// Auth profile key delimiter used by [`FileAuthStore`].
///
/// We treat the key before the delimiter as the connector/provider name, and the key after
/// the delimiter as the profile name.
///
/// Backward compatibility:
/// - Keys without this delimiter are treated as `profile = "default"`.
pub const AUTH_PROFILE_DELIM: &str = "::";
pub const CONFIG_DIR_NAME: &str = "rzn-tools";

pub fn config_base_dir() -> PathBuf {
    crate::paths::config_base_dir()
}

pub fn config_dir() -> PathBuf {
    config_base_dir().join(CONFIG_DIR_NAME)
}

pub fn config_file(relative: impl AsRef<Path>) -> PathBuf {
    config_dir().join(relative)
}

/// A simple in-memory store, mainly for testing.
pub struct MemoryAuthStore {
    map: std::sync::Mutex<std::collections::HashMap<String, AuthDetails>>,
}

impl MemoryAuthStore {
    pub fn new() -> Self {
        Self {
            map: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemoryAuthStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthStore for MemoryAuthStore {
    fn load(&self, provider: &str) -> Option<AuthDetails> {
        self.map.lock().ok()?.get(provider).cloned()
    }
    fn save(&self, provider: &str, auth: &AuthDetails) -> Result<(), StoreError> {
        self.map
            .lock()
            .map_err(|e| StoreError::Persist(format!("lock poisoned: {}", e)))?
            .insert(provider.to_string(), auth.clone());
        Ok(())
    }
}

/// A simple file-backed JSON store at `~/.config/rzn-tools/auth.json` (Unix)
/// or `%APPDATA%/rzn-tools/auth.json` (Windows).
pub struct FileAuthStore {
    path: std::path::PathBuf,
}

impl FileAuthStore {
    pub fn new_default() -> Self {
        let dir = config_dir();
        let path = dir.join("auth.json");
        std::fs::create_dir_all(&dir).ok();
        Self { path }
    }

    /// Returns the path to the auth config file
    pub fn config_path(&self) -> String {
        self.path.display().to_string()
    }

    /// Build a storage key for a `(provider, profile)` pair.
    ///
    /// `profile = "default"` uses the legacy key format (just the provider name), so existing
    /// config files keep working.
    pub fn key_for_profile(provider: &str, profile: &str) -> String {
        if profile == "default" {
            provider.to_string()
        } else {
            format!("{provider}{AUTH_PROFILE_DELIM}{profile}")
        }
    }

    /// Parse a storage key into `(provider, profile)`.
    ///
    /// Keys without the delimiter map to `profile = "default"`.
    pub fn parse_key(key: &str) -> (String, String) {
        if let Some((provider, profile)) = key.split_once(AUTH_PROFILE_DELIM) {
            (provider.to_string(), profile.to_string())
        } else {
            (key.to_string(), "default".to_string())
        }
    }

    /// Load auth details for a provider/profile combination.
    pub fn load_profile(&self, provider: &str, profile: &str) -> Option<AuthDetails> {
        self.load(&Self::key_for_profile(provider, profile))
    }

    /// Save auth details for a provider/profile combination.
    pub fn save_profile(
        &self,
        provider: &str,
        profile: &str,
        auth: &AuthDetails,
    ) -> Result<(), StoreError> {
        self.save(&Self::key_for_profile(provider, profile), auth)
    }

    /// Remove credentials for a specific provider profile.
    pub fn remove_profile(&self, provider: &str, profile: &str) -> Result<bool, StoreError> {
        self.remove(&Self::key_for_profile(provider, profile))
    }

    /// List configured profiles for a provider.
    pub fn list_profiles_for_provider(&self, provider: &str) -> Vec<String> {
        let map = self.read_map();
        let mut profiles = map
            .keys()
            .filter_map(|k| {
                let (p, profile) = Self::parse_key(k);
                (p == provider).then_some(profile)
            })
            .collect::<Vec<_>>();
        profiles.sort();
        profiles.dedup();
        profiles
    }

    /// Resolve an "effective" profile name for a provider when the caller did not specify one.
    ///
    /// Selection rules:
    /// 1) If legacy/default credentials exist under the plain provider key, return `"default"`.
    /// 2) Otherwise, if any `provider::profile` entries exist, return the first profile
    ///    in sorted order (deterministic).
    pub fn resolve_profile_for_provider(&self, provider: &str) -> Option<String> {
        if self.load(provider).is_some() {
            return Some("default".to_string());
        }
        self.list_profiles_for_provider(provider).into_iter().next()
    }

    /// Remove credentials for a specific provider
    pub fn remove(&self, provider: &str) -> Result<bool, StoreError> {
        let mut map = self.read_map();
        let existed = map.remove(provider).is_some();
        if existed {
            self.write_map(&map)?;
        }
        Ok(existed)
    }

    /// List all configured providers
    pub fn list_providers(&self) -> Vec<String> {
        self.read_map().keys().cloned().collect()
    }

    fn read_map(&self) -> std::collections::HashMap<String, AuthDetails> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => std::collections::HashMap::new(),
        }
    }
    fn write_map(
        &self,
        map: &std::collections::HashMap<String, AuthDetails>,
    ) -> Result<(), StoreError> {
        let s = serde_json::to_string_pretty(map)
            .map_err(|e| StoreError::Persist(format!("serde: {}", e)))?;
        std::fs::write(&self.path, &s).map_err(|e| StoreError::Persist(e.to_string()))?;

        // Set restrictive permissions on Unix (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&self.path, perms)
                .map_err(|e| StoreError::Persist(format!("chmod: {}", e)))?;
        }

        Ok(())
    }
}

impl AuthStore for FileAuthStore {
    fn load(&self, provider: &str) -> Option<AuthDetails> {
        let map = self.read_map();
        map.get(provider).cloned()
    }

    fn save(&self, provider: &str, auth: &AuthDetails) -> Result<(), StoreError> {
        let mut map = self.read_map();
        map.insert(provider.to_string(), auth.clone());
        self.write_map(&map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_key_roundtrip() {
        let (p, prof) = FileAuthStore::parse_key("reddit");
        assert_eq!(p, "reddit");
        assert_eq!(prof, "default");

        let key = FileAuthStore::key_for_profile("reddit", "default");
        assert_eq!(key, "reddit");

        let key = FileAuthStore::key_for_profile("reddit", "work");
        assert_eq!(key, "reddit::work");

        let (p, prof) = FileAuthStore::parse_key(&key);
        assert_eq!(p, "reddit");
        assert_eq!(prof, "work");
    }

    #[test]
    fn resolves_default_when_legacy_key_exists() {
        let store = FileAuthStore {
            path: std::env::temp_dir().join(format!(
                "rzn_tools_auth_store_test_{}.json",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )),
        };

        let auth = AuthDetails::from_iter([("token".to_string(), "x".to_string())]);
        store.save("reddit", &auth).unwrap();

        assert_eq!(
            store.resolve_profile_for_provider("reddit").as_deref(),
            Some("default")
        );

        let _ = std::fs::remove_file(&store.path);
    }
}
