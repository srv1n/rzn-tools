//! Outbound HTTP for connectors.
//!
//! Every reqwest client a connector uses is built here, so a connection's `proxy_url` applies to
//! all of its traffic. The proxy URL may carry a user and password: it is never logged, and errors
//! name only its scheme and host.

use std::ops::Deref;
use std::sync::Arc;

use reqwest::{Client, ClientBuilder};
use url::Url;

use crate::auth::AuthDetails;
use crate::capabilities::{Field, FieldType};
use crate::error::ConnectorError;

/// Auth-details key that sends a connector's traffic through a proxy.
pub const PROXY_URL_FIELD: &str = "proxy_url";

const PROXY_SCHEMES: [&str; 4] = ["http", "https", "socks5", "socks5h"];

/// The common `proxy_url` auth field, declared once for every network connector.
pub fn proxy_url_field() -> Field {
    Field {
        name: PROXY_URL_FIELD.to_string(),
        label: "Proxy URL".to_string(),
        field_type: FieldType::Secret,
        required: false,
        description: Some(
            "Optional proxy for all of this connector's traffic: http://, https://, socks5:// or \
             socks5h://host:port, may include user:pass"
                .to_string(),
        ),
        options: None,
    }
}

/// Where a connector's requests go: direct, or through one proxy.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Egress {
    proxy_url: Option<String>,
}

impl std::fmt::Debug for Egress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.proxy_url {
            None => f.write_str("Egress::Direct"),
            Some(url) => write!(f, "Egress::Proxy({})", redact(url)),
        }
    }
}

impl Egress {
    /// Reads `proxy_url` from auth details. Absent or blank means direct.
    pub fn from_auth(details: &AuthDetails) -> Result<Self, ConnectorError> {
        let Some(raw) = details
            .get(PROXY_URL_FIELD)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(Self::default());
        };
        let invalid =
            |reason: &str| ConnectorError::InvalidParams(format!("Invalid proxy_url: {reason}"));
        let url = Url::parse(raw).map_err(|_| {
            invalid("expected scheme://host:port with scheme http, https, socks5 or socks5h")
        })?;
        if !PROXY_SCHEMES.contains(&url.scheme()) || url.host_str().is_none() {
            return Err(invalid(&format!(
                "{} is not a proxy; use http, https, socks5 or socks5h with a host",
                redact(raw)
            )));
        }
        reqwest::Proxy::all(raw).map_err(|_| invalid(&format!("{} was rejected", redact(raw))))?;
        Ok(Self {
            proxy_url: Some(raw.to_string()),
        })
    }

    /// The proxy URL, for subprocesses such as yt-dlp. Never log it.
    pub fn proxy_url(&self) -> Option<&str> {
        self.proxy_url.as_deref()
    }

    /// Builds a client from a fresh builder that `configure` adjusts.
    ///
    /// Direct clients keep a no-proxy fallback for platforms where reading the system proxy
    /// settings panics. A client with a proxy never falls back to direct.
    pub fn client(
        &self,
        configure: impl Fn(ClientBuilder) -> ClientBuilder,
    ) -> Result<Client, ConnectorError> {
        let Some(url) = &self.proxy_url else {
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                configure(Client::builder()).build()
            }));
            return match built {
                Ok(Ok(client)) => Ok(client),
                Ok(Err(e)) => Err(ConnectorError::HttpRequest(e)),
                Err(_) => Ok(configure(Client::builder()).no_proxy().build()?),
            };
        };
        let failed = || {
            ConnectorError::Other(format!(
                "Failed to build HTTP client for proxy {}",
                redact(url)
            ))
        };
        let proxy = reqwest::Proxy::all(url.as_str()).map_err(|_| failed())?;
        configure(Client::builder())
            .proxy(proxy)
            .build()
            .map_err(|_| failed())
    }
}

type Configure = Arc<dyn Fn(ClientBuilder) -> ClientBuilder + Send + Sync>;

/// A connector's stored client. It remembers how it was configured, so a connector rebuilds it
/// for a new `proxy_url` with one call to [`HttpClient::apply_auth`] in `set_auth_details`.
/// Dereferences to [`reqwest::Client`].
#[derive(Clone)]
pub struct HttpClient {
    configure: Configure,
    egress: Egress,
    client: Client,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("egress", &self.egress)
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    pub fn new(
        egress: Egress,
        configure: impl Fn(ClientBuilder) -> ClientBuilder + Send + Sync + 'static,
    ) -> Result<Self, ConnectorError> {
        let client = egress.client(&configure)?;
        Ok(Self {
            configure: Arc::new(configure),
            egress,
            client,
        })
    }

    /// A direct client with default settings.
    pub fn direct() -> Result<Self, ConnectorError> {
        Self::new(Egress::default(), |builder| builder)
    }

    /// Applies the `proxy_url` in `details` (absent means direct), rebuilding only on change.
    pub fn apply_auth(&mut self, details: &AuthDetails) -> Result<(), ConnectorError> {
        let egress = Egress::from_auth(details)?;
        if egress != self.egress {
            self.client = egress.client(&*self.configure)?;
            self.egress = egress;
        }
        Ok(())
    }
}

impl Deref for HttpClient {
    type Target = Client;

    fn deref(&self) -> &Client {
        &self.client
    }
}

fn redact(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(url) => match url.port() {
            Some(port) => format!(
                "{}://{}:{port}",
                url.scheme(),
                url.host_str().unwrap_or("?")
            ),
            None => format!("{}://{}", url.scheme(), url.host_str().unwrap_or("?")),
        },
        Err(_) => "the given URL".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::mpsc;

    fn auth(proxy_url: &str) -> AuthDetails {
        let mut details = AuthDetails::new();
        details.insert(PROXY_URL_FIELD.to_string(), proxy_url.to_string());
        details
    }

    /// One-shot HTTP server, used as a proxy: records the request line, answers 200 "ok".
    /// Returns its address as `host:port`.
    fn spawn_recording_server() -> (String, mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let text = String::from_utf8_lossy(&buf);
            tx.send(text.lines().next().unwrap_or_default().to_string())
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        (addr.to_string(), rx)
    }

    #[tokio::test]
    async fn requests_go_through_the_configured_proxy() {
        let (proxy_addr, request_lines) = spawn_recording_server();
        let mut client = HttpClient::direct().unwrap();
        client
            .apply_auth(&auth(&format!("http://user:secret@{proxy_addr}")))
            .unwrap();

        let body = client
            .get("http://upstream.invalid/probe")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        assert_eq!(body, "ok");
        let line = request_lines.recv().unwrap();
        assert_eq!(line, "GET http://upstream.invalid/probe HTTP/1.1");
    }

    #[tokio::test]
    async fn unreachable_proxy_fails_instead_of_going_direct() {
        // Bind then drop to get a closed local port.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // A live upstream that would answer if the request went direct.
        let (upstream_addr, _requests) = spawn_recording_server();
        let egress = Egress::from_auth(&auth(&format!("socks5h://127.0.0.1:{port}"))).unwrap();
        let client = egress
            .client(|b| b.timeout(std::time::Duration::from_secs(5)))
            .unwrap();

        let result = client.get(format!("http://{upstream_addr}/")).send().await;
        assert!(result.is_err(), "request must not bypass a dead proxy");
    }

    #[test]
    fn invalid_proxy_url_is_rejected_without_echoing_credentials() {
        for raw in ["not a url", "ftp://user:secret@host:21", "localhost:1080"] {
            let err = Egress::from_auth(&auth(raw)).unwrap_err();
            match err {
                ConnectorError::InvalidParams(message) => assert!(!message.contains("secret")),
                other => panic!("expected InvalidParams, got: {other:?}"),
            }
        }
        let egress = Egress::from_auth(&auth("socks5://user:secret@10.0.0.1:1080")).unwrap();
        assert!(!format!("{egress:?}").contains("secret"));
        assert_eq!(Egress::from_auth(&auth("  ")).unwrap(), Egress::default());
    }

    /// Every reqwest client must be built here, so a connection's proxy reaches it.
    #[test]
    fn clients_are_built_only_in_the_http_module() {
        // Local-only code that never reaches the internet, or a non-reqwest client.
        const EXEMPT: [&str; 2] = [
            // Talks to the WuzAPI bridge it starts on this machine.
            "connectors/whatsapp/mod.rs",
            // hyper client handed to the google-apis crates; reqwest's proxy cannot apply.
            "oauth_client.rs",
        ];
        const BANNED: [&str; 5] = [
            "Client::builder",
            "Client::new()",
            "Client::default()",
            "ClientBuilder::new()",
            "reqwest::get(",
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel == "http.rs" || EXEMPT.contains(&rel.as_str()) {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for (n, line) in source.lines().enumerate() {
                    if BANNED.iter().any(|pattern| line.contains(pattern)) {
                        offenders.push(format!("{rel}:{}: {}", n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "build reqwest clients through crate::http (HttpClient or Egress::client):\n{}",
            offenders.join("\n")
        );
    }
}
