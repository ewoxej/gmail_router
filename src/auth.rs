//! Web-based Google OAuth (server-only). Replaces the old desktop InstalledFlow
//! (which popped a local browser and couldn't run headless). Two axum routes,
//! both running *under* the `auth` middleware so they know which user is signing in.

use crate::config;
use crate::db::{self, CurrentUser};
use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Extension,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const SCOPE: &str = "https://mail.google.com/";
const DEFAULT_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

static PENDING_STATES: Mutex<Option<HashSet<String>>> = Mutex::new(None);

#[derive(Debug, Clone, Deserialize)]
struct OauthClient {
    client_id: String,
    client_secret: String,
    auth_uri: Option<String>,
    token_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OauthClientFile {
    web: Option<OauthClient>,
    installed: Option<OauthClient>,
}

fn client_secret_path() -> PathBuf {
    match std::env::var("GOOGLE_CLIENT_SECRET") {
        Ok(p) if !p.is_empty() => config::resolve_credentials_file(&p),
        _ => config::get_config_path("secret.json"),
    }
}

fn load_client() -> anyhow::Result<OauthClient> {
    let path = client_secret_path();
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read OAuth client file {:?}: {}", path, e))?;
    let file: OauthClientFile = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse OAuth client JSON: {}", e))?;
    file.web
        .or(file.installed)
        .ok_or_else(|| anyhow::anyhow!("OAuth client JSON has neither a `web` nor `installed` key"))
}

fn base_url() -> String {
    std::env::var("GMAIL_ROUTER_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://localhost:8080".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn redirect_uri() -> String {
    format!("{}/auth/callback", base_url())
}

fn random_state() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack = &nanos as *const _ as usize;
    format!("{:x}{:x}", nanos, stack)
}

fn remember_state(state: &str) {
    let mut guard = PENDING_STATES.lock().unwrap();
    guard.get_or_insert_with(HashSet::new).insert(state.to_string());
}

fn take_state(state: &str) -> bool {
    let mut guard = PENDING_STATES.lock().unwrap();
    guard
        .as_mut()
        .map(|set| set.remove(state))
        .unwrap_or(false)
}

pub async fn login() -> Response {
    let client = match load_client() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("OAuth is not configured: {e:#}"),
            )
                .into_response()
        }
    };

    let state = random_state();
    remember_state(&state);

    let auth_uri = client.auth_uri.as_deref().unwrap_or(DEFAULT_AUTH_URI);
    let url = format!(
        "{auth_uri}?client_id={client_id}&redirect_uri={redirect}&response_type=code\
         &scope={scope}&access_type=offline&prompt=consent&state={state}",
        client_id = urlencoding::encode(&client.client_id),
        redirect = urlencoding::encode(&redirect_uri()),
        scope = urlencoding::encode(SCOPE),
        state = urlencoding::encode(&state),
    );

    Redirect::to(&url).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[allow(dead_code)]
    access_token: String,
    refresh_token: Option<String>,
}

pub async fn callback(
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if let Some(err) = q.error {
        return (StatusCode::BAD_REQUEST, format!("Google returned an error: {err}"))
            .into_response();
    }

    let (Some(code), Some(state)) = (q.code, q.state) else {
        return (StatusCode::BAD_REQUEST, "Missing code/state in callback").into_response();
    };

    if !take_state(&state) {
        return (StatusCode::BAD_REQUEST, "Invalid or expired OAuth state").into_response();
    }

    let client = match load_client() {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("OAuth misconfigured: {e:#}"))
                .into_response()
        }
    };

    let token_uri = client.token_uri.as_deref().unwrap_or(DEFAULT_TOKEN_URI);
    let redirect = redirect_uri();
    let params = [
        ("code", code.as_str()),
        ("client_id", client.client_id.as_str()),
        ("client_secret", client.client_secret.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("grant_type", "authorization_code"),
    ];

    let http = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("HTTP client error: {e}"))
                .into_response()
        }
    };

    let resp = match http.post(token_uri).form(&params).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("Token exchange request failed: {e}"))
                .into_response()
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return (StatusCode::BAD_GATEWAY, format!("Token exchange failed ({status}): {body}"))
            .into_response();
    }

    let token: TokenResponse = match resp.json().await {
        Ok(t) => t,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("Bad token response: {e}")).into_response()
        }
    };

    let Some(refresh_token) = token.refresh_token else {
        return (
            StatusCode::BAD_GATEWAY,
            "Google did not return a refresh token. Revoke the app's access in your Google \
             account and sign in again (the consent screen must be shown).",
        )
            .into_response();
    };

    if let Err(e) = db::set_google_auth(
        user.user_id,
        &client.client_id,
        &client.client_secret,
        &refresh_token,
    )
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to store credentials: {e:#}"),
        )
            .into_response();
    }

    tracing::info!("Google sign-in complete for user '{}'", user.username);
    Redirect::to("/").into_response()
}
