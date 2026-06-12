use async_trait::async_trait;
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::{Client, Method, StatusCode};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::io::Read as _;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

const ASC_BASE_URL: &str = "https://api.appstoreconnect.apple.com/v1";
const ASC_AUDIENCE: &str = "appstoreconnect-v1";

// Keep this stable to avoid causing App Store Connect WAF false-positives.
const ASC_USER_AGENT: &str = "rzn-tools/app-store-connect";

#[derive(Debug, Deserialize)]
struct AscErrorResponse {
    #[serde(default)]
    errors: Vec<AscError>,
}

#[derive(Debug, Deserialize)]
struct AscError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

impl AscErrorResponse {
    fn summarize(&self) -> Option<String> {
        let first = self.errors.first()?;
        let mut parts = Vec::new();
        if let Some(code) = first.code.as_ref() {
            parts.push(code.as_str());
        }
        if let Some(title) = first.title.as_ref() {
            parts.push(title.as_str());
        }
        if let Some(detail) = first.detail.as_ref() {
            parts.push(detail.as_str());
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(": "))
        }
    }
}

#[derive(Debug, Serialize)]
struct DownloadPreview {
    truncated: bool,
    compressed_kb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    uncompressed_kb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gzip: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_preview: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListAppsInput {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    filter_name: Option<String>,
    #[serde(default)]
    filter_bundle_id: Option<String>,
    #[serde(default)]
    filter_sku: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetAppInput {
    app_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateAnalyticsReportRequestInput {
    app_id: String,
    #[serde(default)]
    access_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListAnalyticsReportsInput {
    report_request_id: String,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    filter_category: Option<String>,
    #[serde(default)]
    filter_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListAnalyticsReportInstancesInput {
    report_id: String,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    filter_processing_date: Option<String>,
    #[serde(default)]
    filter_granularity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListAnalyticsReportSegmentsInput {
    instance_id: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DownloadAnalyticsSegmentInput {
    #[serde(default)]
    segment_url: Option<String>,
    #[serde(default)]
    segment_id: Option<String>,
    #[serde(default)]
    max_kb: Option<u64>,
    #[serde(default)]
    max_uncompressed_kb: Option<u64>,
    #[serde(default)]
    max_rows: Option<usize>,
    #[serde(default)]
    max_preview_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DownloadSalesReportInput {
    #[serde(default)]
    vendor_number: Option<String>,
    #[serde(default)]
    report_type: Option<String>,
    #[serde(default)]
    report_sub_type: Option<String>,
    #[serde(default)]
    frequency: Option<String>,
    #[serde(default)]
    report_date: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    max_kb: Option<u64>,
    #[serde(default)]
    max_uncompressed_kb: Option<u64>,
    #[serde(default)]
    max_rows: Option<usize>,
    #[serde(default)]
    max_preview_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DownloadFinanceReportInput {
    #[serde(default)]
    vendor_number: Option<String>,
    #[serde(default)]
    report_type: Option<String>,
    report_date: String,
    region_code: String,
    #[serde(default)]
    max_kb: Option<u64>,
    #[serde(default)]
    max_uncompressed_kb: Option<u64>,
    #[serde(default)]
    max_rows: Option<usize>,
    #[serde(default)]
    max_preview_chars: Option<usize>,
}

#[derive(Clone, Default)]
pub struct AppStoreConnectConnector {
    auth: AuthDetails,
    client: Client,
}

impl AppStoreConnectConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(ASC_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(ConnectorError::HttpRequest)?;

        Ok(Self { auth, client })
    }

    fn key_id(&self) -> Option<String> {
        self.auth
            .get("key_id")
            .cloned()
            .or_else(|| std::env::var("APP_STORE_CONNECT_KEY_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("key_id").cloned())
            })
    }

    fn issuer_id(&self) -> Option<String> {
        self.auth
            .get("issuer_id")
            .cloned()
            .or_else(|| std::env::var("APP_STORE_CONNECT_ISSUER_ID").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("issuer_id").cloned())
            })
    }

    fn vendor_number(&self) -> Option<String> {
        self.auth
            .get("vendor_number")
            .cloned()
            .or_else(|| std::env::var("APP_STORE_CONNECT_VENDOR_NUMBER").ok())
            .or_else(|| {
                FileAuthStore::new_default()
                    .load(self.name())
                    .and_then(|m| m.get("vendor_number").cloned())
            })
    }

    fn private_key_p8(&self) -> Option<String> {
        self.auth
            .get("private_key_p8")
            .cloned()
            .or_else(|| std::env::var("APP_STORE_CONNECT_PRIVATE_KEY_P8").ok())
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
            .or_else(|| std::env::var("APP_STORE_CONNECT_P8_PATH").ok())
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
            "Missing App Store Connect private key: set private_key_p8 or private_key_path".into(),
        ))
    }

    fn jwt_token(&self) -> Result<String, ConnectorError> {
        let key_id = self.key_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing App Store Connect Key ID: set key_id or APP_STORE_CONNECT_KEY_ID".into(),
            )
        })?;
        let issuer_id = self.issuer_id().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing App Store Connect Issuer ID: set issuer_id or APP_STORE_CONNECT_ISSUER_ID"
                    .into(),
            )
        })?;

        let pem = self.load_private_key_pem()?;
        let encoding_key = EncodingKey::from_ec_pem(pem.as_bytes()).map_err(|e| {
            ConnectorError::Authentication(format!(
                "Invalid App Store Connect private key (expected .p8 / PEM EC key): {e}"
            ))
        })?;

        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            aud: &'static str,
            exp: usize,
            iat: usize,
        }

        let now = Utc::now().timestamp();
        let exp = (Utc::now() + Duration::minutes(20)).timestamp();
        let header = Header {
            alg: Algorithm::ES256,
            kid: Some(key_id),
            ..Header::default()
        };
        let claims = Claims {
            iss: &issuer_id,
            aud: ASC_AUDIENCE,
            exp: exp as usize,
            iat: now as usize,
        };

        encode(&header, &claims, &encoding_key)
            .map_err(|e| ConnectorError::Authentication(format!("Failed to sign JWT: {e}")))
    }

    async fn send_with_backoff<F>(
        &self,
        build: F,
        accept: Option<&'static str>,
    ) -> Result<reqwest::Response, ConnectorError>
    where
        F: Fn(&Client, String) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration as TokioDuration};

        const MAX_RETRIES: usize = 4;
        let mut delay_ms = 700u64;
        for attempt in 0..=MAX_RETRIES {
            let token = self.jwt_token()?;
            let mut req = build(&self.client, token);
            if let Some(accept_value) = accept {
                req = req.header(ACCEPT, accept_value);
            }

            let resp = req.send().await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::TOO_MANY_REQUESTS
                        || (status.is_server_error() && attempt < MAX_RETRIES)
                    {
                        let retry_after = r
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok());

                        if attempt == MAX_RETRIES {
                            return Ok(r);
                        }

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
        let url = format!("{ASC_BASE_URL}{}", path);
        let resp = self
            .send_with_backoff(
                |client, token| {
                    let mut b = client
                        .request(method.clone(), &url)
                        .header(AUTHORIZATION, format!("Bearer {token}"));
                    if !query.is_empty() {
                        b = b.query(&query);
                    }
                    if let Some(ref json_body) = body {
                        b = b.header(CONTENT_TYPE, "application/json").json(json_body);
                    }
                    b
                },
                Some("application/json"),
            )
            .await?;

        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }

        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        if status.is_success() {
            let v: Value =
                serde_json::from_str(&text).map_err(|e| ConnectorError::Other(e.to_string()))?;
            return Ok(v);
        }

        let summary = serde_json::from_str::<AscErrorResponse>(&text)
            .ok()
            .and_then(|e| e.summarize())
            .unwrap_or_else(|| text.clone());

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err(ConnectorError::Authentication(summary))
            }
            _ => Err(ConnectorError::Other(format!(
                "App Store Connect API returned HTTP {}: {}",
                status, summary
            ))),
        }
    }

    fn tsv_to_rows(text: &str, max_rows: usize) -> (Option<Vec<String>>, Vec<Value>) {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let header_line = lines.next();
        let header = header_line
            .map(|h| h.trim_end_matches('\r'))
            .filter(|h| h.contains('\t'))
            .map(|h| h.split('\t').map(|s| s.to_string()).collect::<Vec<_>>());

        let mut rows = Vec::new();
        for line in lines.take(max_rows) {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let vals = line.split('\t').map(|s| Value::String(s.to_string()));
            if let Some(ref cols) = header {
                let mut obj = Map::new();
                for (idx, col) in cols.iter().enumerate() {
                    let v = line
                        .split('\t')
                        .nth(idx)
                        .map(|s| Value::String(s.to_string()))
                        .unwrap_or(Value::Null);
                    obj.insert(col.clone(), v);
                }
                rows.push(Value::Object(obj));
            } else {
                rows.push(Value::Array(vals.collect()));
            }
        }
        (header, rows)
    }

    fn is_gzip(bytes: &[u8]) -> bool {
        bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
    }

    fn gunzip_limited(
        gz: &[u8],
        max_uncompressed_bytes: usize,
    ) -> Result<(Vec<u8>, bool), ConnectorError> {
        let mut decoder = flate2::read::GzDecoder::new(gz);
        let mut out = Vec::new();
        decoder
            .by_ref()
            .take((max_uncompressed_bytes as u64).saturating_add(1))
            .read_to_end(&mut out)
            .map_err(ConnectorError::Io)?;
        let truncated = out.len() > max_uncompressed_bytes;
        if truncated {
            out.truncate(max_uncompressed_bytes);
        }
        Ok((out, truncated))
    }

    async fn download_preview_from_url(
        &self,
        url: &str,
        max_kb: u64,
        max_uncompressed_kb: u64,
        max_rows: usize,
        max_preview_chars: usize,
    ) -> Result<DownloadPreview, ConnectorError> {
        let resp = self
            .send_with_backoff(
                |client, token| {
                    client
                        .get(url)
                        .header(AUTHORIZATION, format!("Bearer {token}"))
                },
                None,
            )
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let maybe_text = resp.text().await.unwrap_or_default();
            let summary = serde_json::from_str::<AscErrorResponse>(&maybe_text)
                .ok()
                .and_then(|e| e.summarize())
                .unwrap_or(maybe_text);
            return Err(match status {
                StatusCode::NOT_FOUND => ConnectorError::ResourceNotFound,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    ConnectorError::Authentication(summary)
                }
                _ => ConnectorError::Other(format!("Download failed (HTTP {status}): {summary}")),
            });
        }

        let max_bytes = max_kb.saturating_mul(1024) as usize;
        if let Some(len) = resp.content_length() {
            if len > max_bytes as u64 {
                return Ok(DownloadPreview {
                    truncated: true,
                    compressed_kb: (len.div_ceil(1024)),
                    uncompressed_kb: None,
                    gzip: None,
                    header: None,
                    rows: None,
                    text_preview: None,
                });
            }
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(ConnectorError::HttpRequest)?
            .to_vec();
        let compressed_kb = (bytes.len() as u64).div_ceil(1024);
        if bytes.len() > max_bytes {
            return Ok(DownloadPreview {
                truncated: true,
                compressed_kb,
                uncompressed_kb: None,
                gzip: None,
                header: None,
                rows: None,
                text_preview: None,
            });
        }

        let gzip = Self::is_gzip(&bytes);
        if gzip {
            let max_uncompressed_bytes = max_uncompressed_kb.saturating_mul(1024) as usize;
            let (unzipped, truncated_unzipped) =
                Self::gunzip_limited(&bytes, max_uncompressed_bytes)?;
            let text = String::from_utf8_lossy(&unzipped).to_string();
            let (header, rows) = Self::tsv_to_rows(&text, max_rows);
            let preview = text.chars().take(max_preview_chars).collect::<String>();
            return Ok(DownloadPreview {
                truncated: truncated_unzipped,
                compressed_kb,
                uncompressed_kb: Some((unzipped.len() as u64).div_ceil(1024)),
                gzip: Some(true),
                header,
                rows: Some(rows),
                text_preview: Some(preview),
            });
        }

        let text = String::from_utf8_lossy(&bytes).to_string();
        let preview = text.chars().take(max_preview_chars).collect::<String>();
        Ok(DownloadPreview {
            truncated: false,
            compressed_kb,
            uncompressed_kb: None,
            gzip: Some(false),
            header: None,
            rows: None,
            text_preview: Some(preview),
        })
    }
}

#[async_trait]
impl Connector for AppStoreConnectConnector {
    fn name(&self) -> &'static str {
        "app-store-connect"
    }

    fn description(&self) -> &'static str {
        "App Store Connect API (apps, App Analytics reports, Sales & Finance reports)."
    }

    fn display_name(&self) -> &'static str {
        "App Store Connect"
    }

    fn icon(&self) -> &'static str {
        "app_store"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["app_store", "developer", "analytics"]
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
                "Create an App Store Connect API key in App Store Connect > Users and Access > \
Keys. Configure `key_id`, `issuer_id`, and `private_key_path` (path to the downloaded .p8) \
or `private_key_p8` (contents). Optionally set `vendor_number` for Sales/Finance reports."
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
                description: Some(Cow::Borrowed("Validate credentials (JWT signing + API access).")),
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
                name: Cow::Borrowed("list_apps"),
                title: None,
                description: Some(Cow::Borrowed("List apps in your App Store Connect account.")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":100},
                            "filter_name":{"type":"string","description":"Maps to filter[name]"},
                            "filter_bundle_id":{"type":"string","description":"Maps to filter[bundleId]"},
                            "filter_sku":{"type":"string","description":"Maps to filter[sku]"}
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
                name: Cow::Borrowed("get_app"),
                title: None,
                description: Some(Cow::Borrowed("Get a single app by App Store Connect app id.")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{"app_id":{"type":"string"}},
                        "required":["app_id"]
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
                name: Cow::Borrowed("create_analytics_report_request"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Create an Analytics report request for an app (ONE_TIME_SNAPSHOT or ONGOING).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "app_id":{"type":"string"},
                            "access_type":{"type":"string","enum":["ONE_TIME_SNAPSHOT","ONGOING"],"default":"ONE_TIME_SNAPSHOT"}
                        },
                        "required":["app_id"]
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
                name: Cow::Borrowed("list_analytics_reports"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List Analytics reports generated for a report request id.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "report_request_id":{"type":"string"},
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":100},
                            "filter_category":{"type":"string","enum":["APP_USAGE","APP_STORE_ENGAGEMENT","COMMERCE","FRAMEWORK_USAGE","PERFORMANCE"]},
                            "filter_name":{"type":"string"}
                        },
                        "required":["report_request_id"]
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
                name: Cow::Borrowed("list_analytics_report_instances"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List Analytics report instances for a report id (filter by processing date and granularity).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "report_id":{"type":"string"},
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":100},
                            "filter_processing_date":{"type":"string","description":"YYYY-MM-DD"},
                            "filter_granularity":{"type":"string","enum":["DAILY","WEEKLY","MONTHLY"]}
                        },
                        "required":["report_id"]
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
                name: Cow::Borrowed("list_analytics_report_segments"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List downloadable segments for an Analytics report instance id.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "instance_id":{"type":"string"},
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":100}
                        },
                        "required":["instance_id"]
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
                name: Cow::Borrowed("download_analytics_report_segment"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Download a report segment URL (usually a gzip TSV). Returns a bounded preview.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "segment_url":{"type":"string"},
                            "segment_id":{"type":"string","description":"Alternative to segment_url (will resolve via /analyticsReportSegments/{id})"},
                            "max_kb":{"type":"integer","description":"Max compressed KB to download (default 1024)"},
                            "max_uncompressed_kb":{"type":"integer","description":"Max uncompressed KB to parse (default 2048)"},
                            "max_rows":{"type":"integer","description":"Max TSV rows to parse into structured objects (default 200)"},
                            "max_preview_chars":{"type":"integer","description":"Max characters for text_preview (default 6000)"}
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
                name: Cow::Borrowed("download_sales_report"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Download a Sales report (gzip TSV). Requires vendor_number (arg or config).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "vendor_number":{"type":"string"},
                            "report_type":{"type":"string","enum":["SALES","PRE_ORDER","NEWSSTAND","SUBSCRIPTION","SUBSCRIPTION_EVENT","SUBSCRIBER","SUBSCRIPTION_OFFER_CODE_REDEMPTION","INSTALLS","FIRST_ANNUAL","WIN_BACK_ELIGIBILITY"],"default":"SALES"},
                            "report_sub_type":{"type":"string","enum":["SUMMARY","DETAILED","SUMMARY_INSTALL_TYPE","SUMMARY_TERRITORY","SUMMARY_CHANNEL"],"default":"SUMMARY"},
                            "frequency":{"type":"string","enum":["DAILY","WEEKLY","MONTHLY","YEARLY"],"default":"MONTHLY"},
                            "report_date":{"type":"string","description":"YYYY-MM-DD (optional in API; recommended)"},
                            "version":{"type":"string"},
                            "max_kb":{"type":"integer","description":"Max compressed KB to download (default 1024)"},
                            "max_uncompressed_kb":{"type":"integer","description":"Max uncompressed KB to parse (default 2048)"},
                            "max_rows":{"type":"integer","description":"Max TSV rows to parse into structured objects (default 200)"},
                            "max_preview_chars":{"type":"integer","description":"Max characters for text_preview (default 6000)"}
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
                name: Cow::Borrowed("download_finance_report"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Download a Finance report (gzip TSV). Requires vendor_number (arg or config).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "vendor_number":{"type":"string"},
                            "report_type":{"type":"string","enum":["FINANCIAL","FINANCE_DETAIL"],"default":"FINANCIAL"},
                            "report_date":{"type":"string","description":"YYYY-MM-DD"},
                            "region_code":{"type":"string","description":"e.g. US"},
                            "max_kb":{"type":"integer","description":"Max compressed KB to download (default 1024)"},
                            "max_uncompressed_kb":{"type":"integer","description":"Max uncompressed KB to parse (default 2048)"},
                            "max_rows":{"type":"integer","description":"Max TSV rows to parse into structured objects (default 200)"},
                            "max_preview_chars":{"type":"integer","description":"Max characters for text_preview (default 6000)"}
                        },
                        "required":["report_date","region_code"]
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
        let args_map = request.arguments.unwrap_or_default();

        match request.name.as_ref() {
            "test_auth" => {
                self.test_auth().await?;
                structured_result_with_text(&json!({"ok": true}), None)
            }
            "list_apps" => {
                let input: ListAppsInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut query = Vec::new();
                if let Some(limit) = input.limit {
                    query.push(("limit".to_string(), limit.to_string()));
                }
                if let Some(name) = input.filter_name {
                    query.push(("filter[name]".to_string(), name));
                }
                if let Some(bundle_id) = input.filter_bundle_id {
                    query.push(("filter[bundleId]".to_string(), bundle_id));
                }
                if let Some(sku) = input.filter_sku {
                    query.push(("filter[sku]".to_string(), sku));
                }
                let v = self.request_json(Method::GET, "/apps", query, None).await?;
                structured_result_with_text(&v, None)
            }
            "get_app" => {
                let input: GetAppInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let path = format!("/apps/{}", input.app_id);
                let v = self
                    .request_json(Method::GET, &path, Vec::new(), None)
                    .await?;
                structured_result_with_text(&v, None)
            }
            "create_analytics_report_request" => {
                let input: CreateAnalyticsReportRequestInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let access_type = input
                    .access_type
                    .unwrap_or_else(|| "ONE_TIME_SNAPSHOT".to_string());
                let body = json!({
                    "data":{
                        "type":"analyticsReportRequests",
                        "attributes":{"accessType": access_type},
                        "relationships":{
                            "app":{"data":{"type":"apps","id": input.app_id}}
                        }
                    }
                });
                let v = self
                    .request_json(
                        Method::POST,
                        "/analyticsReportRequests",
                        Vec::new(),
                        Some(body),
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "list_analytics_reports" => {
                let input: ListAnalyticsReportsInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut query = Vec::new();
                if let Some(limit) = input.limit {
                    query.push(("limit".to_string(), limit.to_string()));
                }
                if let Some(cat) = input.filter_category {
                    query.push(("filter[category]".to_string(), cat));
                }
                if let Some(name) = input.filter_name {
                    query.push(("filter[name]".to_string(), name));
                }
                let path = format!(
                    "/analyticsReportRequests/{}/reports",
                    input.report_request_id
                );
                let v = self.request_json(Method::GET, &path, query, None).await?;
                structured_result_with_text(&v, None)
            }
            "list_analytics_report_instances" => {
                let input: ListAnalyticsReportInstancesInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut query = Vec::new();
                if let Some(limit) = input.limit {
                    query.push(("limit".to_string(), limit.to_string()));
                }
                if let Some(d) = input.filter_processing_date {
                    query.push(("filter[processingDate]".to_string(), d));
                }
                if let Some(g) = input.filter_granularity {
                    query.push(("filter[granularity]".to_string(), g));
                }
                let path = format!("/analyticsReports/{}/instances", input.report_id);
                let v = self.request_json(Method::GET, &path, query, None).await?;
                structured_result_with_text(&v, None)
            }
            "list_analytics_report_segments" => {
                let input: ListAnalyticsReportSegmentsInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut query = Vec::new();
                if let Some(limit) = input.limit {
                    query.push(("limit".to_string(), limit.to_string()));
                }
                let path = format!("/analyticsReportInstances/{}/segments", input.instance_id);
                let v = self.request_json(Method::GET, &path, query, None).await?;
                structured_result_with_text(&v, None)
            }
            "download_analytics_report_segment" => {
                let input: DownloadAnalyticsSegmentInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let max_kb = input.max_kb.unwrap_or(1024);
                let max_uncompressed_kb = input.max_uncompressed_kb.unwrap_or(2048);
                let max_rows = input.max_rows.unwrap_or(200);
                let max_preview_chars = input.max_preview_chars.unwrap_or(6000);

                let url = if let Some(u) = input.segment_url {
                    u
                } else if let Some(id) = input.segment_id {
                    let path = format!("/analyticsReportSegments/{id}");
                    let v = self
                        .request_json(Method::GET, &path, Vec::new(), None)
                        .await?;
                    v.get("data")
                        .and_then(|d| d.get("attributes"))
                        .and_then(|a| a.get("url"))
                        .and_then(|u| u.as_str())
                        .ok_or_else(|| {
                            ConnectorError::Other("Segment did not include attributes.url".into())
                        })?
                        .to_string()
                } else {
                    return Err(ConnectorError::InvalidParams(
                        "Provide segment_url or segment_id".into(),
                    ));
                };

                let preview = self
                    .download_preview_from_url(
                        &url,
                        max_kb,
                        max_uncompressed_kb,
                        max_rows,
                        max_preview_chars,
                    )
                    .await?;
                structured_result_with_text(&preview, None)
            }
            "download_sales_report" => {
                let input: DownloadSalesReportInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let vendor_number = input
                    .vendor_number
                    .or_else(|| self.vendor_number())
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams(
                            "Missing vendor_number (provide as argument or set vendor_number / APP_STORE_CONNECT_VENDOR_NUMBER)".into(),
                        )
                    })?;
                let report_type = input.report_type.unwrap_or_else(|| "SALES".to_string());
                let report_sub_type = input
                    .report_sub_type
                    .unwrap_or_else(|| "SUMMARY".to_string());
                let frequency = input.frequency.unwrap_or_else(|| "MONTHLY".to_string());

                let mut query = vec![
                    ("filter[vendorNumber]".to_string(), vendor_number),
                    ("filter[reportType]".to_string(), report_type),
                    ("filter[reportSubType]".to_string(), report_sub_type),
                    ("filter[frequency]".to_string(), frequency),
                ];
                if let Some(d) = input.report_date {
                    query.push(("filter[reportDate]".to_string(), d));
                }
                if let Some(v) = input.version {
                    query.push(("filter[version]".to_string(), v));
                }

                let url = format!("{ASC_BASE_URL}/salesReports");
                let max_kb = input.max_kb.unwrap_or(1024);
                let max_uncompressed_kb = input.max_uncompressed_kb.unwrap_or(2048);
                let max_rows = input.max_rows.unwrap_or(200);
                let max_preview_chars = input.max_preview_chars.unwrap_or(6000);

                let resp = self
                    .send_with_backoff(
                        |client, token| {
                            let mut b = client
                                .request(Method::GET, &url)
                                .header(AUTHORIZATION, format!("Bearer {token}"))
                                .header(ACCEPT, "application/a-gzip");
                            b = b.query(&query);
                            b
                        },
                        Some("application/a-gzip"),
                    )
                    .await?;

                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let summary = serde_json::from_str::<AscErrorResponse>(&text)
                        .ok()
                        .and_then(|e| e.summarize())
                        .unwrap_or(text);
                    return Err(match status {
                        StatusCode::NOT_FOUND => ConnectorError::ResourceNotFound,
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                            ConnectorError::Authentication(summary)
                        }
                        _ => ConnectorError::Other(format!(
                            "Sales report failed (HTTP {status}): {summary}"
                        )),
                    });
                }

                let max_bytes = max_kb.saturating_mul(1024) as usize;
                if let Some(len) = resp.content_length() {
                    if len > max_bytes as u64 {
                        return structured_result_with_text(
                            &DownloadPreview {
                                truncated: true,
                                compressed_kb: len.div_ceil(1024),
                                uncompressed_kb: None,
                                gzip: None,
                                header: None,
                                rows: None,
                                text_preview: None,
                            },
                            None,
                        );
                    }
                }

                let bytes = resp
                    .bytes()
                    .await
                    .map_err(ConnectorError::HttpRequest)?
                    .to_vec();
                let compressed_kb = (bytes.len() as u64).div_ceil(1024);
                if bytes.len() > max_bytes {
                    return structured_result_with_text(
                        &DownloadPreview {
                            truncated: true,
                            compressed_kb,
                            uncompressed_kb: None,
                            gzip: None,
                            header: None,
                            rows: None,
                            text_preview: None,
                        },
                        None,
                    );
                }

                if !Self::is_gzip(&bytes) {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    return structured_result_with_text(
                        &DownloadPreview {
                            truncated: false,
                            compressed_kb,
                            uncompressed_kb: None,
                            gzip: Some(false),
                            header: None,
                            rows: None,
                            text_preview: Some(text.chars().take(max_preview_chars).collect()),
                        },
                        None,
                    );
                }

                let max_uncompressed_bytes = max_uncompressed_kb.saturating_mul(1024) as usize;
                let (unzipped, truncated_unzipped) =
                    Self::gunzip_limited(&bytes, max_uncompressed_bytes)?;
                let text = String::from_utf8_lossy(&unzipped).to_string();
                let (header, rows) = Self::tsv_to_rows(&text, max_rows);

                structured_result_with_text(
                    &DownloadPreview {
                        truncated: truncated_unzipped,
                        compressed_kb,
                        uncompressed_kb: Some((unzipped.len() as u64).div_ceil(1024)),
                        gzip: Some(true),
                        header,
                        rows: Some(rows),
                        text_preview: Some(text.chars().take(max_preview_chars).collect()),
                    },
                    None,
                )
            }
            "download_finance_report" => {
                let input: DownloadFinanceReportInput =
                    serde_json::from_value(Value::Object(args_map))
                        .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let vendor_number = input
                    .vendor_number
                    .or_else(|| self.vendor_number())
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams(
                            "Missing vendor_number (provide as argument or set vendor_number / APP_STORE_CONNECT_VENDOR_NUMBER)".into(),
                        )
                    })?;
                let report_type = input.report_type.unwrap_or_else(|| "FINANCIAL".to_string());

                let query = vec![
                    ("filter[vendorNumber]".to_string(), vendor_number),
                    ("filter[reportType]".to_string(), report_type),
                    ("filter[regionCode]".to_string(), input.region_code),
                    ("filter[reportDate]".to_string(), input.report_date),
                ];

                let url = format!("{ASC_BASE_URL}/financeReports");
                let max_kb = input.max_kb.unwrap_or(1024);
                let max_uncompressed_kb = input.max_uncompressed_kb.unwrap_or(2048);
                let max_rows = input.max_rows.unwrap_or(200);
                let max_preview_chars = input.max_preview_chars.unwrap_or(6000);

                let resp = self
                    .send_with_backoff(
                        |client, token| {
                            client
                                .request(Method::GET, &url)
                                .header(AUTHORIZATION, format!("Bearer {token}"))
                                .header(ACCEPT, "application/a-gzip")
                                .query(&query)
                        },
                        Some("application/a-gzip"),
                    )
                    .await?;

                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    let summary = serde_json::from_str::<AscErrorResponse>(&text)
                        .ok()
                        .and_then(|e| e.summarize())
                        .unwrap_or(text);
                    return Err(match status {
                        StatusCode::NOT_FOUND => ConnectorError::ResourceNotFound,
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                            ConnectorError::Authentication(summary)
                        }
                        _ => ConnectorError::Other(format!(
                            "Finance report failed (HTTP {status}): {summary}"
                        )),
                    });
                }

                let max_bytes = max_kb.saturating_mul(1024) as usize;
                if let Some(len) = resp.content_length() {
                    if len > max_bytes as u64 {
                        return structured_result_with_text(
                            &DownloadPreview {
                                truncated: true,
                                compressed_kb: len.div_ceil(1024),
                                uncompressed_kb: None,
                                gzip: None,
                                header: None,
                                rows: None,
                                text_preview: None,
                            },
                            None,
                        );
                    }
                }

                let bytes = resp
                    .bytes()
                    .await
                    .map_err(ConnectorError::HttpRequest)?
                    .to_vec();
                let compressed_kb = (bytes.len() as u64).div_ceil(1024);
                if bytes.len() > max_bytes {
                    return structured_result_with_text(
                        &DownloadPreview {
                            truncated: true,
                            compressed_kb,
                            uncompressed_kb: None,
                            gzip: None,
                            header: None,
                            rows: None,
                            text_preview: None,
                        },
                        None,
                    );
                }

                if !Self::is_gzip(&bytes) {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    return structured_result_with_text(
                        &DownloadPreview {
                            truncated: false,
                            compressed_kb,
                            uncompressed_kb: None,
                            gzip: Some(false),
                            header: None,
                            rows: None,
                            text_preview: Some(text.chars().take(max_preview_chars).collect()),
                        },
                        None,
                    );
                }

                let max_uncompressed_bytes = max_uncompressed_kb.saturating_mul(1024) as usize;
                let (unzipped, truncated_unzipped) =
                    Self::gunzip_limited(&bytes, max_uncompressed_bytes)?;
                let text = String::from_utf8_lossy(&unzipped).to_string();
                let (header, rows) = Self::tsv_to_rows(&text, max_rows);

                structured_result_with_text(
                    &DownloadPreview {
                        truncated: truncated_unzipped,
                        compressed_kb,
                        uncompressed_kb: Some((unzipped.len() as u64).div_ceil(1024)),
                        gzip: Some(true),
                        header,
                        rows: Some(rows),
                        text_preview: Some(text.chars().take(max_preview_chars).collect()),
                    },
                    None,
                )
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
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Cheap validation: sign a JWT, then try to list 1 app.
        let _ = self.jwt_token()?;
        let _ = self
            .request_json(
                Method::GET,
                "/apps",
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
                    name: "key_id".into(),
                    label: "Key ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "App Store Connect API Key ID (Users and Access > Keys).".into(),
                    ),
                    options: None,
                },
                Field {
                    name: "issuer_id".into(),
                    label: "Issuer ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "App Store Connect Issuer ID for your team (Users and Access > Keys)."
                            .into(),
                    ),
                    options: None,
                },
                Field {
                    name: "private_key_path".into(),
                    label: "Private Key Path (.p8)".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Filesystem path to the downloaded .p8 key (BEGIN PRIVATE KEY).".into(),
                    ),
                    options: None,
                },
                Field {
                    name: "private_key_p8".into(),
                    label: "Private Key Contents (.p8 PEM)".into(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Contents of the .p8 key. Prefer private_key_path to avoid multi-line secrets in config.".into(),
                    ),
                    options: None,
                },
                Field {
                    name: "vendor_number".into(),
                    label: "Vendor Number".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Optional default vendor number for Sales/Finance reports.".into(),
                    ),
                    options: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppStoreConnectConnector;
    use std::io::Write as _;

    #[test]
    fn is_gzip_magic_bytes() {
        assert!(AppStoreConnectConnector::is_gzip(&[0x1f, 0x8b, 0x08]));
        assert!(!AppStoreConnectConnector::is_gzip(&[0x1f, 0x00, 0x8b]));
    }

    #[test]
    fn gunzip_limited_truncates() {
        let input = "col1\tcol2\n1\t2\n3\t4\n";
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(input.as_bytes()).expect("write gzip");
        let gz = enc.finish().expect("finish gzip");

        let (out, truncated) = AppStoreConnectConnector::gunzip_limited(&gz, 5).expect("gunzip");
        assert!(truncated);
        assert_eq!(out, input.as_bytes()[..5].to_vec());
    }

    #[test]
    fn tsv_to_rows_parses_header_and_rows() {
        let input = "a\tb\n1\t2\n3\t4\n";
        let (header, rows) = AppStoreConnectConnector::tsv_to_rows(input, 10);
        assert_eq!(header.unwrap(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(rows.len(), 2);

        let r0 = rows[0].as_object().expect("row object");
        assert_eq!(r0.get("a").and_then(|v| v.as_str()), Some("1"));
        assert_eq!(r0.get("b").and_then(|v| v.as_str()), Some("2"));
    }
}
