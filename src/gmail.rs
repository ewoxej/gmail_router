use anyhow::{Context, Result};
use google_gmail1::{
    api::{ListMessagesResponse, Message, ModifyMessageRequest},
    hyper_rustls::{self, HttpsConnector},
    hyper_util::{self, client::legacy::connect::HttpConnector, rt::TokioExecutor},
    yup_oauth2 as oauth2, Gmail,
};
use tracing::{debug, info};

const SCOPE: &str = "https://mail.google.com/";

fn https_connector() -> Result<HttpsConnector<HttpConnector>> {
    Ok(hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .context("Failed to load native roots")?
        .https_or_http()
        .enable_http1()
        .build())
}

pub struct GmailClient {
    hub: Gmail<HttpsConnector<HttpConnector>>,
}

impl GmailClient {
    pub async fn new_for_user(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<Self> {
        info!("Initializing Gmail client");

        let secret = oauth2::authorized_user::AuthorizedUserSecret {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            refresh_token: refresh_token.to_string(),
            key_type: "authorized_user".to_string(),
        };

        let auth_client = hyper_util::client::legacy::Client::builder(TokioExecutor::new())
            .build(https_connector()?);
        let auth = oauth2::AuthorizedUserAuthenticator::with_client(
            secret,
            oauth2::client::CustomHyperClientBuilder::from(auth_client),
        )
        .build()
        .await
        .context("Failed to create authenticator")?;

        let client =
            hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(https_connector()?);
        let hub = Gmail::new(client, auth);

        Ok(Self { hub })
    }

    pub async fn list_messages(&self, after_date: &str) -> Result<Vec<String>> {
        info!("Fetching messages after {}", after_date);

        let mut all_message_ids = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut request = self
                .hub
                .users()
                .messages_list("me")
                .add_scope(SCOPE);
            request = request.q(&format!("in:inbox after:{}", after_date));

            if let Some(token) = page_token {
                request = request.page_token(&token);
            }

            let result: ListMessagesResponse = match request.doit().await {
                Ok(res) => res.1,
                Err(e) => {
                    eprintln!("Gmail API error: {:#?}", e);
                    return Err(e.into());
                }
            };

            if let Some(messages) = result.messages {
                for msg in messages {
                    if let Some(id) = msg.id {
                        all_message_ids.push(id);
                    }
                }
            }

            page_token = result.next_page_token;

            if page_token.is_none() {
                break;
            }
        }

        debug!("Found {} messages", all_message_ids.len());
        Ok(all_message_ids)
    }

    pub async fn get_message(&self, message_id: &str) -> Result<Message> {
        let result = self
            .hub
            .users()
            .messages_get("me", message_id)
            .add_scope(SCOPE)
            .format("full")
            .doit()
            .await
            .context("Failed to get message")?;

        Ok(result.1)
    }

    async fn get_message_raw(&self, message_id: &str) -> Result<Vec<u8>> {
        let (_, msg) = self
            .hub
            .users()
            .messages_get("me", message_id)
            .add_scope(SCOPE)
            .format("raw")
            .doit()
            .await
            .context("Failed to get raw message")?;
        msg.raw.context("Message had no raw body")
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        self.hub
            .users()
            .messages_delete("me", message_id)
            .add_scope(SCOPE)
            .doit()
            .await
            .context("Failed to delete message")?;

        debug!("Deleted message {}", message_id);
        Ok(())
    }

    pub async fn trash_message(&self, message_id: &str) -> Result<()> {
        self.hub
            .users()
            .messages_trash("me", message_id)
            .add_scope(SCOPE)
            .doit()
            .await
            .context("Failed to trash message")?;

        debug!("Trashed message {}", message_id);
        Ok(())
    }

    pub async fn move_message_to_spam(&self, message_id: &str) -> Result<()> {
        let req = ModifyMessageRequest {
            add_label_ids: Some(vec!["SPAM".to_string()]),
            remove_label_ids: Some(vec!["INBOX".to_string()]),
        };

        self.hub
            .users()
            .messages_modify(req, "me", message_id)
            .add_scope(SCOPE)
            .doit()
            .await
            .context("Failed to move message to spam")?;

        debug!("Moved message to spam {}", message_id);
        Ok(())
    }

    pub async fn forward_message(&self, message_id: &str, to: &str) -> Result<()> {
        let full = self.get_message(message_id).await?;
        let subject = header_value(&full, "subject").unwrap_or_default();
        let raw = self.get_message_raw(message_id).await?;

        let body = build_forward_mime(to, &subject, &raw);

        self.hub
            .users()
            .messages_send(Message::default(), "me")
            .add_scope(SCOPE)
            .upload(
                std::io::Cursor::new(body),
                "message/rfc822".parse().expect("valid mime"),
            )
            .await
            .context("Failed to send forwarded message")?;

        debug!("Forwarded message {} to {}", message_id, to);
        Ok(())
    }
}

fn header_value(message: &Message, name: &str) -> Option<String> {
    let headers = message.payload.as_ref()?.headers.as_ref()?;
    headers
        .iter()
        .find(|h| {
            h.name
                .as_ref()
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
        })
        .and_then(|h| h.value.clone())
}

fn header_safe(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn build_forward_mime(to: &str, subject: &str, original: &[u8]) -> Vec<u8> {
    let boundary = format!(
        "=_gmailrouter_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    let header = format!(
        "To: {to}\r\n\
         Subject: Fwd: {subject}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=\"UTF-8\"\r\n\
         \r\n\
         Forwarded automatically by Gmail Router. Original message attached.\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: message/rfc822\r\n\
         Content-Disposition: attachment; filename=\"forwarded.eml\"\r\n\
         \r\n",
        to = header_safe(to),
        subject = header_safe(subject),
    );

    let mut out = Vec::with_capacity(header.len() + original.len() + boundary.len() + 8);
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(original);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}
