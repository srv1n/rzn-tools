use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_ALIAS_FILE: &str = "apple_messages_aliases.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AliasSource {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AliasRecord {
    pub alias: String,
    pub identifier: String,
    pub source: AliasSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AliasStoreData {
    #[serde(default = "default_next_alias_index")]
    next_alias_index: u64,
    #[serde(default)]
    records: Vec<AliasRecord>,
}

impl Default for AliasStoreData {
    fn default() -> Self {
        Self {
            next_alias_index: default_next_alias_index(),
            records: Vec::new(),
        }
    }
}

fn default_next_alias_index() -> u64 {
    1
}

pub(crate) struct AliasStore {
    path: PathBuf,
}

impl AliasStore {
    pub(crate) fn new_default() -> Self {
        let dir = crate::auth_store::config_dir();
        Self {
            path: dir.join(DEFAULT_ALIAS_FILE),
        }
    }

    pub(crate) fn load_state(&self) -> Result<AliasStoreState, String> {
        let data = match std::fs::read_to_string(&self.path) {
            Ok(content) => serde_json::from_str::<AliasStoreData>(&content)
                .map_err(|err| format!("Failed to parse alias store: {err}"))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => AliasStoreData::default(),
            Err(err) => return Err(format!("Failed to read alias store: {err}")),
        };

        Ok(AliasStoreState { data, dirty: false })
    }

    pub(crate) fn save_state(&self, state: &mut AliasStoreState) -> Result<(), String> {
        if !state.dirty {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create alias directory: {err}"))?;
        }

        let payload = serde_json::to_string_pretty(&state.data)
            .map_err(|err| format!("Failed to serialize alias store: {err}"))?;
        std::fs::write(&self.path, payload)
            .map_err(|err| format!("Failed to write alias store: {err}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&self.path, perms)
                .map_err(|err| format!("Failed to secure alias store permissions: {err}"))?;
        }

        state.dirty = false;
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) struct AliasStoreState {
    data: AliasStoreData,
    dirty: bool,
}

impl AliasStoreState {
    pub(crate) fn list(&self) -> Vec<AliasRecord> {
        let mut records = self.data.records.clone();
        records.sort_by(|left, right| left.alias.cmp(&right.alias));
        records
    }

    pub(crate) fn resolve_alias(&self, alias: &str) -> Option<AliasRecord> {
        let alias = normalize_alias(alias);
        self.data
            .records
            .iter()
            .find(|record| record.alias == alias)
            .cloned()
    }

    pub(crate) fn resolve_identifier(&self, identifier: &str) -> Option<AliasRecord> {
        let identifier = normalize_identifier(identifier);
        self.data
            .records
            .iter()
            .find(|record| record.identifier == identifier)
            .cloned()
    }

    pub(crate) fn ensure_alias_for_identifier(
        &mut self,
        identifier: &str,
    ) -> Result<AliasRecord, String> {
        let identifier = normalize_identifier(identifier);
        if identifier.is_empty() {
            return Err("Identifier cannot be empty".to_string());
        }

        if let Some(existing) = self.resolve_identifier(&identifier) {
            return Ok(existing);
        }

        let alias = format!("msg-{:03}", self.data.next_alias_index);
        self.data.next_alias_index += 1;

        let record = AliasRecord {
            alias,
            identifier,
            source: AliasSource::Auto,
        };
        self.data.records.push(record.clone());
        self.dirty = true;
        Ok(record)
    }

    pub(crate) fn upsert_manual_alias(
        &mut self,
        alias: &str,
        identifier: &str,
    ) -> Result<AliasRecord, String> {
        let alias = normalize_alias(alias);
        if alias.is_empty() {
            return Err("Alias cannot be empty".to_string());
        }

        let identifier = normalize_identifier(identifier);
        if identifier.is_empty() {
            return Err("Identifier cannot be empty".to_string());
        }

        if let Some(existing_idx) = self
            .data
            .records
            .iter()
            .position(|record| record.identifier == identifier && record.alias != alias)
        {
            self.data.records.remove(existing_idx);
            self.dirty = true;
        }

        if let Some(existing_idx) = self
            .data
            .records
            .iter()
            .position(|record| record.alias == alias)
        {
            let record = &mut self.data.records[existing_idx];
            if record.identifier != identifier || record.source != AliasSource::Manual {
                record.identifier = identifier;
                record.source = AliasSource::Manual;
                self.dirty = true;
            }
            return Ok(record.clone());
        }

        let record = AliasRecord {
            alias,
            identifier,
            source: AliasSource::Manual,
        };
        self.data.records.push(record.clone());
        self.dirty = true;
        Ok(record)
    }

    pub(crate) fn remove_alias(&mut self, alias: &str) -> bool {
        let alias = normalize_alias(alias);
        let before_len = self.data.records.len();
        self.data.records.retain(|record| record.alias != alias);
        let removed = self.data.records.len() != before_len;
        if removed {
            self.dirty = true;
        }
        removed
    }
}

pub(crate) fn normalize_alias(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn normalize_identifier(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.contains('@') {
        return trimmed.to_ascii_lowercase();
    }

    let digits = trimmed
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return trimmed.to_ascii_lowercase();
    }

    if trimmed.starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_identifiers() {
        assert_eq!(normalize_identifier(" +1 (555) 123-4567 "), "+15551234567");
        assert_eq!(normalize_identifier("(555) 123-4567"), "5551234567");
        assert_eq!(
            normalize_identifier("User@Example.com "),
            "user@example.com"
        );
    }

    #[test]
    fn manual_alias_replaces_auto_alias_for_same_identifier() {
        let mut state = AliasStoreState {
            data: AliasStoreData::default(),
            dirty: false,
        };

        let auto = state
            .ensure_alias_for_identifier("+15551234567")
            .expect("auto alias should be created");
        assert_eq!(auto.alias, "msg-001");

        let manual = state
            .upsert_manual_alias("mom", "+1 (555) 123-4567")
            .expect("manual alias should be created");
        assert_eq!(manual.alias, "mom");

        assert!(state.resolve_alias("msg-001").is_none());
        assert_eq!(
            state
                .resolve_identifier("+15551234567")
                .expect("identifier should resolve")
                .alias,
            "mom"
        );
    }
}
