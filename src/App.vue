<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import type { AppState, GitAuthStatus, GitHubProject, LocalGitProject, ManagedProjectStatus, MotionPreference, Repository, RepositoryPathStatus, ThemeMode } from './types'

type Page = 'repositories' | 'projects' | 'automation' | 'auth' | 'settings' | 'logs'
type ToastKind = 'success' | 'error' | 'info'
type ConfirmationTone = 'default' | 'danger'

interface ConfirmationRequest {
  title: string
  message: string
  confirmLabel: string
  tone: ConfirmationTone
  resolve: (confirmed: boolean) => void
}

const requestedPage = new URLSearchParams(window.location.search).get('page')
const page = ref<Page>((['repositories', 'projects', 'automation', 'auth', 'settings', 'logs'] as const).includes(requestedPage as Page) ? requestedPage as Page : 'repositories')
const loading = ref(true)
const busy = ref(false)
const toast = reactive({ message: '', kind: 'info' as ToastKind })
const confirmation = ref<ConfirmationRequest | null>(null)
const confirmButton = ref<HTMLButtonElement | null>(null)
let toastTimer: number | undefined
let unlistenState: (() => void) | undefined
const state = reactive<AppState>({ version: '3.5.1', settings: { repositories: [], githubProjectsRoot: '', startWithWindows: false, closeBehavior: 'background', proxyMode: 'system', proxyAddress: '', autoMaintainLogs: true, maxLogSizeMb: 5, autoCheckUpdates: true, updateEndpoint: 'https://github.com/sleep-into-a-coma/Git-Myself-Pull/releases/latest/download/latest.json', themeMode: 'system', accentColor: '#0169cc', lightBackground: '#ffffff', lightForeground: '#0d0d0d', darkBackground: '#202223', darkForeground: '#f4f4f4', uiFont: "'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif", codeFont: "'Cascadia Mono', Consolas, monospace", translucentSidebar: true, contrast: 45, pointerCursor: false, motionPreference: 'system', uiFontSize: 14, codeFontSize: 12, maxConcurrentGitOperations: 2 }, logs: [], operationQueue: { active: 0, queued: 0, maxConcurrent: 2 } })
const selectedId = ref<string | null>(null)
const blank = (): Repository => ({ id: '', name: '', url: '', localPath: '', branch: '', autoPull: false, intervalMinutes: 30, lastStatus: '尚未更新', isRunning: false, isQueued: false, queuePosition: 0 })
const form = reactive<Repository>(blank())
const selected = computed(() => state.settings.repositories.find(r => r.id === selectedId.value))
const pathStatus = ref<RepositoryPathStatus>({ kind: 'invalid', message: '请先选择本地目录' })
const formDirty = computed(() => !!selected.value && (form.name !== selected.value.name || form.url !== selected.value.url || form.localPath !== selected.value.localPath || form.branch !== selected.value.branch))
const repositorySaveDisabled = computed(() => busy.value || !form.url.trim() || !form.localPath.trim())
const registerSelectedDirectoryDisabled = computed(() => busy.value || form.isRunning || form.isQueued || pathStatus.value.kind !== 'nonGit' || !form.url.trim() || !form.localPath.trim())
const registerSelectedDirectoryLabel = computed(() => form.id && !formDirty.value ? '注册所选目录' : '保存并注册目录')
const repositoryActionLabel = computed(() => {
  if (formDirty.value) return '请先保存修改'
  if (pathStatus.value.kind === 'missing' || pathStatus.value.kind === 'empty') return '克隆项目'
  if (pathStatus.value.kind === 'nonGit') return '注册为 Git 仓库'
  if (pathStatus.value.kind === 'git') return '立即更新'
  return '目录不可用'
})
const repositoryActionDisabled = computed(() => busy.value || form.isRunning || form.isQueued || formDirty.value || pathStatus.value.kind === 'invalid' || pathStatus.value.kind === 'nestedGit')
const authStatus = ref<GitAuthStatus | null>(null)
const authBusy = ref(false)
const authError = ref('')
const githubSignedIn = computed(() => (authStatus.value?.accounts.length || 0) > 0)
const githubProjects = ref<GitHubProject[]>([])
const discoveredProjects = ref<LocalGitProject[]>([])
const projectAccount = ref('')
const projectSearch = ref('')
const selectedProjectKey = ref('')
const projectStatus = ref<ManagedProjectStatus | null>(null)
const projectsBusy = ref(false)
const projectsError = ref('')
const commitMessage = ref('')
const filteredProjects = computed(() => {
  const query = projectSearch.value.trim().toLowerCase()
  return query ? githubProjects.value.filter(project => `${project.fullName} ${project.description || ''} ${project.language || ''}`.toLowerCase().includes(query)) : githubProjects.value
})
const selectedProject = computed(() => githubProjects.value.find(project => project.remoteKey === selectedProjectKey.value) || null)
const linkedRepository = computed(() => selectedProject.value ? repositoryFor(selectedProject.value) : undefined)
const discoveredProject = computed(() => selectedProject.value ? discoveredProjects.value.find(project => project.remoteKey === selectedProject.value!.remoteKey) : undefined)
const activeProjectProfile = computed(() => authStatus.value?.accounts.find(account => account.login.toLowerCase() === projectAccount.value.toLowerCase()))
const darkQuery = window.matchMedia('(prefers-color-scheme: dark)')
const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)')
const systemDark = ref(darkQuery.matches)
const systemReducedMotion = ref(motionQuery.matches)
const effectiveDark = computed(() => state.settings.themeMode === 'dark' || (state.settings.themeMode === 'system' && systemDark.value))
const activeBackground = computed({
  get: () => effectiveDark.value ? state.settings.darkBackground : state.settings.lightBackground,
  set: value => { if (effectiveDark.value) state.settings.darkBackground = value; else state.settings.lightBackground = value },
})
const activeForeground = computed({
  get: () => effectiveDark.value ? state.settings.darkForeground : state.settings.lightForeground,
  set: value => { if (effectiveDark.value) state.settings.darkForeground = value; else state.settings.lightForeground = value },
})
const activeThemeName = computed(() => effectiveDark.value ? '深色' : '浅色')
const queueActivityLabel = computed(() => {
  const { active, queued, maxConcurrent } = state.operationQueue
  if (!active && !queued) return `空闲 · 并发 ${maxConcurrent}`
  return `运行 ${active} · 等待 ${queued} · 并发 ${maxConcurrent}`
})
const pageLabels: Record<Page, string> = { repositories: '仓库管理', projects: '我的项目', automation: '自动更新', auth: 'Git 登录', settings: '设置', logs: '运行日志' }

function validHex(value: string, fallback: string) { return /^#[0-9a-f]{6}$/i.test(value) ? value : fallback }
function mixHex(first: string, second: string, amount: number) {
  const a = validHex(first, '#ffffff').slice(1).match(/.{2}/g)!.map(value => parseInt(value, 16))
  const b = validHex(second, '#0d0d0d').slice(1).match(/.{2}/g)!.map(value => parseInt(value, 16))
  return `#${a.map((value, index) => Math.round(value + (b[index] - value) * amount).toString(16).padStart(2, '0')).join('')}`
}
function rgba(hex: string, alpha: number) {
  const values = validHex(hex, '#ffffff').slice(1).match(/.{2}/g)!.map(value => parseInt(value, 16))
  return `rgba(${values.join(', ')}, ${alpha})`
}
function contrastText(hex: string) {
  const values = validHex(hex, '#0169cc').slice(1).match(/.{2}/g)!.map(value => parseInt(value, 16) / 255)
  const luminance = values.map(value => value <= .03928 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4).reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0)
  return luminance > .48 ? '#0d0d0d' : '#ffffff'
}
function applyAppearance() {
  const root = document.documentElement
  const dark = effectiveDark.value
  const background = validHex(activeBackground.value, dark ? '#202223' : '#ffffff')
  const foreground = validHex(activeForeground.value, dark ? '#f4f4f4' : '#0d0d0d')
  const accent = validHex(state.settings.accentColor, '#0169cc')
  const contrast = Math.max(0, Math.min(100, state.settings.contrast))
  const surfaceAmount = (dark ? 5.5 : 2.2) + contrast * (dark ? .035 : .022)
  const sidebarBase = mixHex(background, accent, dark ? .035 : .045)
  const reduced = state.settings.motionPreference === 'reduce' || (state.settings.motionPreference === 'system' && systemReducedMotion.value)
  const variables: Record<string, string> = {
    '--page': background,
    '--surface': mixHex(background, foreground, surfaceAmount / 100),
    '--surface-hover': mixHex(background, foreground, (surfaceAmount + 3) / 100),
    '--selected': mixHex(background, accent, (11 + contrast * .035) / 100),
    '--text': foreground,
    '--muted': mixHex(background, foreground, dark ? .62 : .56),
    '--line': mixHex(background, foreground, (dark ? 15 : 9) / 100),
    '--input': mixHex(background, foreground, (dark ? 9 : 1.2) / 100),
    '--sidebar': state.settings.translucentSidebar ? rgba(sidebarBase, dark ? .86 : .8) : sidebarBase,
    '--dark': foreground,
    '--accent': accent,
    '--accent-text': contrastText(accent),
    '--ui-font': state.settings.uiFont,
    '--code-font': state.settings.codeFont,
    '--ui-font-size': `${state.settings.uiFontSize}px`,
    '--code-font-size': `${state.settings.codeFontSize}px`,
  }
  Object.entries(variables).forEach(([name, value]) => root.style.setProperty(name, value))
  root.dataset.theme = dark ? 'dark' : 'light'
  root.dataset.translucent = String(state.settings.translucentSidebar)
  root.dataset.pointer = String(state.settings.pointerCursor)
  root.dataset.reduceMotion = String(reduced)
  root.style.colorScheme = dark ? 'dark' : 'light'
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', background)
}
function setTheme(mode: ThemeMode) { state.settings.themeMode = mode; void persistSettings() }
function setMotion(mode: MotionPreference) { state.settings.motionPreference = mode; void persistSettings() }
function resetAppearance() {
  Object.assign(state.settings, { themeMode: 'system', accentColor: '#0169cc', lightBackground: '#ffffff', lightForeground: '#0d0d0d', darkBackground: '#202223', darkForeground: '#f4f4f4', uiFont: "'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif", codeFont: "'Cascadia Mono', Consolas, monospace", translucentSidebar: true, contrast: 45, pointerCursor: false, motionPreference: 'system', uiFontSize: 14, codeFontSize: 12 })
  void persistSettings()
}
function mediaChanged() { systemDark.value = darkQuery.matches; systemReducedMotion.value = motionQuery.matches }

watch(() => state.settings, applyAppearance, { deep: true, immediate: true })
watch(page, value => { document.title = `${pageLabels[value]} · Git Auto Pull` }, { immediate: true })

function apply(next: AppState) {
  state.version = next.version
  state.settings = next.settings
  state.logs = next.logs
  state.operationQueue = next.operationQueue
  const current = state.settings.repositories.find(r => r.id === selectedId.value)
  if (current) Object.assign(form, current)
}

async function refresh() { apply(await api.state()) }
function errorText(error: unknown) {
  return (error instanceof Error ? error.message : String(error)).replace(/^Error:\s*/i, '').trim()
}
function notify(message: string, kind: ToastKind = 'success') {
  window.clearTimeout(toastTimer)
  toast.message = message
  toast.kind = kind
  toastTimer = window.setTimeout(() => { toast.message = '' }, kind === 'error' ? 4200 : 2600)
}
function notifyError(error: unknown) { notify(errorText(error), 'error') }
function askConfirm(title: string, message: string, confirmLabel = '确认', tone: ConfirmationTone = 'default') {
  return new Promise<boolean>(resolve => {
    confirmation.value = { title, message, confirmLabel, tone, resolve }
    void nextTick(() => confirmButton.value?.focus())
  })
}
function answerConfirm(confirmed: boolean) {
  const current = confirmation.value
  if (!current) return
  confirmation.value = null
  current.resolve(confirmed)
}
function navigate(target: Page) {
  page.value = target
  if (target === 'auth') void refreshAuthStatus()
  if (target === 'projects') void refreshProjects()
}
function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && confirmation.value) return answerConfirm(false)
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'n' && page.value === 'repositories') {
    event.preventDefault()
    select()
  }
}
function normalizeRemote(value: string) {
  return value.trim().replace(/[\\/]+$/, '').replace(/\.git$/i, '').replace(/^https?:\/\//i, '').replace(/^ssh:\/\//i, '').replace(/^git@/i, '').replace(':', '/').replaceAll('\\', '/').toLowerCase()
}
function repositoryFor(project: GitHubProject) { return state.settings.repositories.find(repository => normalizeRemote(repository.url) === project.remoteKey) }
function projectLocalPath(project: GitHubProject) { return repositoryFor(project)?.localPath || discoveredProjects.value.find(local => local.remoteKey === project.remoteKey)?.path || '' }
function projectLocalLabel(project: GitHubProject) { return repositoryFor(project) ? '已维护' : discoveredProjects.value.some(local => local.remoteKey === project.remoteKey) ? '已识别' : '未绑定' }
function formatProjectDate(value?: string) { return value ? new Intl.DateTimeFormat('zh-CN', { month: 'short', day: 'numeric' }).format(new Date(value)) : '暂无推送' }
function joinProjectPath(root: string, name: string) { return `${root.replace(/[\\/]+$/, '')}\\${name}` }

async function refreshProjects() {
  projectsBusy.value = true
  projectsError.value = ''
  try {
    if (!authStatus.value) authStatus.value = await api.gitAuthStatus()
    const accounts = authStatus.value.accounts.map(account => account.login)
    if (!accounts.length) { githubProjects.value = []; throw new Error('请先在“Git 登录”中登录 GitHub') }
    if (!accounts.some(account => account.toLowerCase() === projectAccount.value.toLowerCase())) projectAccount.value = accounts[0]
    githubProjects.value = await api.githubProjects(projectAccount.value)
    if (!githubProjects.value.some(project => project.remoteKey === selectedProjectKey.value)) selectedProjectKey.value = githubProjects.value[0]?.remoteKey || ''
    await refreshProjectStatus()
  } catch (error) { projectsError.value = errorText(error) }
  finally { projectsBusy.value = false }
}
async function selectProject(project: GitHubProject) {
  selectedProjectKey.value = project.remoteKey
  commitMessage.value = ''
  await refreshProjectStatus()
}
async function refreshProjectStatus() {
  const project = selectedProject.value
  if (!project) { projectStatus.value = null; return }
  const path = projectLocalPath(project)
  if (!path) { projectStatus.value = null; return }
  const key = project.remoteKey
  try {
    const status = await api.inspectManagedProject(path, project.cloneUrl)
    if (selectedProjectKey.value === key) projectStatus.value = status
  } catch (error) {
    if (selectedProjectKey.value === key) projectsError.value = errorText(error)
  }
}
async function chooseProjectsRoot() {
  const path = await api.chooseFolder()
  if (!path) return
  state.settings.githubProjectsRoot = path
  await persistSettings()
  await scanLocalProjects()
}
async function scanLocalProjects() {
  if (!state.settings.githubProjectsRoot.trim()) return notify('请先选择项目根目录')
  projectsBusy.value = true
  projectsError.value = ''
  try {
    discoveredProjects.value = await api.discoverLocalProjects(state.settings.githubProjectsRoot)
    await refreshProjectStatus()
    const matches = githubProjects.value.filter(project => discoveredProjects.value.some(local => local.remoteKey === project.remoteKey)).length
    notify(`识别到 ${discoveredProjects.value.length} 个本地仓库，匹配 ${matches} 个个人项目`)
  } catch (error) { projectsError.value = errorText(error); notifyError(error) }
  finally { projectsBusy.value = false }
}
async function saveProjectBinding(project: GitHubProject, path: string, branch?: string) {
  const existing = repositoryFor(project)
  const repository: Repository = existing ? structuredClone(existing) : blank()
  Object.assign(repository, { name: project.fullName, url: project.cloneUrl, localPath: path, branch: branch || project.defaultBranch })
  await api.saveRepository(repository)
  await refresh()
}
async function bindProjectFolder() {
  const project = selectedProject.value
  if (!project) return
  const path = await api.chooseFolder()
  if (!path) return
  projectsBusy.value = true
  try {
    const status = await api.inspectManagedProject(path, project.cloneUrl)
    if (status.kind === 'git' && !status.remoteMatches) throw new Error('该目录的 origin 与所选 GitHub 项目不一致')
    if (status.kind === 'nestedGit' || status.kind === 'invalid') throw new Error(status.message)
    await saveProjectBinding(project, path, status.branch)
    await refreshProjectStatus()
    notify(status.kind === 'git' ? '本地 Git 目录已绑定' : '目录已绑定，可继续克隆或注册')
  } catch (error) { notifyError(error); projectsError.value = errorText(error) }
  finally { projectsBusy.value = false }
}
async function bindDiscoveredProject() {
  const project = selectedProject.value
  const local = discoveredProject.value
  if (!project || !local) return
  projectsBusy.value = true
  try {
    await saveProjectBinding(project, local.path, local.branch)
    await refreshProjectStatus()
    notify('已绑定识别到的本地仓库')
  } catch (error) { notifyError(error) }
  finally { projectsBusy.value = false }
}
async function cloneProject() {
  const project = selectedProject.value
  if (!project) return
  if (!state.settings.githubProjectsRoot.trim()) return notify('请先选择项目根目录')
  const path = joinProjectPath(state.settings.githubProjectsRoot, project.name)
  projectsBusy.value = true
  try {
    const status = await api.inspectManagedProject(path, project.cloneUrl)
    if (status.kind === 'git') {
      if (!status.remoteMatches) throw new Error('默认目标目录已被其他 Git 仓库占用')
      await saveProjectBinding(project, path, status.branch)
      notify('已绑定现有 Git 仓库')
    } else {
      if (status.kind !== 'missing' && status.kind !== 'empty') throw new Error('默认目标目录不是空目录，请改用“绑定目录”')
      await saveProjectBinding(project, path)
      const repository = repositoryFor(project)
      if (!repository) throw new Error('保存项目绑定失败')
      await api.updateRepository(repository.id)
      await refresh()
      notify('项目克隆完成')
    }
    await refreshProjectStatus()
  } catch (error) { notifyError(error); projectsError.value = errorText(error); await refresh() }
  finally { projectsBusy.value = false }
}
async function initializeManagedProject() {
  const repository = linkedRepository.value
  if (!repository) return
  if (!await askConfirm('注册为 Git 仓库', '将在该目录创建 .git、关联 GitHub origin，并保留现有文件。', '继续注册')) return
  projectsBusy.value = true
  try { const message = await api.initializeRepository(repository.id); await refresh(); await refreshProjectStatus(); notify(message) }
  catch (error) { notifyError(error); await refreshProjectStatus() }
  finally { projectsBusy.value = false }
}
async function pullManagedProject() {
  const repository = linkedRepository.value
  if (!repository) return
  projectsBusy.value = true
  try { await api.updateRepository(repository.id); await refresh(); await refreshProjectStatus(); notify(repositoryFor(selectedProject.value!)?.lastStatus || '更新完成') }
  catch (error) { notifyError(error); await refresh(); await refreshProjectStatus() }
  finally { projectsBusy.value = false }
}
async function commitAndPushManagedProject() {
  const repository = linkedRepository.value
  const status = projectStatus.value
  if (!repository || !status?.remoteMatches || !commitMessage.value.trim()) return
  if (!await askConfirm('提交并推送', `将暂存该仓库全部改动（${status.changes} 项）、创建提交并推送到 origin。`, '提交并推送')) return
  projectsBusy.value = true
  try {
    const message = await api.commitAndPushProject(repository.id, commitMessage.value)
    commitMessage.value = ''
    await refresh(); await refreshProjectStatus(); notify(message)
  } catch (error) { notifyError(error); await refresh(); await refreshProjectStatus() }
  finally { projectsBusy.value = false }
}
async function pushManagedProject() {
  const repository = linkedRepository.value
  if (!repository) return
  if (!await askConfirm('推送现有提交', '将当前分支尚未推送的提交发送到 origin。', '开始推送')) return
  projectsBusy.value = true
  try { const message = await api.pushProject(repository.id); await refresh(); await refreshProjectStatus(); notify(message) }
  catch (error) { notifyError(error); await refreshProjectStatus() }
  finally { projectsBusy.value = false }
}
function select(repo?: Repository) {
  selectedId.value = repo?.id || null
  Object.assign(form, repo ? structuredClone(repo) : blank())
  pathStatus.value = { kind: 'invalid', message: repo ? '正在检查目录…' : '请先选择本地目录' }
  if (repo?.localPath) void inspectPath()
}

async function inspectPath() {
  const path = form.localPath.trim()
  if (!path) { pathStatus.value = { kind: 'invalid', message: '请先选择本地目录' }; return }
  try {
    const result = await api.inspectRepositoryPath(path)
    if (form.localPath.trim() === path) pathStatus.value = result
  } catch (error) {
    if (form.localPath.trim() === path) pathStatus.value = { kind: 'invalid', message: errorText(error) }
  }
}

async function saveRepository() {
  if (!form.url.trim() || !form.localPath.trim()) return notify('请填写 Git 地址和本地目录')
  busy.value = true
  try { const saved = await api.saveRepository({ ...form }); selectedId.value = saved.id; await refresh(); await inspectPath(); notify('项目已保存') }
  catch (error) { notifyError(error) } finally { busy.value = false }
}

async function removeRepository() {
  if (!selected.value) return
  if (!await askConfirm('删除跟踪项目', `将从应用中移除“${selected.value.name}”，本地文件不会被删除。`, '删除项目', 'danger')) return
  await api.deleteRepository(selected.value.id); select(); await refresh(); notify('项目已删除')
}

async function browse() { const path = await api.chooseFolder(); if (path) { form.localPath = path; await inspectPath() } }
async function detectBranch() { form.branch = await api.detectBranch(form.localPath); if (!form.branch) notify('未检测到分支'); await inspectPath() }
async function registerSelectedDirectory() {
  if (!form.url.trim() || !form.localPath.trim()) return notify('请填写 Git 地址并选择本地目录')
  if (pathStatus.value.kind !== 'nonGit') return notify('所选目录无需注册为 Git 仓库')
  const confirmed = await askConfirm(
    '注册所选目录为 Git 仓库',
    `将在“${form.localPath.trim()}”中创建 .git、绑定 origin 并读取远程分支。现有文件不会被覆盖。`,
    '注册目录'
  )
  if (!confirmed) return
  busy.value = true
  try {
    const saved = await api.saveRepository({ ...form })
    selectedId.value = saved.id
    Object.assign(form, saved)
    const message = await api.initializeRepository(saved.id)
    await refresh()
    await inspectPath()
    notify(message)
  } catch (error) {
    notifyError(error)
    await refresh()
    await inspectPath()
  } finally { busy.value = false }
}
async function run(id: string) {
  busy.value = true
  try {
    await api.updateRepository(id)
    await refresh()
    await inspectPath()
    notify(state.settings.repositories.find(repo => repo.id === id)?.lastStatus || '任务完成')
  } catch (error) { notifyError(error); await refresh(); await inspectPath() } finally { busy.value = false }
}
async function initializeRepository(id: string) {
  if (!await askConfirm('注册为 Git 仓库', '将在目标目录创建 .git、关联 origin 并读取远程分支。现有文件不会被覆盖。', '继续注册')) return
  busy.value = true
  try {
    const message = await api.initializeRepository(id)
    await refresh()
    await inspectPath()
    notify(message)
  } catch (error) { notifyError(error); await refresh(); await inspectPath() } finally { busy.value = false }
}
async function repositoryAction() {
  if (!form.id) return
  if (pathStatus.value.kind === 'nonGit') await initializeRepository(form.id)
  else if (!repositoryActionDisabled.value) await run(form.id)
}
async function runAll() { busy.value = true; try { await api.updateAll(); await refresh(); notify('全部同步任务完成') } catch (error) { notifyError(error); await refresh() } finally { busy.value = false } }
async function refreshAuthStatus() {
  authBusy.value = true
  authError.value = ''
  try { authStatus.value = await api.gitAuthStatus() }
  catch (error) { authError.value = errorText(error) }
  finally { authBusy.value = false }
}
async function loginGitHub() {
  authBusy.value = true
  authError.value = ''
  try {
    const message = await api.loginGitHub()
    authStatus.value = await api.gitAuthStatus()
    notify(message)
  } catch (error) { authError.value = errorText(error); notifyError(error) }
  finally { authBusy.value = false }
}
async function logoutGitHub(account: string) {
  if (!await askConfirm('退出 GitHub 账户', `将删除“${account}”在 Windows 凭据管理器中的对应凭据。`, '退出账户', 'danger')) return
  authBusy.value = true
  authError.value = ''
  try {
    const message = await api.logoutGitHub(account)
    authStatus.value = await api.gitAuthStatus()
    notify(message)
  } catch (error) { authError.value = errorText(error); notifyError(error) }
  finally { authBusy.value = false }
}
async function persistSettings() {
  try { await api.saveSettings(state.settings) }
  catch (error) { notifyError(error); await refresh() }
}
async function toggleAuto(repo: Repository) { repo.autoPull = !repo.autoPull; if (repo.autoPull) repo.lastAttempt = undefined; await persistSettings() }
async function clearLogs() {
  if (!state.logs.length) return notify('当前没有可清理的日志', 'info')
  if (!await askConfirm('清理运行日志', '将永久删除当前保存的全部运行日志。', '清理日志', 'danger')) return
  await api.clearLogs()
  state.logs = []
  notify('运行日志已清理')
}
async function checkUpdate(automatic = false) {
  if (!automatic) busy.value = true
  try {
    const result = await api.checkUpdate()
    if (!result.available) { if (!automatic) notify(result.message); return }
    if (automatic) { notify(result.message); return }
    if (await askConfirm('发现新版本', `${result.message}。更新完成后应用将重新启动。`, '下载并安装')) await api.installUpdate()
  } catch (error) { if (!automatic) notifyError(error) } finally { if (!automatic) busy.value = false }
}

onMounted(async () => {
  darkQuery.addEventListener('change', mediaChanged)
  motionQuery.addEventListener('change', mediaChanged)
  window.addEventListener('keydown', handleKeydown)
  try {
    await refresh()
    unlistenState = await listen<AppState>('state-changed', event => apply(event.payload))
    if (page.value === 'auth') await refreshAuthStatus()
    if (page.value === 'projects') await refreshProjects()
    if (state.settings.autoCheckUpdates && state.settings.updateEndpoint.trim()) window.setTimeout(() => void checkUpdate(true), 1200)
  } catch (error) { notifyError(error) } finally { loading.value = false }
})
onBeforeUnmount(() => {
  darkQuery.removeEventListener('change', mediaChanged)
  motionQuery.removeEventListener('change', mediaChanged)
  window.removeEventListener('keydown', handleKeydown)
  window.clearTimeout(toastTimer)
  unlistenState?.()
  confirmation.value?.resolve(false)
})
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-mark" aria-hidden="true"><svg viewBox="0 0 24 24"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="8" r="2"/><path d="M6 7v10M8 15c6 0 8-2 8-5"/></svg></div>
        <div><strong>Git Auto Pull</strong><small>v{{ state.version }}</small></div>
      </div>
      <nav aria-label="主导航">
        <button :class="{ active: page === 'repositories' }" :aria-current="page === 'repositories' ? 'page' : undefined" title="仓库管理" @click="navigate('repositories')"><svg viewBox="0 0 24 24"><path d="M3.5 7.5h6l2-2h9v13h-17z"/></svg><span>仓库管理</span></button>
        <button :class="{ active: page === 'projects' }" :aria-current="page === 'projects' ? 'page' : undefined" title="我的项目" @click="navigate('projects')"><svg viewBox="0 0 24 24"><rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/></svg><span>我的项目</span></button>
        <button :class="{ active: page === 'automation' }" :aria-current="page === 'automation' ? 'page' : undefined" title="自动更新" @click="navigate('automation')"><svg viewBox="0 0 24 24"><path d="M20 7v5h-5M4 17v-5h5M18.5 12a6.5 6.5 0 0 0-11-4.6L4 12M5.5 12a6.5 6.5 0 0 0 11 4.6L20 12"/></svg><span>自动更新</span></button>
        <button :class="{ active: page === 'auth' }" :aria-current="page === 'auth' ? 'page' : undefined" title="Git 登录" @click="navigate('auth')"><svg viewBox="0 0 24 24"><circle cx="12" cy="8" r="3.5"/><path d="M5 20c.5-4 3-6 7-6s6.5 2 7 6"/></svg><span>Git 登录</span></button>
        <button :class="{ active: page === 'settings' }" :aria-current="page === 'settings' ? 'page' : undefined" title="设置" @click="navigate('settings')"><svg viewBox="0 0 24 24"><path d="M4 7h10M18 7h2M4 17h2M10 17h10M14 5v4M6 15v4"/></svg><span>设置</span></button>
        <button :class="{ active: page === 'logs' }" :aria-current="page === 'logs' ? 'page' : undefined" title="运行日志" @click="navigate('logs')"><svg viewBox="0 0 24 24"><path d="M5 6h14M5 12h14M5 18h9"/></svg><span>运行日志</span></button>
      </nav>
      <button class="exit-button" title="退出程序" @click="api.exit()"><svg viewBox="0 0 24 24"><path d="M12 3v9M6.6 6.6a8 8 0 1 0 10.8 0"/></svg><span>退出程序</span></button>
    </aside>

    <main class="main" :aria-busy="loading || busy || projectsBusy || authBusy">
      <Transition name="activity"><div v-if="busy || projectsBusy || authBusy" class="activity-bar" aria-hidden="true"></div></Transition>
      <div v-if="loading" class="initial-loading" role="status"><span class="spinner"></span><strong>正在准备工作区</strong><small>读取本地仓库与偏好设置</small></div>
      <template v-if="page === 'repositories'">
        <header class="page-header"><div><h1>仓库管理</h1><p>克隆、注册并同步本地 Git 项目</p></div><div class="header-actions"><button class="button ghost" :disabled="busy || !state.settings.repositories.length" @click="runAll">{{ busy ? '同步中…' : '全部同步' }}</button><button class="button primary" title="快捷键 Ctrl+N" @click="select()">注册新项目</button></div></header>
        <section class="repo-layout">
          <div class="repo-list surface">
            <div class="list-head"><span>项目</span><span>分支</span><span>状态</span></div>
            <button v-for="repo in state.settings.repositories" :key="repo.id" class="repo-row" :class="{ selected: selectedId === repo.id }" :aria-pressed="selectedId === repo.id" @click="select(repo)">
              <span class="repo-name"><i :class="{ online: repo.lastSuccess }"></i>{{ repo.name }}</span><span>{{ repo.branch || '当前分支' }}</span><span class="muted ellipsis">{{ repo.lastStatus }}</span>
            </button>
            <div v-if="!state.settings.repositories.length" class="empty"><b>还没有项目</b><span>注册 Git 地址后即可开始同步</span></div>
          </div>
          <form class="editor surface" @submit.prevent="saveRepository">
            <div class="section-title"><h2>{{ form.id ? '编辑项目' : '注册项目' }}</h2><button v-if="form.id" type="button" class="icon-button" title="注册新项目" aria-label="注册新项目" @click="select()">＋</button></div>
            <label><span>名称</span><input v-model="form.name" autocomplete="off" placeholder="自动从地址生成" /></label>
            <label><span>Git 地址</span><input v-model="form.url" autocomplete="off" spellcheck="false" placeholder="https://github.com/user/repository.git" /></label>
            <label><span>本地目录</span><div class="field-action"><input v-model="form.localPath" autocomplete="off" spellcheck="false" placeholder="选择 clone 位置或已有目录" @change="inspectPath" /><button type="button" @click="browse">浏览</button></div><div v-if="form.localPath" class="path-feedback"><span class="path-status" :class="pathStatus.kind" role="status"><i></i>{{ pathStatus.message }}</span><button v-if="pathStatus.kind === 'nonGit'" type="button" class="path-register" :disabled="registerSelectedDirectoryDisabled" @click="registerSelectedDirectory">{{ registerSelectedDirectoryLabel }}</button></div></label>
            <label><span>分支</span><div class="field-action"><input v-model="form.branch" autocomplete="off" spellcheck="false" placeholder="留空使用当前分支" /><button type="button" :disabled="!form.localPath" @click="detectBranch">检测</button></div></label>
            <div class="form-actions"><button type="submit" class="button primary" :disabled="repositorySaveDisabled">{{ busy ? '处理中…' : '保存项目' }}</button><button v-if="form.id && pathStatus.kind !== 'nonGit'" type="button" class="button ghost" :disabled="repositoryActionDisabled" @click="repositoryAction">{{ repositoryActionLabel }}</button><button v-if="form.id" type="button" class="button ghost" :disabled="!form.localPath" @click="api.openFolder(form.localPath)">打开目录</button><button v-if="form.id" type="button" class="button danger" :disabled="busy" @click="removeRepository">删除</button></div>
          </form>
        </section>
      </template>

      <template v-else-if="page === 'projects'">
        <header class="page-header"><div><h1>我的项目</h1><p>读取 GitHub 项目，识别本地仓库并完成日常提交</p></div><button class="button primary" :disabled="projectsBusy" @click="refreshProjects">{{ projectsBusy ? '刷新中…' : '刷新项目' }}</button></header>
        <section class="projects-page">
          <article class="project-source-panel panel">
            <div class="project-account-control">
              <div class="mini-account-avatar"><img v-if="activeProjectProfile?.avatarData" :src="activeProjectProfile.avatarData" alt="" /><span v-else>{{ projectAccount.slice(0, 1).toUpperCase() || 'G' }}</span></div>
              <label><span>GitHub 账户</span><select v-model="projectAccount" :disabled="projectsBusy" @change="refreshProjects"><option v-for="account in authStatus?.accounts" :key="account.login" :value="account.login">{{ account.name || account.login }}</option></select></label>
            </div>
            <div class="project-root-control">
              <label><span>本地项目根目录</span><input v-model="state.settings.githubProjectsRoot" spellcheck="false" placeholder="选择统一存放与识别项目的目录" @change="persistSettings" /></label>
              <button class="button ghost" :disabled="projectsBusy" @click="chooseProjectsRoot">浏览</button>
              <button class="button ghost" :disabled="projectsBusy || !state.settings.githubProjectsRoot" @click="scanLocalProjects">识别本地项目</button>
            </div>
          </article>

          <div v-if="projectsError" class="auth-error">{{ projectsError }}</div>

          <div class="personal-project-layout">
            <article class="project-browser panel">
              <div class="project-browser-head"><div><strong>个人仓库</strong><span>{{ filteredProjects.length }} / {{ githubProjects.length }}</span></div><label class="search-field"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6"/><path d="m16 16 4 4"/></svg><input v-model="projectSearch" aria-label="搜索个人仓库" placeholder="搜索项目" /><button v-if="projectSearch" type="button" aria-label="清空搜索" @click="projectSearch = ''">×</button></label></div>
              <div class="personal-project-list">
                <button v-for="project in filteredProjects" :key="project.id" class="personal-project-row" :class="{ selected: selectedProjectKey === project.remoteKey }" :aria-pressed="selectedProjectKey === project.remoteKey" @click="selectProject(project)">
                  <span class="project-row-main"><strong>{{ project.name }}</strong><small>{{ project.description || project.fullName }}</small></span>
                  <span class="project-row-badges"><i v-if="project.private">私有</i><i v-if="project.fork">Fork</i><i v-if="project.archived">归档</i></span>
                  <span class="project-row-meta"><small>{{ project.language || '—' }}</small><small>{{ formatProjectDate(project.pushedAt) }}</small><b :class="{ linked: projectLocalLabel(project) !== '未绑定' }">{{ projectLocalLabel(project) }}</b></span>
                </button>
                <div v-if="!projectsBusy && !filteredProjects.length" class="auth-empty"><strong>没有可显示的个人项目</strong><span>确认已登录，或清空搜索条件后重试</span></div>
                <div v-if="projectsBusy && !githubProjects.length" class="auth-empty"><span class="spinner"></span><strong>正在读取个人项目</strong><span>请稍候</span></div>
              </div>
            </article>

            <aside class="project-detail panel">
              <template v-if="selectedProject">
                <div class="project-detail-head">
                  <div><span class="detail-eyebrow">{{ selectedProject.private ? '私有仓库' : '公开仓库' }}</span><h2>{{ selectedProject.name }}</h2><p>{{ selectedProject.description || selectedProject.fullName }}</p></div>
                  <span class="project-language">{{ selectedProject.language || 'Git' }}</span>
                </div>

                <div class="local-project-block">
                  <span>本地 Git 目录</span>
                  <code>{{ projectLocalPath(selectedProject) || '尚未绑定本地目录' }}</code>
                  <small v-if="projectStatus">{{ projectStatus.message }}</small>
                  <div class="project-actions">
                    <button v-if="!linkedRepository && discoveredProject" class="button primary" :disabled="projectsBusy" @click="bindDiscoveredProject">绑定识别目录</button>
                    <button class="button ghost" :disabled="projectsBusy" @click="bindProjectFolder">绑定目录</button>
                    <button v-if="!linkedRepository && !discoveredProject" class="button ghost" :disabled="projectsBusy || !state.settings.githubProjectsRoot" @click="cloneProject">克隆到根目录</button>
                    <button v-if="linkedRepository && (projectStatus?.kind === 'missing' || projectStatus?.kind === 'empty')" class="button primary" :disabled="projectsBusy" @click="pullManagedProject">克隆到此目录</button>
                    <button v-if="linkedRepository && projectStatus?.kind === 'nonGit'" class="button primary" :disabled="projectsBusy" @click="initializeManagedProject">注册为 Git</button>
                    <button v-if="linkedRepository && projectStatus?.kind === 'git' && projectStatus.remoteMatches" class="button ghost" :disabled="projectsBusy || projectStatus.changes > 0" @click="pullManagedProject">拉取更新</button>
                    <button v-if="linkedRepository && projectStatus && projectStatus.kind !== 'missing'" class="button ghost" :disabled="projectsBusy" @click="api.openFolder(linkedRepository.localPath)">打开目录</button>
                  </div>
                </div>

                <div v-if="projectStatus?.kind === 'git' && projectStatus.remoteMatches" class="project-status-grid">
                  <div><strong>{{ projectStatus.changes }}</strong><span>改动</span></div><div><strong>{{ projectStatus.staged }}</strong><span>已暂存</span></div><div><strong>{{ projectStatus.untracked }}</strong><span>未跟踪</span></div><div><strong>{{ projectStatus.ahead }}</strong><span>待推送</span></div><div><strong>{{ projectStatus.behind }}</strong><span>待拉取</span></div>
                </div>

                <div v-if="linkedRepository && projectStatus?.kind === 'git' && projectStatus.remoteMatches" class="project-write-block">
                  <div><strong>提交与推送</strong><span>操作前会再次确认，不会记录提交内容</span></div>
                  <textarea v-model="commitMessage" maxlength="500" rows="3" placeholder="填写本次提交说明"></textarea>
                  <div class="project-actions">
                    <button class="button primary" :disabled="projectsBusy || !selectedProject.canPush || !projectStatus.changes || !commitMessage.trim()" @click="commitAndPushManagedProject">提交并推送</button>
                    <button class="button ghost" :disabled="projectsBusy || !selectedProject.canPush || !projectStatus.ahead" @click="pushManagedProject">仅推送现有提交</button>
                  </div>
                  <small v-if="selectedProject.archived">归档仓库不可推送</small>
                </div>
              </template>
              <div v-else class="auth-empty"><strong>选择一个项目</strong><span>在左侧查看并维护个人 GitHub 仓库</span></div>
            </aside>
          </div>
        </section>
      </template>

      <template v-else-if="page === 'automation'">
        <header class="page-header"><div><h1>自动更新</h1><p>为每个项目独立设置检测频率</p></div><div class="header-actions"><span class="queue-summary" :class="{ active: state.operationQueue.active || state.operationQueue.queued }"><i></i>{{ queueActivityLabel }}</span><button class="button primary" :disabled="busy || !state.settings.repositories.length" @click="runAll">{{ busy ? '调度中…' : '立即检查全部' }}</button></div></header>
        <section class="stack">
          <article v-for="repo in state.settings.repositories" :key="repo.id" class="automation-row surface" :class="{ queued: repo.isQueued, running: repo.isRunning }">
            <div class="automation-info"><strong>{{ repo.name }}</strong><span>{{ repo.lastStatus }}</span></div>
            <button class="switch" :class="{ on: repo.autoPull }" role="switch" :aria-label="`${repo.name} 自动更新`" :aria-checked="repo.autoPull" @click="toggleAuto(repo)"><i></i></button>
            <div class="interval"><input v-model.number="repo.intervalMinutes" :aria-label="`${repo.name} 检测间隔`" type="number" min="1" max="10080" :disabled="!repo.autoPull" @change="persistSettings"/><span>分钟</span></div>
            <button class="button ghost" :disabled="busy || repo.isRunning || repo.isQueued" @click="run(repo.id)">{{ repo.isQueued ? `排队 #${repo.queuePosition}` : repo.isRunning ? '执行中…' : '立即检测' }}</button>
          </article>
          <div v-if="!state.settings.repositories.length" class="empty surface"><b>暂无可配置项目</b><span>先在仓库管理中注册项目</span></div>
        </section>
      </template>

      <template v-else-if="page === 'auth'">
        <header class="page-header"><div><h1>Git 登录</h1><p>使用系统凭据安全访问私有仓库</p></div><button class="button ghost" :disabled="authBusy" @click="refreshAuthStatus">刷新状态</button></header>
        <section class="auth-page">
          <article class="panel auth-overview">
            <div class="auth-provider">
              <div class="provider-mark">GH</div>
              <div class="provider-copy"><strong>GitHub</strong><span>通过浏览器 OAuth 登录</span></div>
              <span class="auth-badge" :class="{ signed: githubSignedIn }"><i></i>{{ githubSignedIn ? '已登录' : '未登录' }}</span>
              <button class="button primary" :disabled="authBusy || !authStatus?.credentialManagerAvailable" @click="loginGitHub">{{ githubSignedIn ? '添加账户' : '使用浏览器登录' }}</button>
            </div>
            <div class="auth-meta-grid">
              <div><span>Git for Windows</span><strong>{{ authStatus?.gitVersion || '未检测到' }}</strong></div>
              <div><span>Credential Manager</span><strong>{{ authStatus?.credentialManagerVersion || '未检测到' }}</strong></div>
              <div><span>凭据助手</span><strong class="auth-helper">{{ authStatus?.credentialHelper || '尚未配置' }}</strong></div>
            </div>
          </article>

          <article class="panel auth-accounts">
            <div class="auth-section-title"><div><h2>已登录账户</h2><p>凭据由 Windows 凭据管理器保存</p></div><span>{{ authStatus?.accounts.length || 0 }}</span></div>
            <div v-for="profile in authStatus?.accounts" :key="profile.login" class="auth-account-row">
              <div class="account-avatar">
                <img v-if="profile.avatarData" :src="profile.avatarData" :alt="`${profile.login} 的头像`" />
                <span v-else>{{ profile.login.slice(0, 1).toUpperCase() }}</span>
              </div>
              <div class="account-profile">
                <div class="account-heading"><strong>{{ profile.name || profile.login }}</strong><span>@{{ profile.login }}</span></div>
                <p v-if="profile.bio">{{ profile.bio }}</p>
                <div v-if="!profile.profileError" class="account-meta">
                  <span v-if="profile.company">{{ profile.company }}</span>
                  <span v-if="profile.location">{{ profile.location }}</span>
                  <span>{{ profile.publicRepos }} 个公开仓库</span>
                  <span>{{ profile.followers }} 位关注者</span>
                </div>
                <small v-else>公开资料暂不可用，Git 登录不受影响</small>
              </div>
              <button class="button ghost danger-text" :disabled="authBusy" @click="logoutGitHub(profile.login)">退出</button>
            </div>
            <div v-if="!authStatus?.accounts.length" class="auth-empty"><strong>还没有 GitHub 账户</strong><span>登录后即可克隆和更新有权限访问的私有仓库</span></div>
          </article>

          <div v-if="authError" class="auth-error">{{ authError }}</div>

          <article class="auth-note">
            <div class="note-icon">✓</div>
            <div><strong>登录凭据不会进入前端或日志</strong><p>OAuth 凭据始终由 Git Credential Manager 保管。“我的项目”读取私有仓库时，Rust 后端只在内存中临时使用令牌并立即清理；令牌不会进入 Vue、配置、命令参数或运行日志。</p></div>
          </article>

          <article class="panel other-hosts">
            <div><strong>其他 HTTPS Git 服务</strong><p>GitLab、Bitbucket、Azure DevOps 和自建服务会在首次克隆或更新时由 Git Credential Manager 自动显示对应登录界面。</p></div>
            <span>自动识别</span>
          </article>
        </section>
      </template>

      <template v-else-if="page === 'settings'">
        <header class="page-header settings-header"><div><h1>设置</h1><p>外观、偏好与应用维护</p></div><button class="button ghost" @click="resetAppearance">恢复默认外观</button></header>
        <section class="settings-page">
          <div class="settings-group">
            <h2 class="settings-group-title">外观</h2>
            <h3 class="settings-label">主题</h3>
            <div class="theme-picker">
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'system' }" :aria-pressed="state.settings.themeMode === 'system'" @click="setTheme('system')">
                <span class="theme-preview system-preview"><i></i><b></b><em></em></span><span>系统</span>
              </button>
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'light' }" :aria-pressed="state.settings.themeMode === 'light'" @click="setTheme('light')">
                <span class="theme-preview light-preview"><i></i><b></b><em></em></span><span>浅色</span>
              </button>
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'dark' }" :aria-pressed="state.settings.themeMode === 'dark'" @click="setTheme('dark')">
                <span class="theme-preview dark-preview"><i></i><b></b><em></em></span><span>深色</span>
              </button>
            </div>

            <article class="panel appearance-panel">
              <div class="panel-title-row"><strong>{{ activeThemeName }}主题</strong><div class="theme-summary"><span>Aa</span><b>默认外观</b></div></div>
              <div class="setting-row appearance-row"><div><strong>强调色</strong><small>按钮、开关与选中状态</small></div><label class="color-control accent-control" :style="{ background: state.settings.accentColor, color: 'var(--accent-text)' }"><input v-model="state.settings.accentColor" type="color" @change="persistSettings"/><input v-model="state.settings.accentColor" aria-label="强调色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>背景</strong><small>当前{{ activeThemeName }}主题背景</small></div><label class="color-control"><input v-model="activeBackground" type="color" @change="persistSettings"/><input v-model="activeBackground" aria-label="背景色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>前景</strong><small>文字与主要控件颜色</small></div><label class="color-control dark-control"><input v-model="activeForeground" type="color" @change="persistSettings"/><input v-model="activeForeground" aria-label="前景色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>UI 字体</strong></div><select v-model="state.settings.uiFont" class="font-select" @change="persistSettings"><option value="'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif">系统默认</option><option value="'Microsoft YaHei UI', 'Segoe UI', sans-serif">微软雅黑</option><option value="'Segoe UI', sans-serif">Segoe UI</option></select></div>
              <div class="setting-row appearance-row"><div><strong>代码字体</strong></div><select v-model="state.settings.codeFont" class="font-select code-select" @change="persistSettings"><option value="'Cascadia Mono', Consolas, monospace">Cascadia Mono</option><option value="Consolas, monospace">Consolas</option><option value="ui-monospace, monospace">系统等宽</option></select></div>
              <div class="setting-row appearance-row"><div><strong>半透明侧边栏</strong><small>让侧栏融入窗口背景</small></div><button class="switch" :class="{ on: state.settings.translucentSidebar }" role="switch" aria-label="半透明侧边栏" :aria-checked="state.settings.translucentSidebar" @click="state.settings.translucentSidebar = !state.settings.translucentSidebar; persistSettings()"><i></i></button></div>
              <div class="setting-row appearance-row"><div><strong>对比度</strong><small>调整界面层次与边界强度</small></div><div class="range-control"><input v-model.number="state.settings.contrast" aria-label="界面对比度" type="range" min="0" max="100" @change="persistSettings"/><span>{{ state.settings.contrast }}</span></div></div>
            </article>
          </div>

          <div class="settings-group">
            <h2 class="settings-group-title">偏好设置</h2>
            <article class="panel">
              <div class="setting-row appearance-row"><div><strong>使用指针光标</strong><small>悬停交互元素时切换为指针光标</small></div><button class="switch" :class="{ on: state.settings.pointerCursor }" role="switch" aria-label="使用指针光标" :aria-checked="state.settings.pointerCursor" @click="state.settings.pointerCursor = !state.settings.pointerCursor; persistSettings()"><i></i></button></div>
              <div class="setting-row appearance-row"><div><strong>减少动态效果</strong><small>减少动画效果或匹配系统设置</small></div><div class="segmented" role="group" aria-label="动态效果偏好"><button :class="{ active: state.settings.motionPreference === 'system' }" :aria-pressed="state.settings.motionPreference === 'system'" @click="setMotion('system')">系统</button><button :class="{ active: state.settings.motionPreference === 'reduce' }" :aria-pressed="state.settings.motionPreference === 'reduce'" @click="setMotion('reduce')">开启</button><button :class="{ active: state.settings.motionPreference === 'full' }" :aria-pressed="state.settings.motionPreference === 'full'" @click="setMotion('full')">关闭</button></div></div>
              <div class="setting-row appearance-row"><div><strong>UI 字号</strong><small>调整应用界面使用的基准字号</small></div><div class="number-unit"><input v-model.number="state.settings.uiFontSize" type="number" min="11" max="20" @change="persistSettings"/><span>px</span></div></div>
              <div class="setting-row appearance-row"><div><strong>代码字体大小</strong><small>调整日志等宽文本的字号</small></div><div class="number-unit"><input v-model.number="state.settings.codeFontSize" type="number" min="10" max="20" @change="persistSettings"/><span>px</span></div></div>
            </article>
          </div>

          <div class="settings-group">
            <h2 class="settings-group-title">应用设置</h2>
            <div class="settings-grid compact-settings">
              <article class="settings-section panel">
                <h3>网络与代理</h3>
                <div class="setting-row"><div><strong>代理模式</strong></div><select v-model="state.settings.proxyMode" @change="persistSettings"><option value="system">跟随系统 / VPN</option><option value="disabled">禁用代理</option><option value="custom">自定义代理</option></select></div>
                <div v-if="state.settings.proxyMode === 'custom'" class="setting-row"><div><strong>代理地址</strong></div><input v-model="state.settings.proxyAddress" spellcheck="false" placeholder="http://127.0.0.1:7890" @change="persistSettings"/></div>
              </article>
              <article class="settings-section panel">
                <h3>程序行为</h3>
                <div class="setting-row"><div><strong>随 Windows 启动</strong></div><button class="switch" :class="{ on: state.settings.startWithWindows }" role="switch" aria-label="随 Windows 启动" :aria-checked="state.settings.startWithWindows" @click="state.settings.startWithWindows = !state.settings.startWithWindows; persistSettings()"><i></i></button></div>
                <div class="setting-row"><div><strong>关闭窗口时</strong></div><select v-model="state.settings.closeBehavior" @change="persistSettings"><option value="background">后台运行</option><option value="exit">关闭程序</option></select></div>
              </article>
              <article class="settings-section panel scheduling-panel">
                <h3>Git 任务调度</h3>
                <div class="setting-row"><div><strong>并发任务数</strong><small>设为 1 时所有克隆、拉取与推送严格排队</small></div><div class="number-unit"><input v-model.number="state.settings.maxConcurrentGitOperations" aria-label="Git 并发任务数" type="number" min="1" max="8" @change="persistSettings"/><span>个</span></div></div>
                <div class="setting-row"><div><strong>当前队列</strong><small>同一仓库不会重复加入队列</small></div><output class="queue-status" :class="{ active: state.operationQueue.active || state.operationQueue.queued }"><i></i>{{ queueActivityLabel }}</output></div>
              </article>
              <article class="settings-section panel maintenance-panel">
                <h3>软件维护</h3>
                <div class="setting-row"><div><strong>自动维护日志</strong></div><button class="switch" :class="{ on: state.settings.autoMaintainLogs }" role="switch" aria-label="自动维护日志" :aria-checked="state.settings.autoMaintainLogs" @click="state.settings.autoMaintainLogs = !state.settings.autoMaintainLogs; persistSettings()"><i></i></button></div>
                <div class="setting-row"><div><strong>日志大小上限</strong></div><div class="inline-input"><input v-model.number="state.settings.maxLogSizeMb" aria-label="日志大小上限" type="number" min="1" max="100" @change="persistSettings"/><span>MB</span></div></div>
                <div class="setting-row"><div><strong>自动检测软件更新</strong></div><button class="switch" :class="{ on: state.settings.autoCheckUpdates }" role="switch" aria-label="自动检测软件更新" :aria-checked="state.settings.autoCheckUpdates" @click="state.settings.autoCheckUpdates = !state.settings.autoCheckUpdates; persistSettings()"><i></i></button></div>
                <div class="setting-row endpoint-row"><div><strong>更新服务地址</strong></div><input v-model="state.settings.updateEndpoint" type="url" spellcheck="false" placeholder="https://…/latest.json" @change="persistSettings"/></div>
                <div class="section-actions"><button class="button ghost" :disabled="!state.logs.length" @click="clearLogs">清理日志</button><button class="button primary" :disabled="busy" @click="checkUpdate()">{{ busy ? '检测中…' : '检测软件更新' }}</button></div>
              </article>
            </div>
          </div>
        </section>
      </template>

      <template v-else>
        <header class="page-header"><div><h1>运行日志</h1><p>最近的 Git 与后台任务记录</p></div><button class="button ghost" :disabled="!state.logs.length" @click="clearLogs">清理日志</button></header>
        <section class="log-view surface" aria-label="运行日志"><div v-for="(line, index) in state.logs" :key="index">{{ line }}</div><div v-if="!state.logs.length" class="empty"><b>暂无日志</b><span>执行同步任务后将在这里显示记录</span></div></section>
      </template>
    </main>

    <Transition name="dialog">
      <div v-if="confirmation" class="dialog-backdrop" @mousedown.self="answerConfirm(false)">
        <section class="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-message">
          <div class="dialog-icon" :class="{ danger: confirmation.tone === 'danger' }" aria-hidden="true">{{ confirmation.tone === 'danger' ? '!' : '✓' }}</div>
          <div class="dialog-copy"><h2 id="confirm-title">{{ confirmation.title }}</h2><p id="confirm-message">{{ confirmation.message }}</p></div>
          <div class="dialog-actions"><button class="button ghost" @click="answerConfirm(false)">取消</button><button ref="confirmButton" class="button primary" :class="{ danger: confirmation.tone === 'danger' }" @click="answerConfirm(true)">{{ confirmation.confirmLabel }}</button></div>
        </section>
      </div>
    </Transition>
    <Transition name="toast"><div v-if="toast.message" class="toast" :class="toast.kind" :role="toast.kind === 'error' ? 'alert' : 'status'" aria-live="polite"><i aria-hidden="true">{{ toast.kind === 'error' ? '!' : '✓' }}</i><span>{{ toast.message }}</span></div></Transition>
  </div>
</template>
