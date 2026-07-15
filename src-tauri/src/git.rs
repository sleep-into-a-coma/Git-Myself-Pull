use crate::models::{GitResult, ProxyMode, Repository};
use std::{fs, path::Path, process::Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn detect_branch(path: &str) -> String { run(Some(Path::new(path)), &["branch", "--show-current"], ProxyMode::System, "").map(|s| s.trim().to_string()).unwrap_or_default() }

pub fn update(repo: &Repository, proxy_mode: ProxyMode, proxy_address: &str) -> GitResult {
    match update_inner(repo, proxy_mode, proxy_address) {
        Ok((message, details)) => GitResult { success: true, message, details },
        Err((message, details)) => GitResult { success: false, message, details },
    }
}

fn update_inner(repo: &Repository, proxy_mode: ProxyMode, proxy_address: &str) -> Result<(String, String), (String, String)> {
    run(None, &["--version"], ProxyMode::System, "").map_err(|e| ("未找到 Git，请先安装 Git for Windows".into(), e))?;
    let path = Path::new(&repo.local_path);
    let empty = !path.exists() || fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(true);
    if empty {
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| ("无法创建本地目录".into(), e.to_string()))?; }
        let mut owned = vec!["clone".to_string(), "--progress".into()];
        if !repo.branch.trim().is_empty() { owned.extend(["--branch".into(), repo.branch.trim().into(), "--single-branch".into()]); }
        owned.extend([repo.url.clone(), repo.local_path.clone()]);
        let refs: Vec<_> = owned.iter().map(String::as_str).collect();
        let output = run(None, &refs, proxy_mode, proxy_address).map_err(|e| ("克隆失败".into(), e))?;
        return Ok(("克隆成功".into(), output));
    }

    let inside = run(Some(path), &["rev-parse", "--is-inside-work-tree"], ProxyMode::System, "").map_err(|e| ("目标目录不是 Git 仓库".into(), e))?;
    if !inside.contains("true") { return Err(("目标目录不是 Git 仓库".into(), inside)); }
    let remote = run(Some(path), &["remote", "get-url", "origin"], ProxyMode::System, "").map_err(|e| ("仓库没有 origin 远程地址".into(), e))?;
    if normalize_remote(remote.trim()) != normalize_remote(repo.url.trim()) { return Err(("本地 origin 与注册地址不一致".into(), format!("origin: {}\nregistered: {}", remote.trim(), repo.url))); }
    let status = run(Some(path), &["status", "--porcelain"], ProxyMode::System, "").map_err(|e| ("无法检查工作区状态".into(), e))?;
    if !status.trim().is_empty() { return Err(("检测到未提交的本地改动，已跳过".into(), status)); }
    let fetch = run(Some(path), &["fetch", "--prune", "origin"], proxy_mode.clone(), proxy_address).map_err(|e| ("获取远程更新失败".into(), e))?;
    let current = run(Some(path), &["branch", "--show-current"], ProxyMode::System, "").unwrap_or_default().trim().to_string();
    let branch = if repo.branch.trim().is_empty() { current.clone() } else { repo.branch.trim().to_string() };
    if branch.is_empty() { return Err(("当前处于 detached HEAD，请填写分支名".into(), String::new())); }
    if current != branch { return Err((format!("当前分支为 {current}，与设置的 {branch} 不同"), String::new())); }
    let before = run(Some(path), &["rev-parse", "HEAD"], ProxyMode::System, "").unwrap_or_default().trim().to_string();
    let target = format!("origin/{branch}");
    let merge = run(Some(path), &["merge", "--ff-only", &target], ProxyMode::System, "").map_err(|e| ("无法快进更新".into(), e))?;
    let after = run(Some(path), &["rev-parse", "HEAD"], ProxyMode::System, "").unwrap_or_default().trim().to_string();
    if before == after { Ok(("已是最新".into(), format!("{fetch}\n{merge}"))) } else { Ok((format!("更新成功（{} → {}）", short(&before), short(&after)), format!("{fetch}\n{merge}"))) }
}

fn run(cwd: Option<&Path>, args: &[&str], proxy_mode: ProxyMode, proxy_address: &str) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(path) = cwd { command.current_dir(path); }
    match proxy_mode {
        ProxyMode::Disabled => { command.args(["-c", "http.proxy=", "-c", "https.proxy="]); }
        ProxyMode::Custom if !proxy_address.trim().is_empty() => { command.args(["-c", &format!("http.proxy={}", proxy_address.trim()), "-c", &format!("https.proxy={}", proxy_address.trim())]); }
        _ => {}
    }
    command.args(args);
    #[cfg(windows)] command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).trim().to_string();
    if output.status.success() { Ok(text) } else { Err(text) }
}

fn normalize_remote(value: &str) -> String { value.trim().trim_end_matches(['/', '\\']).trim_end_matches(".git").replace([':', '\\'], "/").replace("git@", "").replace("https://", "").replace("http://", "").replace("ssh://", "").to_lowercase() }
fn short(value: &str) -> &str { value.get(..7).unwrap_or(value) }
