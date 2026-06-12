#[cfg(any(
    feature = "google-drive",
    feature = "google-gmail",
    feature = "google-calendar",
    feature = "google-people"
))]
pub mod google_client {
    use hyper::client::HttpConnector;
    use hyper::Client;
    use hyper_rustls::HttpsConnectorBuilder;
    pub fn new_https_client() -> Client<hyper_rustls::HttpsConnector<HttpConnector>, hyper::Body> {
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .build();
        Client::builder().build::<_, hyper::Body>(https)
    }
}

pub fn should_persist_tokens() -> bool {
    std::env::var("RZN_PERSIST_TOKENS").ok().as_deref() == Some("1")
}

pub fn admin_tools_enabled() -> bool {
    std::env::var("RZN_SHOW_ADMIN_TOOLS").ok().as_deref() == Some("1")
}
