//! Web pages in `rzn.item/v2`: a page is a document (its URL, media type and hash), read by the
//! index's HTML reader as an uploaded file is, so headings survive. Listings come from a URL list, a
//! sitemap, or an RSS/Atom feed; there is no link-following crawler.

use super::WebContent;
use crate::error::ConnectorError;
use crate::item::{
    rfc3339, shared_item_fields, Description, DocumentRef, FieldDescription, FieldType, Item,
    ItemsPage, KindDescription, Listing, Party,
};
use serde_json::json;

/// The page's identity: its URL without fragment, default port or trailing slash, with a
/// lowercase scheme and host.
pub(super) fn normalize_url(raw: &str) -> Result<String, ConnectorError> {
    let mut url = url::Url::parse(raw.trim())
        .map_err(|e| ConnectorError::InvalidParams(format!("invalid URL {raw}: {e}")))?;
    url.set_fragment(None);
    let mut text = url.to_string();
    if url.path() != "/" && text.ends_with('/') && url.query().is_none() {
        text.pop();
    }
    Ok(text)
}

fn site(url: &str) -> Party {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_else(|| url.to_string());
    Party::named(host.clone()).with_id(host)
}

/// The full item for a fetched page: its head and the document the index reads.
pub(super) fn page_item(
    content: &WebContent,
    sha256: &str,
    published_hint: Option<String>,
) -> Result<Item, ConnectorError> {
    let key = normalize_url(&content.url)?;
    let place = site(&key);
    let author = content
        .metadata
        .author
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(Party::named)
        .unwrap_or_else(|| place.clone());
    let mut item = Item::new(key.clone(), "page", key.clone())
        .with_title(content.title.clone().unwrap_or_default())
        .with_author(author)
        .with_published_at(
            content
                .metadata
                .published_date
                .as_deref()
                .and_then(rfc3339)
                .or(published_hint),
        )
        .with_published_in(place)
        .with_marker(Some(sha256.to_string()))
        .with_fields(json!({
            "description": content.metadata.description,
            "keywords": content.metadata.keywords,
        }));
    item.document = Some(DocumentRef {
        uri: key,
        media_type: "text/html".to_string(),
        sha256: Some(sha256.to_string()),
    });
    Ok(item)
}

/// Listing entries for an explicit URL list: everything in scope, so a URL left out was removed.
pub(super) fn url_list_page(urls: &[String]) -> Result<ItemsPage, ConnectorError> {
    let items = urls
        .iter()
        .map(|raw| {
            let key = normalize_url(raw)?;
            Ok(Item::new(key.clone(), "page", key))
        })
        .collect::<Result<Vec<_>, ConnectorError>>()?;
    Ok(ItemsPage::new(items, None, Listing::Full))
}

/// `<loc>` and `<lastmod>` pairs of a sitemap, and whether it is a sitemap index.
pub(super) fn parse_sitemap(xml: &str) -> (Vec<(String, Option<String>)>, bool) {
    let index = xml.contains("<sitemapindex");
    let mut out = Vec::new();
    for block in xml.split("<loc>").skip(1) {
        let Some((loc, rest)) = block.split_once("</loc>") else {
            continue;
        };
        let loc = loc.trim().replace("&amp;", "&");
        let lastmod = rest
            .split_once("<lastmod>")
            .and_then(|(before, after)| {
                (!before.contains("<loc>")).then(|| after.split_once("</lastmod>"))?
            })
            .map(|(value, _)| value.trim().to_string());
        out.push((loc, lastmod));
    }
    (out, index)
}

/// Listing entries for a sitemap's pages; `lastmod` is each page's marker.
pub(super) fn sitemap_page(
    entries: &[(String, Option<String>)],
) -> Result<ItemsPage, ConnectorError> {
    let mut items = Vec::new();
    for (loc, lastmod) in entries {
        let key = normalize_url(loc)?;
        items.push(
            Item::new(key.clone(), "page", key)
                .with_published_at(lastmod.as_deref().and_then(rfc3339))
                .with_marker(lastmod.clone()),
        );
    }
    Ok(ItemsPage::new(items, None, Listing::Full))
}

/// Listing entries for a feed's linked pages: a feed shows recent entries only.
pub(super) fn feed_page(xml: &[u8]) -> Result<ItemsPage, ConnectorError> {
    let feed = feed_rs::parser::parse(xml)
        .map_err(|e| ConnectorError::Other(format!("Failed to parse feed: {e}")))?;
    let mut items = Vec::new();
    for entry in feed.entries {
        let Some(link) = entry.links.first().map(|l| l.href.clone()) else {
            continue;
        };
        let key = normalize_url(&link)?;
        items.push(
            Item::new(key.clone(), "page", key)
                .with_title(entry.title.map(|t| t.content).unwrap_or_default())
                .with_published_at(entry.published.or(entry.updated).map(|d| d.to_rfc3339()))
                .with_marker(entry.updated.map(|d| d.to_rfc3339())),
        );
    }
    Ok(ItemsPage::new(items, None, Listing::Feed))
}

pub(super) fn description() -> Description {
    let mut fields = shared_item_fields();
    fields.extend([
        FieldDescription::item("description", FieldType::String),
        FieldDescription::item("keywords", FieldType::String).list(),
    ]);
    Description::new(
        "web",
        vec![KindDescription {
            kind: "page".to_string(),
            identity: "<normalized URL>".to_string(),
            listing: Listing::Full,
            scopes: vec![
                "urls".to_string(),
                "sitemap".to_string(),
                "feed".to_string(),
            ],
            list_tool: "list_pages".to_string(),
            get_tool: "scrape_url".to_string(),
            fields,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_normalize_to_one_key() {
        assert_eq!(
            normalize_url("HTTPS://Example.COM/a/b/#top").unwrap(),
            "https://example.com/a/b"
        );
        assert_eq!(
            normalize_url("https://example.com/").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            normalize_url("https://example.com:443/x?q=1").unwrap(),
            "https://example.com/x?q=1"
        );
    }

    #[test]
    fn sitemaps_list_every_url_with_its_lastmod() {
        let xml = r#"<?xml version="1.0"?><urlset>
          <url><loc>https://example.com/a</loc><lastmod>2026-01-02</lastmod></url>
          <url><loc>https://example.com/b?x=1&amp;y=2</loc></url>
        </urlset>"#;
        let (entries, index) = parse_sitemap(xml);
        assert!(!index);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1.as_deref(), Some("2026-01-02"));
        assert_eq!(
            entries[1],
            ("https://example.com/b?x=1&y=2".to_string(), None)
        );
        let page = sitemap_page(&entries).unwrap();
        assert_eq!(page.listing, Listing::Full);
        assert_eq!(page.items[0].marker.as_deref(), Some("2026-01-02"));
        assert!(page.items.iter().all(|item| item.check_listed().is_ok()));
    }

    #[test]
    fn a_page_is_a_document_item() {
        let content = WebContent {
            url: "https://example.com/post/".to_string(),
            title: Some("Post".to_string()),
            content: "text".to_string(),
            metadata: Default::default(),
            raw_sha256: "abc".to_string(),
        };
        let item = page_item(&content, "abc", Some("2026-01-02T00:00:00+00:00".into())).unwrap();
        assert_eq!(item.key, "https://example.com/post");
        assert_eq!(item.document.as_ref().unwrap().media_type, "text/html");
        assert!(item.check().is_ok());
    }
}
