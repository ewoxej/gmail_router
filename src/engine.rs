use crate::db;
use crate::gmail::GmailClient;
use crate::models::RouteAction;
use crate::processor;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

fn interval_seconds() -> u64 {
    std::env::var("CHECK_INTERVAL_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3600)
}

fn parse_dt(rfc3339: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(rfc3339)
        .with_context(|| format!("Bad date: {rfc3339}"))?
        .with_timezone(&Utc))
}

async fn build_client(user_id: i64) -> Result<GmailClient> {
    let (client_id, client_secret, refresh_token) = db::get_google_auth(user_id)
        .await?
        .ok_or_else(|| anyhow!("Not signed in with Google"))?;
    GmailClient::new_for_user(&client_id, &client_secret, &refresh_token).await
}

pub async fn scan_now(user_id: i64) -> Result<usize> {
    let settings = db::get_settings(user_id).await?;
    let client = build_client(user_id).await?;

    let after = match &settings.updated_date {
        Some(d) => parse_dt(d)?,
        None => parse_dt(&settings.start_date)?,
    };
    let date_filter = after.format("%Y/%m/%d").to_string();

    info!("[user {user_id}] scanning messages after {date_filter}");
    let ids = client.list_messages(&date_filter).await?;
    info!("[user {user_id}] found {} messages to scan", ids.len());

    let addresses = processor::collect_all_addresses(&client, &ids, &settings.domain).await?;
    let new_count = db::add_addresses(user_id, addresses).await?;

    db::set_updated_date(user_id, &Utc::now().to_rfc3339()).await?;
    info!("[user {user_id}] scan complete: {new_count} new address(es)");
    Ok(new_count)
}

async fn process_user(user_id: i64) -> Result<()> {
    let settings = db::get_settings(user_id).await?;
    let client = build_client(user_id).await?;
    let routes = db::routes_map(user_id).await?;

    let after = match &settings.updated_date {
        Some(d) => parse_dt(d)?,
        None => parse_dt(&settings.start_date)?,
    };
    let date_filter = after.format("%Y/%m/%d").to_string();

    let ids = client.list_messages(&date_filter).await?;
    info!("[user {user_id}] {} messages to process", ids.len());

    let mut acted = 0;
    let mut processed = 0;
    for (idx, id) in ids.iter().enumerate() {
        if idx % 50 == 0 && idx > 0 {
            info!("[user {user_id}] progress {idx}/{}", ids.len());
        }
        match process_single_message(&client, id, &settings.domain, &routes).await {
            Ok(did) => {
                processed += 1;
                if did {
                    acted += 1;
                }
            }
            Err(e) => warn!("[user {user_id}] failed on message {id}: {e:#}"),
        }
    }
    info!("[user {user_id}] processed {processed}, routed {acted}");
    Ok(())
}

async fn process_single_message(
    client: &GmailClient,
    message_id: &str,
    domain: &str,
    routes: &HashMap<String, RouteAction>,
) -> Result<bool> {
    let message = client.get_message(message_id).await?;
    let recipients = processor::extract_recipients(&message, domain)?;
    if recipients.is_empty() {
        return Ok(false);
    }

    match processor::resolve_action(&recipients, routes) {
        RouteAction::Keep => Ok(false),
        RouteAction::Trash => {
            info!("Trashing {message_id} (recipients: {recipients:?})");
            client.trash_message(message_id).await?;
            Ok(true)
        }
        RouteAction::Spam => {
            info!("Spam {message_id} (recipients: {recipients:?})");
            client.move_message_to_spam(message_id).await?;
            Ok(true)
        }
        RouteAction::Delete => {
            info!("Deleting {message_id} (recipients: {recipients:?})");
            client.delete_message(message_id).await?;
            Ok(true)
        }
        RouteAction::Forward { to } => {
            info!("Forwarding {message_id} to {to} then trashing");
            client.forward_message(message_id, &to).await?;
            client.trash_message(message_id).await?;
            Ok(true)
        }
    }
}

async fn tick_all() -> Result<()> {
    let user_ids = db::list_user_ids_with_auth().await?;
    for user_id in user_ids {
        let settings = match db::get_settings(user_id).await {
            Ok(s) => s,
            Err(e) => {
                error!("[user {user_id}] settings load failed: {e:#}");
                continue;
            }
        };
        let result = if settings.updated_date.is_none() {
            scan_now(user_id).await.map(|_| ())
        } else {
            process_user(user_id).await
        };
        if let Err(e) = result {
            error!("[user {user_id}] cycle error: {e:#}");
        }
    }
    Ok(())
}

pub async fn routing_loop() {
    info!("Routing loop started (interval {}s)", interval_seconds());
    loop {
        if let Err(e) = tick_all().await {
            error!("Routing sweep error: {e:#}");
        }
        sleep(Duration::from_secs(interval_seconds())).await;
    }
}
