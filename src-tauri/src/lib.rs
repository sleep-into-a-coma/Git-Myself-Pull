mod git;
mod models;
mod store;

use chrono::Utc;
use models::{AppState, GitAuthStatus, Repository, RepositoryPathKind, RepositoryPathStatus, Settings};
use std::time::Duration;
use store::Store;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::UpdaterExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatus {
    available: bool,
    version: Option<String>,
    message: String,
}

fn publish(app: &AppHandle) { let _ = app.emit("state-changed", app.state::<Store>().snapshot()); }

#[tauri::command]
fn get_state(store: State<'_, Store>) -> AppState { store.snapshot() }

#[tauri::command]
fn save_repository(app: AppHandle, store: State<'_, Store>, mut repository: Repository) -> Result<Repository, String> {
    if repository.url.trim().is_empty() || repository.local_path.trim().is_empty() { return Err("请填写 Git 地址和本地目录".into()) }
    if repository.id.is_empty() { repository.id = uuid::Uuid::new_v4().to_string(); }
    if repository.name.trim().is_empty() { repository.name = guess_name(&repository.url); }
    repository.interval_minutes = repository.interval_minutes.clamp(1, 10080);
    {
        let mut settings = store.settings.lock().unwrap();
        if settings.repositories.iter().any(|r| r.id != repository.id && r.local_path.eq_ignore_ascii_case(&repository.local_path)) { return Err("该本地目录已由其他项目使用".into()) }
        if let Some(existing) = settings.repositories.iter_mut().find(|r| r.id == repository.id) { *existing = repository.clone(); } else { settings.repositories.push(repository.clone()); }
    }
    store.save()?; store.log(format!("已保存项目：{}", repository.name)); publish(&app); Ok(repository)
}

#[tauri::command]
fn delete_repository(app: AppHandle, store: State<'_, Store>, id: String) -> Result<(), String> {
    store.settings.lock().unwrap().repositories.retain(|r| r.id != id); store.save()?; publish(&app); Ok(())
}

#[tauri::command]
fn detect_branch(path: String) -> String { git::detect_branch(&path) }

#[tauri::command]
fn inspect_repository_path(path: String) -> RepositoryPathStatus { git::inspect_path(&path) }

#[tauri::command]
fn get_git_auth_status() -> GitAuthStatus { git::auth_status() }

#[tauri::command]
async fn login_github(store: State<'_, Store>) -> Result<String, String> {
    let (proxy_mode, proxy_address) = {
        let settings = store.settings.lock().unwrap();
        (settings.proxy_mode.clone(), settings.proxy_address.clone())
    };
    tauri::async_runtime::spawn_blocking(move || git::github_login(proxy_mode, &proxy_address))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn logout_github(account: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git::github_logout(&account))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn choose_folder(app: AppHandle) -> Option<String> { app.dialog().file().blocking_pick_folder().map(|p| p.to_string()) }

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() { return Err("本地目录不存在".into()) }
    std::process::Command::new("explorer.exe").arg(path).spawn().map_err(|e| e.to_string())?; Ok(())
}

#[tauri::command]
fn save_settings(app: AppHandle, store: State<'_, Store>, mut settings: Settings) -> Result<(), String> {
    settings.max_log_size_mb = settings.max_log_size_mb.clamp(1, 100);
    settings.contrast = settings.contrast.min(100);
    settings.ui_font_size = settings.ui_font_size.clamp(11, 20);
    settings.code_font_size = settings.code_font_size.clamp(10, 20);
    for repo in &mut settings.repositories { repo.interval_minutes = repo.interval_minutes.clamp(1, 10080); }
    let autostart = app.autolaunch();
    if settings.start_with_windows { autostart.enable().map_err(|e| e.to_string())?; } else { autostart.disable().map_err(|e| e.to_string())?; }
    *store.settings.lock().unwrap() = settings; store.save()?; publish(&app); Ok(())
}

#[tauri::command]
fn clear_logs(app: AppHandle, store: State<'_, Store>) { store.clear_logs(); publish(&app); }

#[tauri::command]
async fn update_repository(app: AppHandle, id: String) -> Result<(), String> { update_one(app, id).await }

#[tauri::command]
async fn initialize_repository(app: AppHandle, id: String) -> Result<String, String> {
    let store = app.state::<Store>();
    let (repo, proxy_mode, proxy_address) = {
        let mut settings = store.settings.lock().unwrap();
        let proxy_mode = settings.proxy_mode.clone();
        let proxy_address = settings.proxy_address.clone();
        let Some(repo) = settings.repositories.iter_mut().find(|repo| repo.id == id) else { return Err("项目不存在".into()) };
        if repo.is_running { return Err("项目正在执行其他任务".into()) }
        repo.is_running = true;
        repo.last_status = "正在注册 Git 仓库…".into();
        (repo.clone(), proxy_mode, proxy_address)
    };
    publish(&app);
    store.log(format!("[{}] 开始注册 Git 仓库", repo.name));
    let repo_name = repo.name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || git::initialize(&repo, proxy_mode, &proxy_address)).await.map_err(|error| error.to_string())?;
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(target) = settings.repositories.iter_mut().find(|repo| repo.id == id) {
            target.is_running = false;
            target.last_status = result.message.clone();
        }
    }
    store.log(format!("[{}] {}", repo_name, result.message));
    if !result.details.is_empty() { store.log(result.details); }
    store.save()?;
    publish(&app);
    if result.success { Ok(result.message) } else { Err(result.message) }
}

#[tauri::command]
async fn update_all(app: AppHandle) -> Result<(), String> {
    let ids: Vec<_> = app.state::<Store>().settings.lock().unwrap().repositories.iter().map(|r| r.id.clone()).collect();
    let mut errors = Vec::new();
    for id in ids {
        if let Err(error) = update_one(app.clone(), id).await { errors.push(error); }
    }
    if errors.is_empty() { Ok(()) } else { Err(format!("{} 个项目更新失败：{}", errors.len(), errors.join("；"))) }
}

async fn update_one(app: AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Store>();
    let (repo, proxy_mode, proxy_address) = {
        let mut settings = store.settings.lock().unwrap();
        let proxy_mode = settings.proxy_mode.clone(); let proxy_address = settings.proxy_address.clone();
        let Some(repo) = settings.repositories.iter_mut().find(|r| r.id == id) else { return Err("项目不存在".into()) };
        if repo.is_running { return Ok(()) }
        let path_kind = git::inspect_path(&repo.local_path).kind;
        repo.is_running = true;
        repo.last_attempt = Some(Utc::now());
        repo.last_status = if matches!(path_kind, RepositoryPathKind::Missing | RepositoryPathKind::Empty) { "正在克隆…".into() } else { "正在更新…".into() };
        (repo.clone(), proxy_mode, proxy_address)
    };
    let cloning = matches!(git::inspect_path(&repo.local_path).kind, RepositoryPathKind::Missing | RepositoryPathKind::Empty);
    publish(&app); store.log(format!("[{}] {}", repo.name, if cloning { "开始克隆" } else { "开始检查更新" }));
    let repo_name = repo.name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || git::update(&repo, proxy_mode, &proxy_address)).await.map_err(|e| e.to_string())?;
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(target) = settings.repositories.iter_mut().find(|r| r.id == id) { target.is_running = false; target.last_status = result.message.clone(); if result.success { target.last_success = Some(Utc::now()); } }
    }
    store.log(format!("[{}] {}", repo_name, result.message)); if !result.details.is_empty() { store.log(result.details); } store.save()?; publish(&app);
    if result.success { Ok(()) } else { Err(result.message) }
}

#[tauri::command]
async fn check_app_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let endpoint = app.state::<Store>().settings.lock().unwrap().update_endpoint.clone();
    if endpoint.trim().is_empty() { return Err("请先填写更新服务地址".into()) }
    let url = url::Url::parse(&endpoint).map_err(|_| "更新服务地址无效")?;
    if url.scheme() != "https" { return Err("更新服务必须使用 HTTPS".into()) }
    let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(UpdateStatus { available: false, version: None, message: "当前已是最新版本".into() })
    };
    let version = update.version.clone();
    Ok(UpdateStatus { available: true, version: Some(version.clone()), message: format!("发现新版本 {version}") })
}

#[tauri::command]
async fn install_app_update(app: AppHandle) -> Result<(), String> {
    let endpoint = app.state::<Store>().settings.lock().unwrap().update_endpoint.clone();
    let url = url::Url::parse(&endpoint).map_err(|_| "更新服务地址无效")?;
    if url.scheme() != "https" { return Err("更新服务必须使用 HTTPS".into()) }
    let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else { return Err("当前已是最新版本".into()) };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}

#[tauri::command]
fn exit_app(app: AppHandle) { app.exit(0); }

fn guess_name(url: &str) -> String { url.trim_end_matches(['/', '\\']).rsplit(['/', ':']).next().unwrap_or("repository").trim_end_matches(".git").to_string() }

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::{menu::{Menu, MenuItem}, tray::TrayIconBuilder};
    let show = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "全部更新", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &update, &quit])?;
    TrayIconBuilder::new().menu(&menu).on_menu_event(|app, event| match event.id.as_ref() {
        "show" => { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } }
        "update" => { let app = app.clone(); tauri::async_runtime::spawn(async move { let _ = update_all(app).await; }); }
        "quit" => app.exit(0), _ => {}
    }).build(app)?;
    Ok(())
}

fn start_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio_sleep(Duration::from_secs(30)).await;
            let now = Utc::now();
            let ids: Vec<_> = app.state::<Store>().settings.lock().unwrap().repositories.iter().filter(|r| r.auto_pull && !r.is_running && r.last_attempt.map(|last| now.signed_duration_since(last).num_minutes() >= r.interval_minutes as i64).unwrap_or(true)).map(|r| r.id.clone()).collect();
            for id in ids { let _ = update_one(app.clone(), id).await; }
        }
    });
}

async fn tokio_sleep(duration: Duration) { tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await.ok(); }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Store::load())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| { setup_tray(app)?; if std::env::args().any(|a| a == "--minimized") { if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); } } start_scheduler(app.handle().clone()); Ok(()) })
        .on_window_event(|window, event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { if window.app_handle().state::<Store>().settings.lock().unwrap().close_behavior == models::CloseBehavior::Background { api.prevent_close(); let _ = window.hide(); } } })
        .invoke_handler(tauri::generate_handler![get_state, save_repository, delete_repository, detect_branch, inspect_repository_path, get_git_auth_status, login_github, logout_github, choose_folder, open_folder, save_settings, clear_logs, update_repository, initialize_repository, update_all, check_app_update, install_app_update, exit_app])
        .run(tauri::generate_context!())
        .expect("error while running Git Auto Pull");
}
