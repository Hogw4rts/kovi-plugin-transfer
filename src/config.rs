use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransferConfig {
    #[serde(default)]
    pub rules: Vec<TransferRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransferRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    pub source: Source,
    pub target: Target,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Source {
    #[serde(rename = "private")]
    Private { qq: i64 },
    #[serde(rename = "group")]
    Group { group_id: i64 },
    #[serde(rename = "group_user")]
    GroupUser { group_id: i64, qq: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Target {
    #[serde(rename = "private")]
    Private { qq: i64 },
    #[serde(rename = "group")]
    Group { group_id: i64 },
}

fn default_true() -> bool {
    true
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self { rules: vec![] }
    }
}

pub(crate) fn load_config(data_dir: &Path) -> TransferConfig {
    let path = data_dir.join("transfer.json");
    match kovi::utils::load_json_data(TransferConfig::default(), &path) {
        Ok(c) => {
            kovi::log::info!("transfer: loaded {} rules", c.rules.len());
            c
        }
        Err(e) => {
            kovi::log::error!("transfer: failed to load config from {}: {}, using defaults", path.display(), e);
            TransferConfig::default()
        }
    }
}

pub(crate) fn save_config(config: &TransferConfig, data_dir: &Path) {
    let path = data_dir.join("transfer.json");
    if let Err(e) = kovi::utils::save_json_data(config, &path) {
        kovi::log::warn!("transfer: failed to save config: {}", e);
    }
}
