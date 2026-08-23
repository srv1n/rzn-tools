use async_trait::async_trait;
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::{Client, Method, StatusCode};
use rmcp::model::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

const ASA_BASE_URL: &str = "https://api.searchads.apple.com/api/v5";
const ASA_PLATFORM_BASE_URL: &str = "https://api.ads.apple.com/v1";
const ASA_TOKEN_URL: &str = "https://appleid.apple.com/auth/oauth2/token";
const ASA_TOKEN_AUDIENCE: &str = "https://appleid.apple.com";
const ASA_TOKEN_SCOPE: &str = "searchadsorg";

const ASA_USER_AGENT: &str = "rzn-tools/apple-search-ads";

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Clone, Debug)]
struct CachedToken {
    access_token: String,
    token_type: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct ListCampaignsInput {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct KeywordRecommendationsInput {
    app_id: u64,
    storefront_countries: String,
}

#[derive(Debug, Deserialize)]
struct ReportInput {
    body: Value,
}

#[derive(Debug, Deserialize)]
struct ReportCampaignInput {
    campaign_id: String,
    body: Value,
}

#[derive(Debug, Deserialize)]
struct CreateCampaignInput {
    body: Value,
}

#[derive(Debug, Deserialize)]
struct PlatformRequestInput {
    method: String,
    path: String,
    #[serde(default)]
    query: Map<String, Value>,
    #[serde(default)]
    body: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PlatformBodyInput {
    body: Value,
}

#[derive(Debug, Deserialize)]
struct PlatformRecommendationsInput {
    kind: String,
    body: Value,
}

#[derive(Debug, Deserialize)]
struct PlatformReportInput {
    level: String,
    body: Value,
}

#[derive(Clone, Copy)]
enum RequestScope {
    Organization,
    AdAccount,
}

#[derive(Clone)]
pub struct AppleSearchAdsConnector {
    auth: AuthDetails,
    http: Client,
    token: Arc<tokio::sync::Mutex<Option<CachedToken>>>,
}

impl AppleSearchAdsConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(ASA_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let http = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(ConnectorError::HttpRequest)?;

        Ok(Self {
            auth,
            http,
            token: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    fn org_id(&self) -> Option<String> {
        self.auth
            .get("org_id")
            .cloned()
            .or_else(|| std::env::var("ASA_ORG_ID").ok())
            .or_else(|| std::env::var("APPLE_SEARCH_ADS_ORG_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("org_id").cloned())
            })
    }

    fn ad_account_id(&self) -> Option<String> {
        self.auth
            .get("ad_account_id")
            .cloned()
            .or_else(|| std::env::var("ASA_AD_ACCOUNT_ID").ok())
            .or_else(|| std::env::var("APPLE_ADS_AD_ACCOUNT_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("ad_account_id").cloned())
            })
    }

    fn team_id(&self) -> Option<String> {
        self.auth
            .get("team_id")
            .cloned()
            .or_else(|| std::env::var("ASA_TEAM_ID").ok())
            .or_else(|| std::env::var("APPLE_SEARCH_ADS_TEAM_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("team_id").cloned())
            })
    }

    fn key_id(&self) -> Option<String> {
        self.auth
            .get("key_id")
            .cloned()
            .or_else(|| std::env::var("ASA_KEY_ID").ok())
            .or_else(|| std::env::var("APPLE_SEARCH_ADS_KEY_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("key_id").cloned())
            })
    }

    fn oauth_client_id(&self) -> Option<String> {
        self.auth
            .get("oauth_client_id")
            .cloned()
            .or_else(|| std::env::var("ASA_OAUTH_CLIENT_ID").ok())
            .or_else(|| std::env::var("APPLE_SEARCH_ADS_OAUTH_CLIENT_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("oauth_client_id").cloned())
            })
    }

    fn private_key_p8(&self) -> Option<String> {
        self.auth
            .get("private_key_p8")
            .cloned()
            .or_else(|| std::env::var("ASA_PRIVATE_KEY_P8").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("private_key_p8").cloned())
            })
    }

    fn private_key_path(&self) -> Option<String> {
        self.auth
            .get("private_key_path")
            .cloned()
            .or_else(|| std::env::var("ASA_P8_PATH").ok())
            .or_else(|| std::env::var("ASA_PRIVATE_KEY_PATH").ok())
            .or_else(|| std::env::var("APPLE_SEARCH_ADS_P8_PATH").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("private_key_path").cloned())
            })
    }

    fn load_private_key_pem(&self) -> Result<String, ConnectorError> {
        if let Some(pem) = self.private_key_p8() {
            return Ok(pem);
        }
        if let Some(path) = self.private_key_path() {
            let pem = std::fs::read_to_string(&path).map_err(|e| {
                ConnectorError::Authentication(format!(
                    "Failed to read private key from private_key_path={path}: {e}"
                ))
            })?;
            return Ok(pem);
        }
        Err(ConnectorError::Authentication(
            "Missing Apple Search Ads private key: set private_key_p8 or private_key_path".into(),
        ))
    }

    fn client_secret_jwt(&self) -> Result<String, ConnectorError> {
        let team_id = self.team_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing Apple Search Ads Team ID: set team_id or ASA_TEAM_ID".into(),
            )
        })?;
        let key_id = self.key_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing Apple Search Ads Key ID: set key_id or ASA_KEY_ID".into(),
            )
        })?;
        let client_id = self.oauth_client_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing Apple Search Ads OAuth client id: set oauth_client_id or ASA_OAUTH_CLIENT_ID"
                    .into(),
            )
        })?;

        let pem = self.load_private_key_pem()?;
        let encoding_key = EncodingKey::from_ec_pem(pem.as_bytes()).map_err(|e| {
            ConnectorError::Authentication(format!(
                "Invalid Apple Search Ads private key (expected .p8 / PEM EC key): {e}"
            ))
        })?;

        #[derive(serde::Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            sub: &'a str,
            aud: &'static str,
            exp: usize,
            iat: usize,
        }

        let now = Utc::now().timestamp();
        let exp = (Utc::now() + Duration::days(180)).timestamp();
        let header = Header {
            alg: Algorithm::ES256,
            kid: Some(key_id),
            ..Header::default()
        };
        let claims = Claims {
            iss: &team_id,
            sub: &client_id,
            aud: ASA_TOKEN_AUDIENCE,
            exp: exp as usize,
            iat: now as usize,
        };

        encode(&header, &claims, &encoding_key)
            .map_err(|e| ConnectorError::Authentication(format!("Failed to sign JWT: {e}")))
    }

    async fn ensure_access_token(&self) -> Result<(String, String), ConnectorError> {
        let now = Utc::now().timestamp();
        {
            let guard = self.token.lock().await;
            if let Some(tok) = guard.as_ref() {
                if now < tok.expires_at - 60 {
                    return Ok((tok.token_type.clone(), tok.access_token.clone()));
                }
            }
        }

        let client_id = self.oauth_client_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing Apple Search Ads OAuth client id: set oauth_client_id or ASA_OAUTH_CLIENT_ID"
                    .into(),
            )
        })?;
        let client_secret = self.client_secret_jwt()?;

        let resp = self
            .http
            .post(ASA_TOKEN_URL)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("scope", ASA_TOKEN_SCOPE),
            ])
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ConnectorError::Authentication(format!(
                "Token request failed (HTTP {status}): {text}"
            )));
        }

        let token_resp = resp
            .json::<OAuthTokenResponse>()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let token_type = token_resp
            .token_type
            .unwrap_or_else(|| "Bearer".to_string());
        let expires_in = token_resp.expires_in.unwrap_or(3600);
        let cached = CachedToken {
            access_token: token_resp.access_token.clone(),
            token_type: token_type.clone(),
            expires_at: now + expires_in as i64,
        };

        let mut guard = self.token.lock().await;
        *guard = Some(cached);

        Ok((token_type, token_resp.access_token))
    }

    async fn send_with_backoff<F>(
        &self,
        scope: RequestScope,
        build: F,
    ) -> Result<reqwest::Response, ConnectorError>
    where
        F: Fn(&Client, &str, &str, &str) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration as TokioDuration};

        const MAX_RETRIES: usize = 5;
        let mut delay_ms = 700u64;
        for attempt in 0..=MAX_RETRIES {
            let scope_id = match scope {
                RequestScope::Organization => self.org_id().ok_or_else(|| {
                    ConnectorError::Authentication(
                        "Missing Apple Search Ads org id: set org_id or ASA_ORG_ID".into(),
                    )
                })?,
                RequestScope::AdAccount => self.ad_account_id().ok_or_else(|| {
                    ConnectorError::Authentication(
                        "Missing Apple Ads Platform ad account id: set ad_account_id or ASA_AD_ACCOUNT_ID"
                            .into(),
                    )
                })?,
            };
            let (token_type, access_token) = self.ensure_access_token().await?;

            let resp = build(&self.http, &token_type, &access_token, &scope_id)
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        if attempt == MAX_RETRIES {
                            return Ok(r);
                        }
                        let retry_after = r
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok());
                        let wait = retry_after.unwrap_or_else(|| delay_ms.div_ceil(1000));
                        sleep(TokioDuration::from_secs(wait)).await;
                        delay_ms = (delay_ms as f64 * 1.7) as u64;
                        continue;
                    }
                    return Ok(r);
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        return Err(ConnectorError::HttpRequest(e));
                    }
                    sleep(TokioDuration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms as f64 * 1.7) as u64;
                    continue;
                }
            }
        }

        Err(ConnectorError::Other("request failed after retries".into()))
    }

    async fn request_json(
        &self,
        method: Method,
        path: &str,
        query: Vec<(String, String)>,
        body: Option<Value>,
    ) -> Result<Value, ConnectorError> {
        let url = api_url(ASA_BASE_URL, path)?;

        let resp = self
            .send_with_backoff(
                RequestScope::Organization,
                |client, token_type, access_token, org_id| {
                    let mut b = client
                        .request(method.clone(), &url)
                        .header(AUTHORIZATION, format!("{token_type} {}", access_token))
                        .header("X-AdServices-OrgId", org_id)
                        .header("X-AP-Context", format!("orgId={org_id}"));
                    if !query.is_empty() {
                        b = b.query(&query);
                    }
                    if let Some(ref json_body) = body {
                        b = b.header(CONTENT_TYPE, "application/json").json(json_body);
                    }
                    b
                },
            )
            .await?;

        response_json(resp, "Apple Search Ads").await
    }

    async fn request_platform_json(
        &self,
        method: Method,
        path: &str,
        query: Vec<(String, String)>,
        body: Option<Value>,
    ) -> Result<Value, ConnectorError> {
        let url = api_url(ASA_PLATFORM_BASE_URL, path)?;
        let resp = self
            .send_with_backoff(
                RequestScope::AdAccount,
                |client, token_type, access_token, ad_account_id| {
                    let mut b = client
                        .request(method.clone(), &url)
                        .header(AUTHORIZATION, format!("{token_type} {access_token}"))
                        .header("X-AP-Context", format!("adAccountId={ad_account_id};"));
                    if !query.is_empty() {
                        b = b.query(&query);
                    }
                    if let Some(ref json_body) = body {
                        b = b.header(CONTENT_TYPE, "application/json").json(json_body);
                    }
                    b
                },
            )
            .await?;

        response_json(resp, "Apple Ads Platform").await
    }
}

fn api_url(base: &str, path: &str) -> Result<String, ConnectorError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains("?")
        || path.contains("://")
        || path.split('/').any(|part| part == "..")
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ConnectorError::InvalidParams(
            "Apple Ads API path must be an absolute relative path without a query string or traversal"
                .into(),
        ));
    }
    Ok(format!("{base}{path}"))
}

fn parse_platform_method(raw: &str) -> Result<Method, ConnectorError> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "DELETE" => Ok(Method::DELETE),
        _ => Err(ConnectorError::InvalidParams(
            "Apple Ads Platform method must be GET, POST, PUT, or DELETE".into(),
        )),
    }
}

fn query_pairs(query: Map<String, Value>) -> Result<Vec<(String, String)>, ConnectorError> {
    let mut pairs = Vec::new();
    for (name, value) in query {
        if name.is_empty() || name.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(ConnectorError::InvalidParams(
                "Apple Ads Platform query names must be non-empty and printable".into(),
            ));
        }
        match value {
            Value::Null => {}
            Value::Array(values) => {
                for value in values {
                    let value = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    pairs.push((name.clone(), value));
                }
            }
            Value::Object(_) => {
                return Err(ConnectorError::InvalidParams(format!(
                    "Apple Ads Platform query parameter `{name}` must be a scalar or array"
                )))
            }
            value => pairs.push((
                name,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )),
        }
    }
    Ok(pairs)
}

async fn response_json(resp: reqwest::Response, service: &str) -> Result<Value, ConnectorError> {
    let status = resp.status();
    if status == StatusCode::NOT_FOUND {
        return Err(ConnectorError::ResourceNotFound);
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                ConnectorError::Authentication(text)
            }
            _ => ConnectorError::Other(format!("{service} API returned HTTP {status}: {text}")),
        });
    }

    let bytes = resp.bytes().await.map_err(ConnectorError::HttpRequest)?;
    if bytes.is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|e| ConnectorError::Other(format!("{service} API returned invalid JSON: {e}")))
    }
}

fn platform_tool(name: &'static str, description: &'static str, schema: Value) -> Tool {
    Tool {
        name: Cow::Borrowed(name),
        title: None,
        description: Some(Cow::Borrowed(description)),
        input_schema: Arc::new(schema.as_object().expect("schema object").clone()),
        output_schema: None,
        annotations: None,
        icons: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_request_helpers_reject_unsafe_paths_and_encode_queries() {
        assert_eq!(
            api_url(ASA_PLATFORM_BASE_URL, "/campaigns/query").unwrap(),
            "https://api.ads.apple.com/v1/campaigns/query"
        );
        assert!(api_url(ASA_PLATFORM_BASE_URL, "https://example.com").is_err());
        assert!(api_url(ASA_PLATFORM_BASE_URL, "/campaigns?next=1").is_err());
        assert!(api_url(ASA_PLATFORM_BASE_URL, "/campaigns/../me").is_err());

        let query = serde_json::from_value::<Map<String, Value>>(json!({
            "type": "brand",
            "ids": ["one", "two"],
            "skip": null
        }))
        .unwrap();
        assert_eq!(
            query_pairs(query).unwrap(),
            vec![
                ("ids".to_string(), "one".to_string()),
                ("ids".to_string(), "two".to_string()),
                ("type".to_string(), "brand".to_string())
            ]
        );
    }

    #[test]
    fn platform_method_parser_is_strict() {
        assert_eq!(parse_platform_method("post").unwrap(), Method::POST);
        assert!(parse_platform_method("PATCH").is_err());
    }
}

#[async_trait]
impl Connector for AppleSearchAdsConnector {
    fn name(&self) -> &'static str {
        "apple-search-ads"
    }

    fn description(&self) -> &'static str {
        "Apple Ads Platform API v1 plus legacy Apple Search Ads API v5 campaigns and reporting."
    }

    fn display_name(&self) -> &'static str {
        "Apple Search Ads"
    }

    fn icon(&self) -> &'static str {
        "apple"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["marketing", "ads", "analytics"]
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
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Configure oauth_client_id, team_id, key_id, and private_key_path (.p8). Set \
ad_account_id for Apple Ads Platform v1 tools; set org_id for legacy v5 tools. Use \
`platform_request` for any documented v1 endpoint, or the named v1 tools for reports, \
insights, recommendations, Maps, creatives, and change history."
                    .to_string(),
            ),
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("test_auth"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Validate Apple Search Ads credentials (OAuth token + API access).",
                )),
                input_schema: Arc::new(
                    json!({"type":"object","properties":{}})
                        .as_object()
                        .expect("schema object")
                        .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_campaigns"),
                title: None,
                description: Some(Cow::Borrowed("List campaigns.")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":50},
                            "offset":{"type":"integer","minimum":0,"default":0}
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
                name: Cow::Borrowed("keyword_recommendations"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get keyword recommendations for an app (demand proxy / suggested keywords).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "app_id":{"type":"integer","description":"Numeric App Store app id"},
                            "storefront_countries":{"type":"string","description":"Storefront country code(s), e.g. US"}
                        },
                        "required":["app_id","storefront_countries"]
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
                name: Cow::Borrowed("report_keywords"),
                title: None,
                description: Some(Cow::Borrowed("Keyword reporting (POST /reports/keywords).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "body":{"type":"object","description":"Apple Search Ads report request body (JSON)."}
                        },
                        "required":["body"]
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
                name: Cow::Borrowed("report_search_terms"),
                title: None,
                description: Some(Cow::Borrowed("Search terms reporting (POST /reports/searchterms).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "body":{"type":"object","description":"Apple Search Ads report request body (JSON)."}
                        },
                        "required":["body"]
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
                name: Cow::Borrowed("report_campaign_keywords"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Campaign keyword reporting (POST /reports/campaigns/{campaign_id}/keywords).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "campaign_id":{"type":"string"},
                            "body":{"type":"object"}
                        },
                        "required":["campaign_id","body"]
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
                name: Cow::Borrowed("report_campaign_search_terms"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Campaign search terms reporting (POST /reports/campaigns/{campaign_id}/searchterms).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "campaign_id":{"type":"string"},
                            "body":{"type":"object"}
                        },
                        "required":["campaign_id","body"]
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
                name: Cow::Borrowed("create_campaign"),
                title: None,
                description: Some(Cow::Borrowed("Create a campaign (POST /campaigns).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "body":{"type":"object","description":"Campaign create request body (JSON)."}
                        },
                        "required":["body"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            platform_tool(
                "platform_request",
                "Call any Apple Ads Platform API v1 endpoint with a relative path.",
                json!({
                    "type":"object",
                    "properties":{
                        "method":{"type":"string","enum":["GET","POST","PUT","DELETE"]},
                        "path":{"type":"string","description":"Relative API path such as /campaigns/query"},
                        "query":{"type":"object","description":"Optional scalar or array query parameters"},
                        "body":{"type":"object","description":"Optional JSON request body"}
                    },
                    "required":["method","path"]
                }),
            ),
            platform_tool(
                "platform_query_campaigns",
                "Query Apple Ads Platform v1 campaigns with filters, sorting, and pagination.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_search_term_popularity",
                "Query Apple Ads Platform v1 search-term popularity insights.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_impression_share",
                "Query Apple Ads Platform v1 impression-share insights, including single-digit metrics.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_recommendations",
                "Query Apple Ads Platform v1 daily-budget or target-CPA recommendations.",
                json!({
                    "type":"object",
                    "properties":{
                        "kind":{"type":"string","enum":["daily-budgets","target-cpas"]},
                        "body":{"type":"object"}
                    },
                    "required":["kind","body"]
                }),
            ),
            platform_tool(
                "platform_report_apps",
                "Query an Apple Ads Platform v1 App Store report.",
                json!({
                    "type":"object",
                    "properties":{
                        "level":{"type":"string","enum":["campaigns","adgroups","ads","keywords","searchterms"]},
                        "body":{"type":"object"}
                    },
                    "required":["level","body"]
                }),
            ),
            platform_tool(
                "platform_report_brands",
                "Query an Apple Ads Platform v1 Apple Maps brand report.",
                json!({
                    "type":"object",
                    "properties":{
                        "level":{"type":"string","enum":["campaigns","adgroups","ads","keywords","searchterms"]},
                        "body":{"type":"object"}
                    },
                    "required":["level","body"]
                }),
            ),
            platform_tool(
                "platform_query_brands",
                "Query Apple Maps business brands available to the ad account.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_query_creatives",
                "Query Apple Ads Platform v1 ad creatives.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_query_locations",
                "Query Apple Maps business locations for targeting.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_change_history",
                "Query Apple Ads Platform v1 change history grouped by transaction.",
                json!({"type":"object","properties":{"body":{"type":"object"}},"required":["body"]}),
            ),
            platform_tool(
                "platform_test_auth",
                "Validate Apple Ads Platform v1 credentials and ad-account access.",
                json!({"type":"object","properties":{}}),
            ),
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
        let args_map: Map<String, Value> = request.arguments.unwrap_or_default();

        match request.name.as_ref() {
            "test_auth" => {
                self.test_auth().await?;
                structured_result_with_text(&json!({"ok": true}), None)
            }
            "list_campaigns" => {
                let input: ListCampaignsInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut query = Vec::new();
                if let Some(limit) = input.limit {
                    query.push(("limit".to_string(), limit.to_string()));
                }
                if let Some(offset) = input.offset {
                    query.push(("offset".to_string(), offset.to_string()));
                }
                let v = self
                    .request_json(Method::GET, "/campaigns", query, None)
                    .await?;
                structured_result_with_text(&v, None)
            }
            "keyword_recommendations" => {
                let input: KeywordRecommendationsInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_json(
                        Method::GET,
                        "/keywords/recommendations",
                        vec![
                            ("appId".to_string(), input.app_id.to_string()),
                            (
                                "storefrontCountries".to_string(),
                                input.storefront_countries,
                            ),
                        ],
                        None,
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "report_keywords" => {
                let input: ReportInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_json(
                        Method::POST,
                        "/reports/keywords",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "report_search_terms" => {
                let input: ReportInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_json(
                        Method::POST,
                        "/reports/searchterms",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "report_campaign_keywords" => {
                let input: ReportCampaignInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let path = format!("/reports/campaigns/{}/keywords", input.campaign_id);
                let v = self
                    .request_json(Method::POST, &path, Vec::new(), Some(input.body))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "report_campaign_search_terms" => {
                let input: ReportCampaignInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let path = format!("/reports/campaigns/{}/searchterms", input.campaign_id);
                let v = self
                    .request_json(Method::POST, &path, Vec::new(), Some(input.body))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "create_campaign" => {
                let input: CreateCampaignInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_json(Method::POST, "/campaigns", Vec::new(), Some(input.body))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_request" => {
                let input: PlatformRequestInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let method = parse_platform_method(&input.method)?;
                let query = query_pairs(input.query)?;
                let v = self
                    .request_platform_json(method, &input.path, query, input.body)
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_query_campaigns" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/campaigns/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_search_term_popularity" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/insights/apps/search-term-popularity/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_impression_share" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/insights/apps/impression-share/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_recommendations" => {
                let input: PlatformRecommendationsInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let path = match input.kind.as_str() {
                    "daily-budgets" => "/recommendations/daily-budgets/query",
                    "target-cpas" => "/recommendations/target-cpas/query",
                    _ => {
                        return Err(ConnectorError::InvalidParams(
                            "Recommendation kind must be daily-budgets or target-cpas".into(),
                        ))
                    }
                };
                let v = self
                    .request_platform_json(Method::POST, path, Vec::new(), Some(input.body))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_report_apps" | "platform_report_brands" => {
                let input: PlatformReportInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let prefix = if request.name == "platform_report_apps" {
                    "/reports/apps"
                } else {
                    "/reports/business-brands"
                };
                let path = match input.level.as_str() {
                    "campaigns" | "adgroups" | "ads" | "keywords" | "searchterms" => {
                        format!("{prefix}/{}/query", input.level)
                    }
                    _ => return Err(ConnectorError::InvalidParams(
                        "Report level must be campaigns, adgroups, ads, keywords, or searchterms"
                            .into(),
                    )),
                };
                let v = self
                    .request_platform_json(Method::POST, &path, Vec::new(), Some(input.body))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_query_brands" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/business-brands/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_query_creatives" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/creatives/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_query_locations" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/locations/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_change_history" => {
                let input: PlatformBodyInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .request_platform_json(
                        Method::POST,
                        "/change-history/query",
                        Vec::new(),
                        Some(input.body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "platform_test_auth" => {
                let v = self
                    .request_platform_json(Method::GET, "/me", Vec::new(), None)
                    .await?;
                structured_result_with_text(&json!({"ok": true, "response": v}), None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = details.clone();
        if !details.is_empty() {
            let _ = FileAuthStore::new_default().save(self.name(), &details);
        }
        let mut guard = self.token.lock().await;
        *guard = None;
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self.ensure_access_token().await?;
        let _ = self
            .request_json(
                Method::GET,
                "/campaigns",
                vec![("limit".into(), "1".into())],
                None,
            )
            .await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "ad_account_id".into(),
                    label: "Apple Ads Platform Ad Account ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Apple Ads Platform v1 ad account id (ASA_AD_ACCOUNT_ID). Required for v1 tools."
                            .into(),
                    ),
                    options: None,
                },
                Field {
                    name: "org_id".into(),
                    label: "Organization ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Apple Search Ads organization id (ASA_ORG_ID). Used for X-AP-Context headers."
                            .into(),
                    ),
                    options: None,
                },
                Field {
                    name: "oauth_client_id".into(),
                    label: "OAuth Client ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Apple Search Ads OAuth client id.".into()),
                    options: None,
                },
                Field {
                    name: "team_id".into(),
                    label: "Team ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Apple Developer team id (used as iss for client_secret JWT).".into()),
                    options: None,
                },
                Field {
                    name: "key_id".into(),
                    label: "Key ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Key id (kid) for the .p8 private key.".into()),
                    options: None,
                },
                Field {
                    name: "private_key_path".into(),
                    label: "Private Key Path (.p8)".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Filesystem path to the downloaded .p8 key.".into()),
                    options: None,
                },
                Field {
                    name: "private_key_p8".into(),
                    label: "Private Key Contents (.p8 PEM)".into(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Contents of the .p8 key (prefer private_key_path).".into()),
                    options: None,
                },
            ],
        }
    }
}
