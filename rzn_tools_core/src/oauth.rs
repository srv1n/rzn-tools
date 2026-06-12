use serde::{Deserialize, Serialize};

use crate::error::ConnectorError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    pub interval: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
}

pub async fn ms_device_authorize(
    tenant_id: &str,
    client_id: &str,
    scopes: &str,
) -> Result<DeviceAuthStart, ConnectorError> {
    let url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode",
        if tenant_id.is_empty() {
            "common"
        } else {
            tenant_id
        }
    );
    let body = [
        ("client_id", client_id.to_string()),
        ("scope", scopes.to_string()),
    ];
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "device authorize failed: {}",
            v
        )));
    }
    Ok(DeviceAuthStart {
        device_code: v["device_code"].as_str().unwrap_or_default().to_string(),
        user_code: v["user_code"].as_str().unwrap_or_default().to_string(),
        verification_uri: v["verification_uri"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        verification_uri_complete: v
            .get("verification_uri_complete")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v["expires_in"].as_i64().unwrap_or(900),
        interval: v.get("interval").and_then(|i| i.as_i64()),
    })
}

pub async fn ms_device_poll(
    tenant_id: &str,
    client_id: &str,
    device_code: &str,
) -> Result<OAuthTokens, ConnectorError> {
    let url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        if tenant_id.is_empty() {
            "common"
        } else {
            tenant_id
        }
    );
    let body = [
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        ),
        ("client_id", client_id.to_string()),
        ("device_code", device_code.to_string()),
    ];
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "poll failed: {}",
            v
        )));
    }
    Ok(OAuthTokens {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v
            .get("refresh_token")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v.get("expires_in").and_then(|i| i.as_i64()),
        scope: v
            .get("scope")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        token_type: v
            .get("token_type")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
    })
}

pub async fn google_device_authorize(
    client_id: &str,
    scopes: &str,
) -> Result<DeviceAuthStart, ConnectorError> {
    let url = "https://oauth2.googleapis.com/device/code";
    let body = [
        ("client_id", client_id.to_string()),
        ("scope", scopes.to_string()),
    ];
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "device authorize failed: {}",
            v
        )));
    }
    Ok(DeviceAuthStart {
        device_code: v["device_code"].as_str().unwrap_or_default().to_string(),
        user_code: v["user_code"].as_str().unwrap_or_default().to_string(),
        verification_uri: v["verification_url"]
            .as_str()
            .or_else(|| v["verification_uri"].as_str())
            .unwrap_or_default()
            .to_string(),
        verification_uri_complete: v
            .get("verification_url_complete")
            .or_else(|| v.get("verification_uri_complete"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v["expires_in"].as_i64().unwrap_or(1800),
        interval: v.get("interval").and_then(|i| i.as_i64()),
    })
}

pub async fn google_device_poll(
    client_id: &str,
    client_secret: Option<&str>,
    device_code: &str,
) -> Result<OAuthTokens, ConnectorError> {
    let url = "https://oauth2.googleapis.com/token";
    let mut body = vec![
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        ),
        ("client_id", client_id.to_string()),
        ("device_code", device_code.to_string()),
    ];
    if let Some(cs) = client_secret {
        if !cs.is_empty() {
            body.push(("client_secret", cs.to_string()));
        }
    }
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "poll failed: {}",
            v
        )));
    }
    Ok(OAuthTokens {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v
            .get("refresh_token")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v.get("expires_in").and_then(|i| i.as_i64()),
        scope: v
            .get("scope")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        token_type: v
            .get("token_type")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
    })
}
use std::collections::HashMap;
fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn apply_expiry(mut map: HashMap<String, String>, tokens: &OAuthTokens) -> HashMap<String, String> {
    if let Some(ex) = tokens.expires_in {
        let expires_at = now_epoch() + ex - 60;
        map.insert("expires_in".to_string(), ex.to_string());
        map.insert("expires_at".to_string(), expires_at.to_string());
    }
    map
}

pub fn ensure_google_access(auth: &mut HashMap<String, String>) -> Result<String, ConnectorError> {
    if let (Some(at), Some(exp_at)) = (auth.get("access_token"), auth.get("expires_at")) {
        if exp_at.parse::<i64>().unwrap_or(0) > now_epoch() {
            return Ok(at.clone());
        }
    }
    let rt = auth
        .get("refresh_token")
        .cloned()
        .ok_or_else(|| ConnectorError::Authentication("Missing refresh_token".to_string()))?;
    let client_id = auth.get("client_id").cloned().ok_or_else(|| {
        ConnectorError::Authentication("Missing client_id for refresh".to_string())
    })?;
    let client_secret = auth.get("client_secret").cloned();
    let fut = async move { google_refresh_token(&client_id, client_secret.as_deref(), &rt).await };
    let rt_handle = tokio::runtime::Handle::try_current()
        .map_err(|e| ConnectorError::Other(format!("no runtime: {}", e)))?;
    let tokens = rt_handle.block_on(fut)?;
    auth.insert("access_token".to_string(), tokens.access_token.clone());
    if let Some(r) = tokens.refresh_token.clone() {
        auth.insert("refresh_token".to_string(), r);
    }
    let mut copied = auth.clone();
    *auth = apply_expiry(std::mem::take(&mut copied), &tokens);
    Ok(tokens.access_token)
}

pub fn ensure_ms_access(auth: &mut HashMap<String, String>) -> Result<String, ConnectorError> {
    if let (Some(at), Some(exp_at)) = (auth.get("access_token"), auth.get("expires_at")) {
        if exp_at.parse::<i64>().unwrap_or(0) > now_epoch() {
            return Ok(at.clone());
        }
    }
    let rt = auth
        .get("refresh_token")
        .cloned()
        .ok_or_else(|| ConnectorError::Authentication("Missing refresh_token".to_string()))?;
    let client_id = auth.get("client_id").cloned().ok_or_else(|| {
        ConnectorError::Authentication("Missing client_id for refresh".to_string())
    })?;
    let tenant_id = auth
        .get("tenant_id")
        .cloned()
        .unwrap_or_else(|| "common".to_string());
    let client_secret = auth.get("client_secret").cloned();
    let fut = async move {
        ms_refresh_token(&tenant_id, &client_id, client_secret.as_deref(), &rt).await
    };
    let rt_handle = tokio::runtime::Handle::try_current()
        .map_err(|e| ConnectorError::Other(format!("no runtime: {}", e)))?;
    let tokens = rt_handle.block_on(fut)?;
    auth.insert("access_token".to_string(), tokens.access_token.clone());
    if let Some(r) = tokens.refresh_token.clone() {
        auth.insert("refresh_token".to_string(), r);
    }
    let mut copied = auth.clone();
    *auth = apply_expiry(std::mem::take(&mut copied), &tokens);
    Ok(tokens.access_token)
}

pub async fn ms_refresh_token(
    tenant_id: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> Result<OAuthTokens, ConnectorError> {
    let url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        if tenant_id.is_empty() {
            "common"
        } else {
            tenant_id
        }
    );
    let mut body = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id.to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(s) = client_secret {
        if !s.is_empty() {
            body.push(("client_secret", s.to_string()));
        }
    }
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "refresh failed: {}",
            v
        )));
    }
    Ok(OAuthTokens {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v
            .get("refresh_token")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v.get("expires_in").and_then(|i| i.as_i64()),
        scope: v
            .get("scope")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        token_type: v
            .get("token_type")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
    })
}

pub async fn google_refresh_token(
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> Result<OAuthTokens, ConnectorError> {
    let url = "https://oauth2.googleapis.com/token";
    let mut body = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id.to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(cs) = client_secret {
        if !cs.is_empty() {
            body.push(("client_secret", cs.to_string()));
        }
    }
    let resp = reqwest::Client::new()
        .post(url)
        .form(&body)
        .send()
        .await
        .map_err(ConnectorError::HttpRequest)?;
    let status = resp.status();
    let v = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ConnectorError::Other(e.to_string()))?;
    if !status.is_success() {
        return Err(ConnectorError::Authentication(format!(
            "refresh failed: {}",
            v
        )));
    }
    Ok(OAuthTokens {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v
            .get("refresh_token")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        expires_in: v.get("expires_in").and_then(|i| i.as_i64()),
        scope: v
            .get("scope")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        token_type: v
            .get("token_type")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
    })
}
