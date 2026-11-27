//! Web pages in `rzn.item/v2`: a page is a document (its URL, media type, the bytes fetched and a
//! hash of its main text), read by the index's HTML reader as an uploaded file is, so headings
//! survive. Listings come from a URL list, a sitemap, an RSS/Atom feed, a site (its robots.txt
//! sitemaps and llms.txt) or an llms.txt; there is no link-following crawler.

use super::fetch::Validators;
use super::page::PageFields;
use crate::error::ConnectorError;
use crate::item::{
    rfc3339, shared_item_fields, Description, DocumentRef, FieldDescription, FieldType, Item,
    ItemsPage, KindDescription, Listing, Party,
};
use serde_json::json;

/// Query parameters that only track a click and never change the page (design 3.3).
const TRACKING: &[&str] = &[
    "gclid", "fbclid", "msclkid", "dclid", "yclid", "igshid", "mc_cid", "mc_eid", "ref", "ref_src",
    "_hsenc", "_hsmi",
];

fn tracking(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("utm_") || TRACKING.contains(&name.as_str())
}

/// The page's identity: its URL with a lowercase scheme and host, without fragment, default
/// port, tracking parameters or a trailing slash (the root keeps its slash).
pub(super) fn normalize_url(raw: &str) -> Result<String, ConnectorError> {
    let mut url = url::Url::parse(raw.trim())
        .map_err(|e| ConnectorError::InvalidParams(format!("invalid URL {raw}: {e}")))?;
    url.set_fragment(None);
    if url.query().is_some() {
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(name, _)| !tracking(name))
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        if kept.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    Ok(url.to_string())
}

fn bare_host(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| {
        u.host_str()
            .map(|h| h.trim_start_matches("www.").to_string())
    })
}

fn site(url: &str) -> Party {
    let host = bare_host(url).unwrap_or_else(|| url.to_string());
    Party::named(host.clone()).with_id(host)
}

/// The URL a page is cited by: its `rel=canonical` when that is on the same site, otherwise the
/// URL it was served from after redirects.
pub(super) fn cited_url(
    final_url: &str,
    canonical: Option<&str>,
) -> Result<String, ConnectorError> {
    match canonical {
        Some(canonical)
            if bare_host(canonical).is_some() && bare_host(canonical) == bare_host(final_url) =>
        {
            normalize_url(canonical)
        }
        _ => normalize_url(final_url),
    }
}

/// A page as fetched, ready to become an item.
pub(super) struct FetchedPage {
    /// The key the page was asked for (a listed, normalized URL).
    pub key: String,
    /// The URL it was served from after redirects.
    pub final_url: String,
    pub fields: PageFields,
    /// The page's main text; its hash decides whether the page is read again.
    pub main_text: String,
    pub media_type: String,
    pub body: Vec<u8>,
    pub validators: Validators,
    pub shell: Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// The full item for a fetched page: its fields and the document the index reads, with the bytes
/// fetched riding inline so the index never fetches the page again.
pub(super) fn page_item(page: FetchedPage) -> Result<Item, ConnectorError> {
    use base64::Engine as _;
    let url = cited_url(&page.final_url, page.fields.canonical.as_deref())?;
    let mut place = site(&url);
    if let Some(name) = &page.fields.site_name {
        place.name = name.clone();
    }
    let author = page
        .fields
        .author
        .as_deref()
        .map(Party::named)
        .unwrap_or_else(|| place.clone());
    // A full item needs a date: the page's own, else its modified date, else the server's.
    let published_at = [
        page.fields.published.as_deref(),
        page.fields.modified.as_deref(),
        page.validators.last_modified.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(rfc3339)
    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let text_hash = sha256_hex(page.main_text.as_bytes());
    let mut item = Item::new(page.key.clone(), "page", url.clone())
        .with_title(page.fields.title.clone().unwrap_or_default())
        .with_author(author)
        .with_published_at(Some(published_at))
        .with_published_in(place)
        .with_marker(Some(text_hash.clone()))
        .with_fields(json!({
            "description": page.fields.description,
            "keywords": page.fields.keywords,
            "site_name": page.fields.site_name,
            "modified_at": page.fields.modified.as_deref().and_then(rfc3339),
            "image": page.fields.image,
        }));
    item.document = Some(DocumentRef {
        uri: url,
        media_type: page.media_type,
        sha256: Some(text_hash),
        content_base64: Some(base64::engine::general_purpose::STANDARD.encode(&page.body)),
        etag: page.validators.etag,
        last_modified: page.validators.last_modified,
        not_modified: false,
        shell: page.shell,
    });
    Ok(item)
}

/// The item for a page that answered 304 to the validators it was asked with: no bytes. It still
/// passes the full-item check; a caller takes `not_modified` and keeps what it has.
pub(super) fn not_modified_item(key: &str, validators: Validators) -> Item {
    let place = site(key);
    let mut item = Item::new(key, "page", key)
        .with_author(place.clone())
        .with_published_in(place)
        .with_published_at(
            validators
                .last_modified
                .as_deref()
                .and_then(rfc3339)
                .or_else(|| Some(chrono::Utc::now().to_rfc3339())),
        );
    item.document = Some(DocumentRef {
        uri: key.to_string(),
        media_type: "text/html".to_string(),
        etag: validators.etag,
        last_modified: validators.last_modified,
        not_modified: true,
        ..Default::default()
    });
    item
}

/// The marker every listed page carries: the time of the listing. A page is fetched on every run
/// (with its validators, so an unchanged one answers 304); a sitemap's `lastmod` never proves a
/// page unchanged (design 3.4).
fn listing_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Listing entries for page URLs, deduplicated by normalized URL.
pub(super) fn pages_page(
    entries: &[(String, Option<String>)],
    listing: Listing,
) -> Result<ItemsPage, ConnectorError> {
    let stamp = listing_stamp();
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();
    for (loc, lastmod) in entries {
        let key = normalize_url(loc)?;
        if !seen.insert(key.clone()) {
            continue;
        }
        items.push(
            Item::new(key.clone(), "page", key)
                .with_published_at(lastmod.as_deref().and_then(rfc3339))
                .with_marker(Some(stamp.clone())),
        );
    }
    Ok(ItemsPage::new(items, None, listing))
}

/// Listing entries for an explicit URL list: everything in scope, so a URL left out was removed.
pub(super) fn url_list_page(urls: &[String]) -> Result<ItemsPage, ConnectorError> {
    let entries: Vec<_> = urls.iter().map(|url| (url.clone(), None)).collect();
    pages_page(&entries, Listing::Full)
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

/// Listing entries for a feed's linked pages: a feed shows recent entries only.
pub(super) fn feed_page(xml: &[u8]) -> Result<ItemsPage, ConnectorError> {
    let feed = feed_rs::parser::parse(xml)
        .map_err(|e| ConnectorError::Other(format!("Failed to parse feed: {e}")))?;
    let stamp = listing_stamp();
    let mut items = Vec::new();
    for entry in feed.entries {
        let Some(link) = entry.links.first().map(|l| l.href.clone()) else {
            continue;
        };
        let key = normalize_url(&link)?;
        if items.iter().any(|item: &Item| item.key == key) {
            continue;
        }
        items.push(
            Item::new(key.clone(), "page", key)
                .with_title(entry.title.map(|t| t.content).unwrap_or_default())
                .with_published_at(entry.published.or(entry.updated).map(|d| d.to_rfc3339()))
                .with_marker(Some(stamp.clone())),
        );
    }
    Ok(ItemsPage::new(items, None, Listing::Feed))
}

pub(super) fn description() -> Description {
    let mut fields = shared_item_fields();
    fields.extend([
        FieldDescription::item("description", FieldType::String),
        FieldDescription::item("keywords", FieldType::String).list(),
        FieldDescription::item("site_name", FieldType::String),
        FieldDescription::item("modified_at", FieldType::Date),
        FieldDescription::item("image", FieldType::String),
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
                "site".to_string(),
                "llms_txt".to_string(),
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
    fn tracking_parameters_fragments_and_trailing_slashes_make_one_item() {
        let one = "https://example.com/pricing?plan=pro";
        for variant in [
            "https://example.com/pricing/?plan=pro",
            "https://EXAMPLE.com/pricing?utm_source=x&plan=pro&utm_medium=y#faq",
            "https://example.com/pricing?plan=pro&gclid=abc&fbclid=def&ref=hn",
        ] {
            assert_eq!(normalize_url(variant).unwrap(), one, "{variant}");
        }
        assert_eq!(
            normalize_url("https://example.com/a/?utm_campaign=z").unwrap(),
            "https://example.com/a"
        );
        let page = url_list_page(&[
            "https://example.com/a".to_string(),
            "https://example.com/a/".to_string(),
            "https://example.com/a?utm_source=news#top".to_string(),
        ])
        .unwrap();
        assert_eq!(page.items.len(), 1);
    }

    #[test]
    fn sitemaps_list_every_url_and_every_page_is_checked_each_run() {
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
        let page = pages_page(&entries, Listing::Full).unwrap();
        assert_eq!(page.listing, Listing::Full);
        // lastmod is a date hint, never the marker: the marker is the listing's own time.
        assert!(page.items[0].published_at.is_some());
        assert_ne!(page.items[0].marker.as_deref(), Some("2026-01-02"));
        assert!(page.items.iter().all(|item| item.marker.is_some()));
        assert!(page.items.iter().all(|item| item.check_listed().is_ok()));
    }

    fn fetched(final_url: &str, canonical: Option<&str>) -> FetchedPage {
        FetchedPage {
            key: "https://example.com/post".to_string(),
            final_url: final_url.to_string(),
            fields: PageFields {
                title: Some("Post".to_string()),
                canonical: canonical.map(str::to_string),
                site_name: Some("Example".to_string()),
                ..Default::default()
            },
            main_text: "the main text".to_string(),
            media_type: "text/html".to_string(),
            body: b"<html>raw</html>".to_vec(),
            validators: Validators {
                etag: Some("\"v1\"".to_string()),
                last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string()),
            },
            shell: None,
        }
    }

    #[test]
    fn a_page_is_a_document_item_with_its_bytes_and_validators() {
        let item = page_item(fetched("https://example.com/post/", None)).unwrap();
        assert_eq!(item.key, "https://example.com/post");
        let document = item.document.as_ref().unwrap();
        assert_eq!(document.media_type, "text/html");
        assert!(document.content_base64.is_some());
        assert_eq!(document.etag.as_deref(), Some("\"v1\""));
        assert_eq!(item.published_in.as_ref().unwrap().name, "Example");
        // Raw HTML noise does not change the hash; the main text does.
        let mut noisy = fetched("https://example.com/post", None);
        noisy.body = b"<html>raw, other nonce</html>".to_vec();
        assert_eq!(
            page_item(noisy).unwrap().document.unwrap().sha256,
            document.sha256
        );
        assert!(item.check().is_ok());
    }

    #[test]
    fn a_page_is_cited_by_its_same_site_canonical_else_its_final_url() {
        let same = page_item(fetched(
            "https://example.com/post?utm_source=x",
            Some("https://www.example.com/posts/1/"),
        ))
        .unwrap();
        assert_eq!(same.url, "https://www.example.com/posts/1");
        let other = page_item(fetched(
            "https://example.com/post/",
            Some("https://syndicated.org/post"),
        ))
        .unwrap();
        assert_eq!(other.url, "https://example.com/post");
        assert_eq!(other.key, "https://example.com/post");
    }

    #[test]
    fn a_304_is_a_full_item_that_says_not_modified() {
        let item = not_modified_item(
            "https://example.com/post",
            Validators {
                etag: Some("\"v1\"".to_string()),
                last_modified: None,
            },
        );
        assert!(item.check().is_ok());
        let document = item.document.unwrap();
        assert!(document.not_modified);
        assert!(document.content_base64.is_none());
    }
}
