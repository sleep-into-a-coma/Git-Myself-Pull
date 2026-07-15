import { invoke } from '@tauri-apps/api/core'
import type { AppState, Repository, Settings, UpdateStatus } from './types'

export const api = {
  state: () => invoke<AppState>('get_state'),
  saveRepository: (repository: Repository) => invoke<Repository>('save_repository', { repository }),
  deleteRepository: (id: string) => invoke<void>('delete_repository', { id }),
  updateRepository: (id: string) => invoke<void>('update_repository', { id }),
  updateAll: () => invoke<void>('update_all'),
  detectBranch: (path: string) => invoke<string>('detect_branch', { path }),
  chooseFolder: () => invoke<string | null>('choose_folder'),
  openFolder: (path: string) => invoke<void>('open_folder', { path }),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  clearLogs: () => invoke<void>('clear_logs'),
  checkUpdate: () => invoke<UpdateStatus>('check_app_update'),
  installUpdate: () => invoke<void>('install_app_update'),
  exit: () => invoke<void>('exit_app')
}
