use super::{ConnectorError, PubMedArticle, PubMedSearchResult};
use scraper::{Html, Selector};
use std::sync::OnceLock;

fn pubmed_trace_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("RZN_PUBMED_TRACE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

pub struct SearchParseInput {
    pub content: String,
    pub limit: usize,
    pub query: String,
    pub page: usize,
    pub content_len: usize,
}

pub fn parse_pubmed_search_document(
    input: SearchParseInput,
) -> Result<PubMedSearchResult, ConnectorError> {
    let parse_doc_start = std::time::Instant::now();
    let document = Html::parse_document(&input.content);
    let parse_doc_ms = parse_doc_start.elapsed().as_millis();
    let parse_start = std::time::Instant::now();

    let mut articles = Vec::new();

    let result_selector = Selector::parse("article.full-docsum")
        .unwrap_or_else(|_| Selector::parse("div.docsum").unwrap());
    let title_selector = Selector::parse("a.docsum-title").unwrap();
    let authors_selector = Selector::parse("span.docsum-authors").unwrap();
    let journal_selector = Selector::parse("span.docsum-journal-citation").unwrap();
    let pmid_selector = Selector::parse("span.docsum-pmid").unwrap();

    let mut docsum_index: usize = 0;
    for result in document.select(&result_selector).take(input.limit) {
        let iter_start = std::time::Instant::now();

        let select_title_start = std::time::Instant::now();
        let title_element = match result.select(&title_selector).next() {
            Some(el) => el,
            None => continue,
        };
        let select_title_ms = select_title_start.elapsed().as_millis();

        let title_text_start = std::time::Instant::now();
        let title = title_element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        let title_text_ms = title_text_start.elapsed().as_millis();

        let href_select_start = std::time::Instant::now();
        let href = match title_element.value().attr("href") {
            Some(h) => h,
            None => continue,
        };
        let href_select_ms = href_select_start.elapsed().as_millis();
        let article_url = format!("https://pubmed.ncbi.nlm.nih.gov{}", href);

        let authors_start = std::time::Instant::now();
        let authors = result
            .select(&authors_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();
        let authors_ms = authors_start.elapsed().as_millis();

        let citation_start = std::time::Instant::now();
        let citation = result
            .select(&journal_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();
        let citation_ms = citation_start.elapsed().as_millis();

        let pmid_start = std::time::Instant::now();
        let pmid = result
            .select(&pmid_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();
        let pmid_ms = pmid_start.elapsed().as_millis();

        articles.push(PubMedArticle {
            title,
            authors,
            citation,
            pmid,
            url: article_url,
        });

        let iter_elapsed = iter_start.elapsed().as_millis();
        if pubmed_trace_enabled() {
            println!(
                "[PUBMED] docsum {} timings: select_title={}ms title_text={}ms href={}ms authors={}ms citation={}ms pmid={}ms total={}ms",
                docsum_index,
                select_title_ms,
                title_text_ms,
                href_select_ms,
                authors_ms,
                citation_ms,
                pmid_ms,
                iter_elapsed
            );
        }
        docsum_index += 1;
    }

    let parse_ms = parse_start.elapsed().as_millis();
    if pubmed_trace_enabled() {
        println!(
            "[PUBMED] parse completed in {} ms (doc={} ms, body_len={}, limit={})",
            parse_ms, parse_doc_ms, input.content_len, input.limit
        );
    }

    let total_results = articles.len();

    Ok(PubMedSearchResult {
        query: input.query,
        articles,
        total_results,
        page: input.page,
        total_pages: None,
        message: None,
    })
}
