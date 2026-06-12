use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{build_reqwest_client, structured_result_with_text};
use crate::Connector;

const LINKEDIN_API_BASE: &str = "https://api.linkedin.com";
const LINKEDIN_OAUTH_BASE: &str = "https://www.linkedin.com";
const LINKEDIN_DISCOVERY_URL: &str =
    "https://www.linkedin.com/oauth/.well-known/openid-configuration";
const LINKEDIN_JWKS_URL: &str = "https://www.linkedin.com/oauth/openid/jwks";
const LINKEDIN_PROTOCOL_VERSION: &str = "2.0.0";
const LINKEDIN_DEFAULT_API_VERSION: &str = "202603";

#[derive(Debug, Clone, Copy)]
enum AuthMode {
    MemberOauth,
    AppOauth,
}

impl AuthMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::MemberOauth => "member_oauth",
            Self::AppOauth => "app_oauth",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct LinkedInIdTokenClaims {
    iss: Option<String>,
    sub: String,
    aud: Option<AudienceClaim>,
    iat: Option<u64>,
    exp: Option<u64>,
    name: Option<String>,
    given_name: Option<String>,
    family_name: Option<String>,
    picture: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    locale: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum AudienceClaim {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    given_name: Option<String>,
    #[serde(default)]
    family_name: Option<String>,
    #[serde(default)]
    picture: Option<String>,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    refresh_token_expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ApiRequestOptions {
    query: Option<Map<String, Value>>,
    body: Option<Value>,
    headers: Option<Map<String, Value>>,
    include_linkedin_rest_headers: bool,
    linkedin_version: Option<String>,
}

#[derive(Debug, Clone)]
struct HttpResponsePayload {
    status: u16,
    ok: bool,
    headers: Map<String, Value>,
    body: Value,
}

pub struct LinkedInConnector {
    client: Client,
    auth: RwLock<AuthDetails>,
    api_base: String,
    oauth_base: String,
    discovery_url: String,
}

impl LinkedInConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Self::new_with_base_urls(
            auth,
            LINKEDIN_API_BASE,
            LINKEDIN_OAUTH_BASE,
            LINKEDIN_DISCOVERY_URL,
        )
        .await
    }

    async fn new_with_base_urls(
        auth: AuthDetails,
        api_base: &str,
        oauth_base: &str,
        discovery_url: &str,
    ) -> Result<Self, ConnectorError> {
        let client = build_reqwest_client(|| {
            let mut headers = HeaderMap::new();
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
            headers.insert(
                HeaderName::from_static("user-agent"),
                HeaderValue::from_static("rzn-tools/0.2 (linkedin)"),
            );
            Client::builder()
                .default_headers(headers)
                .timeout(std::time::Duration::from_secs(30))
        })?;

        Ok(Self {
            client,
            auth: RwLock::new(auth),
            api_base: api_base.trim_end_matches('/').to_string(),
            oauth_base: oauth_base.trim_end_matches('/').to_string(),
            discovery_url: discovery_url.to_string(),
        })
    }

    fn now_epoch() -> i64 {
        Utc::now().timestamp()
    }

    async fn auth_snapshot(&self) -> AuthDetails {
        self.auth.read().await.clone()
    }

    async fn set_cached_value(&self, key: &str, value: impl Into<String>) {
        let mut guard = self.auth.write().await;
        guard.insert(key.to_string(), value.into());
    }

    fn parse_auth_mode(auth: &AuthDetails) -> AuthMode {
        match auth.get("auth_mode").map(String::as_str) {
            Some("app_oauth") => AuthMode::AppOauth,
            _ => AuthMode::MemberOauth,
        }
    }

    fn scopes_from_auth(auth: &AuthDetails) -> Vec<String> {
        auth.get("scopes")
            .map(|raw| parse_scopes_value(&Value::String(raw.clone())))
            .unwrap_or_default()
    }

    fn has_scope(auth: &AuthDetails, expected: &str) -> bool {
        Self::scopes_from_auth(auth)
            .iter()
            .any(|scope| scope == expected)
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
            .map(|dt| dt.timestamp())
    }

    fn parse_required_string(
        args: &Map<String, Value>,
        key: &str,
    ) -> Result<String, ConnectorError> {
        args.get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| ConnectorError::InvalidParams(format!("Missing '{key}' argument")))
    }

    fn parse_optional_string(args: &Map<String, Value>, key: &str) -> Option<String> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn parse_optional_object(
        args: &Map<String, Value>,
        key: &str,
    ) -> Result<Option<Map<String, Value>>, ConnectorError> {
        match args.get(key) {
            None => Ok(None),
            Some(Value::Object(map)) => Ok(Some(map.clone())),
            Some(Value::String(text)) => serde_json::from_str::<Map<String, Value>>(text)
                .map(Some)
                .map_err(|err| {
                    ConnectorError::InvalidParams(format!("Invalid JSON object for '{key}': {err}"))
                }),
            Some(_) => Err(ConnectorError::InvalidParams(format!(
                "'{key}' must be a JSON object"
            ))),
        }
    }

    fn normalize_visibility(raw: Option<&str>) -> Result<String, ConnectorError> {
        let value = raw.unwrap_or("PUBLIC").to_ascii_uppercase();
        match value.as_str() {
            "PUBLIC" | "CONNECTIONS" | "LOGGED_IN" => Ok(value),
            _ => Err(ConnectorError::InvalidParams(format!(
                "Invalid 'visibility': {value}. Expected one of: PUBLIC, CONNECTIONS, LOGGED_IN"
            ))),
        }
    }

    fn reauth_required(message: impl Into<String>) -> ConnectorError {
        ConnectorError::Authentication(format!("reauth_required: {}", message.into()))
    }

    fn ensure_person_urn(raw: &str) -> String {
        if raw.starts_with("urn:li:person:") {
            raw.to_string()
        } else {
            format!("urn:li:person:{raw}")
        }
    }

    fn ensure_organization_urn(raw: &str) -> String {
        if raw.starts_with("urn:li:organization:") {
            raw.to_string()
        } else {
            format!("urn:li:organization:{raw}")
        }
    }

    fn linkedin_version(auth: &AuthDetails, override_value: Option<&str>) -> String {
        override_value
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| auth.get("linkedin_api_version").cloned())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| LINKEDIN_DEFAULT_API_VERSION.to_string())
    }

    async fn access_token(&self) -> Result<String, ConnectorError> {
        let auth = self.auth_snapshot().await;
        if let Some(token) = auth.get("access_token").cloned() {
            let is_expired = Self::parse_timestamp_maybe(auth.get("expires_at"))
                .is_some_and(|expires_at| expires_at <= Self::now_epoch());
            if !is_expired {
                return Ok(token);
            }
        }

        self.refresh_access_token_internal().await
    }

    async fn decode_id_token_claims(
        &self,
        auth: &AuthDetails,
    ) -> Result<(LinkedInIdTokenClaims, bool, String), ConnectorError> {
        let token = auth
            .get("id_token")
            .cloned()
            .ok_or_else(|| ConnectorError::Authentication("Missing id_token".to_string()))?;

        if let Some(client_id) = auth
            .get("client_id")
            .filter(|value| !value.trim().is_empty())
        {
            let claims = self.validate_id_token(&token, client_id).await?;
            return Ok((claims, true, "jwks".to_string()));
        }

        let claims = decode_id_token_claims_unverified(&token)?;
        Ok((claims, false, "claims_only".to_string()))
    }

    async fn fetch_discovery_document(&self) -> Result<Value, ConnectorError> {
        let response = self
            .client
            .get(&self.discovery_url)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = response.status();
        let body = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));
        if !status.is_success() {
            return Err(ConnectorError::Authentication(format!(
                "LinkedIn discovery request failed with HTTP {status}: {parsed}"
            )));
        }
        Ok(parsed)
    }

    async fn validate_id_token(
        &self,
        token: &str,
        client_id: &str,
    ) -> Result<LinkedInIdTokenClaims, ConnectorError> {
        let header = decode_header(token).map_err(|err| {
            ConnectorError::Authentication(format!("Invalid id_token header: {err}"))
        })?;
        if header.alg != Algorithm::RS256 {
            return Err(ConnectorError::Authentication(format!(
                "Unsupported id_token signing algorithm: {:?}",
                header.alg
            )));
        }
        let kid = header.kid.ok_or_else(|| {
            ConnectorError::Authentication("id_token header is missing 'kid'".to_string())
        })?;

        let discovery = self.fetch_discovery_document().await.unwrap_or_else(|_| {
            json!({
                "issuer": LINKEDIN_OAUTH_BASE,
                "jwks_uri": LINKEDIN_JWKS_URL,
            })
        });

        let issuer = discovery
            .get("issuer")
            .and_then(Value::as_str)
            .unwrap_or(LINKEDIN_OAUTH_BASE);
        let jwks_uri = discovery
            .get("jwks_uri")
            .and_then(Value::as_str)
            .unwrap_or(LINKEDIN_JWKS_URL);

        let jwks_response = self
            .client
            .get(jwks_uri)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = jwks_response.status();
        let jwks_text = jwks_response
            .text()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let jwks_json: Value = serde_json::from_str(&jwks_text).map_err(|err| {
            ConnectorError::Authentication(format!("Invalid JWKS document: {err}"))
        })?;
        if !status.is_success() {
            return Err(ConnectorError::Authentication(format!(
                "LinkedIn JWKS request failed with HTTP {status}: {jwks_json}"
            )));
        }

        let key = jwks_json
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|keys| {
                keys.iter().find(|key| {
                    key.get("kid")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == kid)
                })
            })
            .ok_or_else(|| {
                ConnectorError::Authentication(format!("No JWKS key matched id_token kid '{kid}'"))
            })?;

        let n = key.get("n").and_then(Value::as_str).ok_or_else(|| {
            ConnectorError::Authentication("JWKS RSA key is missing 'n'".to_string())
        })?;
        let e = key.get("e").and_then(Value::as_str).ok_or_else(|| {
            ConnectorError::Authentication("JWKS RSA key is missing 'e'".to_string())
        })?;

        let decoding_key = DecodingKey::from_rsa_components(n, e).map_err(|err| {
            ConnectorError::Authentication(format!("Invalid JWKS RSA key: {err}"))
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[client_id]);
        validation.set_issuer(&[issuer]);
        validation.validate_exp = true;

        let token_data = decode::<LinkedInIdTokenClaims>(token, &decoding_key, &validation)
            .map_err(|err| ConnectorError::Authentication(format!("Invalid id_token: {err}")))?;
        Ok(token_data.claims)
    }

    async fn get_userinfo(&self, token: &str) -> Result<UserInfoResponse, ConnectorError> {
        let response = self
            .client
            .get(format!("{}/v2/userinfo", self.api_base))
            .header(AUTHORIZATION, bearer_header(token)?)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = response.status();
        let body = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let value: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(Self::reauth_required(format!(
                "LinkedIn userinfo request was denied with HTTP {status}: {value}"
            )));
        }
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "LinkedIn userinfo request failed with HTTP {status}: {value}"
            )));
        }

        serde_json::from_value(value).map_err(ConnectorError::SerdeJson)
    }

    async fn resolve_member_urn(
        &self,
        explicit_author: Option<&str>,
    ) -> Result<String, ConnectorError> {
        if let Some(author) = explicit_author.filter(|value| !value.trim().is_empty()) {
            return Ok(Self::ensure_person_urn(author.trim()));
        }

        let auth = self.auth_snapshot().await;
        if let Some(member_urn) = auth
            .get("member_urn")
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Self::ensure_person_urn(member_urn.trim()));
        }

        if auth.get("id_token").is_some() {
            let (claims, _, _) = self.decode_id_token_claims(&auth).await?;
            let person_urn = Self::ensure_person_urn(&claims.sub);
            self.set_cached_value("member_urn", person_urn.clone())
                .await;
            return Ok(person_urn);
        }

        let token = self.access_token().await?;
        let me = self.get_userinfo(&token).await?;
        let person_urn = Self::ensure_person_urn(&me.sub);
        self.set_cached_value("member_urn", person_urn.clone())
            .await;
        Ok(person_urn)
    }

    async fn resolve_organization_urn(
        &self,
        explicit_organization: Option<&str>,
    ) -> Result<String, ConnectorError> {
        if let Some(organization) = explicit_organization.filter(|value| !value.trim().is_empty()) {
            return Ok(Self::ensure_organization_urn(organization.trim()));
        }

        let auth = self.auth_snapshot().await;
        auth.get("organization_urn")
            .filter(|value| !value.trim().is_empty())
            .map(|value| Self::ensure_organization_urn(value.trim()))
            .ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "Missing organization URN. Provide 'organization' or configure 'organization_urn'."
                        .to_string(),
                )
            })
    }

    async fn build_auth_status(&self) -> Result<Value, ConnectorError> {
        let auth = self.auth_snapshot().await;
        let scopes = Self::scopes_from_auth(&auth);
        let access_expires_at = Self::parse_timestamp_maybe(auth.get("expires_at"));
        let refresh_expires_at = Self::parse_timestamp_maybe(auth.get("refresh_token_expires_at"));
        let access_expired = access_expires_at.is_some_and(|exp| exp <= Self::now_epoch());
        let refresh_expired = refresh_expires_at.is_some_and(|exp| exp <= Self::now_epoch());
        let refresh_available = auth.get("refresh_token").is_some()
            && auth.get("client_id").is_some()
            && auth.get("client_secret").is_some()
            && !refresh_expired;

        let id_token_validation = if auth.get("id_token").is_some() {
            match self.decode_id_token_claims(&auth).await {
                Ok((claims, verified, mode)) => json!({
                    "present": true,
                    "validated": verified,
                    "mode": mode,
                    "subject": claims.sub,
                }),
                Err(err) => json!({
                    "present": true,
                    "validated": false,
                    "mode": "error",
                    "error": err.to_string(),
                }),
            }
        } else {
            json!({
                "present": false,
                "validated": false,
                "mode": "none",
            })
        };

        Ok(json!({
            "configured": !auth.is_empty(),
            "auth_mode": Self::parse_auth_mode(&auth).as_str(),
            "token_source": auth.get("token_source").cloned().unwrap_or_else(|| "external_oauth".to_string()),
            "access_token_present": auth.get("access_token").is_some(),
            "access_token_expires_at": access_expires_at,
            "access_token_expired": access_expired,
            "refresh_token_present": auth.get("refresh_token").is_some(),
            "refresh_token_expires_at": refresh_expires_at,
            "refresh_token_expired": refresh_expired,
            "refresh_available": refresh_available,
            "id_token": id_token_validation,
            "scopes": scopes,
            "member_urn": auth.get("member_urn").cloned(),
            "organization_urn": auth.get("organization_urn").cloned(),
            "linkedin_api_version": Self::linkedin_version(&auth, None),
            "capabilities": {
                "get_me": auth.get("access_token").is_some() || auth.get("id_token").is_some(),
                "member_posting": Self::has_scope(&auth, "w_member_social"),
                "organization_posting": Self::has_scope(&auth, "w_organization_social"),
                "raw_api": auth.get("access_token").is_some() || refresh_available,
            },
            "reauth_required": (auth.get("access_token").is_none() && !refresh_available && auth.get("id_token").is_none())
                || (access_expired && !refresh_available && auth.get("id_token").is_none()),
        }))
    }

    async fn refresh_access_token_internal(&self) -> Result<String, ConnectorError> {
        let auth = self.auth_snapshot().await;
        let refresh_token = auth
            .get("refresh_token")
            .cloned()
            .ok_or_else(|| Self::reauth_required("No refresh_token is configured"))?;
        let client_id = auth.get("client_id").cloned().ok_or_else(|| {
            Self::reauth_required("Missing client_id required for LinkedIn refresh")
        })?;
        let client_secret = auth.get("client_secret").cloned().ok_or_else(|| {
            Self::reauth_required("Missing client_secret required for LinkedIn refresh")
        })?;

        if Self::parse_timestamp_maybe(auth.get("refresh_token_expires_at"))
            .is_some_and(|exp| exp <= Self::now_epoch())
        {
            return Err(Self::reauth_required(
                "The configured refresh_token has expired and the member must re-authorize",
            ));
        }

        let token_url = format!("{}/oauth/v2/accessToken", self.oauth_base);
        let response = self
            .client
            .post(token_url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = response.status();
        let body = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let payload: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));

        if !status.is_success() {
            return Err(Self::reauth_required(format!(
                "LinkedIn refresh failed with HTTP {status}: {payload}"
            )));
        }

        let tokens: RefreshResponse = serde_json::from_value(payload.clone()).map_err(|err| {
            ConnectorError::Authentication(format!(
                "LinkedIn refresh response could not be parsed: {err}"
            ))
        })?;

        let expires_at = tokens
            .expires_in
            .map(|expires_in| Self::now_epoch() + expires_in - 60);
        let refresh_expires_at = tokens
            .refresh_token_expires_in
            .map(|expires_in| Self::now_epoch() + expires_in - 60);

        let mut guard = self.auth.write().await;
        guard.insert("access_token".to_string(), tokens.access_token.clone());
        if let Some(expires_at) = expires_at {
            guard.insert("expires_at".to_string(), expires_at.to_string());
        }
        if let Some(refresh_token) = tokens.refresh_token {
            guard.insert("refresh_token".to_string(), refresh_token);
        }
        if let Some(refresh_expires_at) = refresh_expires_at {
            guard.insert(
                "refresh_token_expires_at".to_string(),
                refresh_expires_at.to_string(),
            );
        }
        if let Some(scope) = tokens.scope {
            guard.insert("scopes".to_string(), scope);
        }

        Ok(tokens.access_token)
    }

    async fn build_me_payload(&self) -> Result<Value, ConnectorError> {
        let auth = self.auth_snapshot().await;
        if auth.get("access_token").is_some()
            || (auth.get("refresh_token").is_some()
                && auth.get("client_id").is_some()
                && auth.get("client_secret").is_some())
        {
            let token = self.access_token().await?;
            let me = self.get_userinfo(&token).await?;
            let person_urn = Self::ensure_person_urn(&me.sub);
            self.set_cached_value("member_urn", person_urn.clone())
                .await;
            return Ok(json!({
                "source": "userinfo",
                "validated": true,
                "sub": me.sub,
                "person_urn": person_urn,
                "name": me.name,
                "given_name": me.given_name,
                "family_name": me.family_name,
                "picture": me.picture,
                "locale": me.locale,
                "email": me.email,
                "email_verified": me.email_verified,
            }));
        }

        let (claims, validated, mode) = self.decode_id_token_claims(&auth).await?;
        let person_urn = Self::ensure_person_urn(&claims.sub);
        self.set_cached_value("member_urn", person_urn.clone())
            .await;
        Ok(json!({
            "source": "id_token",
            "validated": validated,
            "validation_mode": mode,
            "sub": claims.sub,
            "person_urn": person_urn,
            "name": claims.name,
            "given_name": claims.given_name,
            "family_name": claims.family_name,
            "picture": claims.picture,
            "locale": claims.locale,
            "email": claims.email,
            "email_verified": claims.email_verified,
        }))
    }

    async fn api_request(
        &self,
        method: Method,
        path: &str,
        options: ApiRequestOptions,
    ) -> Result<HttpResponsePayload, ConnectorError> {
        let token = self.access_token().await?;

        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.api_base, path)
        };

        let mut request = self.client.request(method, &url);
        request = request.header(AUTHORIZATION, bearer_header(&token)?);

        if let Some(query) = options.query.as_ref() {
            let query_pairs: Vec<(String, String)> = query
                .iter()
                .map(|(key, value)| (key.clone(), json_scalar_to_string(value)))
                .collect();
            request = request.query(&query_pairs);
        }

        if options.include_linkedin_rest_headers || url.contains("/rest/") {
            let auth = self.auth_snapshot().await;
            request = request.header(
                HeaderName::from_static("x-restli-protocol-version"),
                LINKEDIN_PROTOCOL_VERSION,
            );
            let version = options
                .linkedin_version
                .clone()
                .unwrap_or_else(|| Self::linkedin_version(&auth, None));
            request = request.header(HeaderName::from_static("linkedin-version"), version);
        }

        if let Some(headers) = options.headers.as_ref() {
            for (key, value) in headers {
                let header_name = HeaderName::try_from(key.as_str()).map_err(|err| {
                    ConnectorError::InvalidParams(format!("Invalid header '{key}': {err}"))
                })?;
                let header_value =
                    HeaderValue::try_from(json_scalar_to_string(value)).map_err(|err| {
                        ConnectorError::InvalidParams(format!(
                            "Invalid header value for '{key}': {err}"
                        ))
                    })?;
                request = request.header(header_name, header_value);
            }
        }

        if let Some(body) = options.body {
            if matches!(body, Value::Object(_) | Value::Array(_)) {
                request = request.header(CONTENT_TYPE, "application/json");
                request = request.json(&body);
            } else if body.is_null() {
                // No-op
            } else {
                request = request.body(json_scalar_to_string(&body));
            }
        }

        let response = request.send().await.map_err(ConnectorError::HttpRequest)?;
        let status = response.status();
        let headers = response.headers().clone();
        let body_text = response.text().await.map_err(ConnectorError::HttpRequest)?;
        let body_json =
            serde_json::from_str::<Value>(&body_text).unwrap_or_else(|_| Value::String(body_text));

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(Self::reauth_required(format!(
                "LinkedIn API request to '{path}' was denied with HTTP {status}"
            )));
        }

        let mut header_map = Map::new();
        for (name, value) in &headers {
            header_map.insert(
                name.to_string(),
                Value::String(value.to_str().unwrap_or_default().to_string()),
            );
        }

        Ok(HttpResponsePayload {
            status: status.as_u16(),
            ok: status.is_success(),
            headers: header_map,
            body: body_json,
        })
    }

    fn post_payload(
        author: String,
        commentary: &str,
        visibility: &str,
        url: Option<&str>,
        image: Option<&str>,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<Value, ConnectorError> {
        if url.is_some() && image.is_some() {
            return Err(ConnectorError::InvalidParams(
                "Provide either 'url' or 'image', not both. Use api_request for richer post bodies."
                    .to_string(),
            ));
        }

        let mut payload = json!({
            "author": author,
            "commentary": commentary,
            "visibility": visibility,
            "distribution": {
                "feedDistribution": "MAIN_FEED",
                "targetEntities": [],
                "thirdPartyDistributionChannels": []
            },
            "lifecycleState": "PUBLISHED",
            "isReshareDisabledByAuthor": false
        });

        if let Some(url) = url {
            payload["content"] = json!({
                "article": {
                    "source": url,
                }
            });
            if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
                payload["content"]["article"]["title"] = Value::String(title.to_string());
            }
            if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
                payload["content"]["article"]["description"] =
                    Value::String(description.to_string());
            }
        } else if let Some(image) = image {
            payload["content"] = json!({
                "media": {
                    "id": image,
                }
            });
            if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
                payload["content"]["media"]["title"] = Value::String(title.to_string());
            }
            if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
                payload["content"]["media"]["description"] = Value::String(description.to_string());
            }
        }

        Ok(payload)
    }

    async fn create_post(
        &self,
        author: String,
        commentary: &str,
        visibility: &str,
        url: Option<&str>,
        image: Option<&str>,
        title: Option<&str>,
        description: Option<&str>,
        linkedin_version: Option<&str>,
    ) -> Result<Value, ConnectorError> {
        let payload = Self::post_payload(
            author.clone(),
            commentary,
            visibility,
            url,
            image,
            title,
            description,
        )?;

        let response = self
            .api_request(
                Method::POST,
                "/rest/posts",
                ApiRequestOptions {
                    body: Some(payload.clone()),
                    include_linkedin_rest_headers: true,
                    linkedin_version: linkedin_version.map(ToString::to_string),
                    ..Default::default()
                },
            )
            .await?;

        Ok(json!({
            "ok": response.ok,
            "status": response.status,
            "author": author,
            "linkedin_version": linkedin_version
                .map(ToString::to_string)
                .unwrap_or_else(|| LINKEDIN_DEFAULT_API_VERSION.to_string()),
            "request": payload,
            "response_headers": response.headers,
            "response_body": response.body,
            "restli_id": response.headers.get("x-restli-id").cloned(),
        }))
    }

    fn set_auth_from_signin_args(
        auth: &mut AuthDetails,
        args: &Map<String, Value>,
    ) -> Result<(), ConnectorError> {
        let keys = [
            "access_token",
            "expires_at",
            "refresh_token",
            "refresh_token_expires_at",
            "id_token",
            "organization_urn",
            "member_urn",
            "client_id",
            "client_secret",
            "linkedin_api_version",
            "token_source",
        ];

        for key in keys {
            if let Some(value) = args.get(key).and_then(Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    auth.insert(key.to_string(), value.to_string());
                }
            }
        }

        if let Some(value) = args.get("auth_mode").and_then(Value::as_str) {
            match value {
                "member_oauth" | "app_oauth" => {
                    auth.insert("auth_mode".to_string(), value.to_string());
                }
                other => {
                    return Err(ConnectorError::InvalidParams(format!(
                        "Invalid auth_mode '{other}'. Expected member_oauth or app_oauth"
                    )));
                }
            }
        }

        if let Some(scopes) = args.get("scopes") {
            let normalized = parse_scopes_value(scopes);
            if !normalized.is_empty() {
                auth.insert("scopes".to_string(), normalized.join(" "));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Connector for LinkedInConnector {
    fn name(&self) -> &'static str {
        "linkedin"
    }

    fn description(&self) -> &'static str {
        "Official LinkedIn OAuth/OIDC connector for auth status, profile identity, posting, and raw authenticated API requests."
    }

    fn display_name(&self) -> &'static str {
        "LinkedIn"
    }

    fn icon(&self) -> &'static str {
        "linkedin"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["social", "oauth", "marketing"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(Default::default()),
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: Some("LinkedIn".to_string()),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: Some("https://learn.microsoft.com/en-us/linkedin/".to_string()),
            },
            instructions: Some(
                "Configure LinkedIn by importing externally obtained OAuth/OIDC tokens (access_token, optional refresh_token, optional id_token). This connector does not run the OAuth browser flow itself and does not use browser cookies.".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
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
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("signin"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Import externally obtained LinkedIn OAuth/OIDC material into the in-memory connector session. Accepts access_token, optional refresh_token, optional id_token, scopes, auth_mode, and token metadata.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "access_token": {"type": "string"},
                            "expires_at": {"type": ["string", "integer"], "description": "Epoch seconds or RFC3339 timestamp."},
                            "refresh_token": {"type": "string"},
                            "refresh_token_expires_at": {"type": ["string", "integer"], "description": "Epoch seconds or RFC3339 timestamp."},
                            "id_token": {"type": "string"},
                            "scopes": {
                                "description": "Scopes as an array or a space/comma-delimited string.",
                                "oneOf": [
                                    {"type": "array", "items": {"type": "string"}},
                                    {"type": "string"}
                                ]
                            },
                            "auth_mode": {"type": "string", "enum": ["member_oauth", "app_oauth"]},
                            "organization_urn": {"type": "string"},
                            "member_urn": {"type": "string"},
                            "client_id": {"type": "string"},
                            "client_secret": {"type": "string"},
                            "linkedin_api_version": {"type": "string"},
                            "token_source": {"type": "string"}
                        }
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_auth_status"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Return LinkedIn auth status including scopes, expiry state, refresh availability, cached actor/org identifiers, and whether re-authorization is required.",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_me"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Resolve the authenticated member profile using LinkedIn userinfo when possible, or the configured id_token otherwise. Returns a derived person_urn for posting.",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_share_update"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Create a member-authored LinkedIn post using the official Posts API. Supports text-only, article URL, or existing LinkedIn image/video/document URN content.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "visibility": {"type": "string", "enum": ["PUBLIC", "CONNECTIONS", "LOGGED_IN"]},
                            "url": {"type": "string"},
                            "image": {"type": "string", "description": "Existing LinkedIn media URN, e.g. urn:li:image:..."},
                            "title": {"type": "string"},
                            "description": {"type": "string"},
                            "author": {"type": "string", "description": "Optional member URN override. Defaults to configured member_urn or the authenticated member."},
                            "linkedin_version": {"type": "string"}
                        },
                        "required": ["text"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_company_update"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Create an organization-authored LinkedIn post using the official Posts API. Requires org-posting permissions and an organization URN.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "organization": {"type": "string", "description": "Organization URN or numeric id."},
                            "text": {"type": "string"},
                            "visibility": {"type": "string", "enum": ["PUBLIC", "CONNECTIONS", "LOGGED_IN"]},
                            "url": {"type": "string"},
                            "image": {"type": "string", "description": "Existing LinkedIn media URN, e.g. urn:li:image:..."},
                            "title": {"type": "string"},
                            "description": {"type": "string"},
                            "linkedin_version": {"type": "string"}
                        },
                        "required": ["text"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("api_request"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Make a raw authenticated LinkedIn HTTP request. Paths can be relative to https://api.linkedin.com or absolute URLs. For /rest endpoints, rzn-tools adds Rest.li and Linkedin-Version headers by default.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
                            "path": {"type": "string"},
                            "query": {"type": "object", "additionalProperties": true},
                            "headers": {"type": "object", "additionalProperties": true},
                            "body": {"description": "Optional JSON body or scalar body."},
                            "linkedin_version": {"type": "string"},
                            "include_linkedin_rest_headers": {"type": "boolean"}
                        },
                        "required": ["method", "path"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("refresh_access_token"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Refresh the configured LinkedIn access token using refresh_token, client_id, and client_secret. If refresh is unavailable or rejected, returns a reauth_required authentication error.",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("schema object").clone()),
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
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();

        match name.as_str() {
            "signin" => {
                let mut auth = self.auth_snapshot().await;
                Self::set_auth_from_signin_args(&mut auth, &args)?;
                let mut guard = self.auth.write().await;
                *guard = auth;
                let payload = self.build_auth_status().await?;
                structured_result_with_text(&payload, None)
            }
            "get_auth_status" => {
                let payload = self.build_auth_status().await?;
                structured_result_with_text(&payload, None)
            }
            "get_me" => {
                let payload = self.build_me_payload().await?;
                structured_result_with_text(&payload, None)
            }
            "create_share_update" => {
                let auth = self.auth_snapshot().await;
                if !Self::has_scope(&auth, "w_member_social") {
                    return Err(ConnectorError::Authentication(
                        "Missing w_member_social scope required for member posting".to_string(),
                    ));
                }
                let author = self
                    .resolve_member_urn(Self::parse_optional_string(&args, "author").as_deref())
                    .await?;
                let payload = self
                    .create_post(
                        author,
                        &Self::parse_required_string(&args, "text")?,
                        &Self::normalize_visibility(
                            args.get("visibility").and_then(Value::as_str),
                        )?,
                        Self::parse_optional_string(&args, "url").as_deref(),
                        Self::parse_optional_string(&args, "image").as_deref(),
                        Self::parse_optional_string(&args, "title").as_deref(),
                        Self::parse_optional_string(&args, "description").as_deref(),
                        Self::parse_optional_string(&args, "linkedin_version").as_deref(),
                    )
                    .await?;
                structured_result_with_text(&payload, None)
            }
            "create_company_update" => {
                let auth = self.auth_snapshot().await;
                if !Self::has_scope(&auth, "w_organization_social") {
                    return Err(ConnectorError::Authentication(
                        "Missing w_organization_social scope required for organization posting"
                            .to_string(),
                    ));
                }
                let organization = self
                    .resolve_organization_urn(
                        Self::parse_optional_string(&args, "organization").as_deref(),
                    )
                    .await?;
                let payload = self
                    .create_post(
                        organization,
                        &Self::parse_required_string(&args, "text")?,
                        &Self::normalize_visibility(
                            args.get("visibility").and_then(Value::as_str),
                        )?,
                        Self::parse_optional_string(&args, "url").as_deref(),
                        Self::parse_optional_string(&args, "image").as_deref(),
                        Self::parse_optional_string(&args, "title").as_deref(),
                        Self::parse_optional_string(&args, "description").as_deref(),
                        Self::parse_optional_string(&args, "linkedin_version").as_deref(),
                    )
                    .await?;
                structured_result_with_text(&payload, None)
            }
            "api_request" => {
                let method =
                    parse_http_method(args.get("method").and_then(Value::as_str).ok_or_else(
                        || ConnectorError::InvalidParams("Missing 'method' argument".to_string()),
                    )?)?;
                let payload = self
                    .api_request(
                        method,
                        &Self::parse_required_string(&args, "path")?,
                        ApiRequestOptions {
                            query: Self::parse_optional_object(&args, "query")?,
                            headers: Self::parse_optional_object(&args, "headers")?,
                            body: args.get("body").cloned(),
                            include_linkedin_rest_headers: args
                                .get("include_linkedin_rest_headers")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            linkedin_version: Self::parse_optional_string(
                                &args,
                                "linkedin_version",
                            ),
                        },
                    )
                    .await?;
                let output = json!({
                    "status": payload.status,
                    "ok": payload.ok,
                    "headers": payload.headers,
                    "body": payload.body,
                });
                structured_result_with_text(&output, None)
            }
            "refresh_access_token" => {
                let access_token = self.refresh_access_token_internal().await?;
                let auth = self.auth_snapshot().await;
                let payload = json!({
                    "ok": true,
                    "access_token_present": !access_token.is_empty(),
                    "expires_at": Self::parse_timestamp_maybe(auth.get("expires_at")),
                    "refresh_token_present": auth.get("refresh_token").is_some(),
                    "refresh_token_expires_at": Self::parse_timestamp_maybe(auth.get("refresh_token_expires_at")),
                    "scopes": Self::scopes_from_auth(&auth),
                });
                structured_result_with_text(&payload, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth_snapshot().await)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let mut guard = self.auth.write().await;
        *guard = details;
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let auth = self.auth_snapshot().await;
        let has_access = auth.get("access_token").is_some();
        let has_id_token = auth.get("id_token").is_some();
        let can_refresh = auth.get("refresh_token").is_some()
            && auth.get("client_id").is_some()
            && auth.get("client_secret").is_some();

        if has_access || has_id_token || can_refresh {
            return Ok(());
        }

        Err(ConnectorError::Authentication(
            "LinkedIn auth not configured. Import tokens via `rzn-tools setup linkedin` or `rzn-tools config set linkedin --key access_token --value ...`."
                .to_string(),
        ))
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "access_token".to_string(),
                    label: "Access Token".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("LinkedIn OAuth access token supplied by an external broker or backend.".to_string()),
                    options: None,
                },
                Field {
                    name: "expires_at".to_string(),
                    label: "Access Token Expires At".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Epoch seconds or RFC3339 timestamp for access-token expiry.".to_string()),
                    options: None,
                },
                Field {
                    name: "refresh_token".to_string(),
                    label: "Refresh Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Optional LinkedIn refresh token. Programmatic refresh is only available to approved LinkedIn partner setups.".to_string()),
                    options: None,
                },
                Field {
                    name: "refresh_token_expires_at".to_string(),
                    label: "Refresh Token Expires At".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Epoch seconds or RFC3339 timestamp for refresh-token expiry.".to_string()),
                    options: None,
                },
                Field {
                    name: "id_token".to_string(),
                    label: "ID Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Optional LinkedIn OIDC ID token for member identity and person_urn derivation.".to_string()),
                    options: None,
                },
                Field {
                    name: "scopes".to_string(),
                    label: "Scopes".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Space- or comma-delimited scopes, e.g. 'openid profile email w_member_social'.".to_string()),
                    options: None,
                },
                Field {
                    name: "organization_urn".to_string(),
                    label: "Organization URN".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional default organization URN used by create_company_update.".to_string()),
                    options: None,
                },
                Field {
                    name: "member_urn".to_string(),
                    label: "Member URN".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional explicit member URN. If omitted, rzn-tools derives it from userinfo or id_token when possible.".to_string()),
                    options: None,
                },
                Field {
                    name: "linkedin_api_version".to_string(),
                    label: "LinkedIn API Version".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional YYYYMM LinkedIn version for /rest endpoints. Defaults to 202603.".to_string()),
                    options: None,
                },
                Field {
                    name: "auth_mode".to_string(),
                    label: "Auth Mode".to_string(),
                    field_type: FieldType::Select {
                        options: vec!["member_oauth".to_string(), "app_oauth".to_string()],
                    },
                    required: false,
                    description: Some("Whether the imported token package represents member OAuth or app OAuth.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_id".to_string(),
                    label: "Client ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("LinkedIn app client id. Needed for refresh-token calls and full OIDC id_token validation.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    label: "Client Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("LinkedIn app client secret. Needed for refresh-token calls.".to_string()),
                    options: None,
                },
            ],
        }
    }
}

fn parse_http_method(raw: &str) -> Result<Method, ConnectorError> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        other => Err(ConnectorError::InvalidParams(format!(
            "Invalid HTTP method '{other}'"
        ))),
    }
}

fn parse_scopes_value(value: &Value) -> Vec<String> {
    let raw_values: Vec<String> = match value {
        Value::String(raw) => raw
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    };

    let mut deduped = Vec::new();
    for value in raw_values {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }
    deduped
}

fn json_scalar_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn bearer_header(token: &str) -> Result<HeaderValue, ConnectorError> {
    HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|err| ConnectorError::InvalidInput(format!("Invalid bearer token header: {err}")))
}

fn decode_id_token_claims_unverified(token: &str) -> Result<LinkedInIdTokenClaims, ConnectorError> {
    let mut segments = token.split('.');
    let _header = segments.next();
    let claims_segment = segments
        .next()
        .ok_or_else(|| ConnectorError::Authentication("Invalid JWT format".to_string()))?;
    let decoded = base64_url_decode(claims_segment)?;
    serde_json::from_slice::<LinkedInIdTokenClaims>(&decoded)
        .map_err(|err| ConnectorError::Authentication(format!("Invalid id_token claims: {err}")))
}

fn base64_url_decode(raw: &str) -> Result<Vec<u8>, ConnectorError> {
    let mut normalized = raw.replace('-', "+").replace('_', "/");
    while normalized.len() % 4 != 0 {
        normalized.push('=');
    }
    STANDARD.decode(normalized).map_err(|err| {
        ConnectorError::Authentication(format!("Invalid base64url token segment: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct TestResponse {
        status_line: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    async fn start_test_server(
        responses: Vec<TestResponse>,
    ) -> (
        String,
        Arc<Mutex<Vec<RecordedRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let queued = Arc::new(Mutex::new(VecDeque::from(responses)));
        let recorded_clone = Arc::clone(&recorded);
        let queued_clone = Arc::clone(&queued);

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };

                let mut buffer = Vec::new();
                let mut header_buf = [0_u8; 4096];
                let header_end;
                loop {
                    let read = match stream.read(&mut header_buf).await {
                        Ok(0) => return,
                        Ok(read) => read,
                        Err(_) => return,
                    };
                    buffer.extend_from_slice(&header_buf[..read]);
                    if let Some(position) =
                        buffer.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        header_end = position + 4;
                        break;
                    }
                }

                let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let mut lines = header_text.split("\r\n").filter(|line| !line.is_empty());
                let request_line = lines.next().unwrap_or_default();
                let mut request_parts = request_line.split_whitespace();
                let method = request_parts.next().unwrap_or_default().to_string();
                let path = request_parts.next().unwrap_or_default().to_string();
                let headers: Vec<(String, String)> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
                    .collect();

                let content_length = headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.parse::<usize>().ok())
                    .unwrap_or(0);

                let mut body = buffer[header_end..].to_vec();
                while body.len() < content_length {
                    let mut chunk = vec![0_u8; content_length - body.len()];
                    let read = stream.read(&mut chunk).await.expect("read body");
                    if read == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..read]);
                }

                recorded_clone
                    .lock()
                    .expect("recorded lock")
                    .push(RecordedRequest {
                        method,
                        path,
                        headers,
                        body: String::from_utf8_lossy(&body).to_string(),
                    });

                let response = queued_clone
                    .lock()
                    .expect("queued lock")
                    .pop_front()
                    .unwrap_or(TestResponse {
                        status_line: "HTTP/1.1 500 Internal Server Error",
                        headers: vec![("Content-Type", "application/json")],
                        body: json!({"error":"missing queued response"}).to_string(),
                    });

                let mut response_text = format!("{}\r\n", response.status_line);
                for (name, value) in response.headers {
                    response_text.push_str(&format!("{name}: {value}\r\n"));
                }
                response_text.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.body.len(),
                    response.body
                ));

                let _ = stream.write_all(response_text.as_bytes()).await;
            }
        });

        (format!("http://{}", addr), recorded, handle)
    }

    #[test]
    fn parses_scope_lists_from_array_and_string() {
        let array = parse_scopes_value(&json!(["openid", "profile", "email"]));
        assert_eq!(array, vec!["openid", "profile", "email"]);

        let string = parse_scopes_value(&json!("openid, profile email w_member_social"));
        assert_eq!(
            string,
            vec!["openid", "profile", "email", "w_member_social"]
        );
    }

    #[test]
    fn decodes_id_token_claims_without_validation() {
        let claims = json!({
            "sub": "member123",
            "name": "Test User",
            "email": "user@example.com",
            "exp": 4_102_444_800_u64
        });
        let token = format!(
            "{}.{}.sig",
            URL_SAFE_NO_PAD.encode("{}"),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );

        let decoded = decode_id_token_claims_unverified(&token).expect("decode token");
        assert_eq!(decoded.sub, "member123");
        assert_eq!(decoded.name.as_deref(), Some("Test User"));
        assert_eq!(decoded.email.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn get_me_uses_userinfo_and_caches_member_urn() {
        let (api_base, recorded, handle) = start_test_server(vec![TestResponse {
            status_line: "HTTP/1.1 200 OK",
            headers: vec![("Content-Type", "application/json")],
            body: json!({
                "sub": "abc123",
                "name": "Ada Lovelace",
                "given_name": "Ada",
                "family_name": "Lovelace",
                "email": "ada@example.com",
                "email_verified": true
            })
            .to_string(),
        }])
        .await;

        let connector = LinkedInConnector::new_with_base_urls(
            AuthDetails::from(std::collections::HashMap::from([
                ("access_token".to_string(), "token-1".to_string()),
                ("scopes".to_string(), "openid profile email".to_string()),
            ])),
            &api_base,
            LINKEDIN_OAUTH_BASE,
            LINKEDIN_DISCOVERY_URL,
        )
        .await
        .expect("connector");

        let payload = connector.build_me_payload().await.expect("me payload");
        assert_eq!(payload["source"], "userinfo");
        assert_eq!(payload["person_urn"], "urn:li:person:abc123");

        let auth = connector.get_auth_details().await.expect("auth");
        assert_eq!(
            auth.get("member_urn").map(String::as_str),
            Some("urn:li:person:abc123")
        );

        let requests = recorded.lock().expect("recorded");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v2/userinfo");
        drop(requests);
        handle.abort();
    }

    #[tokio::test]
    async fn create_company_update_uses_rest_posts_and_version_headers() {
        let (api_base, recorded, handle) = start_test_server(vec![TestResponse {
            status_line: "HTTP/1.1 201 Created",
            headers: vec![
                ("Content-Type", "application/json"),
                ("x-restli-id", "urn:li:share:123"),
            ],
            body: json!({
                "id": "urn:li:share:123"
            })
            .to_string(),
        }])
        .await;

        let connector = LinkedInConnector::new_with_base_urls(
            AuthDetails::from(std::collections::HashMap::from([
                ("access_token".to_string(), "token-2".to_string()),
                ("scopes".to_string(), "w_organization_social".to_string()),
                (
                    "organization_urn".to_string(),
                    "urn:li:organization:999".to_string(),
                ),
                ("linkedin_api_version".to_string(), "202504".to_string()),
            ])),
            &api_base,
            LINKEDIN_OAUTH_BASE,
            LINKEDIN_DISCOVERY_URL,
        )
        .await
        .expect("connector");

        let payload = connector
            .create_post(
                "urn:li:organization:999".to_string(),
                "Hello LinkedIn",
                "PUBLIC",
                Some("https://example.com"),
                None,
                Some("Example"),
                Some("Description"),
                None,
            )
            .await
            .expect("post payload");

        assert_eq!(payload["status"], 201);
        assert_eq!(payload["restli_id"], "urn:li:share:123");

        let requests = recorded.lock().expect("recorded");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/rest/posts");
        assert!(requests[0].headers.iter().any(|(name, value)| name
            .eq_ignore_ascii_case("linkedin-version")
            && value == "202504"));
        assert!(requests[0].headers.iter().any(|(name, value)| name
            .eq_ignore_ascii_case("x-restli-protocol-version")
            && value == "2.0.0"));
        assert!(requests[0]
            .body
            .contains("\"author\":\"urn:li:organization:999\""));
        assert!(requests[0]
            .body
            .contains("\"source\":\"https://example.com\""));
        drop(requests);
        handle.abort();
    }

    #[tokio::test]
    async fn refresh_access_token_updates_cached_auth() {
        let (api_base, _recorded, handle) = start_test_server(vec![]).await;
        let (oauth_base, recorded_oauth, handle_oauth) = start_test_server(vec![TestResponse {
            status_line: "HTTP/1.1 200 OK",
            headers: vec![("Content-Type", "application/json")],
            body: json!({
                "access_token": "new-access-token",
                "expires_in": 3600,
                "refresh_token": "new-refresh-token",
                "refresh_token_expires_in": 86400,
                "scope": "openid profile email w_member_social"
            })
            .to_string(),
        }])
        .await;

        let connector = LinkedInConnector::new_with_base_urls(
            AuthDetails::from(std::collections::HashMap::from([
                ("refresh_token".to_string(), "refresh-token".to_string()),
                ("client_id".to_string(), "client-1".to_string()),
                ("client_secret".to_string(), "secret-1".to_string()),
            ])),
            &api_base,
            &oauth_base,
            LINKEDIN_DISCOVERY_URL,
        )
        .await
        .expect("connector");

        let access_token = connector
            .refresh_access_token_internal()
            .await
            .expect("refresh token");
        assert_eq!(access_token, "new-access-token");

        let auth = connector.get_auth_details().await.expect("auth");
        assert_eq!(
            auth.get("access_token").map(String::as_str),
            Some("new-access-token")
        );
        assert_eq!(
            auth.get("refresh_token").map(String::as_str),
            Some("new-refresh-token")
        );
        assert_eq!(
            auth.get("scopes").map(String::as_str),
            Some("openid profile email w_member_social")
        );

        let requests = recorded_oauth.lock().expect("recorded");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/oauth/v2/accessToken");
        assert!(requests[0].body.contains("grant_type=refresh_token"));
        drop(requests);

        handle.abort();
        handle_oauth.abort();
    }
}
