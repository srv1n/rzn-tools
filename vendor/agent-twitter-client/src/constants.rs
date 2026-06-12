use crate::error::{Result, TwitterError};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Client;

pub const BEARER_TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

const X_BEARER_TOKEN_ENV: &str = "X_WEB_BEARER_TOKEN";
const X_HOME_URL: &str = "https://x.com";
const X_ASSET_HOST: &str = "https://abs.twimg.com";
const X_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";

lazy_static! {
    static ref CLIENT_WEB_MAIN_BUNDLE_RE: Regex = Regex::new(
        r#"(https://abs\.twimg\.com/responsive-web/client-web/main\.[^"' ]+\.js|/responsive-web/client-web/main\.[^"' ]+\.js)"#
    )
    .unwrap();
    static ref BEARER_TOKEN_RE: Regex = Regex::new(r#"AAAA[A-Za-z0-9%]{80,200}"#).unwrap();
}

pub async fn resolve_bearer_token() -> String {
    if let Ok(token) = std::env::var(X_BEARER_TOKEN_ENV) {
        let token = token.trim();
        if !token.is_empty() {
            return token.to_string();
        }
    }

    match fetch_live_bearer_token().await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(
                "Failed to fetch live X web bearer token, falling back to bundled token: {}",
                error
            );
            BEARER_TOKEN.to_string()
        }
    }
}

async fn fetch_live_bearer_token() -> Result<String> {
    let client = Client::builder().user_agent(X_USER_AGENT).build()?;
    let home_html = client
        .get(X_HOME_URL)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let bundle_url = extract_client_web_bundle_url(&home_html)
        .ok_or_else(|| TwitterError::Auth("Failed to locate X main web bundle".into()))?;
    let bundle_js = client
        .get(&bundle_url)
        .header("Accept", "*/*")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    extract_bearer_token(&bundle_js)
        .ok_or_else(|| TwitterError::Auth("Failed to extract X bearer token from web bundle".into()))
}

fn extract_client_web_bundle_url(html: &str) -> Option<String> {
    CLIENT_WEB_MAIN_BUNDLE_RE.find(html).map(|match_| {
        let bundle_url = match_.as_str();
        if bundle_url.starts_with("http") {
            bundle_url.to_string()
        } else {
            format!("{X_ASSET_HOST}{bundle_url}")
        }
    })
}

fn extract_bearer_token(bundle_js: &str) -> Option<String> {
    BEARER_TOKEN_RE
        .find(bundle_js)
        .map(|match_| match_.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::{extract_bearer_token, extract_client_web_bundle_url};

    #[test]
    fn extracts_absolute_main_bundle_url() {
        let html = r#"<script src="https://abs.twimg.com/responsive-web/client-web/main.5a7b24ea.js"></script>"#;
        assert_eq!(
            extract_client_web_bundle_url(html).as_deref(),
            Some("https://abs.twimg.com/responsive-web/client-web/main.5a7b24ea.js")
        );
    }

    #[test]
    fn extracts_relative_main_bundle_url() {
        let html = r#"<script src="/responsive-web/client-web/main.5a7b24ea.js"></script>"#;
        assert_eq!(
            extract_client_web_bundle_url(html).as_deref(),
            Some("https://abs.twimg.com/responsive-web/client-web/main.5a7b24ea.js")
        );
    }

    #[test]
    fn extracts_bearer_token_from_bundle() {
        let bundle = "window.__SOMETHING__='AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA';";
        assert_eq!(
            extract_bearer_token(bundle).as_deref(),
            Some(
                "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA"
            )
        );
    }
}
