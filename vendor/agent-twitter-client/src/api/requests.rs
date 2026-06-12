use crate::error::{Result, TwitterError};
use reqwest::multipart::Form;
use reqwest::{header::HeaderMap, Client, Method};
use serde::de::DeserializeOwned;

pub async fn request_api<T>(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    method: Method,
    body: Option<serde_json::Value>,
) -> Result<(T, HeaderMap)>
where
    T: DeserializeOwned,
{
    let is_get = method == Method::GET;
    let mut request = client.request(method, url).headers(headers);

    if let Some(json_body) = body {
        if is_get {
            request = request.query(&json_body_to_query_params(&json_body)?);
        } else {
            request = request.json(&json_body);
        }
    }

    let response = request.send().await?;

    if response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await?;
        let parsed: T = serde_json::from_str(&text)?;
        Ok((parsed, headers))
    } else {
        Err(crate::error::TwitterError::Api(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

pub async fn get_guest_token(client: &Client, bearer_token: &str) -> Result<String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {}", bearer_token).parse().unwrap(),
    );

    let (response, _) = request_api::<serde_json::Value>(
        client,
        "https://api.twitter.com/1.1/guest/activate.json",
        headers,
        Method::POST,
        None,
    )
    .await?;

    response
        .get("guest_token")
        .and_then(|token| token.as_str())
        .map(String::from)
        .ok_or_else(|| crate::error::TwitterError::Auth("Failed to get guest token".into()))
}

fn json_body_to_query_params(json_body: &serde_json::Value) -> Result<Vec<(String, String)>> {
    let object = json_body.as_object().ok_or_else(|| {
        TwitterError::InvalidResponse("GET request params must be a JSON object".into())
    })?;

    object
        .iter()
        .map(|(key, value)| {
            let value = match value {
                serde_json::Value::String(value) => value.clone(),
                _ => serde_json::to_string(value)?,
            };
            Ok((key.clone(), value))
        })
        .collect()
}

pub async fn request_multipart_api<T>(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    form: Form,
) -> Result<(T, HeaderMap)>
where
    T: DeserializeOwned,
{
    let request = client
        .request(Method::POST, url)
        .headers(headers)
        .multipart(form);

    let response = request.send().await?;

    if response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await?;
        let parsed: T = serde_json::from_str(&text)?;
        Ok((parsed, headers))
    } else {
        Err(crate::error::TwitterError::Api(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

pub async fn request_form_api<T>(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    form_data: Vec<(String, String)>,
) -> Result<(T, HeaderMap)>
where
    T: DeserializeOwned,
{
    let request = client
        .request(Method::POST, url)
        .headers(headers)
        .form(&form_data);

    let response = request.send().await?;

    if response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await?;
        let parsed: T = serde_json::from_str(&text)?;
        Ok((parsed, headers))
    } else {
        Err(crate::error::TwitterError::Api(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::json_body_to_query_params;
    use serde_json::json;

    #[test]
    fn converts_get_body_objects_to_query_params() {
        let params = json_body_to_query_params(&json!({
            "variables": {
                "screen_name": "atmoio",
                "withSafetyModeUserFields": true
            },
            "features": {
                "responsive_web_graphql_timeline_navigation_enabled": true
            }
        }))
        .unwrap();

        assert_eq!(
            params,
            vec![
                (
                    "variables".to_string(),
                    "{\"screen_name\":\"atmoio\",\"withSafetyModeUserFields\":true}".to_string()
                ),
                (
                    "features".to_string(),
                    "{\"responsive_web_graphql_timeline_navigation_enabled\":true}"
                        .to_string()
                )
            ]
        );
    }
}
