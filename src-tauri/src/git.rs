use crate::models::{
    GitAuthStatus, GitResult, ProxyMode, Repository, RepositoryPathKind, RepositoryPathStatus,
};
use std::{fs, path::Path, process::Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

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

pub fn auth_status() -> GitAuthStatus {
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
    let accounts = if credential_manager_version.is_empty() {
        Vec::new()
    } else {
        github_accounts().unwrap_or_default()
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if output.status.success() {
        Ok(text)
    } else {
        Err(text)
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
        default_branch, initialize, inspect_path, normalize_remote, run, update, ProxyMode, Repository,
        RepositoryPathKind,
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
        let mut repository = Repository::default();
        repository.url = remote.to_string_lossy().into_owned();
        repository.local_path = existing.to_string_lossy().into_owned();
        repository.branch = "main".into();
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
        let mut repository = Repository::default();
        repository.url = remote.to_string_lossy().into_owned();
        repository.local_path = local.to_string_lossy().into_owned();
        let result = update(&repository, ProxyMode::System, "");
        assert!(result.success, "{}\n{}", result.message, result.details);
        assert_eq!(result.message, "克隆完成");
        assert_eq!(inspect_path(local.to_str().unwrap()).kind, RepositoryPathKind::Git);
        fs::remove_dir_all(root).unwrap();
    }
}
