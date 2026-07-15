export type ProxyMode = 'system' | 'disabled' | 'custom'
export type CloseBehavior = 'background' | 'exit'

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
