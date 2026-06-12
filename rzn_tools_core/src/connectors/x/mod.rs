use std::borrow::Cow;
use std::sync::{Arc, RwLock};

use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method};
use rmcp::model::*;
use serde_json::{json, Value};
use sha1::Sha1;
use std::collections::{HashMap, HashSet};
use url::Url;
use urlencoding::encode;

type HmacSha1 = Hmac<Sha1>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XAuthMode {
    Auto,
    Bearer,
    OAuth2,
    OAuth1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthRequirement {
    PublicRead,
    UserContext,
}

#[derive(Debug, Clone)]
struct OAuth1Credentials {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
}

#[derive(Debug, Clone, Default)]
struct OAuth2State {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    scope: Option<String>,
    token_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct XOperationSpec {
    operation_id: &'static str,
    method: &'static str,
    path_template: &'static str,
    auth_requirement: AuthRequirement,
}

pub struct XApiConnector {
    client: Client,
    bearer_token: Option<String>,
    oauth2_access_token: Option<String>,
    oauth2_refresh_token: Option<String>,
    oauth2_expires_at: Option<String>,
    oauth2_scope: Option<String>,
    oauth2_token_type: Option<String>,
    oauth2_state: RwLock<OAuth2State>,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    oauth1_consumer_key: Option<String>,
    oauth1_consumer_secret: Option<String>,
    oauth1_access_token: Option<String>,
    oauth1_access_token_secret: Option<String>,
    base_url: String,
}

impl XApiConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/x")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let bearer_token = auth
            .get("bearer_token")
            .cloned()
            .or_else(|| std::env::var("X_BEARER_TOKEN").ok())
            .or_else(|| std::env::var("TWITTER_BEARER_TOKEN").ok());
        let oauth2_access_token = auth
            .get("oauth2_access_token")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH2_ACCESS_TOKEN").ok());
        let oauth2_refresh_token = auth
            .get("oauth2_refresh_token")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH2_REFRESH_TOKEN").ok());
        let oauth2_expires_at = auth
            .get("oauth2_expires_at")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH2_EXPIRES_AT").ok());
        let oauth2_scope = auth
            .get("oauth2_scope")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH2_SCOPE").ok());
        let oauth2_token_type = auth
            .get("oauth2_token_type")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH2_TOKEN_TYPE").ok());
        let client_id = auth
            .get("client_id")
            .cloned()
            .or_else(|| std::env::var("X_CLIENT_ID").ok());
        let client_secret = auth
            .get("client_secret")
            .cloned()
            .or_else(|| std::env::var("X_CLIENT_SECRET").ok());
        let redirect_uri = auth
            .get("redirect_uri")
            .cloned()
            .or_else(|| std::env::var("X_REDIRECT_URI").ok());
        let oauth1_consumer_key = auth
            .get("oauth1_consumer_key")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH_CONSUMER_KEY").ok());
        let oauth1_consumer_secret = auth
            .get("oauth1_consumer_secret")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH_CONSUMER_SECRET").ok());
        let oauth1_access_token = auth
            .get("oauth1_access_token")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH_ACCESS_TOKEN").ok());
        let oauth1_access_token_secret = auth
            .get("oauth1_access_token_secret")
            .cloned()
            .or_else(|| std::env::var("X_OAUTH_ACCESS_TOKEN_SECRET").ok());

        let base_url = auth
            .get("base_url")
            .cloned()
            .or_else(|| std::env::var("X_API_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.twitter.com/2".to_string());

        Ok(Self {
            client,
            bearer_token,
            oauth2_access_token: oauth2_access_token.clone(),
            oauth2_refresh_token: oauth2_refresh_token.clone(),
            oauth2_expires_at: oauth2_expires_at.clone(),
            oauth2_scope: oauth2_scope.clone(),
            oauth2_token_type: oauth2_token_type.clone(),
            oauth2_state: RwLock::new(OAuth2State {
                access_token: oauth2_access_token.clone(),
                refresh_token: oauth2_refresh_token.clone(),
                expires_at: oauth2_expires_at.clone(),
                scope: oauth2_scope.clone(),
                token_type: oauth2_token_type.clone(),
            }),
            client_id,
            client_secret,
            redirect_uri,
            oauth1_consumer_key,
            oauth1_consumer_secret,
            oauth1_access_token,
            oauth1_access_token_secret,
            base_url,
        })
    }

    fn oauth2_snapshot(&self) -> OAuth2State {
        self.oauth2_state.read().expect("oauth2 state read").clone()
    }

    fn operation_registry() -> &'static [XOperationSpec] {
        &[
            XOperationSpec {
                operation_id: "getUsersMe",
                method: "GET",
                path_template: "users/me",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "getUsersMentions",
                method: "GET",
                path_template: "users/{id}/mentions",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "getUsersTimeline",
                method: "GET",
                path_template: "users/{id}/timelines/reverse_chronological",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "searchPostsRecent",
                method: "GET",
                path_template: "tweets/search/recent",
                auth_requirement: AuthRequirement::PublicRead,
            },
            XOperationSpec {
                operation_id: "searchPostsAll",
                method: "GET",
                path_template: "tweets/search/all",
                auth_requirement: AuthRequirement::PublicRead,
            },
            XOperationSpec {
                operation_id: "createPosts",
                method: "POST",
                path_template: "tweets",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "deletePosts",
                method: "DELETE",
                path_template: "tweets/{id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "likePost",
                method: "POST",
                path_template: "users/{id}/likes",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "unlikePost",
                method: "DELETE",
                path_template: "users/{id}/likes/{tweet_id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "repostPost",
                method: "POST",
                path_template: "users/{id}/retweets",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "unrepostPost",
                method: "DELETE",
                path_template: "users/{id}/retweets/{source_tweet_id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "followUser",
                method: "POST",
                path_template: "users/{id}/following",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "unfollowUser",
                method: "DELETE",
                path_template: "users/{source_user_id}/following/{target_user_id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "getUsersBookmarks",
                method: "GET",
                path_template: "users/{id}/bookmarks",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "createUsersBookmark",
                method: "POST",
                path_template: "users/{id}/bookmarks",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "deleteUsersBookmark",
                method: "DELETE",
                path_template: "users/{id}/bookmarks/{tweet_id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "createLists",
                method: "POST",
                path_template: "lists",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "updateLists",
                method: "PUT",
                path_template: "lists/{id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "deleteLists",
                method: "DELETE",
                path_template: "lists/{id}",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "createDirectMessagesConversation",
                method: "POST",
                path_template: "dm_conversations",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "getDirectMessagesEventsByConversationId",
                method: "GET",
                path_template: "dm_conversations/{id}/dm_events",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "initializeMediaUpload",
                method: "POST",
                path_template: "media/upload/initialize",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "appendMediaUpload",
                method: "POST",
                path_template: "media/upload/{id}/append",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "finalizeMediaUpload",
                method: "POST",
                path_template: "media/upload/{id}/finalize",
                auth_requirement: AuthRequirement::UserContext,
            },
            XOperationSpec {
                operation_id: "getUsage",
                method: "GET",
                path_template: "usage/tweets",
                auth_requirement: AuthRequirement::UserContext,
            },
        ]
    }

    fn operation_spec(operation_id: &str) -> Option<&'static XOperationSpec> {
        Self::operation_registry()
            .iter()
            .find(|spec| spec.operation_id == operation_id)
    }

    fn write_oauth2_state(&self, state: OAuth2State) {
        *self.oauth2_state.write().expect("oauth2 state write") = state;
    }

    fn value_to_query_string(value: &Value) -> Result<String, ConnectorError> {
        match value {
            Value::String(s) => Ok(s.clone()),
            Value::Number(n) => Ok(n.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Null => Ok(String::new()),
            Value::Array(arr) => {
                let mut parts = Vec::with_capacity(arr.len());
                for item in arr {
                    parts.push(Self::value_to_query_string(item)?);
                }
                Ok(parts.join(","))
            }
            Value::Object(_) => Err(ConnectorError::InvalidParams(
                "Object values are not supported in query params".to_string(),
            )),
        }
    }

    fn render_path_template(
        template: &str,
        path_params: &serde_json::Map<String, Value>,
    ) -> Result<String, ConnectorError> {
        let mut rendered = template.to_string();
        while let Some(start) = rendered.find('{') {
            let end = rendered[start..]
                .find('}')
                .map(|idx| start + idx)
                .ok_or_else(|| ConnectorError::Other("Unclosed path template token".into()))?;
            let key = &rendered[start + 1..end];
            let replacement = path_params
                .get(key)
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(format!(
                        "Missing path_params.{key} for X raw operation"
                    ))
                })
                .and_then(Self::value_to_query_string)?;
            rendered.replace_range(start..=end, &replacement);
        }
        Ok(rendered)
    }

    fn bearer_headers(&self, token: &str) -> Result<HeaderMap, ConnectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            Self::header_value(&format!("Bearer {token}"))?,
        );
        Ok(headers)
    }

    fn header_value(raw: &str) -> Result<HeaderValue, ConnectorError> {
        HeaderValue::from_str(raw).map_err(|e| ConnectorError::Other(e.to_string()))
    }

    fn now_epoch() -> i64 {
        Utc::now().timestamp()
    }

    fn parse_timestamp_maybe(raw: Option<&String>) -> Option<i64> {
        let raw = raw?.trim();
        if raw.is_empty() {
            return None;
        }
        if let Ok(epoch) = raw.parse::<i64>() {
            return Some(epoch);
        }
        DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|dt| dt.with_timezone(&Utc).timestamp())
    }

    fn oauth2_expired(&self) -> bool {
        let state = self.oauth2_snapshot();
        Self::parse_timestamp_maybe(state.expires_at.as_ref())
            .is_some_and(|exp| exp <= Self::now_epoch())
    }

    fn oauth2_available(&self) -> bool {
        let state = self.oauth2_snapshot();
        state.access_token.is_some() && !self.oauth2_expired()
    }

    fn oauth1_credentials(&self) -> Option<OAuth1Credentials> {
        Some(OAuth1Credentials {
            consumer_key: self.oauth1_consumer_key.clone()?,
            consumer_secret: self.oauth1_consumer_secret.clone()?,
            access_token: self.oauth1_access_token.clone()?,
            access_token_secret: self.oauth1_access_token_secret.clone()?,
        })
    }

    fn parse_auth_mode(value: Option<&str>) -> Result<XAuthMode, ConnectorError> {
        match value.unwrap_or("auto").trim() {
            "" | "auto" => Ok(XAuthMode::Auto),
            "bearer" => Ok(XAuthMode::Bearer),
            "oauth2" | "oauth2_user" => Ok(XAuthMode::OAuth2),
            "oauth1" => Ok(XAuthMode::OAuth1),
            other => Err(ConnectorError::InvalidParams(format!(
                "Invalid auth_mode: {other}. Expected one of: auto, bearer, oauth2, oauth1"
            ))),
        }
    }

    fn resolve_auth_mode(
        &self,
        explicit: XAuthMode,
        requirement: AuthRequirement,
    ) -> Result<XAuthMode, ConnectorError> {
        if explicit != XAuthMode::Auto {
            return self.ensure_auth_mode_available(explicit, requirement);
        }

        match requirement {
            AuthRequirement::PublicRead => {
                if self.bearer_token.is_some() {
                    Ok(XAuthMode::Bearer)
                } else if self.oauth2_available() {
                    Ok(XAuthMode::OAuth2)
                } else if self.oauth1_credentials().is_some() {
                    Ok(XAuthMode::OAuth1)
                } else {
                    Err(ConnectorError::Authentication(
                        "Missing credentials for X public read access. Configure bearer_token, oauth2_access_token, or oauth1_* credentials.".to_string(),
                    ))
                }
            }
            AuthRequirement::UserContext => {
                if self.oauth2_available() {
                    Ok(XAuthMode::OAuth2)
                } else if self.oauth1_credentials().is_some() {
                    Ok(XAuthMode::OAuth1)
                } else {
                    Err(ConnectorError::Authentication(
                        "Missing user-context credentials for X. Configure oauth2_access_token (preferred) or oauth1_* credentials.".to_string(),
                    ))
                }
            }
        }
    }

    fn ensure_auth_mode_available(
        &self,
        mode: XAuthMode,
        requirement: AuthRequirement,
    ) -> Result<XAuthMode, ConnectorError> {
        match mode {
            XAuthMode::Auto => self.resolve_auth_mode(XAuthMode::Auto, requirement),
            XAuthMode::Bearer => {
                if requirement == AuthRequirement::UserContext {
                    return Err(ConnectorError::Authentication(
                        "Bearer auth cannot satisfy this user-context X API operation. Use oauth2 or oauth1."
                            .to_string(),
                    ));
                }
                if self.bearer_token.is_some() {
                    Ok(XAuthMode::Bearer)
                } else {
                    Err(ConnectorError::Authentication(
                        "Missing bearer_token for X bearer auth".to_string(),
                    ))
                }
            }
            XAuthMode::OAuth2 => {
                if self.oauth2_available() {
                    Ok(XAuthMode::OAuth2)
                } else {
                    Err(ConnectorError::Authentication(
                        "Missing usable oauth2_access_token for X OAuth2 auth".to_string(),
                    ))
                }
            }
            XAuthMode::OAuth1 => {
                if self.oauth1_credentials().is_some() {
                    Ok(XAuthMode::OAuth1)
                } else {
                    Err(ConnectorError::Authentication(
                        "Missing oauth1_* credentials for X OAuth1 auth".to_string(),
                    ))
                }
            }
        }
    }

    fn oauth_nonce() -> String {
        format!("{}{}", Self::now_epoch(), std::process::id())
    }

    fn build_oauth1_authorization(
        &self,
        method: &Method,
        url: &str,
        query: &[(&str, String)],
        creds: &OAuth1Credentials,
    ) -> Result<String, ConnectorError> {
        let timestamp = Self::now_epoch().to_string();
        let nonce = Self::oauth_nonce();
        let mut signing_pairs: Vec<(String, String)> = Vec::new();

        let parsed = Url::parse(url).map_err(|e| ConnectorError::Other(e.to_string()))?;
        for (k, v) in parsed.query_pairs() {
            signing_pairs.push((k.into_owned(), v.into_owned()));
        }
        for (k, v) in query {
            signing_pairs.push(((*k).to_string(), v.clone()));
        }

        let oauth_pairs = vec![
            ("oauth_consumer_key".to_string(), creds.consumer_key.clone()),
            ("oauth_nonce".to_string(), nonce.clone()),
            (
                "oauth_signature_method".to_string(),
                "HMAC-SHA1".to_string(),
            ),
            ("oauth_timestamp".to_string(), timestamp.clone()),
            ("oauth_token".to_string(), creds.access_token.clone()),
            ("oauth_version".to_string(), "1.0".to_string()),
        ];
        signing_pairs.extend(oauth_pairs.clone());
        signing_pairs.sort_by(|a, b| match a.0.cmp(&b.0) {
            std::cmp::Ordering::Equal => a.1.cmp(&b.1),
            other => other,
        });

        let normalized_params = signing_pairs
            .into_iter()
            .map(|(k, v)| format!("{}={}", encode(&k), encode(&v)))
            .collect::<Vec<_>>()
            .join("&");

        let base_url = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default(),
            parsed.path()
        );
        let signature_base = format!(
            "{}&{}&{}",
            method.as_str().to_uppercase(),
            encode(&base_url),
            encode(&normalized_params)
        );
        let signing_key = format!(
            "{}&{}",
            encode(&creds.consumer_secret),
            encode(&creds.access_token_secret)
        );

        let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes())
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        mac.update(signature_base.as_bytes());
        let signature = BASE64_STANDARD.encode(mac.finalize().into_bytes());

        let mut auth_pairs = oauth_pairs;
        auth_pairs.push(("oauth_signature".to_string(), signature));
        auth_pairs.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(format!(
            "OAuth {}",
            auth_pairs
                .into_iter()
                .map(|(k, v)| format!("{}=\"{}\"", encode(&k), encode(&v)))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    async fn send_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        requirement: AuthRequirement,
        explicit_mode: XAuthMode,
    ) -> Result<Value, ConnectorError> {
        let mode = self.resolve_auth_mode(explicit_mode, requirement)?;
        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let mut request = self.client.request(method.clone(), &url);
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(body) = body.clone() {
            request = request.json(&body);
        }

        match mode {
            XAuthMode::Bearer => {
                request = request.headers(self.bearer_headers(
                    self.bearer_token.as_deref().ok_or_else(|| {
                        ConnectorError::Authentication("Missing bearer_token".to_string())
                    })?,
                )?);
            }
            XAuthMode::OAuth2 => {
                if self.oauth2_expired()
                    && self.oauth2_snapshot().refresh_token.is_some()
                    && self.client_id.is_some()
                {
                    let _ = self.refresh_oauth2_access_token().await?;
                }
                let oauth2 = self.oauth2_snapshot();
                request = request.headers(self.bearer_headers(
                    oauth2.access_token.as_deref().ok_or_else(|| {
                        ConnectorError::Authentication("Missing oauth2_access_token".to_string())
                    })?,
                )?);
            }
            XAuthMode::OAuth1 => {
                let creds = self.oauth1_credentials().ok_or_else(|| {
                    ConnectorError::Authentication("Missing oauth1_* credentials".to_string())
                })?;
                let mut headers = HeaderMap::new();
                headers.insert(
                    AUTHORIZATION,
                    Self::header_value(
                        &self.build_oauth1_authorization(&method, &url, query, &creds)?,
                    )?,
                );
                if body.is_some() {
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                }
                request = request.headers(headers);
            }
            XAuthMode::Auto => unreachable!(),
        }

        let resp = request.send().await.map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let body = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "X API request failed ({status}): {body}"
            )));
        }
        serde_json::from_str(&body).map_err(|e| ConnectorError::Other(e.to_string()))
    }

    async fn get_json_as(
        &self,
        path: &str,
        query: &[(&str, String)],
        requirement: AuthRequirement,
        explicit_mode: XAuthMode,
    ) -> Result<Value, ConnectorError> {
        self.send_json(Method::GET, path, query, None, requirement, explicit_mode)
            .await
    }

    async fn post_json_as(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Value,
        requirement: AuthRequirement,
        explicit_mode: XAuthMode,
    ) -> Result<Value, ConnectorError> {
        self.send_json(
            Method::POST,
            path,
            query,
            Some(body),
            requirement,
            explicit_mode,
        )
        .await
    }

    async fn delete_json_as(
        &self,
        path: &str,
        query: &[(&str, String)],
        requirement: AuthRequirement,
        explicit_mode: XAuthMode,
    ) -> Result<Value, ConnectorError> {
        self.send_json(
            Method::DELETE,
            path,
            query,
            None,
            requirement,
            explicit_mode,
        )
        .await
    }

    fn auth_mode_from_args(
        args: &serde_json::Map<String, Value>,
    ) -> Result<XAuthMode, ConnectorError> {
        Self::parse_auth_mode(args.get("auth_mode").and_then(Value::as_str))
    }

    async fn build_auth_status(&self) -> Result<Value, ConnectorError> {
        let oauth2 = self.oauth2_snapshot();
        Ok(json!({
            "configured": self.bearer_token.is_some()
                || oauth2.access_token.is_some()
                || self.oauth1_credentials().is_some(),
            "base_url": self.base_url,
            "bearer": {
                "present": self.bearer_token.is_some(),
            },
            "oauth2": {
                "present": oauth2.access_token.is_some(),
                "expires_at": Self::parse_timestamp_maybe(oauth2.expires_at.as_ref()),
                "expired": self.oauth2_expired(),
                "refresh_token_present": oauth2.refresh_token.is_some(),
                "scopes": oauth2.scope.as_ref()
                    .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
                    .unwrap_or_default(),
                "token_type": oauth2.token_type,
                "client_id_present": self.client_id.is_some(),
                "client_secret_present": self.client_secret.is_some(),
                "redirect_uri": self.redirect_uri.clone(),
            },
            "oauth1": {
                "consumer_key_present": self.oauth1_consumer_key.is_some(),
                "consumer_secret_present": self.oauth1_consumer_secret.is_some(),
                "access_token_present": self.oauth1_access_token.is_some(),
                "access_token_secret_present": self.oauth1_access_token_secret.is_some(),
                "ready": self.oauth1_credentials().is_some(),
            },
            "preferred_modes": {
                "public_read": match self.resolve_auth_mode(XAuthMode::Auto, AuthRequirement::PublicRead) {
                    Ok(mode) => format!("{mode:?}").to_lowercase(),
                    Err(err) => err.to_string(),
                },
                "user_context": match self.resolve_auth_mode(XAuthMode::Auto, AuthRequirement::UserContext) {
                    Ok(mode) => format!("{mode:?}").to_lowercase(),
                    Err(err) => err.to_string(),
                },
            }
        }))
    }

    async fn authenticated_user_id(&self, auth_mode: XAuthMode) -> Result<String, ConnectorError> {
        let me = self
            .get_json_as("users/me", &[], AuthRequirement::UserContext, auth_mode)
            .await?;
        me.get("data")
            .and_then(|d| d.get("id"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| ConnectorError::Other("X /users/me returned no id".into()))
    }

    async fn refresh_oauth2_access_token(&self) -> Result<Value, ConnectorError> {
        let oauth2 = self.oauth2_snapshot();
        let refresh_token = oauth2.refresh_token.ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing oauth2_refresh_token for X OAuth2 refresh".to_string(),
            )
        })?;
        let client_id = self.client_id.clone().ok_or_else(|| {
            ConnectorError::Authentication("Missing client_id for X OAuth2 refresh".to_string())
        })?;

        let token_url = "https://api.x.com/2/oauth2/token";
        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ];
        if let Some(secret) = self.client_secret.clone() {
            if !secret.trim().is_empty() {
                form.push(("client_secret", secret));
            }
        }

        let response = self
            .client
            .post(token_url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&form)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = response.status();
        let payload: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Authentication(format!(
                "X OAuth2 refresh failed with HTTP {status}: {payload}"
            )));
        }

        let access_token = payload
            .get("access_token")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ConnectorError::Authentication(
                    "X OAuth2 refresh response did not include access_token".to_string(),
                )
            })?;
        let refresh_token = payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| self.oauth2_snapshot().refresh_token);
        let expires_at = payload
            .get("expires_in")
            .and_then(Value::as_i64)
            .map(|seconds| (Self::now_epoch() + seconds - 60).to_string());
        let scope = payload
            .get("scope")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| self.oauth2_snapshot().scope);
        let token_type = payload
            .get("token_type")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| self.oauth2_snapshot().token_type);

        self.write_oauth2_state(OAuth2State {
            access_token: Some(access_token.clone()),
            refresh_token: refresh_token.clone(),
            expires_at: expires_at.clone(),
            scope: scope.clone(),
            token_type: token_type.clone(),
        });

        Ok(json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "oauth2_expires_at": expires_at,
            "oauth2_scope": scope,
            "oauth2_token_type": token_type,
            "raw": payload
        }))
    }

    fn parse_rfc3339_or_date(
        raw: &str,
        date_is_end_of_day: bool,
    ) -> Result<DateTime<Utc>, ConnectorError> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
            return Ok(dt.with_timezone(&Utc));
        }

        let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
            ConnectorError::InvalidParams(format!(
                "Invalid datetime/date: {raw}. Expected RFC3339 (e.g. 2026-02-24T13:45:00Z) or \
YYYY-MM-DD"
            ))
        })?;

        let time = if date_is_end_of_day {
            NaiveTime::from_hms_opt(23, 59, 59).unwrap()
        } else {
            NaiveTime::MIN
        };
        Ok(DateTime::<Utc>::from_naive_utc_and_offset(
            date.and_time(time),
            Utc,
        ))
    }

    fn parse_since(raw: &str) -> Result<Duration, ConnectorError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "Invalid since: empty string".into(),
            ));
        }

        let (num, unit) = raw.split_at(raw.len() - 1);
        let n: i64 = num.parse().map_err(|_| {
            ConnectorError::InvalidParams(format!(
                "Invalid since: {raw}. Expected like 15m, 2h, 7d, 4w"
            ))
        })?;

        if n <= 0 {
            return Err(ConnectorError::InvalidParams(format!(
                "Invalid since: {raw}. Value must be > 0"
            )));
        }

        match unit {
            "m" => Ok(Duration::minutes(n)),
            "h" => Ok(Duration::hours(n)),
            "d" => Ok(Duration::days(n)),
            "w" => Ok(Duration::weeks(n)),
            _ => Err(ConnectorError::InvalidParams(format!(
                "Invalid since unit: {unit}. Expected one of: m, h, d, w"
            ))),
        }
    }

    fn get_time_arg(
        args: &serde_json::Map<String, Value>,
        key: &str,
        date_is_end_of_day: bool,
    ) -> Result<Option<String>, ConnectorError> {
        match args.get(key).and_then(Value::as_str) {
            Some(raw) if !raw.trim().is_empty() => {
                let dt = Self::parse_rfc3339_or_date(raw.trim(), date_is_end_of_day)?;
                Ok(Some(dt.to_rfc3339()))
            }
            _ => Ok(None),
        }
    }

    fn metric_i64(tweet: &Value, key: &str) -> i64 {
        tweet
            .get("public_metrics")
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
            .unwrap_or(0)
    }

    fn views_i64(tweet: &Value) -> i64 {
        // X API fields have changed over time; tolerate either key.
        Self::metric_i64(tweet, "view_count").max(Self::metric_i64(tweet, "impression_count"))
    }

    fn created_at_ts(tweet: &Value) -> i64 {
        tweet
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0)
    }

    fn compact_user(user: &Value) -> Value {
        let mut out = serde_json::Map::new();
        for k in [
            "id",
            "username",
            "name",
            "verified",
            "created_at",
            "profile_image_url",
        ] {
            if let Some(v) = user.get(k) {
                out.insert(k.to_string(), v.clone());
            }
        }
        Value::Object(out)
    }

    fn users_by_id(v: &Value) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        let users = v
            .get("includes")
            .and_then(|i| i.get("users"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for user in users {
            if let Some(id) = user.get("id").and_then(Value::as_str) {
                map.insert(id.to_string(), Self::compact_user(&user));
            }
        }
        map
    }

    fn compact_tweet(tweet: &Value, users: &HashMap<String, Value>) -> Value {
        let mut out = serde_json::Map::new();
        for k in [
            "id",
            "text",
            "created_at",
            "author_id",
            "conversation_id",
            "lang",
        ] {
            if let Some(v) = tweet.get(k) {
                out.insert(k.to_string(), v.clone());
            }
        }
        if let Some(v) = tweet.get("public_metrics") {
            out.insert("public_metrics".to_string(), v.clone());
        }
        if let Some(author_id) = tweet.get("author_id").and_then(Value::as_str) {
            if let Some(user) = users.get(author_id) {
                out.insert("author".to_string(), user.clone());
            }
        }
        Value::Object(out)
    }
}

#[async_trait]
impl Connector for XApiConnector {
    fn name(&self) -> &'static str {
        "x"
    }

    fn description(&self) -> &'static str {
        "Official X (Twitter) API v2 connector with bearer, OAuth 2.0, and OAuth 1.0a support."
    }

    fn display_name(&self) -> &'static str {
        "X (Twitter)"
    }

    fn icon(&self) -> &'static str {
        "x"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["social", "news", "api"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut auth = AuthDetails::new();
        let oauth2 = self.oauth2_snapshot();
        for (key, value) in [
            ("bearer_token", self.bearer_token.clone()),
            ("oauth2_access_token", oauth2.access_token.clone()),
            ("oauth2_refresh_token", oauth2.refresh_token.clone()),
            ("oauth2_expires_at", oauth2.expires_at.clone()),
            ("oauth2_scope", oauth2.scope.clone()),
            ("oauth2_token_type", oauth2.token_type.clone()),
            ("client_id", self.client_id.clone()),
            ("client_secret", self.client_secret.clone()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("oauth1_consumer_key", self.oauth1_consumer_key.clone()),
            (
                "oauth1_consumer_secret",
                self.oauth1_consumer_secret.clone(),
            ),
            ("oauth1_access_token", self.oauth1_access_token.clone()),
            (
                "oauth1_access_token_secret",
                self.oauth1_access_token_secret.clone(),
            ),
            ("base_url", Some(self.base_url.clone())),
        ] {
            if let Some(value) = value {
                auth.insert(key.to_string(), value);
            }
        }
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.bearer_token = details
            .get("bearer_token")
            .cloned()
            .or_else(|| self.bearer_token.clone());
        self.oauth2_access_token = details
            .get("oauth2_access_token")
            .cloned()
            .or_else(|| self.oauth2_access_token.clone());
        self.oauth2_refresh_token = details
            .get("oauth2_refresh_token")
            .cloned()
            .or_else(|| self.oauth2_refresh_token.clone());
        self.oauth2_expires_at = details
            .get("oauth2_expires_at")
            .cloned()
            .or_else(|| self.oauth2_expires_at.clone());
        self.oauth2_scope = details
            .get("oauth2_scope")
            .cloned()
            .or_else(|| self.oauth2_scope.clone());
        self.oauth2_token_type = details
            .get("oauth2_token_type")
            .cloned()
            .or_else(|| self.oauth2_token_type.clone());
        self.write_oauth2_state(OAuth2State {
            access_token: self.oauth2_access_token.clone(),
            refresh_token: self.oauth2_refresh_token.clone(),
            expires_at: self.oauth2_expires_at.clone(),
            scope: self.oauth2_scope.clone(),
            token_type: self.oauth2_token_type.clone(),
        });
        self.client_id = details
            .get("client_id")
            .cloned()
            .or_else(|| self.client_id.clone());
        self.client_secret = details
            .get("client_secret")
            .cloned()
            .or_else(|| self.client_secret.clone());
        self.redirect_uri = details
            .get("redirect_uri")
            .cloned()
            .or_else(|| self.redirect_uri.clone());
        self.oauth1_consumer_key = details
            .get("oauth1_consumer_key")
            .cloned()
            .or_else(|| self.oauth1_consumer_key.clone());
        self.oauth1_consumer_secret = details
            .get("oauth1_consumer_secret")
            .cloned()
            .or_else(|| self.oauth1_consumer_secret.clone());
        self.oauth1_access_token = details
            .get("oauth1_access_token")
            .cloned()
            .or_else(|| self.oauth1_access_token.clone());
        self.oauth1_access_token_secret = details
            .get("oauth1_access_token_secret")
            .cloned()
            .or_else(|| self.oauth1_access_token_secret.clone());
        if let Some(base_url) = details.get("base_url") {
            self.base_url = base_url.clone();
        }
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        if self.bearer_token.is_some() {
            let _ = self
                .get_json_as(
                    "users/by/username/TwitterDev",
                    &[("user.fields", "created_at,public_metrics".to_string())],
                    AuthRequirement::PublicRead,
                    XAuthMode::Bearer,
                )
                .await?;
            return Ok(());
        }
        let _ = self
            .get_json_as(
                "users/me",
                &[],
                AuthRequirement::UserContext,
                XAuthMode::Auto,
            )
            .await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "bearer_token".to_string(),
                    label: "X API Bearer Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "X API v2 bearer token. You can also set X_BEARER_TOKEN or \
TWITTER_BEARER_TOKEN."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth2_access_token".to_string(),
                    label: "OAuth2 Access Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "OAuth 2.0 user access token for user-context X operations.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth2_refresh_token".to_string(),
                    label: "OAuth2 Refresh Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional OAuth 2.0 refresh token for future refresh support.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth2_expires_at".to_string(),
                    label: "OAuth2 Expires At".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "OAuth 2.0 access token expiry as epoch seconds or RFC3339 timestamp."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth2_scope".to_string(),
                    label: "OAuth2 Scope".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Space-delimited OAuth 2.0 scopes granted to the token.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth2_token_type".to_string(),
                    label: "OAuth2 Token Type".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional OAuth 2.0 token type metadata.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_id".to_string(),
                    label: "OAuth Client ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "OAuth client id for future OAuth2 refresh/browser flows.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    label: "OAuth Client Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional OAuth client secret when your X app uses one.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "redirect_uri".to_string(),
                    label: "OAuth Redirect URI".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Optional redirect URI metadata for OAuth2 user flows.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth1_consumer_key".to_string(),
                    label: "OAuth1 Consumer Key".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "OAuth 1.0a consumer key for legacy X user-context operations.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth1_consumer_secret".to_string(),
                    label: "OAuth1 Consumer Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "OAuth 1.0a consumer secret for legacy X user-context operations."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth1_access_token".to_string(),
                    label: "OAuth1 Access Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("OAuth 1.0a access token.".to_string()),
                    options: None,
                },
                Field {
                    name: "oauth1_access_token_secret".to_string(),
                    label: "OAuth1 Access Token Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("OAuth 1.0a access token secret.".to_string()),
                    options: None,
                },
                Field {
                    name: "base_url".to_string(),
                    label: "Base URL".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Optional override. Defaults to https://api.twitter.com/2".to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }

    async fn initialize(
        &self,
        _r: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().into(),
                version: "0.1.0".into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "X (Twitter) official API connector. Supports bearer for public reads and OAuth 2.0 / OAuth 1.0a for user-context operations. Use get_auth_status to inspect configured auth. Use whoami to validate user-context auth."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);

        let tools = vec![
            Tool {
                name: Cow::Borrowed("get_auth_status"),
                title: None,
                description: Some(Cow::Borrowed("Inspect which X auth families are configured and which mode will be preferred.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {}
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("whoami"),
                title: None,
                description: Some(Cow::Borrowed("Validate user-context auth by calling the authenticated /users/me endpoint.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_user_by_username"),
                title: None,
                description: Some(Cow::Borrowed("Get a user by username (no @).")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "username": { "type": "string", "description": "Username without @" },
                            "user_fields": { "type": "string", "description": "Comma-separated user.fields override (optional)." },
                            "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_profile"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Alias for get_user_by_username (kept for URL resolver compatibility).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "username": { "type": "string", "description": "Username without @." },
                            "user_fields": { "type": "string", "description": "Comma-separated user.fields override (optional)." },
                            "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_tweet"),
                title: None,
                description: Some(Cow::Borrowed("Get a tweet by tweet_id via the official API.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "tweet_id": { "type": "string", "description": "Tweet id." },
                            "tweet_fields": { "type": "string", "description": "Comma-separated tweet.fields override (optional)." },
                            "expansions": { "type": "string", "description": "Comma-separated expansions override (optional)." },
                            "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                        },
                        "required": ["tweet_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_recent_tweets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search recent tweets via X API v2. Supports pagination using next_token and \
time filtering. Time inputs accept RFC3339 or YYYY-MM-DD (UTC). You can also use `since` like \
`12h`/`7d` (UTC, relative to now).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query." },
                            "max_results": { "type": "integer", "description": "Tweets per page (10..100)." },
                            "pages": { "type": "integer", "description": "Number of pages to fetch (1..5). Each page uses next_token.", "minimum": 1, "maximum": 5 },
                            "limit": { "type": "integer", "description": "Maximum number of tweets to return after filtering/sorting (optional)." },
                            "next_token": { "type": "string", "description": "Pagination token from previous response." },
                            "since": { "type": "string", "description": "Relative lookback like 15m, 2h, 7d, 4w. Ignored if start_time is set." },
                            "start_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "end_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "sort_order": { "type": "string", "enum": ["recency", "relevancy"], "description": "Server-side ranking." },
                            "sort_by": { "type": "string", "enum": ["time", "likes", "retweets", "replies", "quotes", "views", "engagement"], "description": "Client-side sort applied to fetched pages." },
                            "order": { "type": "string", "enum": ["asc", "desc"], "description": "Client-side sort direction." },
                            "exclude_replies": { "type": "boolean", "description": "Append -is:reply to query (and filter defensively)." },
                            "exclude_retweets": { "type": "boolean", "description": "Append -is:retweet to query (and filter defensively)." },
                            "min_likes": { "type": "integer", "description": "Filter: require >= this many likes." },
                            "min_retweets": { "type": "integer", "description": "Filter: require >= this many retweets." },
                            "min_replies": { "type": "integer", "description": "Filter: require >= this many replies." },
                            "min_quotes": { "type": "integer", "description": "Filter: require >= this many quotes." },
                            "min_views": { "type": "integer", "description": "Filter: require >= this many views (best-effort; may be 0 if not returned)." },
                            "from_username": { "type": "string", "description": "Convenience: append from:username to query if no from: is present." },
                            "quick": { "type": "boolean", "description": "Convenience: fetch 1 page, return <= 10 tweets, and default to excluding replies/retweets unless query already specifies otherwise." },
                            "quality": { "type": "boolean", "description": "Convenience: if min_likes is not set, default it to 10." },
                            "include_raw": { "type": "boolean", "description": "If true, include raw API pages in the response." },
                            "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                        },
                        "required": ["query"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_thread"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a recent conversation/thread snapshot for a tweet_id. This uses \
conversation_id:<id> on the recent-search endpoint, so it is limited to recent results.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "tweet_id": { "type": "string", "description": "Tweet id." },
                            "max_results": { "type": "integer", "description": "Tweets per page (10..100)." },
                            "pages": { "type": "integer", "minimum": 1, "maximum": 5, "description": "Number of pages to fetch (1..5)." },
                            "limit": { "type": "integer", "description": "Maximum number of tweets to return after filtering/sorting (optional)." },
                            "next_token": { "type": "string", "description": "Pagination token from previous response." },
                            "since": { "type": "string", "description": "Relative lookback like 12h/7d. Ignored if start_time is set." },
                            "start_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "end_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "exclude_replies": { "type": "boolean", "description": "Exclude replies (best-effort; also filters defensively)." },
                            "exclude_retweets": { "type": "boolean", "description": "Exclude retweets (best-effort; also filters defensively)." },
                            "order": { "type": "string", "enum": ["asc", "desc"], "description": "Sort direction by created_at." },
                            "include_root": { "type": "boolean", "description": "Include the root tweet object in the output." },
                            "include_raw": { "type": "boolean", "description": "If true, include raw API pages in the response." },
                            "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                        },
                        "required": ["tweet_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_user_tweets"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a user's tweets via X API v2. Supports pagination using pagination_token \
and optional time filtering. Time inputs accept RFC3339 or YYYY-MM-DD (UTC).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "user_id": { "type": "string", "description": "Numeric user id." },
                            "max_results": { "type": "integer", "description": "5..100." },
                            "pagination_token": { "type": "string", "description": "Pagination token from previous response." },
                            "start_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "end_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "exclude_replies": { "type": "boolean", "description": "Exclude replies." },
                            "exclude_retweets": { "type": "boolean", "description": "Exclude retweets." },
                            "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                        },
                        "required": ["user_id"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_user_tweets_by_username"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Convenience wrapper: resolve username -> user_id, then fetch tweets. Supports \
the same time filtering as get_user_tweets.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "username": { "type": "string", "description": "Username without @." },
                            "max_results": { "type": "integer", "description": "5..100." },
                            "pagination_token": { "type": "string", "description": "Pagination token from previous response." },
                            "start_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "end_time": { "type": "string", "description": "RFC3339 or YYYY-MM-DD (UTC)."},
                            "exclude_replies": { "type": "boolean", "description": "Exclude replies." },
                            "exclude_retweets": { "type": "boolean", "description": "Exclude retweets." },
                            "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                        },
                        "required": ["username"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_post"),
                title: None,
                description: Some(Cow::Borrowed("Create a post using user-context auth.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "reply_to_tweet_id": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["text"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("delete_post"),
                title: None,
                description: Some(Cow::Borrowed("Delete a post using user-context auth.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_mentions"),
                title: None,
                description: Some(Cow::Borrowed("Get mentions for the authenticated user or a provided user id.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "max_results": { "type": "integer" },
                        "pagination_token": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_home_timeline"),
                title: None,
                description: Some(Cow::Borrowed("Get the authenticated user's home timeline.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "max_results": { "type": "integer" },
                        "pagination_token": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_all_tweets"),
                title: None,
                description: Some(Cow::Borrowed("Search full-archive tweets via the official API when your X access tier allows it.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "max_results": { "type": "integer" },
                        "next_token": { "type": "string" },
                        "start_time": { "type": "string" },
                        "end_time": { "type": "string" },
                        "sort_order": { "type": "string", "enum": ["recency", "relevancy"] },
                        "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                    },
                    "required": ["query"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_usage"),
                title: None,
                description: Some(Cow::Borrowed("Get app usage/consumption data from the official X API.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "days": { "type": "integer" },
                        "usage_fields": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("like_post"),
                title: None,
                description: Some(Cow::Borrowed("Like a post as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("unlike_post"),
                title: None,
                description: Some(Cow::Borrowed("Unlike a post as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("repost_post"),
                title: None,
                description: Some(Cow::Borrowed("Repost a post as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("unrepost_post"),
                title: None,
                description: Some(Cow::Borrowed("Undo a repost as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("follow_user"),
                title: None,
                description: Some(Cow::Borrowed("Follow a target user as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "target_user_id": { "type": "string" },
                        "source_user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["target_user_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("unfollow_user"),
                title: None,
                description: Some(Cow::Borrowed("Unfollow a target user as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "target_user_id": { "type": "string" },
                        "source_user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["target_user_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_bookmarks"),
                title: None,
                description: Some(Cow::Borrowed("List bookmarks for the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "max_results": { "type": "integer" },
                        "pagination_token": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("add_bookmark"),
                title: None,
                description: Some(Cow::Borrowed("Bookmark a post as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("remove_bookmark"),
                title: None,
                description: Some(Cow::Borrowed("Remove a bookmark as the authenticated user.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "tweet_id": { "type": "string" },
                        "user_id": { "type": "string", "description": "Optional authenticated user id override." },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["tweet_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("refresh_oauth2"),
                title: None,
                description: Some(Cow::Borrowed("Refresh the configured X OAuth2 access token using oauth2_refresh_token and client_id.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {}
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_list"),
                title: None,
                description: Some(Cow::Borrowed("Create an X list.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "private": { "type": "boolean" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["name"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("update_list"),
                title: None,
                description: Some(Cow::Borrowed("Update an X list.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "list_id": { "type": "string" },
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "private": { "type": "boolean" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["list_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("delete_list"),
                title: None,
                description: Some(Cow::Borrowed("Delete an X list.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "list_id": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["list_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_dm_conversation"),
                title: None,
                description: Some(Cow::Borrowed("Create a group DM conversation with an initial text message.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "participant_ids": { "type": "array", "items": { "type": "string" } },
                        "text": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["participant_ids", "text"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_dm_events"),
                title: None,
                description: Some(Cow::Borrowed("Fetch DM events for a DM conversation.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "conversation_id": { "type": "string" },
                        "max_results": { "type": "integer" },
                        "pagination_token": { "type": "string" },
                        "event_types": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["conversation_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("initialize_media_upload"),
                title: None,
                description: Some(Cow::Borrowed("Initialize a media upload session.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "media_type": { "type": "string" },
                        "total_bytes": { "type": "integer" },
                        "media_category": { "type": "string" },
                        "shared": { "type": "boolean" },
                        "additional_owners": { "type": "array", "items": { "type": "string" } },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["media_type", "total_bytes"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("append_media_upload"),
                title: None,
                description: Some(Cow::Borrowed("Append a media chunk to an upload session using base64-encoded data.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "upload_id": { "type": "string" },
                        "segment_index": { "type": "integer" },
                        "media_base64": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["upload_id", "segment_index", "media_base64"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("finalize_media_upload"),
                title: None,
                description: Some(Cow::Borrowed("Finalize a media upload session.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "upload_id": { "type": "string" },
                        "auth_mode": { "type": "string", "enum": ["auto", "oauth2", "oauth1"] }
                    },
                    "required": ["upload_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("raw_operation"),
                title: None,
                description: Some(Cow::Borrowed("Call a registered official X operation by operation_id with path_params/query/body.")),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "operation_id": { "type": "string" },
                        "path_params": { "type": "object", "additionalProperties": true },
                        "query": { "type": "object", "additionalProperties": true },
                        "body": {},
                        "auth_mode": { "type": "string", "enum": ["auto", "bearer", "oauth2", "oauth1"] }
                    },
                    "required": ["operation_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let name: &str = &request.name;
        let args = request.arguments.unwrap_or_default();

        match name {
            "get_auth_status" => {
                let payload = self.build_auth_status().await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "whoami" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let payload = self
                    .get_json_as("users/me", &[], AuthRequirement::UserContext, auth_mode)
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_user_by_username" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let username = args
                    .get("username")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'username'".into()))?
                    .trim()
                    .trim_start_matches('@');

                let user_fields = args
                    .get("user_fields")
                    .and_then(Value::as_str)
                    .unwrap_or("created_at,description,public_metrics,verified,profile_image_url");

                let v = self
                    .get_json_as(
                        &format!("users/by/username/{username}"),
                        &[("user.fields", user_fields.to_string())],
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&v)?;
                structured_result_with_text(&v, Some(text))
            }
            "get_profile" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                // Alias kept intentionally small: same behavior as get_user_by_username.
                let username = args
                    .get("username")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'username'".into()))?
                    .trim()
                    .trim_start_matches('@');

                let user_fields = args
                    .get("user_fields")
                    .and_then(Value::as_str)
                    .unwrap_or("created_at,description,public_metrics,verified,profile_image_url");

                let v = self
                    .get_json_as(
                        &format!("users/by/username/{username}"),
                        &[("user.fields", user_fields.to_string())],
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&v)?;
                structured_result_with_text(&v, Some(text))
            }
            "get_tweet" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?
                    .trim();

                let tweet_fields = args
                    .get("tweet_fields")
                    .and_then(Value::as_str)
                    .unwrap_or("created_at,public_metrics,conversation_id,lang,author_id");
                let expansions = args
                    .get("expansions")
                    .and_then(Value::as_str)
                    .unwrap_or("author_id");

                let v = self
                    .get_json_as(
                        &format!("tweets/{tweet_id}"),
                        &[
                            ("tweet.fields", tweet_fields.to_string()),
                            ("expansions", expansions.to_string()),
                        ],
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&v)?;
                structured_result_with_text(&v, Some(text))
            }
            "search_recent_tweets" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;

                let mut effective_query = query.trim().to_string();

                let quick = args.get("quick").and_then(Value::as_bool) == Some(true);
                let quality = args.get("quality").and_then(Value::as_bool) == Some(true);

                let exclude_retweets = args
                    .get("exclude_retweets")
                    .and_then(Value::as_bool)
                    .unwrap_or(quick);
                let exclude_replies = args
                    .get("exclude_replies")
                    .and_then(Value::as_bool)
                    .unwrap_or(quick);

                if exclude_retweets && !effective_query.contains("is:retweet") {
                    effective_query.push_str(" -is:retweet");
                }
                if exclude_replies && !effective_query.contains("is:reply") {
                    effective_query.push_str(" -is:reply");
                }
                if let Some(from_username) = args.get("from_username").and_then(Value::as_str) {
                    let u = from_username.trim().trim_start_matches('@');
                    if !u.is_empty() && !effective_query.contains("from:") {
                        effective_query.push_str(&format!(" from:{u}"));
                    }
                }

                let pages = args
                    .get("pages")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 5) as usize;

                let max_results_default = if quick { 10 } else { 100 };
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(max_results_default)
                    .clamp(10, 100)
                    .to_string();

                let limit = match args.get("limit").and_then(Value::as_u64) {
                    Some(v) => Some(v as usize),
                    None if quick => Some(10),
                    None => None,
                };
                let include_raw = args.get("include_raw").and_then(Value::as_bool) == Some(true);

                let sort_by = args
                    .get("sort_by")
                    .and_then(Value::as_str)
                    .unwrap_or("time");
                let order = args.get("order").and_then(Value::as_str).unwrap_or("desc");

                let min_likes = match args.get("min_likes").and_then(Value::as_i64) {
                    Some(v) => Some(v),
                    None if quality => Some(10),
                    None => None,
                };
                let min_retweets = args.get("min_retweets").and_then(Value::as_i64);
                let min_replies = args.get("min_replies").and_then(Value::as_i64);
                let min_quotes = args.get("min_quotes").and_then(Value::as_i64);
                let min_views = args.get("min_views").and_then(Value::as_i64);

                let mut start_time = Self::get_time_arg(&args, "start_time", false)?;
                if start_time.is_none() {
                    if let Some(since) = args.get("since").and_then(Value::as_str) {
                        let dur = Self::parse_since(since)?;
                        start_time = Some((Utc::now() - dur).to_rfc3339());
                    }
                }
                let end_time = Self::get_time_arg(&args, "end_time", true)?;
                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    let start_dt = DateTime::parse_from_rfc3339(start)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid start_time".into()))?
                        .with_timezone(&Utc);
                    let end_dt = DateTime::parse_from_rfc3339(end)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid end_time".into()))?
                        .with_timezone(&Utc);
                    if start_dt > end_dt {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }

                let mut next_token = args
                    .get("next_token")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string());
                let mut all_tweets: Vec<Value> = Vec::new();
                let mut users: HashMap<String, Value> = HashMap::new();
                let mut raw_pages: Vec<Value> = Vec::new();

                for _ in 0..pages {
                    let mut qp: Vec<(&str, String)> = vec![
                        ("query", effective_query.clone()),
                        ("max_results", max_results.clone()),
                        (
                            "tweet.fields",
                            "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                        ),
                        ("expansions", "author_id".to_string()),
                        (
                            "user.fields",
                            "created_at,username,name,verified,profile_image_url".to_string(),
                        ),
                    ];

                    if let Some(token) = next_token.as_deref() {
                        if !token.is_empty() {
                            qp.push(("next_token", token.to_string()));
                        }
                    }
                    if let Some(start) = start_time.as_ref() {
                        qp.push(("start_time", start.clone()));
                    }
                    if let Some(end) = end_time.as_ref() {
                        qp.push(("end_time", end.clone()));
                    }
                    if let Some(sort_order) = args.get("sort_order").and_then(Value::as_str) {
                        qp.push(("sort_order", sort_order.to_string()));
                    }

                    let v = self
                        .get_json_as(
                            "tweets/search/recent",
                            &qp,
                            AuthRequirement::PublicRead,
                            auth_mode,
                        )
                        .await?;
                    users.extend(Self::users_by_id(&v));

                    if include_raw {
                        raw_pages.push(v.clone());
                    }

                    if let Some(data) = v.get("data").and_then(Value::as_array) {
                        all_tweets.extend(data.iter().cloned());
                    }

                    let token = v.get("meta").and_then(|m| m.get("next_token"));
                    next_token = token.and_then(Value::as_str).map(|s| s.trim().to_string());
                    if next_token.is_none() {
                        break;
                    }
                }

                let mut seen: HashSet<String> = HashSet::new();
                let mut compact: Vec<Value> = Vec::new();
                for tweet in all_tweets {
                    let id = tweet
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if id.is_empty() || seen.contains(&id) {
                        continue;
                    }
                    seen.insert(id);

                    if let Some(min) = min_likes {
                        if Self::metric_i64(&tweet, "like_count") < min {
                            continue;
                        }
                    }
                    if let Some(min) = min_retweets {
                        if Self::metric_i64(&tweet, "retweet_count") < min {
                            continue;
                        }
                    }
                    if let Some(min) = min_replies {
                        if Self::metric_i64(&tweet, "reply_count") < min {
                            continue;
                        }
                    }
                    if let Some(min) = min_quotes {
                        if Self::metric_i64(&tweet, "quote_count") < min {
                            continue;
                        }
                    }
                    if let Some(min) = min_views {
                        if Self::views_i64(&tweet) < min {
                            continue;
                        }
                    }

                    compact.push(Self::compact_tweet(&tweet, &users));
                }

                match sort_by {
                    "time" => compact.sort_by_key(Self::created_at_ts),
                    "likes" => compact.sort_by_key(|t| Self::metric_i64(t, "like_count")),
                    "retweets" => compact.sort_by_key(|t| Self::metric_i64(t, "retweet_count")),
                    "replies" => compact.sort_by_key(|t| Self::metric_i64(t, "reply_count")),
                    "quotes" => compact.sort_by_key(|t| Self::metric_i64(t, "quote_count")),
                    "views" => compact.sort_by_key(Self::views_i64),
                    "engagement" => compact.sort_by_key(|t| {
                        Self::metric_i64(t, "like_count")
                            + Self::metric_i64(t, "retweet_count")
                            + Self::metric_i64(t, "reply_count")
                            + Self::metric_i64(t, "quote_count")
                    }),
                    other => {
                        return Err(ConnectorError::InvalidParams(format!(
                            "Invalid sort_by: {other}. Expected one of: time, likes, retweets, replies, quotes, views, engagement"
                        )));
                    }
                }

                if order == "desc" {
                    compact.reverse();
                } else if order != "asc" {
                    return Err(ConnectorError::InvalidParams(format!(
                        "Invalid order: {order}. Expected asc or desc"
                    )));
                }

                if let Some(limit) = limit {
                    if compact.len() > limit {
                        compact.truncate(limit);
                    }
                }

                let payload = json!({
                    "query": effective_query,
                    "start_time": start_time,
                    "end_time": end_time,
                    "tweets": compact,
                    "next_token": next_token,
                    "raw_pages": if include_raw { Value::Array(raw_pages) } else { Value::Null },
                });
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_thread" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?
                    .trim();

                let include_root = args.get("include_root").and_then(Value::as_bool) != Some(false);
                let root = self
                    .get_json_as(
                        &format!("tweets/{tweet_id}"),
                        &[(
                            "tweet.fields",
                            "created_at,public_metrics,conversation_id,lang,author_id,in_reply_to_user_id"
                                .to_string(),
                        )],
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;

                let conversation_id = root
                    .get("data")
                    .and_then(|d| d.get("conversation_id"))
                    .and_then(Value::as_str)
                    .unwrap_or(tweet_id)
                    .to_string();

                let mut thread_query = format!("conversation_id:{conversation_id}");
                if args.get("exclude_retweets").and_then(Value::as_bool) == Some(true)
                    && !thread_query.contains("is:retweet")
                {
                    thread_query.push_str(" -is:retweet");
                }

                // Replies are the essence of a conversation; exclude_replies is honored by defensive filtering only.
                let exclude_replies =
                    args.get("exclude_replies").and_then(Value::as_bool) == Some(true);

                let pages = args
                    .get("pages")
                    .and_then(Value::as_u64)
                    .unwrap_or(3)
                    .clamp(1, 5) as usize;

                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(100)
                    .clamp(10, 100)
                    .to_string();

                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize);
                let include_raw = args.get("include_raw").and_then(Value::as_bool) == Some(true);

                let mut start_time = Self::get_time_arg(&args, "start_time", false)?;
                if start_time.is_none() {
                    if let Some(since) = args.get("since").and_then(Value::as_str) {
                        let dur = Self::parse_since(since)?;
                        start_time = Some((Utc::now() - dur).to_rfc3339());
                    }
                }
                let end_time = Self::get_time_arg(&args, "end_time", true)?;
                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    let start_dt = DateTime::parse_from_rfc3339(start)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid start_time".into()))?
                        .with_timezone(&Utc);
                    let end_dt = DateTime::parse_from_rfc3339(end)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid end_time".into()))?
                        .with_timezone(&Utc);
                    if start_dt > end_dt {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }

                let order = args.get("order").and_then(Value::as_str).unwrap_or("asc");

                let mut next_token = args
                    .get("next_token")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string());
                let mut all_tweets: Vec<Value> = Vec::new();
                let mut users: HashMap<String, Value> = HashMap::new();
                let mut raw_pages: Vec<Value> = Vec::new();

                for _ in 0..pages {
                    let mut qp: Vec<(&str, String)> = vec![
                        ("query", thread_query.clone()),
                        ("max_results", max_results.clone()),
                        (
                            "tweet.fields",
                            "created_at,public_metrics,conversation_id,lang,author_id,in_reply_to_user_id"
                                .to_string(),
                        ),
                        ("expansions", "author_id".to_string()),
                        (
                            "user.fields",
                            "created_at,username,name,verified,profile_image_url".to_string(),
                        ),
                    ];

                    if let Some(token) = next_token.as_deref() {
                        if !token.is_empty() {
                            qp.push(("next_token", token.to_string()));
                        }
                    }
                    if let Some(start) = start_time.as_ref() {
                        qp.push(("start_time", start.clone()));
                    }
                    if let Some(end) = end_time.as_ref() {
                        qp.push(("end_time", end.clone()));
                    }

                    let v = self
                        .get_json_as(
                            "tweets/search/recent",
                            &qp,
                            AuthRequirement::PublicRead,
                            auth_mode,
                        )
                        .await?;
                    users.extend(Self::users_by_id(&v));
                    if include_raw {
                        raw_pages.push(v.clone());
                    }
                    if let Some(data) = v.get("data").and_then(Value::as_array) {
                        all_tweets.extend(data.iter().cloned());
                    }
                    let token = v.get("meta").and_then(|m| m.get("next_token"));
                    next_token = token.and_then(Value::as_str).map(|s| s.trim().to_string());
                    if next_token.is_none() {
                        break;
                    }
                }

                let mut seen: HashSet<String> = HashSet::new();
                let mut compact: Vec<Value> = Vec::new();
                for tweet in all_tweets {
                    let id = tweet
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if id.is_empty() || seen.contains(&id) {
                        continue;
                    }
                    seen.insert(id);

                    if exclude_replies {
                        // Best-effort: if the API returns referenced_tweets, check type=retweeted/...
                        // If absent, keep the tweet.
                        if tweet.get("in_reply_to_user_id").is_some() {
                            continue;
                        }
                    }

                    compact.push(Self::compact_tweet(&tweet, &users));
                }

                compact.sort_by_key(Self::created_at_ts);
                match order {
                    "asc" => {}
                    "desc" => compact.reverse(),
                    other => {
                        return Err(ConnectorError::InvalidParams(format!(
                            "Invalid order: {other}. Expected asc or desc"
                        )));
                    }
                }

                if let Some(limit) = limit {
                    if compact.len() > limit {
                        compact.truncate(limit);
                    }
                }

                let payload = json!({
                    "tweet_id": tweet_id,
                    "conversation_id": conversation_id,
                    "root": if include_root { root.get("data").cloned().unwrap_or(Value::Null) } else { Value::Null },
                    "start_time": start_time,
                    "end_time": end_time,
                    "tweets": compact,
                    "next_token": next_token,
                    "raw_pages": if include_raw { Value::Array(raw_pages) } else { Value::Null },
                });
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_user_tweets" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let user_id = args
                    .get("user_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'user_id'".into()))?;

                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(10);
                let max_results = max_results.clamp(5, 100).to_string();

                let mut qp: Vec<(&str, String)> = vec![
                    ("max_results", max_results),
                    (
                        "tweet.fields",
                        "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                    ),
                ];

                if let Some(token) = args.get("pagination_token").and_then(Value::as_str) {
                    if !token.trim().is_empty() {
                        qp.push(("pagination_token", token.trim().to_string()));
                    }
                }

                let start_time = Self::get_time_arg(&args, "start_time", false)?;
                let end_time = Self::get_time_arg(&args, "end_time", true)?;
                if let (Some(start), Some(end)) = (start_time.as_ref(), end_time.as_ref()) {
                    let start_dt = DateTime::parse_from_rfc3339(start)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid start_time".into()))?
                        .with_timezone(&Utc);
                    let end_dt = DateTime::parse_from_rfc3339(end)
                        .map_err(|_| ConnectorError::InvalidParams("Invalid end_time".into()))?
                        .with_timezone(&Utc);
                    if start_dt > end_dt {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid time range: start_time is after end_time".to_string(),
                        ));
                    }
                }
                if let Some(start) = start_time {
                    qp.push(("start_time", start));
                }
                if let Some(end) = end_time {
                    qp.push(("end_time", end));
                }

                let mut exclude: Vec<&str> = Vec::new();
                if args.get("exclude_replies").and_then(Value::as_bool) == Some(true) {
                    exclude.push("replies");
                }
                if args.get("exclude_retweets").and_then(Value::as_bool) == Some(true) {
                    exclude.push("retweets");
                }
                if !exclude.is_empty() {
                    qp.push(("exclude", exclude.join(",")));
                }

                let v = self
                    .get_json_as(
                        &format!("users/{user_id}/tweets"),
                        &qp,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let next_token = v
                    .get("meta")
                    .and_then(|m| m.get("next_token"))
                    .cloned()
                    .unwrap_or(Value::Null);

                let payload = json!({
                    "user_id": user_id,
                    "next_token": next_token,
                    "raw": v,
                });
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_user_tweets_by_username" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let username = args
                    .get("username")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'username'".into()))?
                    .trim()
                    .trim_start_matches('@');

                let user = self
                    .get_json_as(
                        &format!("users/by/username/{username}"),
                        &[("user.fields", "id".to_string())],
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;
                let user_id = user
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::Other("X API user lookup returned no id".into())
                    })?;

                // Reuse get_user_tweets codepath by mapping arguments.
                let mut remapped = args.clone();
                remapped.insert("user_id".to_string(), Value::String(user_id.to_string()));
                let req = CallToolRequestParam {
                    name: "get_user_tweets".into(),
                    arguments: Some(remapped),
                };
                self.call_tool(req).await
            }
            "create_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'text'".into()))?;
                let mut body = json!({ "text": text });
                if let Some(reply_to) = args.get("reply_to_tweet_id").and_then(Value::as_str) {
                    body["reply"] = json!({ "in_reply_to_tweet_id": reply_to });
                }
                let payload = self
                    .post_json_as("tweets", &[], body, AuthRequirement::UserContext, auth_mode)
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "delete_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let payload = self
                    .delete_json_as(
                        &format!("tweets/{tweet_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_mentions" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .clamp(5, 100)
                    .to_string();
                let mut qp = vec![
                    ("max_results", max_results),
                    (
                        "tweet.fields",
                        "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                    ),
                    ("expansions", "author_id".to_string()),
                    (
                        "user.fields",
                        "created_at,username,name,verified,profile_image_url".to_string(),
                    ),
                ];
                if let Some(token) = args.get("pagination_token").and_then(Value::as_str) {
                    if !token.trim().is_empty() {
                        qp.push(("pagination_token", token.trim().to_string()));
                    }
                }
                let payload = self
                    .get_json_as(
                        &format!("users/{user_id}/mentions"),
                        &qp,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_home_timeline" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .clamp(5, 100)
                    .to_string();
                let mut qp = vec![
                    ("max_results", max_results),
                    (
                        "tweet.fields",
                        "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                    ),
                    ("expansions", "author_id".to_string()),
                    (
                        "user.fields",
                        "created_at,username,name,verified,profile_image_url".to_string(),
                    ),
                ];
                if let Some(token) = args.get("pagination_token").and_then(Value::as_str) {
                    if !token.trim().is_empty() {
                        qp.push(("pagination_token", token.trim().to_string()));
                    }
                }
                let payload = self
                    .get_json_as(
                        "users/me/timelines/reverse_chronological",
                        &qp,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "search_all_tweets" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;
                let mut qp = vec![
                    ("query", query.to_string()),
                    (
                        "tweet.fields",
                        "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                    ),
                    ("expansions", "author_id".to_string()),
                    (
                        "user.fields",
                        "created_at,username,name,verified,profile_image_url".to_string(),
                    ),
                ];
                if let Some(v) = args.get("max_results").and_then(Value::as_u64) {
                    qp.push(("max_results", v.clamp(10, 100).to_string()));
                }
                if let Some(v) = args.get("next_token").and_then(Value::as_str) {
                    qp.push(("next_token", v.to_string()));
                }
                if let Some(v) = Self::get_time_arg(&args, "start_time", false)? {
                    qp.push(("start_time", v));
                }
                if let Some(v) = Self::get_time_arg(&args, "end_time", true)? {
                    qp.push(("end_time", v));
                }
                if let Some(v) = args.get("sort_order").and_then(Value::as_str) {
                    qp.push(("sort_order", v.to_string()));
                }
                let payload = self
                    .get_json_as(
                        "tweets/search/all",
                        &qp,
                        AuthRequirement::PublicRead,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_usage" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let mut qp = Vec::new();
                if let Some(days) = args.get("days").and_then(Value::as_u64) {
                    qp.push(("days", days.to_string()));
                }
                if let Some(fields) = args.get("usage_fields").and_then(Value::as_str) {
                    qp.push(("usage.fields", fields.to_string()));
                }
                let payload = self
                    .get_json_as("usage/tweets", &qp, AuthRequirement::UserContext, auth_mode)
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "refresh_oauth2" => {
                let payload = self.refresh_oauth2_access_token().await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "create_list" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'name'".into()))?;
                let mut body = json!({ "name": name });
                if let Some(v) = args.get("description").and_then(Value::as_str) {
                    body["description"] = json!(v);
                }
                if let Some(v) = args.get("private").and_then(Value::as_bool) {
                    body["private"] = json!(v);
                }
                let payload = self
                    .post_json_as("lists", &[], body, AuthRequirement::UserContext, auth_mode)
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "update_list" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let list_id = args
                    .get("list_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'list_id'".into()))?;
                let mut body = json!({});
                if let Some(v) = args.get("name").and_then(Value::as_str) {
                    body["name"] = json!(v);
                }
                if let Some(v) = args.get("description").and_then(Value::as_str) {
                    body["description"] = json!(v);
                }
                if let Some(v) = args.get("private").and_then(Value::as_bool) {
                    body["private"] = json!(v);
                }
                let payload = self
                    .send_json(
                        Method::PUT,
                        &format!("lists/{list_id}"),
                        &[],
                        Some(body),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "delete_list" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let list_id = args
                    .get("list_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'list_id'".into()))?;
                let payload = self
                    .delete_json_as(
                        &format!("lists/{list_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "create_dm_conversation" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let participant_ids = args
                    .get("participant_ids")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'participant_ids'".into())
                    })?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'text'".into()))?;
                let body = json!({
                    "conversation_type": "Group",
                    "participant_ids": participant_ids,
                    "message": { "text": text }
                });
                let payload = self
                    .post_json_as(
                        "dm_conversations",
                        &[],
                        body,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_dm_events" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let conversation_id = args
                    .get("conversation_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'conversation_id'".into())
                    })?;
                let mut qp = Vec::new();
                if let Some(v) = args.get("max_results").and_then(Value::as_u64) {
                    qp.push(("max_results", v.to_string()));
                }
                if let Some(v) = args.get("pagination_token").and_then(Value::as_str) {
                    qp.push(("pagination_token", v.to_string()));
                }
                if let Some(v) = args.get("event_types").and_then(Value::as_str) {
                    qp.push(("event_types", v.to_string()));
                }
                let payload = self
                    .get_json_as(
                        &format!("dm_conversations/{conversation_id}/dm_events"),
                        &qp,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "initialize_media_upload" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let media_type = args
                    .get("media_type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'media_type'".into()))?;
                let total_bytes = args
                    .get("total_bytes")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'total_bytes'".into()))?;
                let mut body = json!({
                    "media_type": media_type,
                    "total_bytes": total_bytes
                });
                if let Some(v) = args.get("media_category").and_then(Value::as_str) {
                    body["media_category"] = json!(v);
                }
                if let Some(v) = args.get("shared").and_then(Value::as_bool) {
                    body["shared"] = json!(v);
                }
                if let Some(v) = args.get("additional_owners").and_then(Value::as_array) {
                    body["additional_owners"] = json!(v);
                }
                let payload = self
                    .post_json_as(
                        "media/upload/initialize",
                        &[],
                        body,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "append_media_upload" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let upload_id = args
                    .get("upload_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'upload_id'".into()))?;
                let segment_index = args
                    .get("segment_index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'segment_index'".into())
                    })?;
                let media_base64 = args
                    .get("media_base64")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'media_base64'".into())
                    })?;
                let payload = self
                    .post_json_as(
                        &format!("media/upload/{upload_id}/append"),
                        &[],
                        json!({ "segment_index": segment_index, "media": media_base64 }),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "finalize_media_upload" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let upload_id = args
                    .get("upload_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'upload_id'".into()))?;
                let payload = self
                    .post_json_as(
                        &format!("media/upload/{upload_id}/finalize"),
                        &[],
                        json!({}),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "raw_operation" => {
                let operation_id = args
                    .get("operation_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'operation_id'".into())
                    })?;
                let spec = Self::operation_spec(operation_id).ok_or_else(|| {
                    ConnectorError::InvalidParams(format!(
                        "Unknown or unsupported X operation_id: {operation_id}"
                    ))
                })?;
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let path_params = args
                    .get("path_params")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let path = Self::render_path_template(spec.path_template, &path_params)?;
                let query_map = args
                    .get("query")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let mut query = Vec::new();
                for (key, value) in query_map {
                    let value = Self::value_to_query_string(&value)?;
                    if !value.is_empty() {
                        query.push((Box::leak(key.into_boxed_str()) as &str, value));
                    }
                }
                let body = args.get("body").cloned();
                let method = match spec.method {
                    "GET" => Method::GET,
                    "POST" => Method::POST,
                    "PUT" => Method::PUT,
                    "DELETE" => Method::DELETE,
                    other => {
                        return Err(ConnectorError::Other(format!(
                            "Unsupported HTTP method in registry: {other}"
                        )))
                    }
                };
                let payload = self
                    .send_json(
                        method,
                        &path,
                        &query,
                        body,
                        spec.auth_requirement,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "like_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .post_json_as(
                        &format!("users/{user_id}/likes"),
                        &[],
                        json!({ "tweet_id": tweet_id }),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "unlike_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .delete_json_as(
                        &format!("users/{user_id}/likes/{tweet_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "repost_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .post_json_as(
                        &format!("users/{user_id}/retweets"),
                        &[],
                        json!({ "tweet_id": tweet_id }),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "unrepost_post" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .delete_json_as(
                        &format!("users/{user_id}/retweets/{tweet_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "follow_user" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let target_user_id = args
                    .get("target_user_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'target_user_id'".into())
                    })?;
                let source_user_id =
                    if let Some(id) = args.get("source_user_id").and_then(Value::as_str) {
                        id.to_string()
                    } else {
                        self.authenticated_user_id(auth_mode).await?
                    };
                let payload = self
                    .post_json_as(
                        &format!("users/{source_user_id}/following"),
                        &[],
                        json!({ "target_user_id": target_user_id }),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "unfollow_user" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let target_user_id = args
                    .get("target_user_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'target_user_id'".into())
                    })?;
                let source_user_id =
                    if let Some(id) = args.get("source_user_id").and_then(Value::as_str) {
                        id.to_string()
                    } else {
                        self.authenticated_user_id(auth_mode).await?
                    };
                let payload = self
                    .delete_json_as(
                        &format!("users/{source_user_id}/following/{target_user_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "get_bookmarks" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .clamp(5, 100)
                    .to_string();
                let mut qp = vec![
                    ("max_results", max_results),
                    (
                        "tweet.fields",
                        "created_at,public_metrics,conversation_id,lang,author_id".to_string(),
                    ),
                    ("expansions", "author_id".to_string()),
                    (
                        "user.fields",
                        "created_at,username,name,verified,profile_image_url".to_string(),
                    ),
                ];
                if let Some(token) = args.get("pagination_token").and_then(Value::as_str) {
                    if !token.trim().is_empty() {
                        qp.push(("pagination_token", token.trim().to_string()));
                    }
                }
                let payload = self
                    .get_json_as(
                        &format!("users/{user_id}/bookmarks"),
                        &qp,
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "add_bookmark" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .post_json_as(
                        &format!("users/{user_id}/bookmarks"),
                        &[],
                        json!({ "tweet_id": tweet_id }),
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            "remove_bookmark" => {
                let auth_mode = Self::auth_mode_from_args(&args)?;
                let tweet_id = args
                    .get("tweet_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'tweet_id'".into()))?;
                let user_id = if let Some(id) = args.get("user_id").and_then(Value::as_str) {
                    id.to_string()
                } else {
                    self.authenticated_user_id(auth_mode).await?
                };
                let payload = self
                    .delete_json_as(
                        &format!("users/{user_id}/bookmarks/{tweet_id}"),
                        &[],
                        AuthRequirement::UserContext,
                        auth_mode,
                    )
                    .await?;
                let text = serde_json::to_string(&payload)?;
                structured_result_with_text(&payload, Some(text))
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        Ok(ListPromptsResult {
            prompts: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt with name {} not found",
            name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn connector_with_auth(auth: &[(&str, &str)]) -> XApiConnector {
        let details: AuthDetails = auth
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        XApiConnector::new(details).await.expect("connector")
    }

    #[tokio::test]
    async fn resolve_auth_mode_prefers_bearer_for_public_reads() {
        let connector = connector_with_auth(&[("bearer_token", "bearer-123")]).await;
        let mode = connector
            .resolve_auth_mode(XAuthMode::Auto, AuthRequirement::PublicRead)
            .expect("mode");
        assert_eq!(mode, XAuthMode::Bearer);
    }

    #[tokio::test]
    async fn resolve_auth_mode_prefers_oauth2_for_user_context() {
        let connector = connector_with_auth(&[
            ("bearer_token", "bearer-123"),
            ("oauth2_access_token", "oauth2-token"),
            ("oauth2_expires_at", "4102444800"),
        ])
        .await;
        let mode = connector
            .resolve_auth_mode(XAuthMode::Auto, AuthRequirement::UserContext)
            .expect("mode");
        assert_eq!(mode, XAuthMode::OAuth2);
    }

    #[tokio::test]
    async fn resolve_auth_mode_rejects_bearer_for_user_context() {
        let connector = connector_with_auth(&[("bearer_token", "bearer-123")]).await;
        let err = connector
            .resolve_auth_mode(XAuthMode::Bearer, AuthRequirement::UserContext)
            .expect_err("bearer should be rejected");
        match err {
            ConnectorError::Authentication(message) => {
                assert!(message.contains("Bearer auth cannot satisfy"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn oauth1_authorization_contains_signature_fields() {
        let connector = connector_with_auth(&[
            ("oauth1_consumer_key", "consumer"),
            ("oauth1_consumer_secret", "consumer-secret"),
            ("oauth1_access_token", "access"),
            ("oauth1_access_token_secret", "access-secret"),
        ])
        .await;
        let creds = connector.oauth1_credentials().expect("oauth1 creds");
        let header = connector
            .build_oauth1_authorization(
                &Method::GET,
                "https://api.twitter.com/2/users/me",
                &[("tweet.fields", "created_at".to_string())],
                &creds,
            )
            .expect("oauth header");

        assert!(header.starts_with("OAuth "));
        assert!(header.contains("oauth_consumer_key=\"consumer\""));
        assert!(header.contains("oauth_token=\"access\""));
        assert!(header.contains("oauth_signature="));
    }
}
