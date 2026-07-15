use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_UPDATE_ENDPOINT: &str = "https://github.com/sleep-into-a-coma/Git-Myself-Pull/releases/latest/download/latest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    #[serde(default, alias = "Id")]
    pub id: String,
    #[serde(default, alias = "Name")]
    pub name: String,
    #[serde(default, alias = "Url")]
    pub url: String,
    #[serde(default, alias = "LocalPath")]
    pub local_path: String,
    #[serde(default, alias = "Branch")]
    pub branch: String,
    #[serde(default, alias = "AutoPull")]
    pub auto_pull: bool,
    #[serde(default = "default_interval", alias = "IntervalMinutes")]
    pub interval_minutes: u32,
    #[serde(default, alias = "LastAttempt")]
    pub last_attempt: Option<DateTime<Utc>>,
    #[serde(default, alias = "LastSuccess")]
    pub last_success: Option<DateTime<Utc>>,
    #[serde(default = "default_status", alias = "LastStatus")]
    pub last_status: String,
    #[serde(default, skip_deserializing)]
    pub is_running: bool,
}

fn default_interval() -> u32 { 30 }
fn default_status() -> String { "尚未更新".into() }

impl Default for Repository {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), url: String::new(), local_path: String::new(), branch: String::new(), auto_pull: false, interval_minutes: 30, last_attempt: None, last_success: None, last_status: default_status(), is_running: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode { #[default] System, Disabled, Custom }

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CloseBehavior { #[default] Background, Exit }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)] pub repositories: Vec<Repository>,
    #[serde(default)] pub start_with_windows: bool,
    #[serde(default)] pub close_behavior: CloseBehavior,
    #[serde(default)] pub proxy_mode: ProxyMode,
    #[serde(default)] pub proxy_address: String,
    #[serde(default = "yes")] pub auto_maintain_logs: bool,
    #[serde(default = "default_log_size")] pub max_log_size_mb: u32,
    #[serde(default = "yes")] pub auto_check_updates: bool,
    #[serde(default = "default_update_endpoint")] pub update_endpoint: String,
}

fn yes() -> bool { true }
fn default_log_size() -> u32 { 5 }
fn default_update_endpoint() -> String { DEFAULT_UPDATE_ENDPOINT.into() }

impl Default for Settings {
    fn default() -> Self { Self { repositories: vec![], start_with_windows: false, close_behavior: CloseBehavior::Background, proxy_mode: ProxyMode::System, proxy_address: String::new(), auto_maintain_logs: true, max_log_size_mb: 5, auto_check_updates: true, update_endpoint: default_update_endpoint() } }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState { pub version: String, pub settings: Settings, pub logs: Vec<String> }

#[derive(Debug)]
pub struct GitResult { pub success: bool, pub message: String, pub details: String }
