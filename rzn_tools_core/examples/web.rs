use htmd::HtmlToMarkdown;
use reqwest::blocking::Client;
use rookie::{common::enums::CookieToString, firefox};
use rzn_tools_core::{
    connectors::web::find_main_content,
    error::ConnectorError,
    utils::{get_domain, strip_multiple_newlines},
};
use scraper::Html;
use termimad::{crossterm::style::Color, MadSkin, StyledChar};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a custom cookie store
    let client = Client::new();

    // let url = "https://gwern.net/scaling-hypothesis";
    let url = "https://carelesswhisper.app";
    let domain = get_domain(url).map_err(|e| ConnectorError::Other(e.to_string()))?;
    let cookies = firefox(Some(vec![domain.into()]))?;

    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
Chrome/117.0.0.0 Safari/537.36",
        )
        .header("Cookie", cookies.to_string())
        .send()?;

    let content = response.text()?;
    let content = strip_multiple_newlines(&content);
    let html = Html::parse_document(&content);

    // Try to find the main content using common content selectors
    // This is similar to how readability algorithms like Turndown.js work
    let content_element = find_main_content(&html);

    // Convert the content element to HTML string
    let content_html = content_element;
    // println!("Content: {:#?}", content_html);

    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "footer", "header", "aside", "href", "img", "src",
        ])
        .build();

    let mut skin = MadSkin::default();
    skin.set_headers_fg(Color::Rgb {
        r: 255,
        g: 187,
        b: 0,
    });

    skin.bold.set_fg(Color::Rgb {
        r: 255,
        g: 215,
        b: 0,
    });
    skin.italic.set_fg(Color::Rgb {
        r: 245,
        g: 245,
        b: 220,
    });
    skin.inline_code.set_fg(Color::Rgb {
        r: 255,
        g: 215,
        b: 0,
    });

    // Add proper styling for code blocks
    skin.code_block.set_fg(Color::Rgb {
        r: 255,
        g: 215,
        b: 0,
    });
    skin.code_block.set_bg(Color::Rgb {
        r: 40,
        g: 44,
        b: 52,
    });

    // Set pre block styling

    // skin.paragraph.set_fgbg(Color::Magenta, Color::Rgb { r: 30, g: 30, b: 40 });
    skin.bullet = StyledChar::from_fg_char(Color::Yellow, '•');

    let markdown = converter
        .convert(&content_html)
        .unwrap_or_else(|_| content_html.clone());
    // println!("Markdown: {:?}", markdown);
    let markdown = markdown
        .replace("\n\n```", "\n```")
        .replace("```\n\n", "```\n")
        .replace("\n\n>", "\n>")
        .replace(">\n\n", ">\n");

    let markdown = strip_multiple_newlines(&markdown);

    skin.print_text(&markdown);

    Ok(())
}

// Function to find the main content of a webpage using common selectors and heuristics
