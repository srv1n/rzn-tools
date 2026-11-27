//! Live checks, run by hand (`--ignored`), never in CI.
//!
//! `RZN_WEB_LIVE_SITE=https://example.com`: list the site, fetch every page, then fetch every
//! page again with the validators of the first fetch, and print how many answered 304, how many
//! answered 200 with the same main text, and how many changed.
//!
//! `RZN_WEB_CHECK_DIR=<dir>`: classify saved pages (`NN.html`, `index.tsv`) as blocked, empty
//! shell or page, with their main-text word counts.

use super::*;
use crate::item::{item_from_value, items_page_from_value};

async fn call(
    web: &WebConnector,
    tool: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, ConnectorError> {
    let result = web
        .call_tool(CallToolRequestParam {
            name: tool.to_string().into(),
            arguments: args.as_object().cloned(),
        })
        .await?;
    Ok(result.structured_content.unwrap_or_default())
}

#[tokio::test]
#[ignore = "fetches a live site named by RZN_WEB_LIVE_SITE"]
async fn live_site_twice() {
    let Ok(site) = std::env::var("RZN_WEB_LIVE_SITE") else {
        return;
    };
    let web = WebConnector::new(AuthDetails::new()).await.unwrap();
    let started = std::time::Instant::now();
    let listing = items_page_from_value(
        call(
            &web,
            "list_pages",
            json!({"site": site, "output_format": "item_v2"}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    println!("listed {} pages from {site}", listing.items.len());
    let mut first = Vec::new();
    let (mut fetched, mut failed, mut shells) = (0, 0, 0);
    for entry in &listing.items {
        match call(
            &web,
            "scrape_url",
            json!({"item_ref": entry.key, "output_format": "item_v2"}),
        )
        .await
        {
            Ok(value) => {
                let item = item_from_value(value).unwrap();
                let document = item.document.clone().unwrap();
                fetched += 1;
                shells += usize::from(document.shell.is_some());
                println!(
                    "run1 200 {} etag={} last_modified={} shell={:?} title={:?}",
                    entry.key,
                    document.etag.is_some(),
                    document.last_modified.is_some(),
                    document.shell,
                    item.title
                );
                first.push((entry.key.clone(), document));
            }
            Err(error) => {
                failed += 1;
                println!("run1 FAIL {} {error}", entry.key);
            }
        }
    }
    println!("run1: fetched {fetched}, failed {failed}, empty shells {shells}");
    let (mut not_modified, mut same_text, mut changed) = (0, 0, 0);
    for (key, prior) in &first {
        let mut args = json!({"item_ref": key, "output_format": "item_v2"});
        if let Some(etag) = &prior.etag {
            args["if_none_match"] = json!(etag);
        }
        if let Some(since) = &prior.last_modified {
            args["if_modified_since"] = json!(since);
        }
        let item = item_from_value(call(&web, "scrape_url", args).await.unwrap()).unwrap();
        let document = item.document.unwrap();
        let verdict = if document.not_modified {
            not_modified += 1;
            "304"
        } else if document.sha256 == prior.sha256 {
            same_text += 1;
            "200-same-text"
        } else {
            changed += 1;
            "200-changed"
        };
        println!("run2 {verdict} {key}");
    }
    println!(
        "run2: 304 {not_modified}, 200 with the same main text {same_text}, changed {changed}; {:.1} s in all",
        started.elapsed().as_secs_f64()
    );
}

#[test]
#[ignore = "reads saved pages from RZN_WEB_CHECK_DIR"]
fn saved_pages_classified() {
    let Ok(dir) = std::env::var("RZN_WEB_CHECK_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let index = std::fs::read_to_string(dir.join("index.tsv")).unwrap();
    for row in index.lines() {
        let cols: Vec<&str> = row.split('\t').collect();
        let body =
            std::fs::read_to_string(dir.join(format!("{}.html", cols[0]))).unwrap_or_default();
        let status = cols[2]
            .parse::<u16>()
            .ok()
            .and_then(|code| reqwest::StatusCode::from_u16(code).ok())
            .unwrap_or(reqwest::StatusCode::OK);
        let html = Html::parse_document(&body);
        let main = page_text(&html);
        let words = page::words(&main);
        let verdict = page::blocked(status, &body)
            .or_else(|| page::empty_shell(&html, words))
            .unwrap_or_else(|| "page".to_string());
        println!(
            "{}\t{}\t{}\t{words}\t{verdict}\t{}",
            cols[0], cols[1], cols[2], cols[4]
        );
    }
}
