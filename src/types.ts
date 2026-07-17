export type ProxyMode = 'system' | 'disabled' | 'custom'
export type CloseBehavior = 'background' | 'exit'
export type ThemeMode = 'system' | 'light' | 'dark'
export type MotionPreference = 'system' | 'reduce' | 'full'
export type RepositoryPathKind = 'missing' | 'empty' | 'git' | 'nonGit' | 'nestedGit' | 'invalid'

export interface RepositoryPathStatus {
  kind: RepositoryPathKind
  message: string
}

export interface GitAuthStatus {
  gitAvailable: boolean
  gitVersion: string
  credentialManagerAvailable: boolean
  credentialManagerVersion: string
  credentialHelper: string
  accounts: GitAccountProfile[]
}

export interface GitAccountProfile {
  login: string
  name?: string
  bio?: string
  company?: string
  location?: string
  publicRepos: number
  followers: number
  avatarData?: string
  profileError?: string
}

export interface GitHubProject {
  id: number
  name: string
  fullName: string
  description?: string
  cloneUrl: string
  remoteKey: string
  defaultBranch: string
  private: boolean
  fork: boolean
  archived: boolean
  language?: string
  stars: number
  canPush: boolean
  pushedAt?: string
}

export interface LocalGitProject {
  name: string
  path: string
  originUrl: string
  remoteKey: string
  branch: string
}

export interface ManagedProjectStatus {
  kind: RepositoryPathKind
  message: string
  branch: string
  originUrl: string
  remoteMatches: boolean
  changes: number
  staged: number
  unstaged: number
  untracked: number
  ahead: number
  behind: number
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
  isQueued: boolean
  queuePosition: number
}

export interface Settings {
  repositories: Repository[]
  githubProjectsRoot: string
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
  maxConcurrentGitOperations: number
}

export interface OperationQueueStatus {
  active: number
  queued: number
  maxConcurrent: number
}

export interface AppState {
  version: string
  settings: Settings
  logs: string[]
  operationQueue: OperationQueueStatus
}

export interface UpdateStatus {
  available: boolean
  version?: string
  message: string
}
