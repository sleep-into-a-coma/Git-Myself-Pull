export type ProxyMode = 'system' | 'disabled' | 'custom'
export type CloseBehavior = 'background' | 'exit'
export type ThemeMode = 'system' | 'light' | 'dark'
export type MotionPreference = 'system' | 'reduce' | 'full'
export type RepositoryPathKind = 'missing' | 'empty' | 'git' | 'nonGit' | 'nestedGit' | 'invalid'

export interface RepositoryPathStatus {
  kind: RepositoryPathKind
  message: string
}

export interface Repository {
  id: string
  name: string
  url: string
  localPath: string
  branch: string
  autoPull: boolean
  intervalMinutes: number
  lastAttempt?: string
  lastSuccess?: string
  lastStatus: string
  isRunning: boolean
}

export interface Settings {
  repositories: Repository[]
  startWithWindows: boolean
  closeBehavior: CloseBehavior
  proxyMode: ProxyMode
  proxyAddress: string
  autoMaintainLogs: boolean
  maxLogSizeMb: number
  autoCheckUpdates: boolean
  updateEndpoint: string
  themeMode: ThemeMode
  accentColor: string
  lightBackground: string
  lightForeground: string
  darkBackground: string
  darkForeground: string
  uiFont: string
  codeFont: string
  translucentSidebar: boolean
  contrast: number
  pointerCursor: boolean
  motionPreference: MotionPreference
  uiFontSize: number
  codeFontSize: number
}

export interface AppState {
  version: string
  settings: Settings
  logs: string[]
}

export interface UpdateStatus {
  available: boolean
  version?: string
  message: string
}
