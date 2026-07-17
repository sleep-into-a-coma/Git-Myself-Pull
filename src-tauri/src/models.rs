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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode { #[default] System, Light, Dark }

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MotionPreference { #[default] System, Reduce, Full }

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
    #[serde(default)] pub theme_mode: ThemeMode,
    #[serde(default = "default_accent_color")] pub accent_color: String,
    #[serde(default = "default_light_background")] pub light_background: String,
    #[serde(default = "default_light_foreground")] pub light_foreground: String,
    #[serde(default = "default_dark_background")] pub dark_background: String,
    #[serde(default = "default_dark_foreground")] pub dark_foreground: String,
    #[serde(default = "default_ui_font")] pub ui_font: String,
    #[serde(default = "default_code_font")] pub code_font: String,
    #[serde(default = "yes")] pub translucent_sidebar: bool,
    #[serde(default = "default_contrast")] pub contrast: u8,
    #[serde(default)] pub pointer_cursor: bool,
    #[serde(default)] pub motion_preference: MotionPreference,
    #[serde(default = "default_ui_font_size")] pub ui_font_size: u8,
    #[serde(default = "default_code_font_size")] pub code_font_size: u8,
}

fn yes() -> bool { true }
fn default_log_size() -> u32 { 5 }
fn default_update_endpoint() -> String { DEFAULT_UPDATE_ENDPOINT.into() }
fn default_accent_color() -> String { "#0169cc".into() }
fn default_light_background() -> String { "#ffffff".into() }
fn default_light_foreground() -> String { "#0d0d0d".into() }
fn default_dark_background() -> String { "#202223".into() }
fn default_dark_foreground() -> String { "#f4f4f4".into() }
fn default_ui_font() -> String { "'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif".into() }
fn default_code_font() -> String { "'Cascadia Mono', Consolas, monospace".into() }
fn default_contrast() -> u8 { 45 }
fn default_ui_font_size() -> u8 { 14 }
fn default_code_font_size() -> u8 { 12 }

impl Default for Settings {
    fn default() -> Self { Self { repositories: vec![], start_with_windows: false, close_behavior: CloseBehavior::Background, proxy_mode: ProxyMode::System, proxy_address: String::new(), auto_maintain_logs: true, max_log_size_mb: 5, auto_check_updates: true, update_endpoint: default_update_endpoint(), theme_mode: ThemeMode::System, accent_color: default_accent_color(), light_background: default_light_background(), light_foreground: default_light_foreground(), dark_background: default_dark_background(), dark_foreground: default_dark_foreground(), ui_font: default_ui_font(), code_font: default_code_font(), translucent_sidebar: true, contrast: default_contrast(), pointer_cursor: false, motion_preference: MotionPreference::System, ui_font_size: default_ui_font_size(), code_font_size: default_code_font_size() } }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState { pub version: String, pub settings: Settings, pub logs: Vec<String> }

#[derive(Debug)]
pub struct GitResult { pub success: bool, pub message: String, pub details: String }

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryPathKind { Missing, Empty, Git, NonGit, NestedGit, Invalid }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPathStatus { pub kind: RepositoryPathKind, pub message: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAuthStatus {
    pub git_available: bool,
    pub git_version: String,
    pub credential_manager_available: bool,
    pub credential_manager_version: String,
    pub credential_helper: String,
    pub accounts: Vec<GitAccountProfile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAccountProfile {
    pub login: String,
    pub name: Option<String>,
    pub bio: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    pub public_repos: u32,
    pub followers: u32,
    pub avatar_data: Option<String>,
    pub profile_error: Option<String>,
}
