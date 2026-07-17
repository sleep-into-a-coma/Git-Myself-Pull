mod git;
mod models;
mod operation_queue;
mod store;

use chrono::Utc;
use models::{
    AppState, GitAuthStatus, GitHubProject, GitResult, LocalGitProject, ManagedProjectStatus,
    OperationQueueStatus, Repository, RepositoryPathKind, RepositoryPathStatus, Settings,
};
use operation_queue::{OperationPermit, OperationQueue};
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

fn snapshot(app: &AppHandle) -> AppState {
    let queue = app.state::<OperationQueue>().snapshot();
    let mut state = app.state::<Store>().snapshot();
    for repository in &mut state.settings.repositories {
        repository.is_running = queue.active.contains(&repository.id);
        repository.is_queued = false;
        repository.queue_position = 0;
        if let Some((position, (_, label))) = queue.waiting.iter().enumerate().find(|(_, (id, _))| id == &repository.id) {
            repository.is_queued = true;
            repository.queue_position = position as u32 + 1;
            repository.last_status = format!("{label} · 队列第 {} 位", position + 1);
        }
    }
    state.operation_queue = OperationQueueStatus {
        active: queue.active.len() as u32,
        queued: queue.waiting.len() as u32,
        max_concurrent: queue.limit as u8,
    };
    state
}

fn publish(app: &AppHandle) { let _ = app.emit("state-changed", snapshot(app)); }

#[tauri::command]
fn get_state(app: AppHandle) -> AppState { snapshot(&app) }

#[tauri::command]
fn save_repository(app: AppHandle, store: State<'_, Store>, mut repository: Repository) -> Result<Repository, String> {
    if repository.url.trim().is_empty() || repository.local_path.trim().is_empty() { return Err("请填写 Git 地址和本地目录".into()) }
    if !repository.id.is_empty() && app.state::<OperationQueue>().contains(&repository.id) { return Err("项目有正在执行或等待中的 Git 任务".into()) }
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
    if app.state::<OperationQueue>().contains(&id) { return Err("请等待该项目的 Git 任务结束后再删除".into()) }
    store.settings.lock().unwrap().repositories.retain(|r| r.id != id); store.save()?; publish(&app); Ok(())
}

#[tauri::command]
fn detect_branch(path: String) -> String { git::detect_branch(&path) }

#[tauri::command]
fn inspect_repository_path(path: String) -> RepositoryPathStatus { git::inspect_path(&path) }

#[tauri::command]
async fn get_git_auth_status(store: State<'_, Store>) -> Result<GitAuthStatus, String> {
    let (proxy_mode, proxy_address) = {
        let settings = store.settings.lock().unwrap();
        (settings.proxy_mode.clone(), settings.proxy_address.clone())
    };
    tauri::async_runtime::spawn_blocking(move || git::auth_status(proxy_mode, &proxy_address))
        .await
        .map_err(|error| error.to_string())
}

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
async fn list_github_projects(
    store: State<'_, Store>,
    account: String,
) -> Result<Vec<GitHubProject>, String> {
    let (proxy_mode, proxy_address) = {
        let settings = store.settings.lock().unwrap();
        (settings.proxy_mode.clone(), settings.proxy_address.clone())
    };
    tauri::async_runtime::spawn_blocking(move || {
        git::github_projects(&account, proxy_mode, &proxy_address)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn discover_local_projects(root: String) -> Result<Vec<LocalGitProject>, String> {
    tauri::async_runtime::spawn_blocking(move || git::discover_local_projects(&root))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn inspect_managed_project(
    path: String,
    expected_url: String,
) -> Result<ManagedProjectStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git::managed_project_status(&path, &expected_url)
    })
    .await
    .map_err(|error| error.to_string())
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
    settings.max_concurrent_git_operations = settings.max_concurrent_git_operations.clamp(1, 8);
    for repo in &mut settings.repositories { repo.interval_minutes = repo.interval_minutes.clamp(1, 10080); }
    let queue_snapshot = app.state::<OperationQueue>().snapshot();
    {
        let current = store.settings.lock().unwrap();
        if current.repositories.iter().any(|repository| {
            let scheduled = queue_snapshot.active.contains(&repository.id) || queue_snapshot.waiting.iter().any(|(id, _)| id == &repository.id);
            scheduled && !settings.repositories.iter().any(|incoming| incoming.id == repository.id)
        }) {
            return Err("不能移除正在执行或等待中的项目".into());
        }
    }
    let autostart = app.autolaunch();
    if settings.start_with_windows { autostart.enable().map_err(|e| e.to_string())?; } else { autostart.disable().map_err(|e| e.to_string())?; }
    {
        let current = store.settings.lock().unwrap();
        for repository in &mut settings.repositories {
            if let Some(existing) = current.repositories.iter().find(|item| item.id == repository.id) {
                repository.last_attempt = existing.last_attempt;
                repository.last_success = existing.last_success;
                repository.last_status = existing.last_status.clone();
                repository.is_running = existing.is_running;
                repository.is_queued = existing.is_queued;
                repository.queue_position = existing.queue_position;
            }
        }
    }
    app.state::<OperationQueue>().set_limit(settings.max_concurrent_git_operations as usize);
    *store.settings.lock().unwrap() = settings; store.save()?; publish(&app); Ok(())
}

#[tauri::command]
fn clear_logs(app: AppHandle, store: State<'_, Store>) { store.clear_logs(); publish(&app); }

#[tauri::command]
async fn update_repository(app: AppHandle, id: String) -> Result<(), String> { update_one(app, id).await }

struct RepositoryOperation {
    repo: Repository,
    proxy_mode: models::ProxyMode,
    proxy_address: String,
    permit: OperationPermit,
}

async fn begin_repository_operation(
    app: &AppHandle,
    id: &str,
    queued_message: &str,
    running_message: &str,
) -> Result<RepositoryOperation, String> {
    let store = app.state::<Store>();
    let repository_name = {
        let settings = store.settings.lock().unwrap();
        settings.repositories.iter().find(|repository| repository.id == id).map(|repository| repository.name.clone()).ok_or("项目不存在")?
    };
    let queue = app.state::<OperationQueue>().inner().clone();
    let (ticket, position) = queue.enqueue(id.to_string(), queued_message.to_string())?;
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(repository) = settings.repositories.iter_mut().find(|repository| repository.id == id) {
            repository.is_queued = true;
            repository.queue_position = position as u32;
            repository.last_status = format!("{queued_message} · 队列第 {position} 位");
        }
    }
    store.log(format!("[{repository_name}] 已加入 Git 任务队列"));
    publish(app);
    let waiting_queue = queue.clone();
    let permit = tauri::async_runtime::spawn_blocking(move || waiting_queue.wait(ticket)).await.map_err(|error| format!("任务队列异常结束：{error}"))??;
    let operation = {
        let mut settings = store.settings.lock().unwrap();
        let proxy_mode = settings.proxy_mode.clone();
        let proxy_address = settings.proxy_address.clone();
        let Some(repository) = settings.repositories.iter_mut().find(|repository| repository.id == id) else { return Err("项目已被删除".into()) };
        repository.is_queued = false;
        repository.queue_position = 0;
        repository.is_running = true;
        repository.last_status = running_message.into();
        RepositoryOperation { repo: repository.clone(), proxy_mode, proxy_address, permit }
    };
    publish(app);
    Ok(operation)
}

#[tauri::command]
async fn commit_and_push_project(
    app: AppHandle,
    id: String,
    message: String,
) -> Result<String, String> {
    project_write_operation(app, id, "等待提交并推送", "正在提交并推送…", move |repo, mode, address| {
        git::commit_and_push(&repo, &message, mode, &address)
    })
    .await
}

#[tauri::command]
async fn push_project(app: AppHandle, id: String) -> Result<String, String> {
    project_write_operation(app, id, "等待推送", "正在推送…", move |repo, mode, address| {
        git::push_project(&repo, mode, &address)
    })
    .await
}

#[tauri::command]
async fn initialize_repository(app: AppHandle, id: String) -> Result<String, String> {
    let store = app.state::<Store>();
    let RepositoryOperation { repo, proxy_mode, proxy_address, permit } = begin_repository_operation(&app, &id, "等待注册 Git 仓库", "正在注册 Git 仓库…").await?;
    store.log(format!("[{}] 开始注册 Git 仓库", repo.name));
    let repo_name = repo.name.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || git::initialize(&repo, proxy_mode, &proxy_address)).await {
        Ok(result) => result,
        Err(error) => GitResult { success: false, message: format!("项目任务异常结束：{error}"), details: String::new() },
    };
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(target) = settings.repositories.iter_mut().find(|repo| repo.id == id) {
            target.is_running = false;
            target.last_status = result.message.clone();
        }
    }
    store.log(format!("[{}] {}", repo_name, result.message));
    if !result.details.is_empty() { store.log(result.details); }
    let save_result = store.save();
    drop(permit);
    publish(&app);
    save_result?;
    if result.success { Ok(result.message) } else { Err(result.message) }
}

#[tauri::command]
async fn update_all(app: AppHandle) -> Result<(), String> {
    let queue = app.state::<OperationQueue>().inner().clone();
    let ids: Vec<_> = app.state::<Store>().settings.lock().unwrap().repositories.iter().filter(|repository| !queue.contains(&repository.id)).map(|repository| repository.id.clone()).collect();
    let handles: Vec<_> = ids.into_iter().map(|id| {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { update_one(app, id).await })
    }).collect();
    let mut errors = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => errors.push(error),
            Err(error) => errors.push(format!("任务异常结束：{error}")),
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(format!("{} 个项目更新失败：{}", errors.len(), errors.join("；"))) }
}

async fn update_one(app: AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Store>();
    let cloning = {
        let settings = store.settings.lock().unwrap();
        let Some(repository) = settings.repositories.iter().find(|repository| repository.id == id) else { return Err("项目不存在".into()) };
        matches!(git::inspect_path(&repository.local_path).kind, RepositoryPathKind::Missing | RepositoryPathKind::Empty)
    };
    let queued_label = if cloning { "等待克隆" } else { "等待更新" };
    let running_label = if cloning { "正在克隆…" } else { "正在更新…" };
    let RepositoryOperation { repo, proxy_mode, proxy_address, permit } = begin_repository_operation(&app, &id, queued_label, running_label).await?;
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(repository) = settings.repositories.iter_mut().find(|repository| repository.id == id) { repository.last_attempt = Some(Utc::now()); }
    }
    store.log(format!("[{}] {}", repo.name, if cloning { "开始克隆" } else { "开始检查更新" }));
    let repo_name = repo.name.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || git::update(&repo, proxy_mode, &proxy_address)).await {
        Ok(result) => result,
        Err(error) => GitResult { success: false, message: format!("项目任务异常结束：{error}"), details: String::new() },
    };
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(target) = settings.repositories.iter_mut().find(|r| r.id == id) { target.is_running = false; target.last_status = result.message.clone(); if result.success { target.last_success = Some(Utc::now()); } }
    }
    store.log(format!("[{}] {}", repo_name, result.message)); if !result.details.is_empty() { store.log(result.details); }
    let save_result = store.save();
    drop(permit);
    publish(&app);
    save_result?;
    if result.success { Ok(()) } else { Err(result.message) }
}

async fn project_write_operation<F>(
    app: AppHandle,
    id: String,
    queued_message: &str,
    running_message: &str,
    operation: F,
) -> Result<String, String>
where
    F: FnOnce(Repository, models::ProxyMode, String) -> Result<String, String> + Send + 'static,
{
    let store = app.state::<Store>();
    let RepositoryOperation { repo, proxy_mode, proxy_address, permit } = begin_repository_operation(&app, &id, queued_message, running_message).await?;
    let repo_name = repo.name.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        operation(repo, proxy_mode, proxy_address)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("项目任务异常结束：{error}")),
    };
    {
        let mut settings = store.settings.lock().unwrap();
        if let Some(repo) = settings.repositories.iter_mut().find(|repo| repo.id == id) {
            repo.is_running = false;
            repo.last_status = result
                .as_ref()
                .map_or_else(|error| error.clone(), |message| message.clone());
            if result.is_ok() {
                repo.last_success = Some(Utc::now());
            }
        }
    }
    if let Ok(message) = &result {
        store.log(format!("[{repo_name}] {message}"));
    } else {
        store.log(format!("[{repo_name}] 项目写入操作失败"));
    }
    let save_result = store.save();
    drop(permit);
    publish(&app);
    save_result?;
    result
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
            let queue = app.state::<OperationQueue>().inner().clone();
            let ids: Vec<_> = app.state::<Store>().settings.lock().unwrap().repositories.iter().filter(|repository| repository.auto_pull && !queue.contains(&repository.id) && repository.last_attempt.map(|last| now.signed_duration_since(last).num_minutes() >= repository.interval_minutes as i64).unwrap_or(true)).map(|repository| repository.id.clone()).collect();
            for id in ids {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { let _ = update_one(app, id).await; });
            }
        }
    });
}

async fn tokio_sleep(duration: Duration) { tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await.ok(); }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = Store::load();
    let queue = OperationQueue::new(store.settings.lock().unwrap().max_concurrent_git_operations as usize);
    tauri::Builder::default()
        .manage(store)
        .manage(queue)
        .plugin(tauri_plugin_single_instance::init(|app, _, _| { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| { setup_tray(app)?; if std::env::args().any(|a| a == "--minimized") && let Some(window) = app.get_webview_window("main") { let _ = window.hide(); } start_scheduler(app.handle().clone()); Ok(()) })
        .on_window_event(|window, event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event && window.app_handle().state::<Store>().settings.lock().unwrap().close_behavior == models::CloseBehavior::Background { api.prevent_close(); let _ = window.hide(); } })
        .invoke_handler(tauri::generate_handler![get_state, save_repository, delete_repository, detect_branch, inspect_repository_path, get_git_auth_status, login_github, logout_github, list_github_projects, discover_local_projects, inspect_managed_project, choose_folder, open_folder, save_settings, clear_logs, update_repository, commit_and_push_project, push_project, initialize_repository, update_all, check_app_update, install_app_update, exit_app])
        .run(tauri::generate_context!())
        .expect("error while running Git Auto Pull");
}
