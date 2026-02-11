use anyhow::{Context, Result};
use google_gmail1::api::Message;
use std::collections::HashSet;
use tracing::debug;

pub fn extract_recipients(message: &Message, domain: &str) -> Result<Vec<String>> {
    let headers = message
        .payload
        .as_ref()
        .and_then(|p| p.headers.as_ref())
        .context("Message has no headers")?;

    let mut recipients = Vec::new();

    for header in headers {
        if let (Some(name), Some(value)) = (&header.name, &header.value) {
            if name.to_lowercase() == "to" {
                let addrs = parse_email_addresses(value, domain);
                recipients.extend(addrs);
            }
        }
    }

    Ok(recipients)
}

/// Supported formats: "email@domain.com", "Name <email@domain.com>", "email1, email2"
fn parse_email_addresses(header_value: &str, domain: &str) -> Vec<String> {
    let mut addresses = Vec::new();

    for part in header_value.split(',') {
        let part = part.trim();
        let email = if let Some(start) = part.find('<') {
            if let Some(end) = part.find('>') {
                &part[start + 1..end]
            } else {
                part
            }
        } else {
            part
        };

        let email = email.trim();
        if email.contains('@') && email.ends_with(&format!("@{}", domain)) {
            if let Some(local_part) = email.split('@').next() {
                addresses.push(local_part.to_lowercase());
            }
        }
    }

    addresses
}

pub async fn collect_all_addresses(
    gmail_client: &crate::gmail::GmailClient,
    message_ids: &[String],
    domain: &str,
) -> Result<HashSet<String>> {
    let mut all_addresses = HashSet::new();

    for (idx, msg_id) in message_ids.iter().enumerate() {
        if idx % 100 == 0 {
            debug!("Processing message {}/{}", idx + 1, message_ids.len());
        }

        let message = gmail_client.get_message(msg_id).await?;
        let recipients = extract_recipients(&message, domain)?;

        for recipient in recipients {
            all_addresses.insert(recipient);
        }
    }

    Ok(all_addresses)
}

pub fn should_delete_message(
    recipients: &[String],
    routing_config: &crate::config::RoutingConfig,
) -> bool {
    for recipient in recipients {
        if !routing_config.is_allowed(recipient) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_email_addresses() {
        let domain = "example.com";

        let addrs = parse_email_addresses("test@example.com", domain);
        assert_eq!(addrs, vec!["test"]);

        let addrs = parse_email_addresses("John Doe <john@example.com>", domain);
        assert_eq!(addrs, vec!["john"]);

        let addrs = parse_email_addresses("test1@example.com, test2@example.com", domain);
        assert_eq!(addrs, vec!["test1", "test2"]);

        let addrs = parse_email_addresses("test1@example.com, John <john@example.com>", domain);
        assert_eq!(addrs, vec!["test1", "john"]);

        let addrs = parse_email_addresses("test@other.com", domain);
        assert_eq!(addrs.len(), 0);
    }

    #[test]
    fn test_should_delete_message() {
        use crate::config::RoutingConfig;

        let mut config = RoutingConfig::default();
        config.addresses.insert("allowed".to_string(), true);
        config.addresses.insert("blocked".to_string(), false);

        assert!(!should_delete_message(
            &vec!["allowed".to_string()],
            &config
        ));

        assert!(should_delete_message(&vec!["blocked".to_string()], &config));

        assert!(should_delete_message(
            &vec!["allowed".to_string(), "blocked".to_string()],
            &config
        ));
    }
}
