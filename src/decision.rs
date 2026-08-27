use crate::db::{self, CurrentUser};
use crate::models::RouteAction;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct DecisionQuery {
    address: Option<String>,
    local_part: Option<String>,
}

#[derive(Debug, Serialize)]
struct DecisionResponse {
    local_part: String,
    action: String,
    forward_to: Option<String>,
    allow: bool,
}

fn resolve_local_part(q: &DecisionQuery) -> Option<String> {
    if let Some(addr) = q.address.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let lp = addr.split('@').next().unwrap_or(addr);
        return Some(lp.to_lowercase());
    }
    q.local_part
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

pub async fn route_decision(
    Extension(user): Extension<CurrentUser>,
    Query(q): Query<DecisionQuery>,
) -> Response {
    let Some(local_part) = resolve_local_part(&q) else {
        return (StatusCode::BAD_REQUEST, "missing `address` or `local_part`").into_response();
    };

    match db::get_or_create_route_action(user.user_id, &local_part).await {
        Ok(action) => {
            let forward_to = match &action {
                RouteAction::Forward { to } => Some(to.clone()),
                _ => None,
            };
            Json(DecisionResponse {
                local_part,
                action: action.kind().to_string(),
                forward_to,
                allow: action.is_keep(),
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e:#}")).into_response(),
    }
}
