// Meta-connectors (always available)
pub mod federated;

// Local filesystem connector
#[cfg(feature = "localfs")]
pub mod localfs;

#[cfg(feature = "arxiv")]
pub mod arxiv;
#[cfg(feature = "atlassian")]
pub mod atlassian;
#[cfg(feature = "biorxiv")]
pub mod biorxiv;
#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "github")]
pub mod github;
#[cfg(feature = "google-scholar")]
pub mod google_scholar;
#[cfg(feature = "hackernews")]
pub mod hackernews;
#[cfg(feature = "imap")]
pub mod imap;
#[cfg(feature = "kalshi")]
pub mod kalshi;
#[cfg(feature = "linkedin")]
pub mod linkedin;
#[cfg(feature = "macos-automation")]
pub mod macos;
#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(all(target_os = "macos", feature = "macos-spotlight"))]
pub mod spotlight;
// EXPERIMENTAL - NOT READY: HealthKit data store not available on macOS
// See: rzn_tools_core/src/connectors/apple_health/NOT_READY.md
// #[cfg(all(target_os = "macos", feature = "apple-health"))]
// pub mod apple_health;
#[cfg(feature = "app-store")]
pub mod app_store;
#[cfg(feature = "app-store-connect")]
pub mod app_store_connect;
#[cfg(feature = "apple-search-ads")]
pub mod apple_search_ads;
#[cfg(feature = "play-store")]
pub mod play_store;
#[cfg(feature = "polymarket")]
pub mod polymarket;
#[cfg(feature = "pubmed")]
pub mod pubmed;
#[cfg(feature = "reddit")]
pub mod reddit;
#[cfg(feature = "rss")]
pub mod rss;
#[cfg(feature = "scihub")]
pub mod scihub;
#[cfg(feature = "semantic-scholar")]
pub mod semantic_scholar;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(feature = "weather")]
pub mod weather;
#[cfg(any(feature = "web", feature = "web-lite"))]
pub mod web;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;
#[cfg(feature = "wikipedia")]
pub mod wikipedia;
#[cfg(feature = "x-api")]
pub mod x;
#[cfg(feature = "x-twitter")]
pub mod x_browser;
#[cfg(feature = "youtube")]
pub mod youtube;

// LLM provider web search
#[cfg(feature = "anthropic-search")]
pub mod anthropic_search;
#[cfg(feature = "exa-search")]
pub mod exa_search;
#[cfg(feature = "firecrawl-search")]
pub mod firecrawl_search;
#[cfg(feature = "gemini-search")]
pub mod gemini_search;
#[cfg(feature = "openai-search")]
pub mod openai_search;
#[cfg(feature = "parallel-search")]
pub mod parallel_search;
#[cfg(feature = "perplexity-search")]
pub mod perplexity_search;
#[cfg(feature = "serpapi-search")]
pub mod serpapi_search;
#[cfg(feature = "serper-search")]
pub mod serper_search;
#[cfg(feature = "tavily-search")]
pub mod tavily_search;
#[cfg(feature = "xai-search")]
pub mod xai_search;

// Productivity & Cloud (Phase 1)
#[cfg(feature = "bing-webmaster-tools")]
pub mod bing_webmaster_tools;
#[cfg(feature = "caldav")]
pub mod caldav;
#[cfg(feature = "google-calendar")]
pub mod google_calendar;
#[cfg(feature = "google-drive")]
pub mod google_drive;
#[cfg(feature = "google-gmail")]
pub mod google_gmail;
#[cfg(feature = "google-people")]
pub mod google_people;
#[cfg(feature = "google-search-console")]
pub mod google_search_console;
#[cfg(feature = "microsoft-graph")]
pub mod microsoft;

// Apple Ecosystem (macOS only) - Native app integrations via AppleScript
// These connectors require macOS and use system apps without separate credentials
#[cfg(all(
    target_os = "macos",
    any(
        feature = "apple-mail",
        feature = "apple-notes",
        feature = "apple-messages",
        feature = "apple-reminders",
        feature = "apple-contacts"
    )
))]
pub mod apple_common;

#[cfg(all(target_os = "macos", feature = "apple-contacts"))]
pub mod apple_contacts;
#[cfg(all(target_os = "macos", feature = "apple-mail"))]
pub mod apple_mail;
#[cfg(all(target_os = "macos", feature = "apple-messages"))]
pub mod apple_messages;
#[cfg(all(target_os = "macos", feature = "apple-notes"))]
pub mod apple_notes;
#[cfg(all(target_os = "macos", feature = "apple-reminders"))]
pub mod apple_reminders;
