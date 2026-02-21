use anyhow::{Context, Result};
use chrono::Utc;
use gmail_router::config::{get_config_path, CREDENTIALS_FILE, ROUTING_FILE};
use gmail_router::{config, gmail, processor};
use std::path::Path;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting Gmail Router");

    let credentials_path = get_config_path(CREDENTIALS_FILE);
    let routing_path = get_config_path(ROUTING_FILE);

    let creds_config = config::CredentialsConfig::load(&credentials_path).with_context(|| {
        format!(
            "Failed to load credentials config. Make sure {:?} exists",
            credentials_path
        )
    })?;

    info!("Domain: {}", creds_config.domain);
    info!(
        "Check interval: {} seconds",
        creds_config.check_interval_seconds
    );
    info!("Start date: {}", creds_config.start_date);

    let gmail_client = gmail::GmailClient::new(&creds_config.google_credentials_path)
        .await
        .context("Failed to create Gmail client")?;

    if !Path::new(&routing_path).exists() {
        info!("Routing config not found. Initializing...");
        initialize_routing_config(&gmail_client, &creds_config, &mut None).await?;
    } else {
        let routing_config = config::RoutingConfig::load(routing_path)
            .context("Failed to load routing config. Make sure routing.yaml exists")?;
        initialize_routing_config(&gmail_client, &creds_config, &mut Some(routing_config)).await?;
    }

    loop {
        match process_emails(&gmail_client, &creds_config).await {
            Ok(_) => info!("Email processing completed successfully"),
            Err(e) => error!("Error processing emails: {:#}", e),
        }

        info!(
            "Waiting {} seconds before next check...",
            creds_config.check_interval_seconds
        );
        sleep(Duration::from_secs(creds_config.check_interval_seconds)).await;
    }
}

async fn initialize_routing_config(
    gmail_client: &gmail::GmailClient,
    creds_config: &config::CredentialsConfig,
    routing_config: &mut Option<config::RoutingConfig>,
) -> Result<()> {
    info!("Scanning emails to build address list...");

    // Date format for Gmail API: YYYY/MM/DD
    let date_filter: String;
    if routing_config.is_none() {
        date_filter = creds_config.start_date.format("%Y/%m/%d").to_string();
    } else {
        date_filter = routing_config
            .as_ref()
            .unwrap()
            .updated_date
            .format("%Y/%m/%d")
            .to_string();
    }

    let message_ids = gmail_client
        .list_messages(&date_filter)
        .await
        .context("Failed to list messages")?;

    info!("Found {} messages to scan", message_ids.len());

    let addresses =
        processor::collect_all_addresses(gmail_client, &message_ids, &creds_config.domain).await?;

    info!("Found {} unique addresses", addresses.len());

    let routing_config = routing_config.get_or_insert(config::RoutingConfig::default());
    for addr in addresses {
        routing_config.add_address(addr);
    }

    routing_config.update_date(Utc::now());

    routing_config
        .save(get_config_path(ROUTING_FILE))
        .context("Failed to save routing config")?;

    info!(
        "Routing config created at {}",
        get_config_path(ROUTING_FILE).to_str().unwrap()
    );
    info!("Please review and edit the config to block specific addresses");

    Ok(())
}

async fn process_emails(
    gmail_client: &gmail::GmailClient,
    creds_config: &config::CredentialsConfig,
) -> Result<()> {
    info!("Starting email processing cycle");

    let routing_config = config::RoutingConfig::load(get_config_path(ROUTING_FILE))
        .context("Failed to load routing config")?;

    let date_filter = routing_config.updated_date.format("%Y/%m/%d").to_string();

    let message_ids = gmail_client
        .list_messages(&date_filter)
        .await
        .context("Failed to list messages")?;

    info!("Found {} messages to process", message_ids.len());

    let mut deleted_count = 0;
    let mut processed_count = 0;

    for (idx, msg_id) in message_ids.iter().enumerate() {
        if idx % 50 == 0 && idx > 0 {
            info!("Progress: {}/{} messages processed", idx, message_ids.len());
        }

        match process_single_message(gmail_client, msg_id, &creds_config.domain, &routing_config)
            .await
        {
            Ok(deleted) => {
                processed_count += 1;
                if deleted {
                    deleted_count += 1;
                }
            }
            Err(e) => {
                warn!("Failed to process message {}: {:#}", msg_id, e);
            }
        }
    }

    info!(
        "Processing complete: {} processed, {} deleted",
        processed_count, deleted_count
    );

    Ok(())
}

async fn process_single_message(
    gmail_client: &gmail::GmailClient,
    message_id: &str,
    domain: &str,
    routing_config: &config::RoutingConfig,
) -> Result<bool> {
    let message = gmail_client.get_message(message_id).await?;
    let recipients = processor::extract_recipients(&message, domain)?;

    if recipients.is_empty() {
        return Ok(false);
    }

    if processor::should_delete_message(&recipients, routing_config) {
        info!(
            "Deleting message {} (recipients: {:?})",
            message_id, recipients
        );
        gmail_client.delete_message(message_id).await?;
        return Ok(true);
    }

    Ok(false)
}
