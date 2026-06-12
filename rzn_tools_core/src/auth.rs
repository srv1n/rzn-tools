use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize};

/// Connector authentication/config details.
///
/// Why it's a string map (even when schemas expose "number"/"boolean" fields):
/// - Most connector implementations ultimately parse scalars from strings (e.g. IMAP port).
/// - Across YAML/JSON/UI forms, it's common to represent scalar inputs as either strings or
///   native JSON types. To keep connectors simple while staying UX-friendly, we accept
///   string/number/bool/null on deserialize and coerce scalars to strings.
///
/// `null` is treated as unset and omitted.
/// Non-scalar values (arrays/objects) are rejected on deserialize.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AuthDetails(HashMap<String, String>);

impl AuthDetails {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
}

impl Deref for AuthDetails {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AuthDetails {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<HashMap<String, String>> for AuthDetails {
    fn from(value: HashMap<String, String>) -> Self {
        Self(value)
    }
}

impl From<AuthDetails> for HashMap<String, String> {
    fn from(value: AuthDetails) -> Self {
        value.0
    }
}

impl FromIterator<(String, String)> for AuthDetails {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        Self(HashMap::from_iter(iter))
    }
}

impl<'de> Deserialize<'de> for AuthDetails {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut map = HashMap::with_capacity(raw.len());

        for (key, value) in raw {
            let value_str = match value {
                serde_json::Value::String(s) => Some(s),
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                serde_json::Value::Null => None,
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                    return Err(serde::de::Error::custom(format!(
                        "invalid auth detail `{key}`: expected scalar (string/number/bool/null)"
                    )));
                }
            };
            if let Some(value_str) = value_str {
                map.insert(key, value_str);
            }
        }

        Ok(Self(map))
    }
}

#[cfg(test)]
mod tests {
    use super::AuthDetails;

    #[test]
    fn auth_details_deserialize_coerces_scalars_to_strings() {
        let value = serde_json::json!({
            "host": "imap.example.com",
            "port": 993,
            "tls": true,
            "optional": null
        });

        let details: AuthDetails = serde_json::from_value(value).expect("deserialize AuthDetails");
        assert_eq!(details.get("host").expect("host"), "imap.example.com");
        assert_eq!(details.get("port").expect("port"), "993");
        assert_eq!(details.get("tls").expect("tls"), "true");
        assert!(!details.contains_key("optional"));
    }

    #[test]
    fn auth_details_deserialize_rejects_objects() {
        let value = serde_json::json!({
            "nested": {"a": 1}
        });

        let err = serde_json::from_value::<AuthDetails>(value)
            .expect_err("should error")
            .to_string();
        assert!(err.contains("invalid auth detail `nested`"), "{err}");
        assert!(err.contains("expected scalar"), "{err}");
    }

    #[test]
    fn auth_details_deserialize_rejects_arrays() {
        let value = serde_json::json!({
            "scopes": ["imap", "smtp"]
        });

        let err = serde_json::from_value::<AuthDetails>(value)
            .expect_err("should error")
            .to_string();
        assert!(err.contains("invalid auth detail `scopes`"), "{err}");
        assert!(err.contains("expected scalar"), "{err}");
    }

    #[test]
    fn auth_details_deserialize_from_yaml_coerces_scalars_to_strings() {
        let yaml = r#"
host: imap.example.com
port: 993
tls: true
optional: null
"#;

        let details: AuthDetails = serde_yaml::from_str(yaml).expect("deserialize AuthDetails");
        assert_eq!(details.get("host").expect("host"), "imap.example.com");
        assert_eq!(details.get("port").expect("port"), "993");
        assert_eq!(details.get("tls").expect("tls"), "true");
        assert!(!details.contains_key("optional"));
    }
}
