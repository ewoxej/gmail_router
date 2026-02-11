use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub fn get_config_path(filename: &str) -> PathBuf {
    let mut path = dirs::config_dir().expect("Cannot find config dir");
    path.push("my_app");
    std::fs::create_dir_all(&path).expect("Cannot create config dir");
    path.push(filename);
    path
}

pub const CREDENTIALS_FILE: &str = "credentials.yaml";
pub const ROUTING_FILE: &str = "routing.yaml";

#[derive(Debug, Deserialize, Serialize)]
pub struct CredentialsConfig {
    pub google_credentials_path: String,
    pub domain: String,
    pub check_interval_seconds: u64,
    pub start_date: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct RoutingConfig {
    pub addresses: HashMap<String, bool>,
    pub updated_date: DateTime<Utc>,
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

        let config: RoutingConfig =
            serde_yaml::from_str(&contents).context("Failed to parse routing config YAML")?;

        Ok(config)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let yaml =
            serde_yaml::to_string(&self).context("Failed to serialize routing config to YAML")?;

        fs::write(path.as_ref(), yaml).context("Failed to write routing config file")?;

        Ok(())
    }

    pub fn is_allowed(&self, local_part: &str) -> bool {
        self.addresses.get(local_part).copied().unwrap_or(true)
    }

    pub fn add_address(&mut self, local_part: String) {
        self.addresses.entry(local_part).or_insert(true);
    }

    pub fn update_date(&mut self, date: DateTime<Utc>) {
        self.updated_date = date;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowed_default() {
        let config = RoutingConfig::default();
        assert!(config.is_allowed("test"));
    }

    #[test]
    fn test_is_allowed_explicit() {
        let mut config = RoutingConfig::default();
        config.addresses.insert("allowed".to_string(), true);
        config.addresses.insert("blocked".to_string(), false);

        assert!(config.is_allowed("allowed"));
        assert!(!config.is_allowed("blocked"));
        assert!(config.is_allowed("unknown"));
    }
}
