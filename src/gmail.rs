use anyhow::{Context, Result};
use google_gmail1::{
    api::{ListMessagesResponse, Message},
    hyper::{self, client::HttpConnector},
    hyper_rustls::{self, HttpsConnector},
    oauth2::{self},
    Gmail,
};
use std::path::Path;
use tracing::{debug, info};

pub struct GmailClient {
    hub: Gmail<HttpsConnector<HttpConnector>>,
}

impl GmailClient {
    pub async fn new<P: AsRef<Path>>(credentials_path: P) -> Result<Self> {
        info!("Initializing Gmail client");

        let secret = oauth2::read_application_secret(credentials_path)
            .await
            .context("Failed to read OAuth2 credentials")?;

        let mut path = dirs::config_dir().expect("Cannot find config dir");
        path.push("gmail_router");
        std::fs::create_dir_all(&path).expect("Cannot create config dir");
        path.push("token_cache.json");
        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPPortRedirect(14500),
        )
        .persist_tokens_to_disk(path)
        .build()
        .await
        .context("Failed to create authenticator")?;

        let scopes = &["https://mail.google.com/"];
        let token = auth
            .token(scopes)
            .await
            .context("Failed to obtain access token")?;
        println!("Token obtained: {:?}", token.token().is_some());
        if let Some(t) = token.token() {
            println!("Token value: {}", t);
        }
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("Failed to load native roots")?
            .https_or_http()
            .enable_http1()
            .build();

        let client = hyper::Client::builder().build(https);
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
                .add_scope("https://mail.google.com/");
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
            .add_scope("https://mail.google.com/")
            .format("full")
            .doit()
            .await
            .context("Failed to get message")?;

        Ok(result.1)
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        self.hub
            .users()
            .messages_delete("me", message_id)
            .add_scope("https://mail.google.com/")
            .doit()
            .await
            .context("Failed to delete message")?;

        debug!("Deleted message {}", message_id);
        Ok(())
    }

    pub async fn move_message_to_spam(&self, message_id: &str) -> Result<()> {
        let req = google_gmail1::api::ModifyMessageRequest {
            add_label_ids: Some(vec!["SPAM".to_string()]),
            remove_label_ids: Some(vec!["INBOX".to_string()]),
        };

        self.hub
            .users()
            .messages_modify(req, "me", message_id)
            .add_scope("https://mail.google.com/")
            .doit()
            .await
            .context("Failed to move message to spam")?;

        debug!("Moved message to spam {}", message_id);
        Ok(())
    }
}
