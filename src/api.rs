use crate::models::{AddressEntry, ApiTokenRow, RouteAction, RouterStatus};
use dioxus::prelude::*;

#[get("/api/status")]
pub async fn router_status() -> Result<RouterStatus, ServerFnError> {
    let uid = crate::db::current_user().user_id;
    Ok(crate::db::router_status(uid).await.map_err(anyhow::Error::from)?)
}

#[get("/api/addresses")]
pub async fn list_addresses() -> Result<Vec<AddressEntry>, ServerFnError> {
    let uid = crate::db::current_user().user_id;
    Ok(crate::db::list_routes(uid).await.map_err(anyhow::Error::from)?)
}

#[post("/api/set-action")]
pub async fn set_action(local_part: String, action: RouteAction) -> Result<(), ServerFnError> {
    let uid = crate::db::current_user().user_id;
    crate::db::set_route_action(uid, &local_part, action)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(())
}

#[post("/api/scan")]
pub async fn scan_now() -> Result<usize, ServerFnError> {
    let uid = crate::db::current_user().user_id;
    let new_count = crate::engine::scan_now(uid).await.map_err(anyhow::Error::from)?;
    Ok(new_count)
}

#[post("/api/tokens/create")]
pub async fn create_api_token(name: String) -> Result<String, ServerFnError> {
    let uid = crate::db::current_user().user_id;
    let name = if name.trim().is_empty() { "api token".to_string() } else { name };
    let raw = crate::db::create_token(uid, &name).await.map_err(anyhow::Error::from)?;
    Ok(raw)
}

#[get("/api/tokens")]
pub async fn list_api_tokens() -> Result<Vec<ApiTokenRow>, ServerFnError> {
    let uid = crate::db::current_user().user_id;
    Ok(crate::db::list_tokens(uid).await.map_err(anyhow::Error::from)?)
}

#[post("/api/tokens/revoke")]
pub async fn revoke_api_token(id: i64) -> Result<(), ServerFnError> {
    let uid = crate::db::current_user().user_id;
    crate::db::revoke_token(uid, id).await.map_err(anyhow::Error::from)?;
    Ok(())
}
