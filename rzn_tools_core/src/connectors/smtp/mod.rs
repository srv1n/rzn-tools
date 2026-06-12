use async_trait::async_trait;
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::Error as SmtpError;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

#[derive(Clone, Debug)]
struct SmtpConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    security: SmtpSecurity,
    skip_tls_verify: bool,
    from_address: Option<String>,
    from_name: Option<String>,
    timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmtpSecurity {
    StartTls,
    Tls,
    Plaintext,
}

impl SmtpSecurity {
    fn from_str(value: Option<&str>) -> Result<Self, ConnectorError> {
        let normalized = value.unwrap_or("starttls").trim().to_lowercase();
        match normalized.as_str() {
            "starttls" | "start_tls" | "start-tls" => Ok(Self::StartTls),
            "tls" | "ssl" => Ok(Self::Tls),
            "plain" | "plaintext" | "none" => Ok(Self::Plaintext),
            other => Err(ConnectorError::InvalidInput(format!(
                "Unsupported SMTP security mode: {}",
                other
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::StartTls => "starttls",
            Self::Tls => "tls",
            Self::Plaintext => "plaintext",
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::StartTls => 587,
            Self::Tls => 465,
            Self::Plaintext => 25,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RecipientInput {
    Single(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct SendMailArgs {
    /// Recipient email(s). Accepts a string ("a@x.com,b@y.com") or string array.
    to: RecipientInput,
    /// Subject line
    subject: String,
    /// Plain text body
    body: String,
    /// Optional HTML body (sent as multipart/alternative with the plain text body)
    #[serde(default)]
    html_body: Option<String>,
    /// Optional "From" override (e.g. "Name <sender@example.com>")
    #[serde(default)]
    from: Option<String>,
    /// Optional Reply-To address
    #[serde(default)]
    reply_to: Option<String>,
    /// Optional CC recipient(s), string or array
    #[serde(default)]
    cc: Option<RecipientInput>,
    /// Optional BCC recipient(s), string or array
    #[serde(default)]
    bcc: Option<RecipientInput>,
    /// Build and validate the email without sending
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct SendMailResponse {
    success: bool,
    dry_run: bool,
    from: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    server_response_code: Option<String>,
    server_response: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TestConnectionResponse {
    success: bool,
    connected: bool,
    host: String,
    port: u16,
    security: String,
}

pub struct SmtpConnector {
    config: Option<SmtpConfig>,
}

impl SmtpConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut connector = Self { config: None };
        if !auth.is_empty() {
            connector.set_auth_details(auth).await?;
        }
        Ok(connector)
    }

    fn ensure_config(&self) -> Result<&SmtpConfig, ConnectorError> {
        self.config.as_ref().ok_or_else(|| {
            ConnectorError::Authentication("SMTP credentials are not configured".to_string())
        })
    }

    fn build_transport(
        config: &SmtpConfig,
    ) -> Result<AsyncSmtpTransport<Tokio1Executor>, ConnectorError> {
        let mut builder = match config.security {
            SmtpSecurity::Tls => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host).map_err(map_smtp_error)?
            }
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                    .map_err(map_smtp_error)?
            }
            SmtpSecurity::Plaintext => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            }
        };

        builder = builder
            .port(config.port)
            .credentials(Credentials::new(
                config.username.clone(),
                config.password.clone(),
            ))
            .timeout(Some(Duration::from_secs(config.timeout_secs)));

        if config.skip_tls_verify && config.security != SmtpSecurity::Plaintext {
            let tls_params = TlsParameters::builder(config.host.clone())
                .dangerous_accept_invalid_certs(true)
                .dangerous_accept_invalid_hostnames(true)
                .build()
                .map_err(map_smtp_error)?;

            let tls_mode = match config.security {
                SmtpSecurity::StartTls => Tls::Required(tls_params),
                SmtpSecurity::Tls => Tls::Wrapper(tls_params),
                SmtpSecurity::Plaintext => unreachable!("plaintext mode has no TLS parameters"),
            };
            builder = builder.tls(tls_mode);
        }

        Ok(builder.build())
    }

    async fn send_mail(&self, args: SendMailArgs) -> Result<SendMailResponse, ConnectorError> {
        let config = self.ensure_config()?;

        let from = build_from_mailbox(config, args.from.as_deref())?;

        let to_values = recipient_input_to_values(args.to);
        let cc_values = args.cc.map(recipient_input_to_values).unwrap_or_default();
        let bcc_values = args.bcc.map(recipient_input_to_values).unwrap_or_default();

        let to_recipients = parse_recipients(&to_values, "to")?;
        let cc_recipients = parse_recipients(&cc_values, "cc")?;
        let bcc_recipients = parse_recipients(&bcc_values, "bcc")?;

        let mut builder = Message::builder()
            .from(from.clone())
            .subject(args.subject.clone());

        for recipient in &to_recipients {
            builder = builder.to(recipient.clone());
        }
        for recipient in &cc_recipients {
            builder = builder.cc(recipient.clone());
        }
        for recipient in &bcc_recipients {
            builder = builder.bcc(recipient.clone());
        }

        if let Some(reply_to) = args.reply_to.as_deref() {
            builder = builder.reply_to(parse_mailbox(reply_to, "reply_to")?);
        }

        let message = if let Some(html_body) = args.html_body {
            builder
                .multipart(MultiPart::alternative_plain_html(args.body, html_body))
                .map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid email message: {}", err))
                })?
        } else {
            builder
                .singlepart(SinglePart::plain(args.body))
                .map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid email message: {}", err))
                })?
        };

        if args.dry_run {
            return Ok(SendMailResponse {
                success: true,
                dry_run: true,
                from: mailbox_to_string(&from),
                to: to_values,
                cc: cc_values,
                bcc: bcc_values,
                subject: args.subject,
                server_response_code: None,
                server_response: Vec::new(),
            });
        }

        let transport = Self::build_transport(config)?;
        let server_response = transport.send(message).await.map_err(map_smtp_error)?;

        Ok(SendMailResponse {
            success: true,
            dry_run: false,
            from: mailbox_to_string(&from),
            to: to_values,
            cc: cc_values,
            bcc: bcc_values,
            subject: args.subject,
            server_response_code: Some(server_response.code().to_string()),
            server_response: server_response.message().map(ToString::to_string).collect(),
        })
    }

    async fn test_connection(&self) -> Result<TestConnectionResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let transport = Self::build_transport(config)?;
        let connected = transport.test_connection().await.map_err(map_smtp_error)?;

        Ok(TestConnectionResponse {
            success: connected,
            connected,
            host: config.host.clone(),
            port: config.port,
            security: config.security.as_str().to_string(),
        })
    }
}

fn parse_recipients(values: &[String], field_name: &str) -> Result<Vec<Mailbox>, ConnectorError> {
    if values.is_empty() {
        if field_name == "to" {
            return Err(ConnectorError::InvalidInput(
                "At least one recipient is required in 'to'".to_string(),
            ));
        }
        return Ok(Vec::new());
    }

    values
        .iter()
        .map(|value| parse_mailbox(value, field_name))
        .collect()
}

fn parse_mailbox(value: &str, field_name: &str) -> Result<Mailbox, ConnectorError> {
    value.parse::<Mailbox>().map_err(|err| {
        ConnectorError::InvalidInput(format!("Invalid {} address: {}", field_name, err))
    })
}

fn build_from_mailbox(
    config: &SmtpConfig,
    from_override: Option<&str>,
) -> Result<Mailbox, ConnectorError> {
    if let Some(from) = from_override {
        return parse_mailbox(from, "from");
    }

    let from_address = config
        .from_address
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(config.username.as_str());

    if let Some(from_name) = config
        .from_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let formatted = format!("{} <{}>", from_name, from_address);
        return parse_mailbox(&formatted, "from");
    }

    parse_mailbox(from_address, "from")
}

fn mailbox_to_string(mailbox: &Mailbox) -> String {
    let email = mailbox.email.to_string();
    if let Some(name) = mailbox.name.as_deref().filter(|value| !value.is_empty()) {
        format!("{} <{}>", name, email)
    } else {
        email
    }
}

fn recipient_input_to_values(input: RecipientInput) -> Vec<String> {
    match input {
        RecipientInput::Single(value) => split_recipient_values(&value),
        RecipientInput::Many(values) => values
            .into_iter()
            .flat_map(|value| split_recipient_values(&value))
            .collect(),
    }
}

fn split_recipient_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_bool(value: Option<&String>) -> bool {
    value.is_some_and(|v| matches!(v.as_str(), "true" | "1" | "yes" | "on"))
}

fn required_field(details: &AuthDetails, key: &str, label: &str) -> Result<String, ConnectorError> {
    let value = details
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ConnectorError::InvalidInput(format!("SMTP {} is required", label)))?;
    Ok(value)
}

fn map_smtp_error(err: SmtpError) -> ConnectorError {
    if err.is_timeout() {
        return ConnectorError::Timeout(format!("SMTP timeout: {}", err));
    }

    if let Some(status) = err.status() {
        let code = status.to_string();
        if code.starts_with("53") {
            return ConnectorError::Authentication(format!("SMTP authentication failed: {}", err));
        }
        if code.starts_with("55") {
            return ConnectorError::InvalidInput(format!("SMTP rejected the message: {}", err));
        }
    }

    if err.is_response() || err.is_client() {
        return ConnectorError::InvalidInput(format!("SMTP request failed: {}", err));
    }

    ConnectorError::Other(format!("SMTP error: {}", err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_recipient_values_handles_csv_and_whitespace() {
        let values = split_recipient_values("a@example.com, b@example.com , ,c@example.com");
        assert_eq!(
            values,
            vec![
                "a@example.com".to_string(),
                "b@example.com".to_string(),
                "c@example.com".to_string()
            ]
        );
    }

    #[test]
    fn recipient_input_to_values_accepts_array_and_csv() {
        let values = recipient_input_to_values(RecipientInput::Many(vec![
            "a@example.com,b@example.com".to_string(),
            "c@example.com".to_string(),
        ]));
        assert_eq!(
            values,
            vec![
                "a@example.com".to_string(),
                "b@example.com".to_string(),
                "c@example.com".to_string()
            ]
        );
    }

    #[test]
    fn smtp_security_parses_expected_aliases() {
        assert_eq!(
            SmtpSecurity::from_str(Some("start_tls")).expect("starttls alias should parse"),
            SmtpSecurity::StartTls
        );
        assert_eq!(
            SmtpSecurity::from_str(Some("ssl")).expect("ssl alias should parse"),
            SmtpSecurity::Tls
        );
        assert_eq!(
            SmtpSecurity::from_str(Some("plain")).expect("plain alias should parse"),
            SmtpSecurity::Plaintext
        );
    }

    #[tokio::test]
    async fn set_auth_details_applies_defaults() {
        let mut connector = SmtpConnector::new(AuthDetails::new())
            .await
            .expect("connector should initialize");

        let mut auth = AuthDetails::new();
        auth.insert("host".to_string(), "smtp.example.com".to_string());
        auth.insert("username".to_string(), "user@example.com".to_string());
        auth.insert("password".to_string(), "secret".to_string());

        connector
            .set_auth_details(auth)
            .await
            .expect("set_auth_details should succeed");

        let details = connector
            .get_auth_details()
            .await
            .expect("auth details should be available");

        assert_eq!(details.get("host"), Some(&"smtp.example.com".to_string()));
        assert_eq!(details.get("port"), Some(&"587".to_string()));
        assert_eq!(details.get("security"), Some(&"starttls".to_string()));
        assert_eq!(details.get("timeout_secs"), Some(&"60".to_string()));
    }

    #[tokio::test]
    async fn set_auth_details_rejects_invalid_port() {
        let mut connector = SmtpConnector::new(AuthDetails::new())
            .await
            .expect("connector should initialize");

        let mut auth = AuthDetails::new();
        auth.insert("host".to_string(), "smtp.example.com".to_string());
        auth.insert("port".to_string(), "invalid".to_string());
        auth.insert("username".to_string(), "user@example.com".to_string());
        auth.insert("password".to_string(), "secret".to_string());

        let err = connector
            .set_auth_details(auth)
            .await
            .expect_err("invalid port should fail");

        assert!(err.to_string().contains("Invalid SMTP port"));
    }
}

#[async_trait]
impl Connector for SmtpConnector {
    fn name(&self) -> &'static str {
        "smtp"
    }

    fn description(&self) -> &'static str {
        "An SMTP connector for sending outbound email."
    }

    fn display_name(&self) -> &'static str {
        "SMTP Mail"
    }

    fn icon(&self) -> &'static str {
        "mailgun"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["email", "productivity"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: Some("https://lettre.rs".to_string()),
            },
            instructions: Some(
                "Use the SMTP connector to send outbound emails after explicit user confirmation."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("send_mail"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Send an outbound email via SMTP (requires explicit user permission).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "to": {
                                "oneOf": [
                                    { "type": "string", "description": "Single address or comma-separated addresses." },
                                    { "type": "array", "items": { "type": "string" }, "description": "Recipient list." }
                                ]
                            },
                            "subject": { "type": "string", "description": "Email subject line." },
                            "body": { "type": "string", "description": "Plain-text email body." },
                            "html_body": { "type": "string", "description": "Optional HTML body sent as multipart/alternative." },
                            "from": { "type": "string", "description": "Optional From override (e.g. 'Name <sender@example.com>')." },
                            "reply_to": { "type": "string", "description": "Optional Reply-To address." },
                            "cc": {
                                "oneOf": [
                                    { "type": "string" },
                                    { "type": "array", "items": { "type": "string" } }
                                ],
                                "description": "Optional CC recipient(s)."
                            },
                            "bcc": {
                                "oneOf": [
                                    { "type": "string" },
                                    { "type": "array", "items": { "type": "string" } }
                                ],
                                "description": "Optional BCC recipient(s)."
                            },
                            "dry_run": { "type": "boolean", "description": "If true, validate/build but do not send." }
                        },
                        "required": ["to", "subject", "body"]
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("test_connection"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Verify SMTP connectivity and authentication using NOOP.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {}
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let args_value = Value::Object(request.arguments.unwrap_or_default());

        match request.name.as_ref() {
            "send_mail" => {
                let args: SendMailArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.send_mail(args).await?;
                structured_result_with_text(&result, None)
            }
            "test_connection" => {
                let result = self.test_connection().await?;
                structured_result_with_text(&result, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut auth = AuthDetails::new();
        if let Some(config) = self.config.as_ref() {
            auth.insert("host".to_string(), config.host.clone());
            auth.insert("port".to_string(), config.port.to_string());
            auth.insert("username".to_string(), config.username.clone());
            auth.insert("security".to_string(), config.security.as_str().to_string());
            auth.insert(
                "skip_tls_verify".to_string(),
                config.skip_tls_verify.to_string(),
            );
            auth.insert("timeout_secs".to_string(), config.timeout_secs.to_string());

            if let Some(from_address) = config.from_address.as_ref() {
                auth.insert("from_address".to_string(), from_address.clone());
            }
            if let Some(from_name) = config.from_name.as_ref() {
                auth.insert("from_name".to_string(), from_name.clone());
            }
        }
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let host = required_field(&details, "host", "host")?;
        let username = required_field(&details, "username", "username")?;
        let password = required_field(&details, "password", "password")?;

        let security = if let Some(security) = details.get("security") {
            SmtpSecurity::from_str(Some(security.as_str()))?
        } else if parse_bool(details.get("tls")) {
            SmtpSecurity::Tls
        } else {
            SmtpSecurity::from_str(None)?
        };

        let port = details
            .get("port")
            .map(|value| {
                value.parse::<u16>().map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid SMTP port: {}", err))
                })
            })
            .transpose()?
            .unwrap_or_else(|| security.default_port());

        let skip_tls_verify = parse_bool(details.get("skip_tls_verify"));

        let from_address = details
            .get("from_address")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let from_name = details
            .get("from_name")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let timeout_secs = details
            .get("timeout_secs")
            .or_else(|| details.get("timeout"))
            .map(|value| {
                value.parse::<u64>().map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid timeout seconds: {}", err))
                })
            })
            .transpose()?
            .filter(|value| *value > 0)
            .unwrap_or(60);

        self.config = Some(SmtpConfig {
            host,
            port,
            username,
            password,
            security,
            skip_tls_verify,
            from_address,
            from_name,
            timeout_secs,
        });

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let status = self.test_connection().await?;
        if status.connected {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "SMTP test connection failed".to_string(),
            ))
        }
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "host".to_string(),
                    label: "SMTP Host".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    description: Some("Hostname of the SMTP server.".to_string()),
                    options: None,
                },
                Field {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some(
                        "SMTP port (defaults: 587 starttls, 465 tls, 25 plaintext).".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    description: Some("SMTP account username, usually an email address.".to_string()),
                    options: None,
                },
                Field {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("SMTP password or app-specific password.".to_string()),
                    options: None,
                },
                Field {
                    name: "security".to_string(),
                    label: "Security".to_string(),
                    field_type: FieldType::Select {
                        options: vec![
                            "starttls".to_string(),
                            "tls".to_string(),
                            "plaintext".to_string(),
                        ],
                    },
                    required: false,
                    description: Some("Connection security mode.".to_string()),
                    options: None,
                },
                Field {
                    name: "tls".to_string(),
                    label: "Use TLS".to_string(),
                    field_type: FieldType::Boolean,
                    required: false,
                    description: Some(
                        "Legacy toggle. If security is omitted, true selects tls and false keeps starttls."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "skip_tls_verify".to_string(),
                    label: "Skip TLS Verification".to_string(),
                    field_type: FieldType::Boolean,
                    required: false,
                    description: Some(
                        "Allow invalid TLS certificates/hostnames (not recommended).".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "from_address".to_string(),
                    label: "Default From Address".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Default sender address for send_mail. Falls back to username.".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "from_name".to_string(),
                    label: "Default From Name".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional display name for the sender.".to_string()),
                    options: None,
                },
                Field {
                    name: "timeout_secs".to_string(),
                    label: "Timeout Seconds".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some("SMTP command timeout in seconds (default 60).".to_string()),
                    options: None,
                },
            ],
        }
    }
}
