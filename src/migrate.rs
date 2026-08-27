use crate::{config, db};
use anyhow::Result;
use serde::Deserialize;
use tracing::info;

#[derive(Deserialize)]
struct LegacyAuthorizedUser {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

pub async fn migrate_legacy_config() -> Result<()> {
    let username = std::env::var("MIGRATE_AS_USER").unwrap_or_else(|_| "dev".to_string());
    if username.is_empty() {
        return Ok(());
    }

    let creds_path = config::get_config_path(config::CREDENTIALS_FILE);
    let routing_path = config::get_config_path(config::ROUTING_FILE);
    let auth_path = config::get_config_path(config::AUTHORIZED_USER_FILE);

    if !creds_path.exists() && !routing_path.exists() && !auth_path.exists() {
        return Ok(());
    }

    let user_id = db::ensure_user(&username).await?;

    if db::has_settings_row(user_id).await? {
        return Ok(());
    }

    info!("Migrating legacy file config into the DB as user '{username}'");

    let (domain, start_date) = match config::CredentialsConfig::load(&creds_path) {
        Ok(c) => (c.domain, c.start_date.to_rfc3339()),
        Err(_) => (String::new(), "1970-01-01T00:00:00Z".to_string()),
    };
    db::set_settings(user_id, &domain, &start_date).await?;

    if let Ok(routing) = config::RoutingConfig::load(&routing_path) {
        for (local_part, action) in &routing.addresses {
            db::upsert_route(user_id, local_part, action.clone()).await?;
        }
        db::set_updated_date(user_id, &routing.updated_date.to_rfc3339()).await?;
        info!("  imported {} route(s)", routing.addresses.len());
    }

    if let Ok(bytes) = std::fs::read(&auth_path) {
        if let Ok(au) = serde_json::from_slice::<LegacyAuthorizedUser>(&bytes) {
            db::set_google_auth(user_id, &au.client_id, &au.client_secret, &au.refresh_token).await?;
            info!("  imported stored Google credentials");
        }
    }

    info!("Migration complete for user '{username}'");
    Ok(())
}
