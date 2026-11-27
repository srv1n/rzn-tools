//! The web connector against a local server. The server is also the source's proxy: requests for
//! `http://site.test/...` (a name that does not resolve) reach it only through the proxy, so a
//! direct request would fail, and the server sees each request's absolute URL.

use super::*;
use crate::item::{item_from_value, items_page_from_value};
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Requests the server saw: method and absolute URL, plus `If-None-Match`.
type Seen = Arc<StdMutex<Vec<(String, Option<String>)>>>;

const PAGE_A: &str = r#"<html><head><title>A</title>
  <meta property="og:title" content="Page A"><meta property="og:site_name" content="Site">
  <meta property="article:published_time" content="2026-11-01T00:00:00Z">
  <link rel="canonical" href="http://site.test/a"></head>
  <body><nav>Home Pricing</nav><main><h1>Page A</h1><p>Alpha words.</p></main></body></html>"#;

fn answer(path: &str, if_none_match: Option<&str>) -> (u16, &'static str, String) {
    match path {
        "/robots.txt" => (
            200,
            "\r\nContent-Type: text/plain",
            "User-agent: *\nDisallow: /\nSitemap: http://site.test/sitemap.xml\n".into(),
        ),
        "/sitemap.xml" => (
            200,
            "\r\nContent-Type: application/xml",
            "<urlset><url><loc>http://site.test/a?utm_source=x</loc></url>\
             <url><loc>http://site.test/a/</loc></url>\
             <url><loc>http://site.test/b</loc></url></urlset>"
                .into(),
        ),
        "/llms.txt" => (404, "", String::new()),
        "/a" if if_none_match == Some("\"a1\"") => (304, "\r\nETag: \"a1\"", String::new()),
        "/a" => (200, "\r\nContent-Type: text/html\r\nETag: \"a1\"", PAGE_A.into()),
        "/b" => (403, "", "denied".into()),
        _ => (404, "", String::new()),
    }
}

async fn serve(seen: Seen) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let inm = head
                    .lines()
                    .find_map(|l| l.strip_prefix("if-none-match: ").or_else(|| l.strip_prefix("If-None-Match: ")))
                    .map(str::to_string);
                seen.lock().unwrap().push((target.clone(), inm.clone()));
                let path = url::Url::parse(&target)
                    .map(|u| u.path().to_string())
                    .unwrap_or(target);
                let (status, headers, body) = answer(&path, inm.as_deref());
                let reply = format!(
                    "HTTP/1.1 {status} X{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    port
}

async fn connector(port: u16) -> WebConnector {
    let auth = AuthDetails::from_iter([(
        "proxy_url".to_string(),
        format!("http://127.0.0.1:{port}"),
    )]);
    let mut connector = WebConnector::new(auth).await.unwrap();
    connector.gap = Duration::ZERO;
    connector
}

async fn call(connector: &WebConnector, tool: &str, args: serde_json::Value) -> Result<serde_json::Value, ConnectorError> {
    let result = connector
        .call_tool(CallToolRequestParam {
            name: tool.to_string().into(),
            arguments: args.as_object().cloned(),
        })
        .await?;
    Ok(result.structured_content.unwrap_or_default())
}

#[tokio::test]
async fn a_proxied_site_is_listed_and_fetched_only_through_its_proxy() {
    let seen: Seen = Default::default();
    let port = serve(seen.clone()).await;
    let web = connector(port).await;

    let listing = call(&web, "list_pages", json!({"site": "http://site.test/", "output_format": "item_v2"}))
        .await
        .unwrap();
    let page = items_page_from_value(listing).unwrap();
    let keys: Vec<_> = page.items.iter().map(|i| i.key.as_str()).collect();
    // `?utm_source=x` and the trailing slash are the same page.
    assert_eq!(keys, vec!["http://site.test/a", "http://site.test/b"]);

    let item = item_from_value(
        call(&web, "scrape_url", json!({"item_ref": "http://site.test/a", "output_format": "item_v2"}))
            .await
            .unwrap(),
    )
    .unwrap();
    let document = item.document.clone().unwrap();
    assert_eq!(item.title.as_deref(), Some("Page A"));
    assert_eq!(document.etag.as_deref(), Some("\"a1\""));
    assert!(document.content_base64.is_some(), "the bytes ride with the item");

    // The next run carries the validators and the server answers 304.
    let again = item_from_value(
        call(
            &web,
            "scrape_url",
            json!({"item_ref": "http://site.test/a", "if_none_match": "\"a1\"", "output_format": "item_v2"}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(again.document.unwrap().not_modified);

    // A 403 is a block, named as one.
    let error = call(&web, "scrape_url", json!({"item_ref": "http://site.test/b", "output_format": "item_v2"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("blocked: HTTP 403"), "{error}");

    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty());
    // Every request reached the proxy in absolute form: none went direct.
    assert!(
        seen.iter().all(|(target, _)| target.starts_with("http://site.test/")),
        "{seen:?}"
    );
    assert!(seen.iter().any(|(target, inm)| target.ends_with("/a") && inm.is_some()));
}
