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

    async fn send_with_backoff<F>(&self, build: F) -> Result<reqwest::Response, ConnectorError>
    where
        F: Fn(&Client, &str, &str, &str) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration as TokioDuration};

        const MAX_RETRIES: usize = 5;
        let mut delay_ms = 700u64;
        for attempt in 0..=MAX_RETRIES {
            let org_id = self.org_id().ok_or_else(|| {
                ConnectorError::Authentication(
                    "Missing Apple Search Ads org id: set org_id or ASA_ORG_ID".into(),
                )
            })?;
            let (token_type, access_token) = self.ensure_access_token().await?;

            let resp = build(&self.http, &token_type, &access_token, &org_id)
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
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{ASA_BASE_URL}{}", path)
        };

        let resp = self
            .send_with_backoff(|client, token_type, access_token, org_id| {
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
            })
            .await?;

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
                _ => ConnectorError::Other(format!(
                    "Apple Search Ads API returned HTTP {}: {}",
                    status, text
                )),
            });
        }

        resp.json::<Value>()
            .await
            .map_err(ConnectorError::HttpRequest)
    }
}

#[async_trait]
impl Connector for AppleSearchAdsConnector {
    fn name(&self) -> &'static str {
        "apple-search-ads"
    }

    fn description(&self) -> &'static str {
        "Apple Search Ads API v5 (keyword recommendations and reporting)."
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
                "Configure org_id, oauth_client_id, team_id, key_id, and private_key_path \
(.p8 from Apple Search Ads). Then use `keyword_recommendations` for keyword discovery and \
`report_*` tools for metrics."
                    .to_string(),
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
