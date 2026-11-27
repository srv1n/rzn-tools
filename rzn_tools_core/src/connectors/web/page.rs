//! What a fetched page says about itself (source-network design 3.3, 3.6 and 3.7): its fields from
//! Open Graph, then JSON-LD, then plain HTML tags, in that order of trust; its canonical URL; and
//! whether the bytes are the page at all or a bot wall or an empty JavaScript shell.

use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

/// A page's own fields. Each is the first one found in Open Graph, JSON-LD, then HTML.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct PageFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site_name: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    pub modified: Option<String>,
    pub image: Option<String>,
    pub keywords: Vec<String>,
    /// `rel=canonical`, resolved against the page URL.
    pub canonical: Option<String>,
}

fn select<'a>(html: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
    match Selector::parse(css) {
        Ok(selector) => html.select(&selector).collect(),
        Err(_) => Vec::new(),
    }
}

fn clean(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

fn meta(html: &Html, attr: &str, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        select(html, &format!("meta[{attr}=\"{name}\"]"))
            .into_iter()
            .find_map(|el| el.value().attr("content").and_then(clean))
    })
}

fn first<const N: usize>(choices: [Option<String>; N]) -> Option<String> {
    choices.into_iter().flatten().next()
}

/// JSON-LD types read for page fields.
const LD_TYPES: &[&str] = &[
    "Article",
    "NewsArticle",
    "BlogPosting",
    "TechArticle",
    "Report",
    "WebPage",
    "AboutPage",
    "FAQPage",
    "Product",
    "Organization",
    "Corporation",
    "WebSite",
    "SoftwareApplication",
    "BreadcrumbList",
];

/// Fields from the page's JSON-LD blocks; the first typed object that has a field wins it.
#[derive(Default)]
struct LdFields {
    title: Option<String>,
    description: Option<String>,
    site_name: Option<String>,
    author: Option<String>,
    published: Option<String>,
    modified: Option<String>,
    image: Option<String>,
}

fn ld_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => clean(text),
        Value::Object(object) => object
            .get("name")
            .or_else(|| object.get("url"))
            .and_then(ld_text),
        Value::Array(values) => values.iter().find_map(ld_text),
        _ => None,
    }
}

fn ld_types(object: &serde_json::Map<String, Value>) -> Vec<String> {
    match object.get("@type") {
        Some(Value::String(kind)) => vec![kind.clone()],
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn ld_objects(value: &Value, out: &mut Vec<serde_json::Map<String, Value>>) {
    match value {
        Value::Array(values) => values.iter().for_each(|v| ld_objects(v, out)),
        Value::Object(object) => {
            if let Some(graph) = object.get("@graph") {
                ld_objects(graph, out);
            }
            out.push(object.clone());
        }
        _ => {}
    }
}

fn read_ld(html: &Html) -> LdFields {
    let mut objects = Vec::new();
    for script in select(html, "script[type=\"application/ld+json\"]") {
        let text = script.text().collect::<String>();
        if let Ok(value) = serde_json::from_str::<Value>(text.trim()) {
            ld_objects(&value, &mut objects);
        }
    }
    let mut out = LdFields::default();
    for object in &objects {
        let types = ld_types(object);
        if !types.iter().any(|t| LD_TYPES.contains(&t.as_str())) {
            continue;
        }
        let site = types
            .iter()
            .any(|t| matches!(t.as_str(), "Organization" | "Corporation" | "WebSite"));
        let get = |name: &str| object.get(name).and_then(ld_text);
        if site {
            out.site_name = out.site_name.take().or_else(|| get("name"));
            continue;
        }
        out.title = out
            .title
            .take()
            .or_else(|| get("headline"))
            .or_else(|| get("name"));
        out.description = out.description.take().or_else(|| get("description"));
        out.author = out.author.take().or_else(|| get("author"));
        out.published = out.published.take().or_else(|| get("datePublished"));
        out.modified = out.modified.take().or_else(|| get("dateModified"));
        out.image = out.image.take().or_else(|| get("image"));
        out.site_name = out.site_name.take().or_else(|| get("publisher"));
    }
    out
}

fn resolve(base: &url::Url, href: &str) -> Option<String> {
    base.join(href.trim()).ok().map(|url| url.to_string())
}

/// The page's fields and canonical URL. `base` is the URL the page was fetched from.
pub(super) fn read_fields(html: &Html, base: &url::Url) -> PageFields {
    let ld = read_ld(html);
    let title_tag = select(html, "title")
        .into_iter()
        .find_map(|el| clean(&el.text().collect::<String>()));
    let heading = select(html, "h1")
        .into_iter()
        .find_map(|el| clean(&el.text().collect::<String>()));
    let og = |names: &[&str]| meta(html, "property", names);
    let named = |names: &[&str]| meta(html, "name", names);
    // An `article:author` is often a profile URL, which is no name.
    let og_author = og(&["article:author", "og:article:author"]).filter(|a| !a.starts_with("http"));
    let ld_author = ld.author.filter(|a| !a.starts_with("http"));
    PageFields {
        title: first([og(&["og:title"]), ld.title, title_tag, heading]),
        description: first([
            og(&["og:description"]),
            ld.description,
            named(&["description"]),
        ]),
        site_name: first([
            og(&["og:site_name"]),
            ld.site_name,
            named(&["application-name"]),
        ]),
        author: first([og_author, ld_author, named(&["author"])]),
        published: first([
            og(&["article:published_time", "og:published_time"]),
            ld.published,
            named(&["date", "publish-date", "pubdate"]),
        ]),
        modified: first([
            og(&["article:modified_time", "og:updated_time"]),
            ld.modified,
            named(&["last-modified"]),
        ]),
        image: first([
            og(&["og:image", "og:image:url"]),
            ld.image,
            named(&["twitter:image"]),
        ])
        .and_then(|src| resolve(base, &src)),
        keywords: named(&["keywords"])
            .map(|k| k.split(',').filter_map(clean).collect::<Vec<_>>())
            .unwrap_or_default(),
        canonical: select(html, "link[rel=\"canonical\"]")
            .into_iter()
            .find_map(|el| el.value().attr("href"))
            .and_then(|href| resolve(base, href)),
    }
}

/// Words in a text.
pub(super) fn words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// The fewest words of main text a page must have unless it shows no sign of rendering in a
/// browser (design 3.7).
pub(super) const SHELL_WORDS: usize = 50;

/// Element ids an app mounts into.
const MOUNTS: &[&str] = &[
    "#root",
    "#app",
    "#__next",
    "#__nuxt",
    "#___gatsby",
    "#svelte",
    "app-root",
];

const JS_NOTICES: &[&str] = &[
    "enable javascript",
    "javascript is required",
    "javascript is disabled",
    "requires javascript",
    "turn on javascript",
    "you need to enable javascript",
];

/// Why a fetched page is an empty shell, or `None` when its main text is the page.
///
/// A page is a shell when its main text is under [`SHELL_WORDS`] words and it shows a sign it is
/// rendered by script: an empty app mount, an enable-JavaScript notice, or almost no text beside a
/// script. A short static page (a contact page, a stub) is not a shell.
pub(super) fn empty_shell(html: &Html, main_words: usize) -> Option<String> {
    if main_words >= SHELL_WORDS {
        return None;
    }
    let body_text = select(html, "body")
        .into_iter()
        .map(|el| el.text().collect::<String>().to_lowercase())
        .collect::<String>();
    if let Some(notice) = JS_NOTICES.iter().find(|notice| body_text.contains(*notice)) {
        return Some(format!(
            "empty_shell: {main_words} words and a \"{notice}\" notice"
        ));
    }
    for mount in MOUNTS {
        if let Some(el) = select(html, mount).into_iter().next() {
            if words(&el.text().collect::<String>()) < 5 {
                return Some(format!(
                    "empty_shell: {main_words} words and an empty {mount} mount"
                ));
            }
        }
    }
    let scripted = !select(html, "script[src], script[type=\"module\"]").is_empty();
    if scripted && main_words < 10 {
        return Some(format!("empty_shell: {main_words} words beside scripts"));
    }
    None
}

/// Text that only a bot wall or challenge page shows.
const WALLS: &[&str] = &[
    "just a moment...",
    "attention required! | cloudflare",
    "checking your browser before accessing",
    "cf-browser-verification",
    "/cdn-cgi/challenge-platform",
    "px-captcha",
    "verify you are a human",
    "are you a robot",
    "sign in to confirm you",
    "unusual traffic from your computer",
];

/// Why a fetched answer is a block rather than the page, or `None`.
pub(super) fn blocked(status: reqwest::StatusCode, body: &str) -> Option<String> {
    use reqwest::StatusCode;
    let lower = body.to_lowercase();
    let wall = WALLS.iter().find(|marker| lower.contains(*marker));
    match status {
        StatusCode::FORBIDDEN => Some("blocked: HTTP 403".to_string()),
        StatusCode::TOO_MANY_REQUESTS => Some("blocked: HTTP 429 after back-off".to_string()),
        s if s.is_server_error() && wall.is_some() => {
            Some(format!("blocked: HTTP {} bot wall", s.as_u16()))
        }
        s if s.is_success() => wall
            .filter(|_| words(&lower) < 400)
            .map(|marker| format!("blocked: bot wall ({marker})")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> url::Url {
        url::Url::parse("https://example.com/post/1").unwrap()
    }

    #[test]
    fn open_graph_wins_over_json_ld_which_wins_over_html() {
        let html = Html::parse_document(
            r#"<html><head>
            <title>Tag title</title>
            <meta name="description" content="Tag description">
            <meta name="author" content="Tag Author">
            <meta property="og:title" content="OG title">
            <meta property="og:image" content="/img/a.png">
            <link rel="canonical" href="/post/1?">
            <script type="application/ld+json">{"@context":"https://schema.org","@graph":[
              {"@type":"Organization","name":"Example Inc"},
              {"@type":"BlogPosting","headline":"LD title","description":"LD description",
               "author":{"@type":"Person","name":"LD Author"},
               "datePublished":"2026-01-02","dateModified":"2026-02-03"}]}</script>
            </head><body><h1>Heading</h1></body></html>"#,
        );
        let fields = read_fields(&html, &base());
        assert_eq!(fields.title.as_deref(), Some("OG title"));
        assert_eq!(fields.description.as_deref(), Some("LD description"));
        assert_eq!(fields.author.as_deref(), Some("LD Author"));
        assert_eq!(fields.site_name.as_deref(), Some("Example Inc"));
        assert_eq!(fields.published.as_deref(), Some("2026-01-02"));
        assert_eq!(fields.modified.as_deref(), Some("2026-02-03"));
        assert_eq!(
            fields.image.as_deref(),
            Some("https://example.com/img/a.png")
        );
        assert_eq!(
            fields.canonical.as_deref(),
            Some("https://example.com/post/1?")
        );
    }

    #[test]
    fn plain_tags_fill_what_nothing_richer_gives() {
        let html = Html::parse_document(
            r#"<html><head><title> Plain   title </title>
            <meta name="description" content="Plain description">
            <meta name="author" content="Plain Author">
            <meta name="keywords" content="a, b ,, c"></head><body></body></html>"#,
        );
        let fields = read_fields(&html, &base());
        assert_eq!(fields.title.as_deref(), Some("Plain title"));
        assert_eq!(fields.description.as_deref(), Some("Plain description"));
        assert_eq!(fields.author.as_deref(), Some("Plain Author"));
        assert_eq!(fields.keywords, vec!["a", "b", "c"]);
        assert_eq!(fields.site_name, None);
    }

    #[test]
    fn an_empty_app_mount_is_a_shell_and_a_short_static_page_is_not() {
        let shell = Html::parse_document(
            r#"<html><head><script type="module" src="/assets/index.js"></script></head>
            <body><div id="root"></div></body></html>"#,
        );
        assert!(empty_shell(&shell, 0).unwrap().contains("#root"));
        let notice = Html::parse_document(
            r#"<html><body><noscript>You need to enable JavaScript to run this app.</noscript>
            <div id="app"></div></body></html>"#,
        );
        assert!(empty_shell(&notice, 8)
            .unwrap()
            .contains("enable javascript"));
        let short = Html::parse_document(
            r#"<html><head><script src="/analytics.js"></script></head><body><main>
            <h1>Contact</h1><p>Write to us at hello at example dot com, or call during office
            hours on weekdays. We answer within two days.</p></main></body></html>"#,
        );
        assert_eq!(empty_shell(&short, 21), None);
        let long_app = Html::parse_document(r#"<html><body><div id="root"></div></body></html>"#);
        assert_eq!(empty_shell(&long_app, 300), None);
    }

    #[test]
    fn a_403_a_429_and_a_challenge_page_are_blocks_and_a_page_is_not() {
        use reqwest::StatusCode;
        assert!(blocked(StatusCode::FORBIDDEN, "").is_some());
        assert!(blocked(StatusCode::TOO_MANY_REQUESTS, "").is_some());
        let wall = "<html><title>Just a moment...</title><body>Checking</body></html>";
        assert!(blocked(StatusCode::OK, wall).unwrap().contains("bot wall"));
        assert!(blocked(StatusCode::SERVICE_UNAVAILABLE, wall).is_some());
        assert_eq!(
            blocked(StatusCode::OK, "<html><body>Hello</body></html>"),
            None
        );
        assert_eq!(blocked(StatusCode::NOT_FOUND, "missing"), None);
    }
}
