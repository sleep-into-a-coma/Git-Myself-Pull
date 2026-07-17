use crate::models::{
    GitAccountProfile, GitAuthStatus, GitHubProject, GitResult, LocalGitProject,
    ManagedProjectStatus, ProxyMode, Repository, RepositoryPathKind, RepositoryPathStatus,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const MAX_PROFILE_BYTES: u64 = 128 * 1024;
const MAX_AVATAR_BYTES: u64 = 1536 * 1024;
const MAX_REPOSITORIES_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REPOSITORY_PAGES: u32 = 50;

#[derive(Deserialize)]
struct GitHubPublicProfile {
    login: String,
    name: Option<String>,
    bio: Option<String>,
    company: Option<String>,
    location: Option<String>,
    #[serde(default)]
    public_repos: u32,
    #[serde(default)]
    followers: u32,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRepositoryResponse {
    id: u64,
    name: String,
    full_name: String,
    description: Option<String>,
    clone_url: String,
    default_branch: String,
    private: bool,
    fork: bool,
    archived: bool,
    language: Option<String>,
    #[serde(default)]
    stargazers_count: u32,
    pushed_at: Option<DateTime<Utc>>,
    permissions: Option<GitHubRepositoryPermissions>,
}

#[derive(Deserialize)]
struct GitHubRepositoryPermissions {
    #[serde(default)]
    push: bool,
}

struct GitHubCredential {
    username: String,
    token: Zeroizing<String>,
}

pub fn detect_branch(path: &str) -> String {
    run(
        Some(Path::new(path)),
        &["branch", "--show-current"],
        ProxyMode::System,
        "",
    )
    .map(|value| value.trim().to_string())
    .unwrap_or_default()
}

pub fn inspect_path(value: &str) -> RepositoryPathStatus {
    let path = Path::new(value.trim());
    if value.trim().is_empty() {
        return path_status(RepositoryPathKind::Invalid, "请先选择本地目录");
    }
    if !path.exists() {
        return path_status(
            RepositoryPathKind::Missing,
            "目录尚不存在，首次同步时将执行克隆",
        );
    }
    if !path.is_dir() {
        return path_status(RepositoryPathKind::Invalid, "目标路径不是文件夹");
    }
    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            return path_status(
                RepositoryPathKind::Invalid,
                format!("无法读取目标目录：{error}"),
            )
        }
    };
    if entries.next().is_none() {
        return path_status(
            RepositoryPathKind::Empty,
            "目录为空，首次同步时将执行克隆",
        );
    }
    if let Ok(top) = run(
        Some(path),
        &["rev-parse", "--show-toplevel"],
        ProxyMode::System,
        "",
    ) {
        if same_path(Path::new(top.trim()), path) {
            return path_status(RepositoryPathKind::Git, "已识别为 Git 仓库");
        }
        return path_status(
            RepositoryPathKind::NestedGit,
            format!("目录位于其他 Git 仓库内：{}", top.trim()),
        );
    }
    path_status(
        RepositoryPathKind::NonGit,
        "目录包含现有文件，可注册为 Git 仓库",
    )
}

pub fn auth_status(proxy_mode: ProxyMode, proxy_address: &str) -> GitAuthStatus {
    let git_version = run(None, &["--version"], ProxyMode::System, "").unwrap_or_default();
    let credential_manager_version = run(
        None,
        &["credential-manager", "--version"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let credential_helper = run(
        None,
        &["config", "--show-origin", "--get-all", "credential.helper"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let account_names = if credential_manager_version.is_empty() {
        Vec::new()
    } else {
        github_accounts().unwrap_or_default()
    };
    let accounts = match profile_client(&proxy_mode, proxy_address) {
        Ok(client) => account_names
            .into_iter()
            .map(|account| github_profile(&client, &account))
            .collect(),
        Err(_) => account_names
            .into_iter()
            .map(unavailable_profile)
            .collect(),
    };
    GitAuthStatus {
        git_available: !git_version.is_empty(),
        git_version,
        credential_manager_available: !credential_manager_version.is_empty(),
        credential_manager_version,
        credential_helper,
        accounts,
    }
}

pub fn github_login(proxy_mode: ProxyMode, proxy_address: &str) -> Result<String, String> {
    ensure_git().map_err(|(message, details)| join_error(message, details))?;
    run(
        None,
        &["credential-manager", "--version"],
        ProxyMode::System,
        "",
    )
    .map_err(|_| "未找到 Git Credential Manager，请重新安装 Git for Windows".to_string())?;
    if run(
        None,
        &["config", "--get-all", "credential.helper"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default()
    .trim()
    .is_empty()
    {
        run(
            None,
            &["credential-manager", "configure"],
            ProxyMode::System,
            "",
        )
        .map_err(|error| format!("配置 Git Credential Manager 失败：{error}"))?;
    }
    run(
        None,
        &["credential-manager", "github", "login", "--browser"],
        proxy_mode,
        proxy_address,
    )
    .map_err(|error| format!("GitHub 登录失败：{error}"))?;
    let accounts = github_accounts().unwrap_or_default();
    if accounts.is_empty() {
        Err("登录流程已结束，但没有检测到 GitHub 账户".into())
    } else {
        Ok(format!("GitHub 登录完成：{}", accounts.join("、")))
    }
}

pub fn github_logout(account: &str) -> Result<String, String> {
    let account = account.trim();
    if account.is_empty() {
        return Err("账户名不能为空".into());
    }
    let accounts = github_accounts().map_err(|error| format!("无法读取 GitHub 账户：{error}"))?;
    let Some(stored) = accounts
        .iter()
        .find(|stored| stored.eq_ignore_ascii_case(account))
    else {
        return Err("该 GitHub 账户不在凭据管理器中".into());
    };
    run(
        None,
        &["credential-manager", "github", "logout", stored],
        ProxyMode::System,
        "",
    )
    .map_err(|error| format!("退出 GitHub 失败：{error}"))?;
    Ok(format!("已退出 GitHub 账户 {stored}"))
}

pub fn github_projects(
    account: &str,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<Vec<GitHubProject>, String> {
    if !valid_github_login(account.trim()) {
        return Err("GitHub 账户名无效".into());
    }
    let credential = github_credential(account.trim(), &proxy_mode, proxy_address)?;
    if !credential.username.eq_ignore_ascii_case(account.trim()) {
        return Err("未找到指定 GitHub 账户的安全凭据".into());
    }
    let client = profile_client(&proxy_mode, proxy_address)?;
    let mut projects = Vec::new();
    for page in 1..=MAX_REPOSITORY_PAGES {
        let url = format!(
            "https://api.github.com/user/repos?affiliation=owner&visibility=all&sort=pushed&direction=desc&per_page=100&page={page}"
        );
        let mut response = client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(credential.token.as_str())
            .send()
            .map_err(|_| "无法连接 GitHub，请检查网络或代理设置".to_string())?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 => "GitHub 登录凭据已失效，请重新登录".into(),
                403 => "GitHub 拒绝了仓库访问，请检查授权范围或稍后重试".into(),
                _ => format!("GitHub 仓库请求失败（HTTP {}）", response.status().as_u16()),
            });
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_REPOSITORIES_BYTES)
        {
            return Err("GitHub 仓库响应超过安全大小限制".into());
        }
        let bytes = Zeroizing::new(read_limited(&mut response, MAX_REPOSITORIES_BYTES)?);
        let repositories: Vec<GitHubRepositoryResponse> = serde_json::from_slice(&bytes)
            .map_err(|_| "GitHub 仓库数据格式无效".to_string())?;
        let count = repositories.len();
        projects.extend(repositories.into_iter().map(|repository| GitHubProject {
            id: repository.id,
            name: repository.name,
            full_name: repository.full_name,
            description: clean_profile_text(repository.description, 240),
            remote_key: normalize_remote(&repository.clone_url),
            clone_url: repository.clone_url,
            default_branch: repository.default_branch,
            private: repository.private,
            fork: repository.fork,
            archived: repository.archived,
            language: clean_profile_text(repository.language, 60),
            stars: repository.stargazers_count,
            can_push: repository
                .permissions
                .map(|permissions| permissions.push)
                .unwrap_or(true)
                && !repository.archived,
            pushed_at: repository.pushed_at,
        }));
        if count < 100 {
            break;
        }
    }
    Ok(projects)
}

pub fn discover_local_projects(root: &str) -> Result<Vec<LocalGitProject>, String> {
    let root = Path::new(root.trim());
    if root.as_os_str().is_empty() || !root.is_dir() {
        return Err("请选择存在的项目根目录".into());
    }
    let mut projects = Vec::new();
    let mut visited = 0usize;
    scan_project_directories(root, 0, &mut visited, &mut projects);
    projects.sort_by_key(|project| project.path.to_lowercase());
    Ok(projects)
}

pub fn managed_project_status(path: &str, expected_url: &str) -> ManagedProjectStatus {
    let path_status = inspect_path(path);
    if path_status.kind != RepositoryPathKind::Git {
        return empty_managed_status(path_status.kind, path_status.message);
    }
    let path = Path::new(path.trim());
    let origin_url = run(
        Some(path),
        &["remote", "get-url", "origin"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let remote_matches = !origin_url.trim().is_empty()
        && normalize_remote(origin_url.trim()) == normalize_remote(expected_url.trim());
    let branch = run(
        Some(path),
        &["branch", "--show-current"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let porcelain = run(
        Some(path),
        &["status", "--porcelain=v1"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let (changes, staged, unstaged, untracked) = count_worktree_changes(&porcelain);
    let (behind, ahead) = upstream_counts(path);
    let message = if !remote_matches {
        "origin 与所选 GitHub 项目不一致".into()
    } else if changes > 0 {
        format!("{changes} 项本地改动待处理")
    } else if ahead > 0 {
        format!("有 {ahead} 个本地提交待推送")
    } else if behind > 0 {
        format!("远程领先 {behind} 个提交")
    } else {
        "本地工作区干净".into()
    };
    ManagedProjectStatus {
        kind: RepositoryPathKind::Git,
        message,
        branch: branch.trim().to_string(),
        origin_url: origin_url.trim().to_string(),
        remote_matches,
        changes,
        staged,
        unstaged,
        untracked,
        ahead,
        behind,
    }
}

pub fn commit_and_push(
    repo: &Repository,
    message: &str,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("请填写提交说明".into());
    }
    let message: String = message.chars().take(500).collect();
    let (path, branch) = validate_managed_repository(repo)?;
    let status = run(
        Some(path),
        &["status", "--porcelain=v1"],
        ProxyMode::System,
        "",
    )?;
    if status.trim().is_empty() {
        return Err("没有可提交的本地改动".into());
    }
    ensure_commit_identity(path)?;
    run(Some(path), &["add", "--all"], ProxyMode::System, "")?;
    let staged = run(
        Some(path),
        &["diff", "--cached", "--name-only"],
        ProxyMode::System,
        "",
    )?;
    let files = staged.lines().filter(|line| !line.trim().is_empty()).count();
    if files == 0 {
        return Err("没有可提交的改动，可能全部文件均被忽略".into());
    }
    run(
        Some(path),
        &["commit", "-m", &message],
        ProxyMode::System,
        "",
    )?;
    if let Err(error) = run(
        Some(path),
        &["push", "--set-upstream", "origin", &branch],
        proxy_mode,
        proxy_address,
    ) {
        return Err(format!("本地提交已创建，但推送失败：{error}"));
    }
    Ok(format!("已提交并推送 {files} 个文件"))
}

pub fn push_project(
    repo: &Repository,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<String, String> {
    let (path, branch) = validate_managed_repository(repo)?;
    run(
        Some(path),
        &["push", "--set-upstream", "origin", &branch],
        proxy_mode,
        proxy_address,
    )?;
    Ok(format!("已推送分支 {branch}"))
}

pub fn update(repo: &Repository, proxy_mode: ProxyMode, proxy_address: &str) -> GitResult {
    into_result(update_inner(repo, proxy_mode, proxy_address))
}

pub fn initialize(
    repo: &Repository,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> GitResult {
    into_result(initialize_inner(repo, proxy_mode, proxy_address))
}

fn update_inner(
    repo: &Repository,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<(String, String), (String, String)> {
    ensure_git()?;
    let path = Path::new(&repo.local_path);
    match inspect_path(&repo.local_path).kind {
        RepositoryPathKind::Missing | RepositoryPathKind::Empty => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| ("无法创建本地目录".into(), error.to_string()))?;
            }
            let mut owned = vec!["clone".to_string(), "--progress".into()];
            if !repo.branch.trim().is_empty() {
                owned.extend([
                    "--branch".into(),
                    repo.branch.trim().into(),
                    "--single-branch".into(),
                ]);
            }
            owned.extend([repo.url.clone(), repo.local_path.clone()]);
            let arguments: Vec<_> = owned.iter().map(String::as_str).collect();
            let output = run(None, &arguments, proxy_mode, proxy_address)
                .map_err(|error| ("克隆失败".into(), error))?;
            return Ok(("克隆完成".into(), output));
        }
        RepositoryPathKind::Git => {}
        RepositoryPathKind::NonGit => {
            return Err((
                "目标目录不是 Git 仓库，请先注册为 Git 仓库".into(),
                String::new(),
            ))
        }
        RepositoryPathKind::NestedGit => {
            return Err((
                "目标目录位于另一个 Git 仓库中，不能作为独立项目更新".into(),
                String::new(),
            ))
        }
        RepositoryPathKind::Invalid => {
            return Err(("目标目录无效".into(), String::new()))
        }
    }

    let remote = run(
        Some(path),
        &["remote", "get-url", "origin"],
        ProxyMode::System,
        "",
    )
    .map_err(|error| ("仓库没有 origin 远程地址".into(), error))?;
    if normalize_remote(remote.trim()) != normalize_remote(repo.url.trim()) {
        return Err((
            "本地 origin 与注册地址不一致".into(),
            format!("origin: {}\nregistered: {}", remote.trim(), repo.url),
        ));
    }
    let status = run(
        Some(path),
        &["status", "--porcelain"],
        ProxyMode::System,
        "",
    )
    .map_err(|error| ("无法检查工作区状态".into(), error))?;
    if !status.trim().is_empty() {
        return Err(("检测到未提交的本地改动，已跳过".into(), status));
    }
    let fetch = run(
        Some(path),
        &["fetch", "--prune", "origin"],
        proxy_mode,
        proxy_address,
    )
    .map_err(|error| ("获取远程更新失败".into(), error))?;
    let current = run(
        Some(path),
        &["branch", "--show-current"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default()
    .trim()
    .to_string();
    let branch = if repo.branch.trim().is_empty() {
        current.clone()
    } else {
        repo.branch.trim().to_string()
    };
    if branch.is_empty() {
        return Err((
            "当前处于 detached HEAD，请填写分支名".into(),
            String::new(),
        ));
    }
    if current != branch {
        return Err((
            format!("当前分支为 {current}，与设置的 {branch} 不同"),
            String::new(),
        ));
    }
    let before = run(
        Some(path),
        &["rev-parse", "HEAD"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default()
    .trim()
    .to_string();
    let target = format!("origin/{branch}");
    let merge = run(
        Some(path),
        &["merge", "--ff-only", &target],
        ProxyMode::System,
        "",
    )
    .map_err(|error| ("无法快进更新".into(), error))?;
    let after = run(
        Some(path),
        &["rev-parse", "HEAD"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default()
    .trim()
    .to_string();
    if before == after {
        Ok(("已是最新".into(), format!("{fetch}\n{merge}")))
    } else {
        Ok((
            format!("更新完成（{} → {}）", short(&before), short(&after)),
            format!("{fetch}\n{merge}"),
        ))
    }
}

fn initialize_inner(
    repo: &Repository,
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<(String, String), (String, String)> {
    ensure_git()?;
    let path = Path::new(&repo.local_path);
    match inspect_path(&repo.local_path).kind {
        RepositoryPathKind::NonGit => {}
        RepositoryPathKind::Git => {
            return Err(("目标目录已经是 Git 仓库".into(), String::new()))
        }
        RepositoryPathKind::Missing | RepositoryPathKind::Empty => {
            return Err(("空目录无需注册，请直接执行克隆".into(), String::new()))
        }
        RepositoryPathKind::NestedGit => {
            return Err((
                "目标目录位于另一个 Git 仓库中，已停止注册".into(),
                String::new(),
            ))
        }
        RepositoryPathKind::Invalid => {
            return Err(("目标目录无效".into(), String::new()))
        }
    }
    let git_dir = path.join(".git");
    if git_dir.exists() {
        return Err((
            "目录中已存在无效的 .git，请先手动检查".into(),
            String::new(),
        ));
    }

    let initialized = run(Some(path), &["init"], ProxyMode::System, "")
        .map_err(|error| ("初始化 Git 仓库失败".into(), error))?;
    let configured = (|| {
        run(
            Some(path),
            &["remote", "add", "origin", repo.url.trim()],
            ProxyMode::System,
            "",
        )
        .map_err(|error| ("关联 origin 失败".into(), error))?;
        let advertised = run(
            Some(path),
            &["ls-remote", "--symref", "origin", "HEAD"],
            proxy_mode.clone(),
            proxy_address,
        )
        .map_err(|error| ("无法读取远程默认分支".into(), error))?;
        let branch = if repo.branch.trim().is_empty() {
            default_branch(&advertised).ok_or_else(|| {
                (
                    "无法识别远程默认分支，请先填写分支名".into(),
                    advertised.clone(),
                )
            })?
        } else {
            repo.branch.trim().to_string()
        };
        let fetch = run(
            Some(path),
            &["fetch", "--prune", "origin"],
            proxy_mode,
            proxy_address,
        )
        .map_err(|error| ("获取远程仓库失败".into(), error))?;
        let remote_ref = format!("refs/remotes/origin/{branch}");
        run(
            Some(path),
            &["show-ref", "--verify", &remote_ref],
            ProxyMode::System,
            "",
        )
        .map_err(|error| (format!("远程不存在分支 {branch}"), error))?;
        let local_ref = format!("refs/heads/{branch}");
        run(
            Some(path),
            &["symbolic-ref", "HEAD", &local_ref],
            ProxyMode::System,
            "",
        )
        .map_err(|error| ("设置本地分支失败".into(), error))?;
        let target = format!("origin/{branch}");
        let reset = run(
            Some(path),
            &["reset", "--mixed", &target],
            ProxyMode::System,
            "",
        )
        .map_err(|error| ("建立远程基线失败".into(), error))?;
        let status = run(
            Some(path),
            &["status", "--porcelain"],
            ProxyMode::System,
            "",
        )
        .map_err(|error| ("无法检查注册结果".into(), error))?;
        let changes = status.lines().filter(|line| !line.trim().is_empty()).count();
        let message = if changes == 0 {
            "Git 仓库注册完成".into()
        } else {
            format!("Git 仓库注册完成，保留了 {changes} 项本地改动")
        };
        Ok((
            message,
            [initialized, advertised, fetch, reset, status]
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    })();
    if configured.is_err() {
        let _ = fs::remove_dir_all(git_dir);
    }
    configured
}

fn ensure_git() -> Result<(), (String, String)> {
    run(None, &["--version"], ProxyMode::System, "")
        .map(|_| ())
        .map_err(|error| ("未找到 Git，请先安装 Git for Windows".into(), error))
}

fn github_accounts() -> Result<Vec<String>, String> {
    let output = run(
        None,
        &["credential-manager", "github", "list"],
        ProxyMode::System,
        "",
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("warning:"))
        .map(str::to_string)
        .collect())
}

fn github_credential(
    account: &str,
    proxy_mode: &ProxyMode,
    proxy_address: &str,
) -> Result<GitHubCredential, String> {
    let mut command = Command::new("git");
    match proxy_mode {
        ProxyMode::Disabled => {
            command.args(["-c", "http.proxy=", "-c", "https.proxy="]);
        }
        ProxyMode::Custom if !proxy_address.trim().is_empty() => {
            command.args([
                "-c",
                &format!("http.proxy={}", proxy_address.trim()),
                "-c",
                &format!("https.proxy={}", proxy_address.trim()),
            ]);
        }
        _ => {}
    }
    command
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|_| "无法启动 Git Credential Manager".to_string())?;
    let input = format!("protocol=https\nhost=github.com\nusername={account}\n\n");
    child
        .stdin
        .take()
        .ok_or_else(|| "无法连接 Git Credential Manager".to_string())?
        .write_all(input.as_bytes())
        .map_err(|_| "无法读取 GitHub 安全凭据".to_string())?;
    let mut output = child
        .wait_with_output()
        .map_err(|_| "无法读取 GitHub 安全凭据".to_string())?;
    if !output.status.success() {
        output.stdout.zeroize();
        output.stderr.zeroize();
        return Err("GitHub 凭据不可用，请重新登录".into());
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut username = String::new();
    let mut token = String::new();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once('=') {
            match name {
                "username" => username = value.to_string(),
                "password" => token = value.to_string(),
                _ => {}
            }
        }
    }
    text.zeroize();
    output.stdout.zeroize();
    output.stderr.zeroize();
    if username.is_empty() || token.is_empty() {
        token.zeroize();
        return Err("GitHub 凭据不完整，请重新登录".into());
    }
    Ok(GitHubCredential {
        username,
        token: Zeroizing::new(token),
    })
}

fn profile_client(
    proxy_mode: &ProxyMode,
    proxy_address: &str,
) -> Result<reqwest::blocking::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Git-Auto-Pull/", env!("CARGO_PKG_VERSION")));
    match proxy_mode {
        ProxyMode::Disabled => builder = builder.no_proxy(),
        ProxyMode::Custom if !proxy_address.trim().is_empty() => {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy_address.trim())
                    .map_err(|error| format!("代理地址无效：{error}"))?,
            );
        }
        ProxyMode::System => {
            if let Ok(address) = run(
                None,
                &["config", "--get", "http.proxy"],
                ProxyMode::System,
                "",
            ) && !address.trim().is_empty()
            {
                builder = builder.proxy(
                    reqwest::Proxy::all(address.trim())
                        .map_err(|error| format!("Git 代理地址无效：{error}"))?,
                );
            }
        }
        _ => {}
    }
    builder
        .build()
        .map_err(|error| format!("无法创建公开资料客户端：{error}"))
}

fn github_profile(client: &reqwest::blocking::Client, account: &str) -> GitAccountProfile {
    if !valid_github_login(account) {
        return unavailable_profile(account.to_string());
    }
    let result = (|| {
        let mut response = client
            .get(format!("https://api.github.com/users/{account}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| error.to_string())?;
        if response.content_length().is_some_and(|size| size > MAX_PROFILE_BYTES) {
            return Err("公开资料响应过大".to_string());
        }
        let bytes = read_limited(&mut response, MAX_PROFILE_BYTES)?;
        let profile: GitHubPublicProfile =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let avatar_data = profile
            .avatar_url
            .as_deref()
            .and_then(|url| download_avatar(client, url).ok());
        Ok(GitAccountProfile {
            login: profile.login,
            name: clean_profile_text(profile.name, 80),
            bio: clean_profile_text(profile.bio, 240),
            company: clean_profile_text(profile.company, 100),
            location: clean_profile_text(profile.location, 100),
            public_repos: profile.public_repos,
            followers: profile.followers,
            avatar_data,
            profile_error: None,
        })
    })();
    result.unwrap_or_else(|_| unavailable_profile(account.to_string()))
}

fn download_avatar(client: &reqwest::blocking::Client, value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "头像地址无效".to_string())?;
    if !is_allowed_avatar_url(&url) {
        return Err("头像地址不受信任".into());
    }
    let mut response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.to_string())?;
    if response.content_length().is_some_and(|size| size > MAX_AVATAR_BYTES) {
        return Err("头像文件过大".into());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| {
            matches!(
                *value,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        })
        .ok_or_else(|| "头像图片类型不受支持".to_string())?
        .to_string();
    let bytes = read_limited(&mut response, MAX_AVATAR_BYTES)?;
    Ok(format!(
        "data:{content_type};base64,{}",
        BASE64.encode(bytes)
    ))
}

fn read_limited(reader: &mut impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        Err("响应超过允许大小".into())
    } else {
        Ok(bytes)
    }
}

fn is_allowed_avatar_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("avatars.githubusercontent.com"))
}

fn valid_github_login(account: &str) -> bool {
    !account.is_empty()
        && account.len() <= 39
        && !account.starts_with('-')
        && !account.ends_with('-')
        && account
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-')
}

fn clean_profile_text(value: Option<String>, limit: usize) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.chars().take(limit).collect())
        }
    })
}

fn unavailable_profile(login: String) -> GitAccountProfile {
    GitAccountProfile {
        login,
        name: None,
        bio: None,
        company: None,
        location: None,
        public_repos: 0,
        followers: 0,
        avatar_data: None,
        profile_error: Some("公开资料暂不可用".into()),
    }
}

fn scan_project_directories(
    path: &Path,
    depth: usize,
    visited: &mut usize,
    projects: &mut Vec<LocalGitProject>,
) {
    if *visited >= 8000 || projects.len() >= 400 {
        return;
    }
    *visited += 1;
    if path.join(".git").exists() {
        if let Ok(origin_url) = run(
            Some(path),
            &["remote", "get-url", "origin"],
            ProxyMode::System,
            "",
        ) && !origin_url.trim().is_empty()
        {
            projects.push(LocalGitProject {
                name: path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                path: path.to_string_lossy().into_owned(),
                remote_key: normalize_remote(origin_url.trim()),
                origin_url: origin_url.trim().to_string(),
                branch: detect_branch(&path.to_string_lossy()),
            });
        }
        return;
    }
    if depth >= 5 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        if *visited >= 8000 || projects.len() >= 400 {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if matches!(
            name.as_str(),
            ".git"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".venv"
                | "venv"
                | "vendor"
                | ".cache"
        ) {
            continue;
        }
        scan_project_directories(&entry.path(), depth + 1, visited, projects);
    }
}

fn empty_managed_status(
    kind: RepositoryPathKind,
    message: String,
) -> ManagedProjectStatus {
    ManagedProjectStatus {
        kind,
        message,
        branch: String::new(),
        origin_url: String::new(),
        remote_matches: false,
        changes: 0,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        ahead: 0,
        behind: 0,
    }
}

fn count_worktree_changes(value: &str) -> (u32, u32, u32, u32) {
    let mut changes = 0;
    let mut staged = 0;
    let mut unstaged = 0;
    let mut untracked = 0;
    for line in value.lines().filter(|line| !line.trim().is_empty()) {
        changes += 1;
        let status = line.as_bytes();
        if status.starts_with(b"??") {
            untracked += 1;
            continue;
        }
        if status.first().is_some_and(|value| *value != b' ') {
            staged += 1;
        }
        if status.get(1).is_some_and(|value| *value != b' ') {
            unstaged += 1;
        }
    }
    (changes, staged, unstaged, untracked)
}

fn upstream_counts(path: &Path) -> (u32, u32) {
    let Ok(output) = run(
        Some(path),
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
        ProxyMode::System,
        "",
    ) else {
        return (0, 0);
    };
    let mut values = output.split_whitespace();
    let behind = values.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    let ahead = values.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    (behind, ahead)
}

fn validate_managed_repository(repo: &Repository) -> Result<(&Path, String), String> {
    let path = Path::new(repo.local_path.trim());
    if inspect_path(repo.local_path.trim()).kind != RepositoryPathKind::Git {
        return Err("本地目录不是可用的 Git 仓库".into());
    }
    let origin = run(
        Some(path),
        &["remote", "get-url", "origin"],
        ProxyMode::System,
        "",
    )?;
    if normalize_remote(origin.trim()) != normalize_remote(repo.url.trim()) {
        return Err("本地 origin 与个人项目地址不一致".into());
    }
    let branch = run(
        Some(path),
        &["branch", "--show-current"],
        ProxyMode::System,
        "",
    )?
    .trim()
    .to_string();
    if branch.is_empty() {
        return Err("当前处于 detached HEAD，不能提交或推送".into());
    }
    if !repo.branch.trim().is_empty() && repo.branch.trim() != branch {
        return Err(format!(
            "当前分支为 {branch}，与项目设置的 {} 不一致",
            repo.branch.trim()
        ));
    }
    Ok((path, branch))
}

fn ensure_commit_identity(path: &Path) -> Result<(), String> {
    let name = run(
        Some(path),
        &["config", "user.name"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    let email = run(
        Some(path),
        &["config", "user.email"],
        ProxyMode::System,
        "",
    )
    .unwrap_or_default();
    if name.trim().is_empty() || email.trim().is_empty() {
        Err("尚未配置 Git 提交身份，请先设置 user.name 与 user.email".into())
    } else {
        Ok(())
    }
}

fn join_error(message: String, details: String) -> String {
    if details.trim().is_empty() {
        message
    } else {
        format!("{message}：{details}")
    }
}

fn into_result(result: Result<(String, String), (String, String)>) -> GitResult {
    match result {
        Ok((message, details)) => GitResult {
            success: true,
            message,
            details,
        },
        Err((message, details)) => GitResult {
            success: false,
            message,
            details,
        },
    }
}

fn path_status(kind: RepositoryPathKind, message: impl Into<String>) -> RepositoryPathStatus {
    RepositoryPathStatus {
        kind,
        message: message.into(),
    }
}

fn default_branch(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if parts.next()? != "ref:" {
            return None;
        }
        let reference = parts.next()?;
        if parts.next()? != "HEAD" {
            return None;
        }
        reference
            .strip_prefix("refs/heads/")
            .map(str::to_string)
    })
}

fn same_path(first: &Path, second: &Path) -> bool {
    let first = fs::canonicalize(first).unwrap_or_else(|_| first.to_path_buf());
    let second = fs::canonicalize(second).unwrap_or_else(|_| second.to_path_buf());
    if cfg!(windows) {
        first
            .to_string_lossy()
            .eq_ignore_ascii_case(&second.to_string_lossy())
    } else {
        first == second
    }
}

fn run(
    cwd: Option<&Path>,
    args: &[&str],
    proxy_mode: ProxyMode,
    proxy_address: &str,
) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(path) = cwd {
        command.current_dir(path);
    }
    match proxy_mode {
        ProxyMode::Disabled => {
            command.args(["-c", "http.proxy=", "-c", "https.proxy="]);
        }
        ProxyMode::Custom if !proxy_address.trim().is_empty() => {
            command.args([
                "-c",
                &format!("http.proxy={}", proxy_address.trim()),
                "-c",
                &format!("https.proxy={}", proxy_address.trim()),
            ]);
        }
        _ => {}
    }
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err([stdout, stderr]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

fn normalize_remote(value: &str) -> String {
    let value = value
        .trim()
        .trim_end_matches(['/', '\\'])
        .trim_end_matches(".git");
    let value = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .or_else(|| value.strip_prefix("ssh://"))
        .or_else(|| value.strip_prefix("git@"))
        .unwrap_or(value);
    value.replace([':', '\\'], "/").to_lowercase()
}

fn short(value: &str) -> &str {
    value.get(..7).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::{
        commit_and_push, default_branch, discover_local_projects, initialize, inspect_path,
        is_allowed_avatar_url, managed_project_status, normalize_remote, run, update,
        valid_github_login, ProxyMode, Repository, RepositoryPathKind,
    };
    use std::{fs, path::Path};

    #[test]
    fn parses_default_branch() {
        let output = "ref: refs/heads/main\tHEAD\nabc123\tHEAD";
        assert_eq!(default_branch(output).as_deref(), Some("main"));
    }

    #[test]
    fn normalizes_common_remote_formats() {
        assert_eq!(
            normalize_remote("git@github.com:user/project.git"),
            normalize_remote("https://github.com/user/project/")
        );
    }

    #[test]
    fn validates_github_accounts_and_avatar_hosts() {
        assert!(valid_github_login("sleep-into-a-coma"));
        assert!(!valid_github_login("-invalid"));
        assert!(!valid_github_login("invalid/name"));
        assert!(is_allowed_avatar_url(
            &url::Url::parse("https://avatars.githubusercontent.com/u/1?v=4").unwrap()
        ));
        assert!(!is_allowed_avatar_url(
            &url::Url::parse("https://example.com/avatar.png").unwrap()
        ));
        assert!(!is_allowed_avatar_url(
            &url::Url::parse("http://avatars.githubusercontent.com/u/1").unwrap()
        ));
    }

    #[test]
    fn classifies_clone_and_registration_targets() {
        let root = std::env::temp_dir().join(format!("git-auto-pull-paths-{}", uuid::Uuid::new_v4()));
        let missing = root.join("missing");
        let empty = root.join("empty");
        let existing = root.join("existing");
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("file.txt"), "content").unwrap();
        assert_eq!(inspect_path(missing.to_str().unwrap()).kind, RepositoryPathKind::Missing);
        assert_eq!(inspect_path(empty.to_str().unwrap()).kind, RepositoryPathKind::Empty);
        assert_eq!(inspect_path(existing.to_str().unwrap()).kind, RepositoryPathKind::NonGit);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registers_existing_directory_without_overwriting_files() {
        let root = std::env::temp_dir().join(format!("git-auto-pull-init-{}", uuid::Uuid::new_v4()));
        let seed = root.join("seed");
        let remote = root.join("remote.git");
        let existing = root.join("existing");
        fs::create_dir_all(&seed).unwrap();
        fs::create_dir_all(&existing).unwrap();
        run(Some(&seed), &["init", "--initial-branch=main"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["config", "user.name", "Git Auto Pull Test"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["config", "user.email", "test@example.invalid"], ProxyMode::System, "").unwrap();
        fs::write(seed.join("file.txt"), "remote content").unwrap();
        run(Some(&seed), &["add", "file.txt"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["commit", "-m", "seed"], ProxyMode::System, "").unwrap();
        run(
            None,
            &["clone", "--bare", seed.to_str().unwrap(), remote.to_str().unwrap()],
            ProxyMode::System,
            "",
        )
        .unwrap();
        fs::write(existing.join("file.txt"), "local content").unwrap();
        let repository = Repository {
            url: remote.to_string_lossy().into_owned(),
            local_path: existing.to_string_lossy().into_owned(),
            branch: "main".into(),
            ..Repository::default()
        };
        let result = initialize(&repository, ProxyMode::System, "");
        assert!(result.success, "{}\n{}", result.message, result.details);
        assert_eq!(inspect_path(existing.to_str().unwrap()).kind, RepositoryPathKind::Git);
        assert_eq!(fs::read_to_string(existing.join("file.txt")).unwrap(), "local content");
        let status = run(Some(Path::new(&repository.local_path)), &["status", "--porcelain"], ProxyMode::System, "").unwrap();
        assert!(!status.trim().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_first_sync_as_clone() {
        let root = std::env::temp_dir().join(format!("git-auto-pull-clone-{}", uuid::Uuid::new_v4()));
        let remote = root.join("remote.git");
        let local = root.join("local");
        fs::create_dir_all(&root).unwrap();
        run(
            Some(&root),
            &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()],
            ProxyMode::System,
            "",
        )
        .unwrap();
        let repository = Repository {
            url: remote.to_string_lossy().into_owned(),
            local_path: local.to_string_lossy().into_owned(),
            ..Repository::default()
        };
        let result = update(&repository, ProxyMode::System, "");
        assert!(result.success, "{}\n{}", result.message, result.details);
        assert_eq!(result.message, "克隆完成");
        assert_eq!(inspect_path(local.to_str().unwrap()).kind, RepositoryPathKind::Git);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_status_and_pushes_managed_projects() {
        let root = std::env::temp_dir().join(format!("git-auto-pull-manage-{}", uuid::Uuid::new_v4()));
        let seed = root.join("seed");
        let remote = root.join("remote.git");
        let local = root.join("workspace").join("project");
        fs::create_dir_all(&seed).unwrap();
        run(Some(&seed), &["init", "--initial-branch=main"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["config", "user.name", "Git Auto Pull Test"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["config", "user.email", "test@example.invalid"], ProxyMode::System, "").unwrap();
        fs::write(seed.join("README.md"), "seed").unwrap();
        run(Some(&seed), &["add", "README.md"], ProxyMode::System, "").unwrap();
        run(Some(&seed), &["commit", "-m", "seed"], ProxyMode::System, "").unwrap();
        run(None, &["clone", "--bare", seed.to_str().unwrap(), remote.to_str().unwrap()], ProxyMode::System, "").unwrap();
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        run(None, &["clone", remote.to_str().unwrap(), local.to_str().unwrap()], ProxyMode::System, "").unwrap();
        run(Some(&local), &["config", "user.name", "Git Auto Pull Test"], ProxyMode::System, "").unwrap();
        run(Some(&local), &["config", "user.email", "test@example.invalid"], ProxyMode::System, "").unwrap();
        fs::write(local.join("README.md"), "changed").unwrap();

        let repository = Repository {
            name: "project".into(),
            url: remote.to_string_lossy().into_owned(),
            local_path: local.to_string_lossy().into_owned(),
            branch: "main".into(),
            ..Repository::default()
        };
        let status = managed_project_status(&repository.local_path, &repository.url);
        assert!(status.remote_matches);
        assert_eq!(status.changes, 1);
        let discovered = discover_local_projects(root.to_str().unwrap()).unwrap();
        assert!(discovered.iter().any(|project| project.path == repository.local_path));

        let message = commit_and_push(&repository, "managed update", ProxyMode::System, "").unwrap();
        assert!(message.contains("1 个文件"));
        let status = managed_project_status(&repository.local_path, &repository.url);
        assert_eq!(status.changes, 0);
        assert_eq!(status.ahead, 0);
        let local_head = run(Some(&local), &["rev-parse", "HEAD"], ProxyMode::System, "").unwrap();
        let remote_head = run(Some(&remote), &["rev-parse", "refs/heads/main"], ProxyMode::System, "").unwrap();
        assert_eq!(local_head, remote_head);
        fs::remove_dir_all(root).unwrap();
    }
}
