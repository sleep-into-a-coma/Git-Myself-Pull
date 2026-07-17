use crate::models::{AppState, CloseBehavior, OperationQueueStatus, ProxyMode, Repository, Settings, DEFAULT_UPDATE_ENDPOINT};
use chrono::Utc;
use serde_json::Value;
use std::{collections::VecDeque, fs, path::PathBuf, sync::Mutex};

pub struct Store { pub settings: Mutex<Settings>, logs: Mutex<VecDeque<String>>, config_dir: PathBuf }

impl Store {
    pub fn load() -> Self {
        let config_dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("GitAutoPull");
        let _ = fs::create_dir_all(&config_dir);
        let mut settings = load_settings(&config_dir.join("settings.json"));
        if settings.update_endpoint.trim().is_empty() { settings.update_endpoint = DEFAULT_UPDATE_ENDPOINT.into(); }
        let logs = fs::read_to_string(config_dir.join("GitAutoPull.log")).unwrap_or_default().lines().rev().take(500).map(str::to_owned).collect::<Vec<_>>().into_iter().rev().collect();
        Self { settings: Mutex::new(settings), logs: Mutex::new(logs), config_dir }
    }

    pub fn snapshot(&self) -> AppState {
        AppState { version: env!("CARGO_PKG_VERSION").into(), settings: self.settings.lock().unwrap().clone(), logs: self.logs.lock().unwrap().iter().cloned().collect(), operation_queue: OperationQueueStatus::default() }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = self.config_dir.join("settings.json");
        let json = serde_json::to_string_pretty(&*self.settings.lock().unwrap()).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn log(&self, message: impl AsRef<str>) {
        let line = format!("[{}] {}", Utc::now().format("%Y-%m-%d %H:%M:%S"), message.as_ref());
        let path = self.config_dir.join("GitAutoPull.log");
        let _ = fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| { use std::io::Write; writeln!(f, "{line}") });
        let mut logs = self.logs.lock().unwrap(); logs.push_back(line); while logs.len() > 500 { logs.pop_front(); }
        self.maintain_log();
    }

    pub fn clear_logs(&self) { self.logs.lock().unwrap().clear(); let _ = fs::write(self.config_dir.join("GitAutoPull.log"), ""); }

    fn maintain_log(&self) {
        let settings = self.settings.lock().unwrap().clone();
        if !settings.auto_maintain_logs { return }
        let path = self.config_dir.join("GitAutoPull.log");
        let limit = settings.max_log_size_mb.clamp(1, 100) as u64 * 1024 * 1024;
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) <= limit { return }
        let lines: Vec<_> = fs::read_to_string(&path).unwrap_or_default().lines().map(str::to_owned).collect();
        let _ = fs::write(path, lines[lines.len()/2..].join("\n"));
    }
}

fn load_settings(path: &PathBuf) -> Settings {
    let Ok(text) = fs::read_to_string(path) else { return Settings::default() };
    if let Ok(settings) = serde_json::from_str::<Settings>(&text) { return settings }
    let Ok(value) = serde_json::from_str::<Value>(&text) else { return Settings::default() };
    let mut settings = Settings::default();
    if let Some(repos) = value.get("Repositories").or_else(|| value.get("repositories")) { settings.repositories = serde_json::from_value::<Vec<Repository>>(repos.clone()).unwrap_or_default(); }
    settings.start_with_windows = bool_value(&value, "StartWithWindows", "startWithWindows").unwrap_or(false);
    settings.close_behavior = match value.get("CloseBehavior").or_else(|| value.get("closeBehavior")) { Some(Value::Number(n)) if n.as_i64() == Some(1) => CloseBehavior::Exit, Some(Value::String(s)) if s.eq_ignore_ascii_case("exit") => CloseBehavior::Exit, _ => CloseBehavior::Background };
    if let Some(proxy) = value.get("Proxy") { settings.proxy_mode = match proxy.get("Mode").and_then(Value::as_i64) { Some(1) => ProxyMode::Disabled, Some(2) => ProxyMode::Custom, _ => ProxyMode::System }; settings.proxy_address = proxy.get("Address").and_then(Value::as_str).unwrap_or_default().into(); }
    if let Some(m) = value.get("Maintenance") { settings.auto_maintain_logs = m.get("AutoMaintainLogs").and_then(Value::as_bool).unwrap_or(true); settings.max_log_size_mb = m.get("MaxLogSizeMb").and_then(Value::as_u64).unwrap_or(5) as u32; settings.auto_check_updates = m.get("AutoCheckUpdates").and_then(Value::as_bool).unwrap_or(false); settings.update_endpoint = m.get("UpdateManifestUrl").and_then(Value::as_str).unwrap_or_default().into(); }
    settings
}

fn bool_value(value: &Value, a: &str, b: &str) -> Option<bool> { value.get(a).or_else(|| value.get(b)).and_then(Value::as_bool) }
