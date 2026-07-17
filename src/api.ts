import { invoke } from '@tauri-apps/api/core'
import type { AppState, GitAuthStatus, GitHubProject, LocalGitProject, ManagedProjectStatus, Repository, RepositoryPathStatus, Settings, UpdateStatus } from './types'

export const api = {
  state: () => invoke<AppState>('get_state'),
  saveRepository: (repository: Repository) => invoke<Repository>('save_repository', { repository }),
  deleteRepository: (id: string) => invoke<void>('delete_repository', { id }),
  updateRepository: (id: string) => invoke<void>('update_repository', { id }),
  initializeRepository: (id: string) => invoke<string>('initialize_repository', { id }),
  updateAll: () => invoke<void>('update_all'),
  detectBranch: (path: string) => invoke<string>('detect_branch', { path }),
  inspectRepositoryPath: (path: string) => invoke<RepositoryPathStatus>('inspect_repository_path', { path }),
  gitAuthStatus: () => invoke<GitAuthStatus>('get_git_auth_status'),
  loginGitHub: () => invoke<string>('login_github'),
  logoutGitHub: (account: string) => invoke<string>('logout_github', { account }),
  githubProjects: (account: string) => invoke<GitHubProject[]>('list_github_projects', { account }),
  discoverLocalProjects: (root: string) => invoke<LocalGitProject[]>('discover_local_projects', { root }),
  inspectManagedProject: (path: string, expectedUrl: string) => invoke<ManagedProjectStatus>('inspect_managed_project', { path, expectedUrl }),
  commitAndPushProject: (id: string, message: string) => invoke<string>('commit_and_push_project', { id, message }),
  pushProject: (id: string) => invoke<string>('push_project', { id }),
  chooseFolder: () => invoke<string | null>('choose_folder'),
  openFolder: (path: string) => invoke<void>('open_folder', { path }),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  clearLogs: () => invoke<void>('clear_logs'),
  checkUpdate: () => invoke<UpdateStatus>('check_app_update'),
  installUpdate: () => invoke<void>('install_app_update'),
  exit: () => invoke<void>('exit_app')
}
