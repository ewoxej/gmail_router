use crate::models::RouteAction;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub fn get_config_path(filename: &str) -> PathBuf {
    let mut path = dirs::config_dir().expect("Cannot find config dir");
    path.push("gmail_router");
    std::fs::create_dir_all(&path).expect("Cannot create config dir");
    path.push(filename);
    path
}

pub const CREDENTIALS_FILE: &str = "credentials.yaml";
pub const ROUTING_FILE: &str = "routing.yaml";

pub fn is_decision_mode() -> bool {
    std::env::var("ROUTER_MODE")
        .map(|m| m.eq_ignore_ascii_case("decision"))
        .unwrap_or(false)
}

pub fn mode_str() -> &'static str {
    if is_decision_mode() {
        "decision"
    } else {
        "active"
    }
}

pub const AUTHORIZED_USER_FILE: &str = "authorized_user.json";

pub fn resolve_credentials_file(google_credentials_path: &str) -> PathBuf {
    let p = Path::new(google_credentials_path);
    if p.is_absolute() {
        if p.exists() {
            return p.to_path_buf();
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "secret.json".to_string());
        return get_config_path(&name);
    }
    get_config_path(google_credentials_path)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CredentialsConfig {
    pub google_credentials_path: String,
    pub domain: String,
    pub check_interval_seconds: u64,
    pub start_date: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct RoutingConfig {
    pub addresses: HashMap<String, RouteAction>,
    pub updated_date: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct LegacyRoutingConfig {
    #[serde(default)]
    addresses: HashMap<String, bool>,
    #[serde(default)]
    updated_date: DateTime<Utc>,
}

impl From<LegacyRoutingConfig> for RoutingConfig {
    fn from(legacy: LegacyRoutingConfig) -> Self {
        let addresses = legacy
            .addresses
            .into_iter()
            .map(|(k, allowed)| {
                let action = if allowed {
                    RouteAction::Keep
                } else {
                    RouteAction::Delete
                };
                (k, action)
            })
            .collect();
        RoutingConfig {
            addresses,
            updated_date: legacy.updated_date,
        }
    }
}

impl CredentialsConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents =
            fs::read_to_string(path.as_ref()).context("Failed to read credentials config file")?;

        let config: CredentialsConfig =
            serde_yaml::from_str(&contents).context("Failed to parse credentials config YAML")?;

        Ok(config)
    }
}

impl RoutingConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents =
            fs::read_to_string(path.as_ref()).context("Failed to read routing config file")?;
        if let Ok(config) = serde_yaml::from_str::<RoutingConfig>(&contents) {
            return Ok(config);
        }
        let legacy: LegacyRoutingConfig =
            serde_yaml::from_str(&contents).context("Failed to parse routing config YAML")?;
        Ok(legacy.into())
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let yaml =
            serde_yaml::to_string(&self).context("Failed to serialize routing config to YAML")?;

        fs::write(path.as_ref(), yaml).context("Failed to write routing config file")?;

        Ok(())
    }

    pub fn action_for(&self, local_part: &str) -> RouteAction {
        self.addresses.get(local_part).cloned().unwrap_or_default()
    }

    pub fn add_address(&mut self, local_part: String) {
        self.addresses.entry(local_part).or_insert(RouteAction::Keep);
    }

    pub fn update_date(&mut self, date: DateTime<Utc>) {
        self.updated_date = date;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_for_default() {
        let config = RoutingConfig::default();
        assert_eq!(config.action_for("test"), RouteAction::Keep);
    }

    #[test]
    fn test_action_for_explicit() {
        let mut config = RoutingConfig::default();
        config.addresses.insert("keep".to_string(), RouteAction::Keep);
        config.addresses.insert("trash".to_string(), RouteAction::Trash);

        assert_eq!(config.action_for("keep"), RouteAction::Keep);
        assert_eq!(config.action_for("trash"), RouteAction::Trash);
        assert_eq!(config.action_for("unknown"), RouteAction::Keep);
    }

    #[test]
    fn test_yaml_roundtrip_with_forward() {
        let mut config = RoutingConfig::default();
        config.addresses.insert("del".to_string(), RouteAction::Delete);
        config.addresses.insert(
            "fwd".to_string(),
            RouteAction::Forward {
                to: "me@other.com".to_string(),
            },
        );
        let yaml = serde_yaml::to_string(&config).unwrap();
        let back: RoutingConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.action_for("del"), RouteAction::Delete);
        assert_eq!(
            back.action_for("fwd"),
            RouteAction::Forward {
                to: "me@other.com".to_string()
            }
        );
    }

    #[test]
    fn test_legacy_bool_migration() {
        // Old routing.yaml shape: addresses map of local-part -> bool.
        let yaml = "addresses:\n  ok: true\n  bad: false\nupdated_date: \"2024-01-01T00:00:00Z\"\n";
        let cfg = serde_yaml::from_str::<RoutingConfig>(yaml)
            .ok()
            .unwrap_or_else(|| {
                let legacy: LegacyRoutingConfig = serde_yaml::from_str(yaml).unwrap();
                legacy.into()
            });
        assert_eq!(cfg.action_for("ok"), RouteAction::Keep);
        assert_eq!(cfg.action_for("bad"), RouteAction::Delete);
    }
}
