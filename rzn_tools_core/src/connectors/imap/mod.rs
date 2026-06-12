use async_trait::async_trait;
use base64::Engine;
use imap::Error as ImapError;
use imap::{ClientBuilder, Connection as ImapConnection, ConnectionMode, Session};
use imap_proto::types::NameAttribute;
use mailparse::{parse_mail, ParsedMail};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::task;
use tracing::debug;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;

#[derive(Clone, Debug)]
struct ImapConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    security: SecurityMode,
    skip_tls_verify: bool,
    default_mailbox: String,
    fetch_limit: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SecurityMode {
    AutoTls,
    Auto,
    Tls,
    StartTls,
    Plaintext,
}

impl SecurityMode {
    fn from_str(value: Option<&str>) -> Result<Self, ConnectorError> {
        let normalized = value.unwrap_or("autotls").trim().to_lowercase();
        match normalized.as_str() {
            "autotls" | "auto_tls" | "auto-tls" => Ok(SecurityMode::AutoTls),
            "auto" => Ok(SecurityMode::Auto),
            "tls" => Ok(SecurityMode::Tls),
            "starttls" | "start_tls" | "start-tls" => Ok(SecurityMode::StartTls),
            "plain" | "plaintext" => Ok(SecurityMode::Plaintext),
            other => Err(ConnectorError::InvalidInput(format!(
                "Unsupported IMAP security mode: {}",
                other
            ))),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            SecurityMode::AutoTls => "autotls",
            SecurityMode::Auto => "auto",
            SecurityMode::Tls => "tls",
            SecurityMode::StartTls => "starttls",
            SecurityMode::Plaintext => "plaintext",
        }
    }

    fn to_connection_mode(self) -> ConnectionMode {
        match self {
            SecurityMode::AutoTls => ConnectionMode::AutoTls,
            SecurityMode::Auto => ConnectionMode::Auto,
            SecurityMode::Tls => ConnectionMode::Tls,
            SecurityMode::StartTls => ConnectionMode::StartTls,
            SecurityMode::Plaintext => ConnectionMode::Plaintext,
        }
    }
}

pub struct ImapConnector {
    config: Option<ImapConfig>,
}

impl ImapConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut connector = Self { config: None };
        if !auth.is_empty() {
            connector.set_auth_details(auth).await?;
        }
        Ok(connector)
    }

    fn ensure_config(&self) -> Result<&ImapConfig, ConnectorError> {
        self.config.as_ref().ok_or_else(|| {
            ConnectorError::Authentication("IMAP credentials are not configured".to_string())
        })
    }

    async fn with_session<F, T>(&self, f: F) -> Result<T, ConnectorError>
    where
        F: FnOnce(&mut Session<ImapConnection>) -> Result<T, ConnectorError> + Send + 'static,
        T: Send + 'static,
    {
        let config = self.config.clone().ok_or_else(|| {
            ConnectorError::Authentication("IMAP credentials are not configured".to_string())
        })?;

        task::spawn_blocking(move || {
            let mut session = Self::connect_session(&config)?;
            let result = f(&mut session);
            if let Err(err) = session.logout() {
                debug!("IMAP logout error: {}", err);
            }
            result
        })
        .await
        .map_err(|err| ConnectorError::Other(format!("IMAP task join error: {}", err)))?
    }

    fn connect_session(config: &ImapConfig) -> Result<Session<ImapConnection>, ConnectorError> {
        let builder = ClientBuilder::new(config.host.as_str(), config.port)
            .mode(config.security.to_connection_mode());
        let builder = if config.skip_tls_verify {
            builder.danger_skip_tls_verify(true)
        } else {
            builder
        };

        let client = builder.connect().map_err(map_imap_error)?;
        let session = client
            .login(&config.username, &config.password)
            .map_err(|(err, _)| map_auth_error(err))?;
        Ok(session)
    }

    async fn list_mailboxes(
        &self,
        args: ListMailboxesArgs,
    ) -> Result<Vec<MailboxInfo>, ConnectorError> {
        let reference = args.reference.unwrap_or_default();
        let pattern = args.pattern.unwrap_or_else(|| "*".to_string());
        let include_subscribed = args.include_subscribed;

        self.with_session(move |session| {
            let reference_opt = if reference.trim().is_empty() {
                None
            } else {
                Some(reference.as_str())
            };

            let names = session
                .list(reference_opt, Some(pattern.as_str()))
                .map_err(map_imap_error)?;

            let subscribed = if include_subscribed {
                let subs = session
                    .lsub(reference_opt, Some(pattern.as_str()))
                    .map_err(map_imap_error)?;
                let mut set = HashSet::new();
                for entry in subs.iter() {
                    set.insert(entry.name().to_string());
                }
                Some(set)
            } else {
                None
            };

            let mut results = Vec::new();
            for entry in names.iter() {
                let name_str = entry.name().to_string();
                let is_selectable = !entry
                    .attributes()
                    .iter()
                    .any(|attr| matches!(attr, NameAttribute::NoSelect));
                let is_subscribed = subscribed
                    .as_ref()
                    .is_some_and(|set| set.contains(&name_str));

                results.push(MailboxInfo {
                    name: name_str,
                    delimiter: entry.delimiter().map(|s| s.to_string()),
                    attributes: entry.attributes().iter().map(attribute_to_string).collect(),
                    selectable: is_selectable,
                    subscribed: is_subscribed,
                });
            }

            Ok(results)
        })
        .await
    }

    async fn fetch_messages(
        &self,
        args: FetchMessagesArgs,
    ) -> Result<FetchMessagesResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());

        let mut desired = args.limit.unwrap_or(config.fetch_limit);
        if desired == 0 {
            desired = config.fetch_limit;
        }
        desired = desired.clamp(1, 5_000);

        let start_cursor = Some(ImapFetchCursor {
            before_uid: args.before_uid,
            offset: args.offset.unwrap_or(0),
        });

        #[derive(Default)]
        struct Meta {
            total: Option<usize>,
        }
        let meta = Arc::new(Mutex::new(Meta::default()));
        let mailbox_for_fetch = mailbox.clone();

        let collected = collect_paginated_with_cursor(
            desired,
            20,
            start_cursor,
            |cursor, remaining| {
                let meta = Arc::clone(&meta);
                let mailbox_for_fetch = mailbox_for_fetch.clone();
                async move {
                    let cursor = cursor.unwrap_or(ImapFetchCursor {
                        before_uid: None,
                        offset: 0,
                    });
                    let per_page = remaining.clamp(1, 500);

                    let page = self
                        .fetch_messages_page(
                            mailbox_for_fetch,
                            per_page,
                            cursor.offset,
                            cursor.before_uid,
                        )
                        .await?;

                    {
                        let mut m = meta.lock().map_err(|_| {
                            ConnectorError::Other("IMAP meta lock poisoned".to_string())
                        })?;
                        if m.total.is_none() {
                            m.total = Some(page.total);
                        }
                    }

                    Ok::<_, ConnectorError>(Page {
                        items: page.messages,
                        next_cursor: page.next_cursor.map(|u| ImapFetchCursor {
                            before_uid: Some(u),
                            offset: 0,
                        }),
                    })
                }
            },
            |m: &MessageSummary| m.uid.map(|u| u.to_string()),
        )
        .await?;

        let meta = Arc::try_unwrap(meta)
            .ok()
            .and_then(|m| m.into_inner().ok())
            .unwrap_or_default();

        let next_cursor = collected
            .next_cursor
            .and_then(|c| c.before_uid)
            .filter(|u| *u > 0);

        Ok(FetchMessagesResponse {
            returned: collected.items.len(),
            has_more: next_cursor.is_some(),
            next_cursor,
            messages: collected.items,
            mailbox,
            total: meta.total.unwrap_or(0),
        })
    }

    async fn fetch_messages_page(
        &self,
        mailbox: String,
        limit: usize,
        offset: usize,
        before_uid: Option<u32>,
    ) -> Result<FetchMessagesResponse, ConnectorError> {
        let mailbox_clone = mailbox.clone();
        let limit = limit.clamp(1, 500);

        self.with_session(move |session| {
            session.select(&mailbox_clone).map_err(map_imap_error)?;
            let mut uids: Vec<u32> = session
                .uid_search("ALL")
                .map_err(map_imap_error)?
                .into_iter()
                .collect();

            let total = uids.len();

            if uids.is_empty() {
                return Ok(FetchMessagesResponse {
                    messages: Vec::new(),
                    mailbox: mailbox_clone,
                    total: 0,
                    returned: 0,
                    has_more: false,
                    next_cursor: None,
                });
            }

            uids.sort_unstable();

            // Apply before_uid filter (cursor-based pagination)
            if let Some(before) = before_uid {
                uids.retain(|&uid| uid < before);
            }

            // Calculate pagination window (from end, with offset)
            let available = uids.len();
            let skip_from_end = offset;
            let end_idx = available.saturating_sub(skip_from_end);
            let start_idx = end_idx.saturating_sub(limit);

            let selected: Vec<String> = uids[start_idx..end_idx]
                .iter()
                .map(|uid| uid.to_string())
                .collect();

            if selected.is_empty() {
                return Ok(FetchMessagesResponse {
                    messages: Vec::new(),
                    mailbox: mailbox_clone,
                    total,
                    returned: 0,
                    has_more: false,
                    next_cursor: None,
                });
            }

            let sequence = selected.join(",");
            let fetches = session
                .uid_fetch(&sequence, "(UID ENVELOPE FLAGS INTERNALDATE RFC822.SIZE)")
                .map_err(map_imap_error)?;

            let mut messages = Vec::new();
            for fetch in fetches.iter() {
                messages.push(build_message_summary(fetch));
            }

            // Sort messages by UID descending (newest first)
            messages.sort_by(|a, b| b.uid.cmp(&a.uid));

            let returned = messages.len();
            let has_more = start_idx > 0;
            let next_cursor = if has_more {
                // The smallest UID in current batch - use as before_uid for next page
                messages.last().and_then(|m| m.uid)
            } else {
                None
            };

            Ok(FetchMessagesResponse {
                messages,
                mailbox: mailbox_clone,
                total,
                returned,
                has_more,
                next_cursor,
            })
        })
        .await
    }

    async fn get_message(&self, args: GetMessageArgs) -> Result<MessageDetails, ConnectorError> {
        let config = self.ensure_config()?;
        let mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());
        let include_raw = args.include_raw;
        let include_html = args.include_html;
        let include_headers = args.include_headers;
        let uid = args.uid;

        self.with_session(move |session| {
            session.select(&mailbox).map_err(map_imap_error)?;
            let fetches = session
                .uid_fetch(
                    uid.to_string(),
                    "(UID ENVELOPE FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])",
                )
                .map_err(map_imap_error)?;
            let fetch = fetches
                .iter()
                .next()
                .ok_or(ConnectorError::ResourceNotFound)?;

            let summary = build_message_summary(fetch);
            let raw_body = fetch.body().map(|b| b.to_vec());
            let (text_body, html_body, headers) = raw_body
                .as_ref()
                .map(|body| parse_message_bodies(body))
                .unwrap_or_default();

            // Determine best content: prefer text, fall back to converted HTML
            let (content, content_source) = if let Some(ref text) = text_body {
                if !text.trim().is_empty() {
                    (Some(text.clone()), "text".to_string())
                } else if let Some(ref html) = html_body {
                    let converted = crate::utils::html_to_text(html);
                    if !converted.is_empty() {
                        (Some(converted), "html_converted".to_string())
                    } else {
                        (None, "none".to_string())
                    }
                } else {
                    (None, "none".to_string())
                }
            } else if let Some(ref html) = html_body {
                let converted = crate::utils::html_to_text(html);
                if !converted.is_empty() {
                    (Some(converted), "html_converted".to_string())
                } else {
                    (None, "none".to_string())
                }
            } else {
                (None, "none".to_string())
            };

            let raw_encoded = if include_raw {
                raw_body
                    .as_ref()
                    .map(|body| base64::engine::general_purpose::STANDARD.encode(body))
            } else {
                None
            };

            Ok(MessageDetails {
                summary,
                content,
                content_source,
                // Only include headers if explicitly requested
                headers: if include_headers { Some(headers) } else { None },
                // Only include text_body if different from content (for debugging)
                text_body: None, // Skip to reduce output size
                // Only include HTML if explicitly requested
                html_body: if include_html { html_body } else { None },
                raw: raw_encoded,
            })
        })
        .await
    }

    async fn search(&self, args: SearchArgs) -> Result<SearchResults, ConnectorError> {
        let config = self.ensure_config()?;
        let mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());
        let query = args.query;
        let mut limit = args.limit.unwrap_or(config.fetch_limit);
        if limit == 0 {
            limit = config.fetch_limit;
        }
        limit = limit.clamp(1, 1000);

        self.with_session(move |session| {
            session.select(&mailbox).map_err(map_imap_error)?;
            let mut uids: Vec<u32> = session
                .uid_search(&query)
                .map_err(map_imap_error)?
                .into_iter()
                .collect();
            uids.sort_unstable();
            if uids.len() > limit {
                uids.drain(0..uids.len() - limit);
            }

            Ok(SearchResults {
                mailbox,
                query,
                uids,
            })
        })
        .await
    }

    async fn create_draft(
        &self,
        args: CreateDraftArgs,
    ) -> Result<CreateDraftResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let drafts_mailbox = args.drafts_mailbox.unwrap_or_else(|| "Drafts".to_string());

        // Build RFC 822 message
        let from = &config.username;
        let date = chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S +0000")
            .to_string();
        let message_id = format!(
            "<{}.{:x}@rzn-tools.local>",
            chrono::Utc::now().timestamp_millis(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        let mut headers = vec![
            format!("From: {}", from),
            format!("To: {}", args.to),
            format!("Subject: {}", args.subject),
            format!("Date: {}", date),
            format!("Message-ID: {}", message_id),
            "MIME-Version: 1.0".to_string(),
            "Content-Type: text/plain; charset=utf-8".to_string(),
        ];

        if let Some(cc) = &args.cc {
            if !cc.is_empty() {
                headers.push(format!("Cc: {}", cc));
            }
        }

        if let Some(bcc) = &args.bcc {
            if !bcc.is_empty() {
                headers.push(format!("Bcc: {}", bcc));
            }
        }

        if let Some(in_reply_to) = &args.in_reply_to {
            headers.push(format!("In-Reply-To: {}", in_reply_to));
        }

        if let Some(references) = &args.references {
            headers.push(format!("References: {}", references));
        }

        // Combine headers and body
        let message = format!("{}\r\n\r\n{}", headers.join("\r\n"), args.body);
        let message_bytes: Vec<u8> = message.into_bytes();

        let drafts_mailbox_clone = drafts_mailbox.clone();
        self.with_session(move |session| {
            // Append to drafts folder with \Draft flag
            session
                .append(&drafts_mailbox_clone, &message_bytes)
                .flag(imap::types::Flag::Draft)
                .flag(imap::types::Flag::Seen)
                .finish()
                .map_err(map_imap_error)?;

            let msg = format!("Draft saved to {} folder", drafts_mailbox_clone);
            Ok(CreateDraftResponse {
                success: true,
                mailbox: drafts_mailbox_clone,
                message: msg,
            })
        })
        .await
    }

    async fn move_messages(
        &self,
        args: MoveMessagesArgs,
    ) -> Result<MoveMessagesResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let source_mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());

        if args.destination_mailbox.trim().is_empty() {
            return Err(ConnectorError::InvalidInput(
                "destination_mailbox cannot be empty".to_string(),
            ));
        }

        let uids = normalize_uids(args.uids)?;
        let uid_set = uids_to_sequence_set(&uids);
        let dest = args.destination_mailbox;
        let dry_run = args.dry_run;
        let allow_expunge_all = args.allow_expunge_all;

        self.with_session(move |session| {
            session.select(&source_mailbox).map_err(map_imap_error)?;

            if dry_run {
                return Ok(MoveMessagesResponse {
                    success: true,
                    dry_run: true,
                    source_mailbox,
                    destination_mailbox: dest,
                    uids,
                    used_uid_move: false,
                    copied: false,
                    marked_deleted_in_source: false,
                    expunged: false,
                    expunge_scope: None,
                    message: "Dry run: no changes applied".to_string(),
                });
            }

            let caps = session.capabilities().map_err(map_imap_error)?;

            if caps.has_str("MOVE") {
                session.uid_mv(&uid_set, &dest).map_err(map_imap_error)?;
                return Ok(MoveMessagesResponse {
                    success: true,
                    dry_run: false,
                    source_mailbox,
                    destination_mailbox: dest,
                    uids,
                    used_uid_move: true,
                    copied: false,
                    marked_deleted_in_source: false,
                    expunged: false,
                    expunge_scope: None,
                    message: "Moved via UID MOVE".to_string(),
                });
            }

            session.uid_copy(&uid_set, &dest).map_err(map_imap_error)?;
            session
                .uid_store(&uid_set, "+FLAGS.SILENT (\\Deleted)")
                .map_err(map_imap_error)?;

            // Best effort removal from source: prefer UID EXPUNGE (UIDPLUS), otherwise EXPUNGE.
            let mut expunged = false;
            let mut expunge_scope = None;
            if caps.has_str("UIDPLUS") {
                session.uid_expunge(&uid_set).map_err(map_imap_error)?;
                expunged = true;
                expunge_scope = Some("uid_set".to_string());
            } else if allow_expunge_all {
                session.expunge().map_err(map_imap_error)?;
                expunged = true;
                expunge_scope = Some("mailbox_all_deleted".to_string());
            }

            Ok(MoveMessagesResponse {
                success: true,
                dry_run: false,
                source_mailbox,
                destination_mailbox: dest,
                uids,
                used_uid_move: false,
                copied: true,
                marked_deleted_in_source: true,
                expunged,
                expunge_scope,
                message: if expunged {
                    "Moved via COPY + \\Deleted + EXPUNGE".to_string()
                } else {
                    "Copied to destination and marked \\Deleted in source (not expunged)"
                        .to_string()
                },
            })
        })
        .await
    }

    async fn delete_messages(
        &self,
        args: DeleteMessagesArgs,
    ) -> Result<DeleteMessagesResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());

        let uids = normalize_uids(args.uids)?;
        let uid_set = uids_to_sequence_set(&uids);
        let dry_run = args.dry_run;
        let expunge = args.expunge;
        let allow_expunge_all = args.allow_expunge_all;

        self.with_session(move |session| {
            session.select(&mailbox).map_err(map_imap_error)?;

            if dry_run {
                return Ok(DeleteMessagesResponse {
                    success: true,
                    dry_run: true,
                    mailbox,
                    uids,
                    marked_deleted: false,
                    expunged: false,
                    expunge_scope: None,
                    message: "Dry run: no changes applied".to_string(),
                });
            }

            session
                .uid_store(&uid_set, "+FLAGS.SILENT (\\Deleted)")
                .map_err(map_imap_error)?;

            let caps = session.capabilities().map_err(map_imap_error)?;
            let mut expunged = false;
            let mut expunge_scope = None;

            if expunge {
                if caps.has_str("UIDPLUS") {
                    session.uid_expunge(&uid_set).map_err(map_imap_error)?;
                    expunged = true;
                    expunge_scope = Some("uid_set".to_string());
                } else if allow_expunge_all {
                    session.expunge().map_err(map_imap_error)?;
                    expunged = true;
                    expunge_scope = Some("mailbox_all_deleted".to_string());
                } else {
                    return Err(ConnectorError::InvalidInput(
                        "Server does not support UIDPLUS; refusing to EXPUNGE entire mailbox. Re-run with allow_expunge_all=true if you really want this.".to_string(),
                    ));
                }
            }

            Ok(DeleteMessagesResponse {
                success: true,
                dry_run: false,
                mailbox,
                uids,
                marked_deleted: true,
                expunged,
                expunge_scope,
                message: if expunged {
                    "Marked \\Deleted and expunged".to_string()
                } else {
                    "Marked \\Deleted".to_string()
                },
            })
        })
        .await
    }

    async fn add_flags(
        &self,
        args: UpdateFlagsArgs,
    ) -> Result<UpdateFlagsResponse, ConnectorError> {
        self.update_flags("add".to_string(), "+FLAGS.SILENT", args)
            .await
    }

    async fn remove_flags(
        &self,
        args: UpdateFlagsArgs,
    ) -> Result<UpdateFlagsResponse, ConnectorError> {
        self.update_flags("remove".to_string(), "-FLAGS.SILENT", args)
            .await
    }

    async fn update_flags(
        &self,
        mode: String,
        store_op: &'static str,
        args: UpdateFlagsArgs,
    ) -> Result<UpdateFlagsResponse, ConnectorError> {
        let config = self.ensure_config()?;
        let mailbox = args
            .mailbox
            .unwrap_or_else(|| config.default_mailbox.clone());
        let uids = normalize_uids(args.uids)?;
        let flags = normalize_flags(args.flags)?;
        let uid_set = uids_to_sequence_set(&uids);
        let dry_run = args.dry_run;
        let flags_list = flags.join(" ");
        let query = format!("{} ({})", store_op, flags_list);

        self.with_session(move |session| {
            session.select(&mailbox).map_err(map_imap_error)?;

            if dry_run {
                return Ok(UpdateFlagsResponse {
                    success: true,
                    dry_run: true,
                    mailbox,
                    uids,
                    mode,
                    flags,
                    message: "Dry run: no changes applied".to_string(),
                });
            }

            session
                .uid_store(&uid_set, &query)
                .map_err(map_imap_error)?;

            Ok(UpdateFlagsResponse {
                success: true,
                dry_run: false,
                mailbox,
                uids,
                mode,
                flags,
                message: "Flags updated".to_string(),
            })
        })
        .await
    }
}

fn build_message_summary(fetch: &imap::types::Fetch<'_>) -> MessageSummary {
    let envelope = fetch.envelope();
    let subject = envelope
        .and_then(|env| env.subject.as_ref())
        .map(|s| decode_bytes(s));
    let date = envelope
        .and_then(|env| env.date.as_ref())
        .map(|d| decode_bytes(d));
    let message_id = envelope
        .and_then(|env| env.message_id.as_ref())
        .map(|d| decode_bytes(d));
    let from = envelope
        .and_then(|env| env.from.as_ref())
        .map(|addresses| decode_address_list(addresses))
        .unwrap_or_default();
    let to = envelope
        .and_then(|env| env.to.as_ref())
        .map(|addresses| decode_address_list(addresses))
        .unwrap_or_default();
    let cc = envelope
        .and_then(|env| env.cc.as_ref())
        .map(|addresses| decode_address_list(addresses))
        .unwrap_or_default();
    let bcc = envelope
        .and_then(|env| env.bcc.as_ref())
        .map(|addresses| decode_address_list(addresses))
        .unwrap_or_default();

    MessageSummary {
        uid: fetch.uid,
        sequence: fetch.message,
        subject,
        from,
        to,
        cc,
        bcc,
        date,
        message_id,
        internal_date: fetch.internal_date().map(|dt| dt.to_rfc3339()),
        flags: fetch.flags().iter().map(|flag| flag.to_string()).collect(),
        size: fetch.size,
    }
}

fn decode_bytes(data: &[u8]) -> String {
    let text = String::from_utf8_lossy(data);
    text.trim_matches(|c: char| c == '\r' || c == '\n')
        .to_string()
}

fn decode_address_list(addresses: &[imap_proto::types::Address<'_>]) -> Vec<String> {
    addresses.iter().map(format_address).collect()
}

fn format_address(address: &imap_proto::types::Address<'_>) -> String {
    let mailbox = address
        .mailbox
        .as_ref()
        .map(|v| decode_bytes(v))
        .unwrap_or_default();
    let host = address
        .host
        .as_ref()
        .map(|v| decode_bytes(v))
        .unwrap_or_default();
    let email = if !mailbox.is_empty() && !host.is_empty() {
        format!("{}@{}", mailbox, host)
    } else {
        mailbox.clone()
    };

    match address.name.as_ref().map(|n| decode_bytes(n)) {
        Some(name) if !email.is_empty() => format!("{} <{}>", name, email),
        Some(name) => name,
        None => email,
    }
}

fn attribute_to_string(attr: &NameAttribute<'_>) -> String {
    match attr {
        NameAttribute::NoInferiors => "\\NoInferiors".to_string(),
        NameAttribute::NoSelect => "\\NoSelect".to_string(),
        NameAttribute::Marked => "\\Marked".to_string(),
        NameAttribute::Unmarked => "\\Unmarked".to_string(),
        NameAttribute::All => "\\All".to_string(),
        NameAttribute::Archive => "\\Archive".to_string(),
        NameAttribute::Drafts => "\\Drafts".to_string(),
        NameAttribute::Flagged => "\\Flagged".to_string(),
        NameAttribute::Junk => "\\Junk".to_string(),
        NameAttribute::Sent => "\\Sent".to_string(),
        NameAttribute::Trash => "\\Trash".to_string(),
        NameAttribute::Extension(value) => value.to_string(),
        other => format!("\\{:?}", other),
    }
}

fn parse_message_bodies(raw: &[u8]) -> (Option<String>, Option<String>, Vec<HeaderLine>) {
    if let Ok(parsed) = parse_mail(raw) {
        let headers = parsed
            .headers
            .iter()
            .map(|header| HeaderLine {
                name: header.get_key(),
                value: header.get_value(),
            })
            .collect();
        let plain = extract_body_by_mime(&parsed, "text/plain").or_else(|| parsed.get_body().ok());
        let html = extract_body_by_mime(&parsed, "text/html");
        (plain, html, headers)
    } else {
        (None, None, Vec::new())
    }
}

fn extract_body_by_mime(parsed: &ParsedMail<'_>, target: &str) -> Option<String> {
    if parsed.ctype.mimetype.eq_ignore_ascii_case(target) {
        return parsed.get_body().ok();
    }

    for part in &parsed.subparts {
        if let Some(body) = extract_body_by_mime(part, target) {
            return Some(body);
        }
    }
    None
}

fn normalize_uids(mut uids: Vec<u32>) -> Result<Vec<u32>, ConnectorError> {
    uids.sort_unstable();
    uids.dedup();
    if uids.is_empty() {
        return Err(ConnectorError::InvalidInput(
            "uids cannot be empty".to_string(),
        ));
    }
    Ok(uids)
}

fn normalize_flags(flags: Vec<String>) -> Result<Vec<String>, ConnectorError> {
    let mut out = Vec::new();
    for f in flags {
        let flag = f.trim();
        if flag.is_empty() {
            continue;
        }
        if flag.contains(char::is_whitespace) || flag.contains('(') || flag.contains(')') {
            return Err(ConnectorError::InvalidInput(format!(
                "Invalid flag '{}': flags must not contain whitespace or parentheses",
                flag
            )));
        }
        out.push(flag.to_string());
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err(ConnectorError::InvalidInput(
            "flags cannot be empty".to_string(),
        ));
    }
    Ok(out)
}

fn uids_to_sequence_set(uids: &[u32]) -> String {
    if uids.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    let mut start = uids[0];
    let mut prev = uids[0];

    for &uid in &uids[1..] {
        if uid == prev.saturating_add(1) {
            prev = uid;
            continue;
        }

        if start == prev {
            parts.push(start.to_string());
        } else {
            parts.push(format!("{}:{}", start, prev));
        }
        start = uid;
        prev = uid;
    }

    if start == prev {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{}:{}", start, prev));
    }

    parts.join(",")
}

#[derive(Debug, Deserialize)]
struct ListMailboxesArgs {
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    include_subscribed: bool,
}

#[derive(Debug, Deserialize)]
struct FetchMessagesArgs {
    #[serde(default)]
    mailbox: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Skip this many messages from the end (for offset-based pagination)
    #[serde(default)]
    offset: Option<usize>,
    /// Only fetch messages with UID less than this (for cursor-based pagination)
    #[serde(default)]
    before_uid: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GetMessageArgs {
    uid: u32,
    #[serde(default)]
    mailbox: Option<String>,
    /// Include email headers (default: false, usually not needed)
    #[serde(default)]
    include_headers: bool,
    /// Include the original HTML body (usually not needed, content is converted)
    #[serde(default)]
    include_html: bool,
    /// Include base64-encoded raw message
    #[serde(default)]
    include_raw: bool,
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    mailbox: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CreateDraftArgs {
    /// Email recipient(s), comma-separated for multiple
    to: String,
    /// Email subject
    subject: String,
    /// Email body (plain text)
    body: String,
    /// Optional CC recipients, comma-separated
    #[serde(default)]
    cc: Option<String>,
    /// Optional BCC recipients, comma-separated
    #[serde(default)]
    bcc: Option<String>,
    /// Mailbox to save draft to (defaults to "Drafts")
    #[serde(default)]
    drafts_mailbox: Option<String>,
    /// Optional In-Reply-To message ID for threading
    #[serde(default)]
    in_reply_to: Option<String>,
    /// Optional References header for threading
    #[serde(default)]
    references: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoveMessagesArgs {
    /// Message UIDs to move
    uids: Vec<u32>,
    /// Destination mailbox name
    destination_mailbox: String,
    /// Source mailbox (defaults to configured default mailbox)
    #[serde(default)]
    mailbox: Option<String>,
    /// If true, do not apply changes
    #[serde(default)]
    dry_run: bool,
    /// If true, allow EXPUNGE of all \\Deleted messages when UIDPLUS is unavailable
    #[serde(default)]
    allow_expunge_all: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteMessagesArgs {
    /// Message UIDs to delete
    uids: Vec<u32>,
    /// Mailbox containing the messages (defaults to configured default mailbox)
    #[serde(default)]
    mailbox: Option<String>,
    /// If true, expunge after marking \\Deleted
    #[serde(default)]
    expunge: bool,
    /// If true, allow EXPUNGE of all \\Deleted messages when UIDPLUS is unavailable
    #[serde(default)]
    allow_expunge_all: bool,
    /// If true, do not apply changes
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateFlagsArgs {
    /// Message UIDs to update
    uids: Vec<u32>,
    /// Flags to add/remove (e.g. \\Seen, \\Flagged)
    flags: Vec<String>,
    /// Mailbox containing the messages (defaults to configured default mailbox)
    #[serde(default)]
    mailbox: Option<String>,
    /// If true, do not apply changes
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct CreateDraftResponse {
    success: bool,
    mailbox: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct MoveMessagesResponse {
    success: bool,
    dry_run: bool,
    source_mailbox: String,
    destination_mailbox: String,
    uids: Vec<u32>,
    used_uid_move: bool,
    copied: bool,
    marked_deleted_in_source: bool,
    expunged: bool,
    /// "uid_set" or "mailbox_all_deleted" when expunged
    expunge_scope: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct DeleteMessagesResponse {
    success: bool,
    dry_run: bool,
    mailbox: String,
    uids: Vec<u32>,
    marked_deleted: bool,
    expunged: bool,
    /// "uid_set" or "mailbox_all_deleted" when expunged
    expunge_scope: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct UpdateFlagsResponse {
    success: bool,
    dry_run: bool,
    mailbox: String,
    uids: Vec<u32>,
    /// "add" or "remove"
    mode: String,
    flags: Vec<String>,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_to_sequence_set_collapses_ranges() {
        assert_eq!(uids_to_sequence_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
    }

    #[test]
    fn uids_to_sequence_set_handles_singletons() {
        assert_eq!(uids_to_sequence_set(&[42]), "42");
        assert_eq!(uids_to_sequence_set(&[1, 3, 5]), "1,3,5");
    }

    #[test]
    fn normalize_flags_rejects_whitespace_and_parens() {
        assert!(normalize_flags(vec!["\\Seen".to_string(), "bad flag".to_string()]).is_err());
        assert!(normalize_flags(vec!["(bad)".to_string()]).is_err());
    }
}

#[derive(Debug, Serialize)]
struct MailboxInfo {
    name: String,
    delimiter: Option<String>,
    attributes: Vec<String>,
    selectable: bool,
    subscribed: bool,
}

/// Paginated response for fetch_messages
#[derive(Debug, Serialize)]
struct FetchMessagesResponse {
    messages: Vec<MessageSummary>,
    mailbox: String,
    total: usize,
    returned: usize,
    has_more: bool,
    /// UID to use as before_uid for next page (smallest UID in this batch)
    next_cursor: Option<u32>,
}

#[derive(Debug, Clone)]
struct ImapFetchCursor {
    before_uid: Option<u32>,
    offset: usize,
}

#[derive(Debug, Serialize)]
struct MessageSummary {
    uid: Option<u32>,
    sequence: u32,
    subject: Option<String>,
    from: Vec<String>,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    date: Option<String>,
    message_id: Option<String>,
    internal_date: Option<String>,
    flags: Vec<String>,
    size: Option<u32>,
}

#[derive(Debug, Serialize)]
struct HeaderLine {
    name: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct MessageDetails {
    summary: MessageSummary,
    /// Clean, readable content - prefers text, falls back to converted HTML
    content: Option<String>,
    /// Content source: "text", "html_converted", or "none"
    content_source: String,
    /// Headers - only included if specifically requested
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<Vec<HeaderLine>>,
    /// Original text body (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    text_body: Option<String>,
    /// Original HTML body - only included if specifically requested
    #[serde(skip_serializing_if = "Option::is_none")]
    html_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResults {
    mailbox: String,
    query: String,
    uids: Vec<u32>,
}

fn map_imap_error(err: ImapError) -> ConnectorError {
    match err {
        ImapError::Io(inner) => ConnectorError::Io(inner),
        ImapError::Validate(inner) => ConnectorError::InvalidInput(inner.to_string()),
        ImapError::Parse(inner) => ConnectorError::Other(format!("IMAP parse error: {}", inner)),
        ImapError::Bad(bad) => ConnectorError::Other(format!("IMAP BAD: {}", bad.information)),
        ImapError::No(no) => ConnectorError::Other(format!("IMAP NO: {}", no.information)),
        ImapError::Bye(bye) => ConnectorError::Other(format!("IMAP BYE: {}", bye.information)),
        other => ConnectorError::Other(format!("IMAP error: {}", other)),
    }
}

fn map_auth_error(err: ImapError) -> ConnectorError {
    match err {
        ImapError::Bad(bad) => ConnectorError::Authentication(bad.information),
        ImapError::No(no) => ConnectorError::Authentication(no.information),
        other => map_imap_error(other),
    }
}

#[async_trait]
impl Connector for ImapConnector {
    fn name(&self) -> &'static str {
        "imap"
    }

    fn description(&self) -> &'static str {
        "An IMAP connector providing mailbox discovery and message retrieval."
    }

    fn display_name(&self) -> &'static str {
        "IMAP Mail"
    }

    fn icon(&self) -> &'static str {
        "imap"
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
                website_url: None,
            },
            instructions: Some(
                "Use the IMAP connector to inspect mailboxes, list messages, search, and fetch message details.".to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let config = self.ensure_config()?;
        let mailboxes = self
            .list_mailboxes(ListMailboxesArgs {
                reference: None,
                pattern: None,
                include_subscribed: true,
            })
            .await?;

        let mut resources = Vec::new();
        for mailbox in mailboxes {
            let uri = format!("imap://mailbox/{}", urlencoding::encode(&mailbox.name));
            resources.push(Resource {
                raw: RawResource {
                    uri,
                    name: mailbox.name.clone(),
                    title: None,
                    description: Some(format!(
                        "Mailbox on {} (selectable: {}, subscribed: {})",
                        config.host, mailbox.selectable, mailbox.subscribed
                    )),
                    mime_type: Some("application/vnd.imap.mailbox+json".to_string()),
                    size: None,
                    icons: None,
                },
                annotations: None,
            });
        }

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        let uri = request.uri.as_str();
        let config = self.ensure_config()?;
        if let Some(encoded) = uri.strip_prefix("imap://mailbox/") {
            let mailbox = urlencoding::decode(encoded)
                .map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid mailbox URI: {}", err))
                })?
                .to_string();
            let response = self
                .fetch_messages(FetchMessagesArgs {
                    mailbox: Some(mailbox.clone()),
                    limit: Some(config.fetch_limit),
                    offset: None,
                    before_uid: None,
                })
                .await?;
            let body = serde_json::to_string_pretty(&response)
                .map_err(|err| ConnectorError::Other(err.to_string()))?;
            return Ok(vec![ResourceContents::text(body, uri)]);
        }

        if let Some(rest) = uri.strip_prefix("imap://message/") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() != 2 {
                return Err(ConnectorError::InvalidInput(format!(
                    "Invalid message URI: {}",
                    uri
                )));
            }
            let mailbox = urlencoding::decode(parts[0])
                .map_err(|err| {
                    ConnectorError::InvalidInput(format!("Invalid mailbox in URI: {}", err))
                })?
                .to_string();
            let uid: u32 = parts[1].parse().map_err(|err| {
                ConnectorError::InvalidInput(format!("Invalid UID in URI: {}", err))
            })?;
            let message = self
                .get_message(GetMessageArgs {
                    uid,
                    mailbox: Some(mailbox),
                    include_headers: false,
                    include_html: false,
                    include_raw: false,
                })
                .await?;
            let body = serde_json::to_string_pretty(&message)
                .map_err(|err| ConnectorError::Other(err.to_string()))?;
            return Ok(vec![ResourceContents::text(body, uri)]);
        }

        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list_mailboxes"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List mailboxes available on the IMAP server.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "reference": { "type": "string", "description": "IMAP reference name (defaults to root)." },
                            "pattern": { "type": "string", "description": "Mailbox pattern, defaults to '*'." },
                            "include_subscribed": { "type": "boolean", "description": "Whether to include subscription information." }
                        }
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
                name: Cow::Borrowed("fetch_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch recent message summaries from a mailbox with pagination support. Returns messages sorted newest first, with total count and cursor for fetching more.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox name (defaults to INBOX)." },
	                            "limit": { "type": "integer", "description": "Maximum number of messages to return (default 50, max 5000). Connector paginates internally.", "minimum": 1, "maximum": 5000 },
                            "offset": { "type": "integer", "description": "Skip this many messages from the end for offset-based pagination." },
                            "before_uid": { "type": "integer", "description": "Only fetch messages with UID less than this value for cursor-based pagination. Use next_cursor from previous response." }
                        }
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
                name: Cow::Borrowed("get_message"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a message by UID. Returns clean text content by default.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox containing the message." },
                            "uid": { "type": "integer", "description": "Message UID." },
                            "include_headers": { "type": "boolean", "description": "Include email headers (default: false)." },
                            "include_html": { "type": "boolean", "description": "Include original HTML body (default: false)." },
                            "include_raw": { "type": "boolean", "description": "Include base64 encoded raw message (default: false)." }
                        },
                        "required": ["uid"]
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
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Execute an IMAP search query within a mailbox.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox to search." },
                            "query": { "type": "string", "description": "IMAP search query (e.g. 'UNSEEN', 'FROM \"alice\" SINCE 1-Jan-2024')." },
                            "limit": { "type": "integer", "description": "Maximum number of UIDs to return." }
                        },
                        "required": ["query"]
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
                name: Cow::Borrowed("create_draft"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Create a draft email and save it to the Drafts folder. Useful for preparing emails for later review and sending.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "to": { "type": "string", "description": "Email recipient(s), comma-separated for multiple." },
                            "subject": { "type": "string", "description": "Email subject line." },
                            "body": { "type": "string", "description": "Email body (plain text)." },
                            "cc": { "type": "string", "description": "CC recipients, comma-separated (optional)." },
                            "bcc": { "type": "string", "description": "BCC recipients, comma-separated (optional)." },
                            "drafts_mailbox": { "type": "string", "description": "Mailbox to save draft to (defaults to 'Drafts')." },
                            "in_reply_to": { "type": "string", "description": "Message-ID of email being replied to (for threading)." },
                            "references": { "type": "string", "description": "References header for email threading." }
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
                name: Cow::Borrowed("move_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Move messages (by UID) from one mailbox to another. Uses UID MOVE when supported, otherwise falls back to COPY + \\Deleted (+ UID EXPUNGE when possible).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Source mailbox (defaults to INBOX)." },
                            "destination_mailbox": { "type": "string", "description": "Destination mailbox (e.g., 'INBOX', 'Archive', 'Deleted Messages')." },
                            "uids": { "type": "array", "items": { "type": "integer" }, "description": "Message UIDs to move." },
                            "dry_run": { "type": "boolean", "description": "If true, do not apply changes (default: true in CLI wrappers)." },
                            "allow_expunge_all": { "type": "boolean", "description": "If true, allow EXPUNGE of all \\\\Deleted messages when UIDPLUS is unavailable." }
                        },
                        "required": ["destination_mailbox", "uids"]
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
                name: Cow::Borrowed("delete_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Mark messages (by UID) as \\Deleted, optionally expunging them (UID EXPUNGE preferred).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox containing the messages (defaults to INBOX)." },
                            "uids": { "type": "array", "items": { "type": "integer" }, "description": "Message UIDs to delete." },
                            "expunge": { "type": "boolean", "description": "If true, expunge after marking \\\\Deleted." },
                            "allow_expunge_all": { "type": "boolean", "description": "If true, allow EXPUNGE of all \\\\Deleted messages when UIDPLUS is unavailable." },
                            "dry_run": { "type": "boolean", "description": "If true, do not apply changes (default: true in CLI wrappers)." }
                        },
                        "required": ["uids"]
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
                name: Cow::Borrowed("add_flags"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Add flags (e.g. \\\\Seen, \\\\Flagged) to messages by UID.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox containing the messages (defaults to INBOX)." },
                            "uids": { "type": "array", "items": { "type": "integer" }, "description": "Message UIDs to update." },
                            "flags": { "type": "array", "items": { "type": "string" }, "description": "Flags to add (e.g. \\\\Seen, \\\\Flagged)." },
                            "dry_run": { "type": "boolean", "description": "If true, do not apply changes (default: true in CLI wrappers)." }
                        },
                        "required": ["uids", "flags"]
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
                name: Cow::Borrowed("remove_flags"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Remove flags (e.g. \\\\Seen, \\\\Flagged) from messages by UID.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox containing the messages (defaults to INBOX)." },
                            "uids": { "type": "array", "items": { "type": "integer" }, "description": "Message UIDs to update." },
                            "flags": { "type": "array", "items": { "type": "string" }, "description": "Flags to remove (e.g. \\\\Seen, \\\\Flagged)." },
                            "dry_run": { "type": "boolean", "description": "If true, do not apply changes (default: true in CLI wrappers)." }
                        },
                        "required": ["uids", "flags"]
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
            "list_mailboxes" => {
                let parsed: ListMailboxesArgs = serde_json::from_value(args_value.clone())
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let mailboxes = self.list_mailboxes(parsed).await?;
                structured_result_with_text(&mailboxes, None)
            }
            "fetch_messages" => {
                let parsed: FetchMessagesArgs = serde_json::from_value(args_value.clone())
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let messages = self.fetch_messages(parsed).await?;
                structured_result_with_text(&messages, None)
            }
            "get_message" => {
                let parsed: GetMessageArgs = serde_json::from_value(args_value.clone())
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let message = self.get_message(parsed).await?;
                structured_result_with_text(&message, None)
            }
            "search" => {
                let parsed: SearchArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let results = self.search(parsed).await?;
                structured_result_with_text(&results, None)
            }
            "create_draft" => {
                let parsed: CreateDraftArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.create_draft(parsed).await?;
                structured_result_with_text(&result, None)
            }
            "move_messages" => {
                let parsed: MoveMessagesArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.move_messages(parsed).await?;
                structured_result_with_text(&result, None)
            }
            "delete_messages" => {
                let parsed: DeleteMessagesArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.delete_messages(parsed).await?;
                structured_result_with_text(&result, None)
            }
            "add_flags" => {
                let parsed: UpdateFlagsArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.add_flags(parsed).await?;
                structured_result_with_text(&result, None)
            }
            "remove_flags" => {
                let parsed: UpdateFlagsArgs = serde_json::from_value(args_value)
                    .map_err(|err| ConnectorError::InvalidParams(err.to_string()))?;
                let result = self.remove_flags(parsed).await?;
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
        if let Some(config) = &self.config {
            auth.insert("host".to_string(), config.host.clone());
            auth.insert("port".to_string(), config.port.to_string());
            auth.insert("username".to_string(), config.username.clone());
            auth.insert("security".to_string(), config.security.as_str().to_string());
            auth.insert(
                "tls".to_string(),
                (config.security != SecurityMode::Plaintext).to_string(),
            );
            auth.insert(
                "skip_tls_verify".to_string(),
                config.skip_tls_verify.to_string(),
            );
            auth.insert(
                "default_mailbox".to_string(),
                config.default_mailbox.clone(),
            );
            auth.insert(
                "default_fetch_limit".to_string(),
                config.fetch_limit.to_string(),
            );
        }
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let host = details
            .get("host")
            .ok_or_else(|| ConnectorError::InvalidInput("IMAP host is required".to_string()))?
            .to_string();
        let port_str = details.get("port").map(|v| v.as_str()).unwrap_or("993");
        let port: u16 = port_str
            .parse()
            .map_err(|err| ConnectorError::InvalidInput(format!("Invalid IMAP port: {}", err)))?;
        let username = details
            .get("username")
            .ok_or_else(|| ConnectorError::InvalidInput("IMAP username is required".to_string()))?
            .to_string();
        let password = details
            .get("password")
            .ok_or_else(|| ConnectorError::InvalidInput("IMAP password is required".to_string()))?
            .to_string();
        let security = if let Some(security) = details.get("security") {
            SecurityMode::from_str(Some(security.as_str()))?
        } else if let Some(tls) = details.get("tls") {
            let tls_enabled = matches!(tls.as_str(), "true" | "1" | "yes" | "on");
            if tls_enabled {
                SecurityMode::Tls
            } else {
                SecurityMode::Plaintext
            }
        } else {
            SecurityMode::from_str(None)?
        };
        let skip_tls_verify = details
            .get("skip_tls_verify")
            .map(|v| matches!(v.as_str(), "true" | "1" | "yes" | "on"))
            .unwrap_or(false);
        let default_mailbox = details
            .get("default_mailbox")
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "INBOX".to_string());
        let fetch_limit = details
            .get("default_fetch_limit")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(50);

        self.config = Some(ImapConfig {
            host,
            port,
            username,
            password,
            security,
            skip_tls_verify,
            default_mailbox,
            fetch_limit,
        });

        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        self.with_session(|session| {
            session.noop().map_err(map_imap_error)?;
            Ok(())
        })
        .await
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "host".to_string(),
                    label: "IMAP Host".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    description: Some("Hostname of the IMAP server.".to_string()),
                    options: None,
                },
                Field {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some("IMAP port (default 993).".to_string()),
                    options: None,
                },
                Field {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    description: Some("Account username.".to_string()),
                    options: None,
                },
                Field {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Account password.".to_string()),
                    options: None,
                },
                Field {
                    name: "tls".to_string(),
                    label: "Use TLS".to_string(),
                    field_type: FieldType::Boolean,
                    required: false,
                    description: Some(
                        "If provided (and 'security' is unset), choose between TLS and plaintext."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "security".to_string(),
                    label: "Security".to_string(),
                    field_type: FieldType::Select {
                        options: vec![
                            "autotls".to_string(),
                            "auto".to_string(),
                            "tls".to_string(),
                            "starttls".to_string(),
                            "plaintext".to_string(),
                        ],
                    },
                    required: false,
                    description: Some("Connection security mode.".to_string()),
                    options: None,
                },
                Field {
                    name: "skip_tls_verify".to_string(),
                    label: "Skip TLS Verification".to_string(),
                    field_type: FieldType::Boolean,
                    required: false,
                    description: Some(
                        "Allow invalid TLS certificates (not recommended).".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "default_mailbox".to_string(),
                    label: "Default Mailbox".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Mailbox used when none is specified.".to_string()),
                    options: None,
                },
                Field {
                    name: "default_fetch_limit".to_string(),
                    label: "Default Fetch Limit".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some("Default number of messages to fetch.".to_string()),
                    options: None,
                },
            ],
        }
    }
}
