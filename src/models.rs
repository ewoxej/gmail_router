use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteAction {
    Keep,
    Trash,
    Spam,
    Delete,
    Forward { to: String },
}

impl Default for RouteAction {
    fn default() -> Self {
        RouteAction::Keep
    }
}

impl RouteAction {
    pub fn is_keep(&self) -> bool {
        matches!(self, RouteAction::Keep)
    }

    pub fn kind(&self) -> &'static str {
        match self {
            RouteAction::Keep => "keep",
            RouteAction::Trash => "trash",
            RouteAction::Spam => "spam",
            RouteAction::Delete => "delete",
            RouteAction::Forward { .. } => "forward",
        }
    }

    pub fn forward_to(&self) -> &str {
        match self {
            RouteAction::Forward { to } => to,
            _ => "",
        }
    }

    pub fn from_kind(kind: &str, forward_to: &str) -> RouteAction {
        match kind {
            "trash" => RouteAction::Trash,
            "spam" => RouteAction::Spam,
            "delete" => RouteAction::Delete,
            "forward" => RouteAction::Forward {
                to: forward_to.trim().to_string(),
            },
            _ => RouteAction::Keep,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddressEntry {
    pub local_part: String,
    pub action: RouteAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiTokenRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RouterStatus {
    pub authenticated: bool,
    pub domain: String,
    pub last_scan: Option<String>,
    pub mode: String,
}
