use crate::db::{self, CurrentUser};
use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
};

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

pub fn main_server() {
    init_logging();
    tracing::info!("Starting Gmail Router (web, multi-user)");
    dioxus::serve(|| router());
}

fn proxy_secret_ok(req: &Request) -> bool {
    let expected = match std::env::var("PROXY_AUTH_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => return false,
    };
    req.headers()
        .get("X-Proxy-Auth")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == expected)
        .unwrap_or(false)
}

async fn auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    if req.uri().path().starts_with("/assets/") {
        return Ok(next.run(req).await);
    }

    let auth_disabled = std::env::var("AUTH_ENABLED").as_deref() == Ok("false");
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let remote_user = if proxy_secret_ok(&req) {
        req.headers()
            .get("Remote-User")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    } else {
        None
    };

    let resolved: Option<(i64, String, Option<i64>)> = if auth_disabled {
        let dev_user = std::env::var("DEV_USER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "dev".to_string());
        db::ensure_user(&dev_user).await.ok().map(|id| (id, dev_user, None))
    } else if let Some(token) = bearer {
        db::resolve_device_token(&token)
            .await
            .ok()
            .flatten()
            .map(|(id, username, token_id)| (id, username, Some(token_id)))
    } else if let Some(username) = remote_user {
        db::ensure_user(&username)
            .await
            .ok()
            .map(|id| (id, username, None))
    } else {
        None
    };

    let Some((user_id, username, device_token_id)) = resolved else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    req.extensions_mut().insert(CurrentUser {
        user_id,
        username,
        device_token_id,
    });
    Ok(next.run(req).await)
}

pub async fn router() -> anyhow::Result<axum::Router> {
    db::pool().await?;
    crate::migrate::migrate_legacy_config().await?;

    let auth_enabled = std::env::var("AUTH_ENABLED").as_deref() != Ok("false");
    let proxy_secret_set = std::env::var("PROXY_AUTH_SECRET").map(|s| !s.is_empty()).unwrap_or(false);
    if auth_enabled && !proxy_secret_set {
        eprintln!(
            "WARNING: PROXY_AUTH_SECRET is unset — the Remote-User (Authelia) auth path is \
             disabled. Set PROXY_AUTH_SECRET on both the app and the proxy (injected as the \
             X-Proxy-Auth header) to enable it, or set AUTH_ENABLED=false for local dev."
        );
    }

    if crate::config::is_decision_mode() {
        tracing::info!("ROUTER_MODE=decision — Gmail routing loop disabled; serving /api/route only");
    } else {
        tokio::spawn(crate::engine::routing_loop());
    }

    let app = dioxus::server::router(crate::App)
        .route("/auth/login", get(crate::auth::login))
        .route("/auth/callback", get(crate::auth::callback))
        .route("/api/route", get(crate::decision::route_decision))
        .layer(middleware::from_fn(auth));

    Ok(app)
}
