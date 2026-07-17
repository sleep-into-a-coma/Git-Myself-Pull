<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import type { AppState, GitAuthStatus, MotionPreference, Repository, RepositoryPathStatus, ThemeMode } from './types'

type Page = 'repositories' | 'automation' | 'auth' | 'settings' | 'logs'
const requestedPage = new URLSearchParams(window.location.search).get('page')
const page = ref<Page>((['repositories', 'automation', 'auth', 'settings', 'logs'] as const).includes(requestedPage as Page) ? requestedPage as Page : 'repositories')
const loading = ref(true)
const busy = ref(false)
const toast = ref('')
const state = reactive<AppState>({ version: '3.3.0', settings: { repositories: [], startWithWindows: false, closeBehavior: 'background', proxyMode: 'system', proxyAddress: '', autoMaintainLogs: true, maxLogSizeMb: 5, autoCheckUpdates: true, updateEndpoint: 'https://github.com/sleep-into-a-coma/Git-Myself-Pull/releases/latest/download/latest.json', themeMode: 'system', accentColor: '#0169cc', lightBackground: '#ffffff', lightForeground: '#0d0d0d', darkBackground: '#202223', darkForeground: '#f4f4f4', uiFont: "'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif", codeFont: "'Cascadia Mono', Consolas, monospace", translucentSidebar: true, contrast: 45, pointerCursor: false, motionPreference: 'system', uiFontSize: 14, codeFontSize: 12 }, logs: [] })
const selectedId = ref<string | null>(null)
const blank = (): Repository => ({ id: '', name: '', url: '', localPath: '', branch: '', autoPull: false, intervalMinutes: 30, lastStatus: '尚未更新', isRunning: false })
const form = reactive<Repository>(blank())
const selected = computed(() => state.settings.repositories.find(r => r.id === selectedId.value))
const pathStatus = ref<RepositoryPathStatus>({ kind: 'invalid', message: '请先选择本地目录' })
const formDirty = computed(() => !!selected.value && (form.name !== selected.value.name || form.url !== selected.value.url || form.localPath !== selected.value.localPath || form.branch !== selected.value.branch))
const repositoryActionLabel = computed(() => {
  if (formDirty.value) return '请先保存修改'
  if (pathStatus.value.kind === 'missing' || pathStatus.value.kind === 'empty') return '克隆项目'
  if (pathStatus.value.kind === 'nonGit') return '注册为 Git 仓库'
  if (pathStatus.value.kind === 'git') return '立即更新'
  return '目录不可用'
})
const repositoryActionDisabled = computed(() => busy.value || form.isRunning || formDirty.value || pathStatus.value.kind === 'invalid' || pathStatus.value.kind === 'nestedGit')
const authStatus = ref<GitAuthStatus | null>(null)
const authBusy = ref(false)
const authError = ref('')
const githubSignedIn = computed(() => (authStatus.value?.accounts.length || 0) > 0)
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
}
function setTheme(mode: ThemeMode) { state.settings.themeMode = mode; void persistSettings() }
function setMotion(mode: MotionPreference) { state.settings.motionPreference = mode; void persistSettings() }
function resetAppearance() {
  Object.assign(state.settings, { themeMode: 'system', accentColor: '#0169cc', lightBackground: '#ffffff', lightForeground: '#0d0d0d', darkBackground: '#202223', darkForeground: '#f4f4f4', uiFont: "'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif", codeFont: "'Cascadia Mono', Consolas, monospace", translucentSidebar: true, contrast: 45, pointerCursor: false, motionPreference: 'system', uiFontSize: 14, codeFontSize: 12 })
  void persistSettings()
}
function mediaChanged() { systemDark.value = darkQuery.matches; systemReducedMotion.value = motionQuery.matches }

watch(() => state.settings, applyAppearance, { deep: true, immediate: true })

function apply(next: AppState) {
  state.version = next.version
  state.settings = next.settings
  state.logs = next.logs
  const current = state.settings.repositories.find(r => r.id === selectedId.value)
  if (current) Object.assign(form, current)
}

async function refresh() { apply(await api.state()) }
function notify(message: string) { toast.value = message; window.setTimeout(() => toast.value = '', 2400) }
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
    if (form.localPath.trim() === path) pathStatus.value = { kind: 'invalid', message: String(error) }
  }
}

async function saveRepository() {
  if (!form.url.trim() || !form.localPath.trim()) return notify('请填写 Git 地址和本地目录')
  busy.value = true
  try { const saved = await api.saveRepository({ ...form }); selectedId.value = saved.id; await refresh(); await inspectPath(); notify('项目已保存') }
  catch (e) { notify(String(e)) } finally { busy.value = false }
}

async function removeRepository() {
  if (!selected.value || !confirm(`删除“${selected.value.name}”？不会删除本地文件。`)) return
  await api.deleteRepository(selected.value.id); select(); await refresh(); notify('项目已删除')
}

async function browse() { const path = await api.chooseFolder(); if (path) { form.localPath = path; await inspectPath() } }
async function detectBranch() { form.branch = await api.detectBranch(form.localPath); if (!form.branch) notify('未检测到分支'); await inspectPath() }
async function run(id: string) {
  busy.value = true
  try {
    await api.updateRepository(id)
    await refresh()
    await inspectPath()
    notify(state.settings.repositories.find(repo => repo.id === id)?.lastStatus || '任务完成')
  } catch (e) { notify(String(e)); await refresh(); await inspectPath() } finally { busy.value = false }
}
async function initializeRepository(id: string) {
  if (!confirm('将在目标目录创建 .git、关联 origin 并读取远程分支。现有文件不会被覆盖，是否继续？')) return
  busy.value = true
  try {
    const message = await api.initializeRepository(id)
    await refresh()
    await inspectPath()
    notify(message)
  } catch (e) { notify(String(e)); await refresh(); await inspectPath() } finally { busy.value = false }
}
async function repositoryAction() {
  if (!form.id) return
  if (pathStatus.value.kind === 'nonGit') await initializeRepository(form.id)
  else if (!repositoryActionDisabled.value) await run(form.id)
}
async function runAll() { busy.value = true; try { await api.updateAll(); await refresh(); notify('全部同步任务完成') } catch (e) { notify(String(e)); await refresh() } finally { busy.value = false } }
async function refreshAuthStatus() {
  authBusy.value = true
  authError.value = ''
  try { authStatus.value = await api.gitAuthStatus() }
  catch (error) { authError.value = String(error) }
  finally { authBusy.value = false }
}
function openAuth() { page.value = 'auth'; void refreshAuthStatus() }
async function loginGitHub() {
  authBusy.value = true
  authError.value = ''
  try {
    const message = await api.loginGitHub()
    authStatus.value = await api.gitAuthStatus()
    notify(message)
  } catch (error) { authError.value = String(error); notify(String(error)) }
  finally { authBusy.value = false }
}
async function logoutGitHub(account: string) {
  if (!confirm(`退出 GitHub 账户“${account}”？这会从 Windows 凭据管理器中删除对应凭据。`)) return
  authBusy.value = true
  authError.value = ''
  try {
    const message = await api.logoutGitHub(account)
    authStatus.value = await api.gitAuthStatus()
    notify(message)
  } catch (error) { authError.value = String(error); notify(String(error)) }
  finally { authBusy.value = false }
}
async function persistSettings() { await api.saveSettings(state.settings) }
async function toggleAuto(repo: Repository) { repo.autoPull = !repo.autoPull; if (repo.autoPull) repo.lastAttempt = undefined; await persistSettings() }
async function clearLogs() { if (!confirm('清理全部运行日志？')) return; await api.clearLogs(); state.logs = [] }
async function checkUpdate(automatic = false) {
  if (!automatic) busy.value = true
  try {
    const result = await api.checkUpdate()
    if (!result.available) { if (!automatic) notify(result.message); return }
    if (automatic) { notify(result.message); return }
    if (confirm(`${result.message}，现在下载并安装？`)) await api.installUpdate()
  } catch (e) { if (!automatic) notify(String(e)) } finally { if (!automatic) busy.value = false }
}

onMounted(async () => {
  darkQuery.addEventListener('change', mediaChanged)
  motionQuery.addEventListener('change', mediaChanged)
  try {
    await refresh()
    await listen<AppState>('state-changed', event => apply(event.payload))
    if (page.value === 'auth') await refreshAuthStatus()
    if (state.settings.autoCheckUpdates && state.settings.updateEndpoint.trim()) window.setTimeout(() => void checkUpdate(true), 1200)
  } catch (e) { notify(String(e)) } finally { loading.value = false }
})
onBeforeUnmount(() => {
  darkQuery.removeEventListener('change', mediaChanged)
  motionQuery.removeEventListener('change', mediaChanged)
})
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <div class="brand"><div class="brand-mark">G</div><div><strong>Git Auto Pull</strong><small>v{{ state.version }}</small></div></div>
      <nav>
        <button :class="{ active: page === 'repositories' }" @click="page = 'repositories'"><span>⌂</span>仓库管理</button>
        <button :class="{ active: page === 'automation' }" @click="page = 'automation'"><span>↻</span>自动更新</button>
        <button :class="{ active: page === 'auth' }" @click="openAuth"><span>◎</span>Git 登录</button>
        <button :class="{ active: page === 'settings' }" @click="page = 'settings'"><span>⚙</span>设置</button>
        <button :class="{ active: page === 'logs' }" @click="page = 'logs'"><span>≡</span>运行日志</button>
      </nav>
      <button class="exit-button" @click="api.exit()"><span>×</span>退出程序</button>
    </aside>

    <main class="main" :aria-busy="loading || busy">
      <template v-if="page === 'repositories'">
        <header class="page-header"><div><h1>仓库管理</h1><p>克隆、注册并同步本地 Git 项目</p></div><div class="header-actions"><button class="button ghost" @click="runAll">全部同步</button><button class="button primary" @click="select()">注册新项目</button></div></header>
        <section class="repo-layout">
          <div class="repo-list surface">
            <div class="list-head"><span>项目</span><span>分支</span><span>状态</span></div>
            <button v-for="repo in state.settings.repositories" :key="repo.id" class="repo-row" :class="{ selected: selectedId === repo.id }" @click="select(repo)">
              <span class="repo-name"><i :class="{ online: repo.lastSuccess }"></i>{{ repo.name }}</span><span>{{ repo.branch || '当前分支' }}</span><span class="muted ellipsis">{{ repo.lastStatus }}</span>
            </button>
            <div v-if="!state.settings.repositories.length" class="empty"><b>还没有项目</b><span>点击“注册新项目”开始</span></div>
          </div>
          <form class="editor surface" @submit.prevent="saveRepository">
            <div class="section-title"><h2>{{ form.id ? '编辑项目' : '注册项目' }}</h2><button v-if="form.id" type="button" class="icon-button" @click="select()">＋</button></div>
            <label>名称<input v-model="form.name" placeholder="自动从地址生成" /></label>
            <label>Git 地址<input v-model="form.url" placeholder="https://github.com/user/repository.git" /></label>
            <label>本地目录<div class="field-action"><input v-model="form.localPath" placeholder="选择 clone 位置" @change="inspectPath" /><button type="button" @click="browse">浏览</button></div><span v-if="form.localPath" class="path-status" :class="pathStatus.kind"><i></i>{{ pathStatus.message }}</span></label>
            <label>分支<div class="field-action"><input v-model="form.branch" placeholder="留空使用当前分支" /><button type="button" @click="detectBranch">检测</button></div></label>
            <div class="form-actions"><button type="submit" class="button primary" :disabled="busy">保存项目</button><button v-if="form.id" type="button" class="button ghost" :disabled="repositoryActionDisabled" @click="repositoryAction">{{ repositoryActionLabel }}</button><button v-if="form.id" type="button" class="button ghost" @click="api.openFolder(form.localPath)">打开目录</button><button v-if="form.id" type="button" class="button danger" @click="removeRepository">删除</button></div>
          </form>
        </section>
      </template>

      <template v-else-if="page === 'automation'">
        <header class="page-header"><div><h1>自动更新</h1><p>为每个项目独立设置检测频率</p></div><button class="button primary" @click="runAll">立即检查全部</button></header>
        <section class="stack">
          <article v-for="repo in state.settings.repositories" :key="repo.id" class="automation-row surface">
            <div class="automation-info"><strong>{{ repo.name }}</strong><span>{{ repo.lastStatus }}</span></div>
            <button class="switch" :class="{ on: repo.autoPull }" role="switch" :aria-checked="repo.autoPull" @click="toggleAuto(repo)"><i></i></button>
            <div class="interval"><input v-model.number="repo.intervalMinutes" type="number" min="1" max="10080" @change="persistSettings"/><span>分钟</span></div>
            <button class="button ghost" @click="run(repo.id)">立即检测</button>
          </article>
          <div v-if="!state.settings.repositories.length" class="empty surface"><b>暂无可配置项目</b><span>先在仓库管理中注册项目</span></div>
        </section>
      </template>

      <template v-else-if="page === 'auth'">
        <header class="page-header"><div><h1>Git 登录</h1><p>使用系统凭据安全访问私有仓库</p></div><button class="button ghost" :disabled="authBusy" @click="refreshAuthStatus">刷新状态</button></header>
        <section class="auth-page">
          <article class="codex-panel auth-overview">
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

          <article class="codex-panel auth-accounts">
            <div class="auth-section-title"><div><h2>已登录账户</h2><p>凭据由 Windows 凭据管理器保存</p></div><span>{{ authStatus?.accounts.length || 0 }}</span></div>
            <div v-for="profile in authStatus?.accounts" :key="profile.login" class="auth-account-row">
              <div class="account-avatar">
                <img v-if="profile.avatarData" :src="profile.avatarData" alt="" />
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
            <div><strong>应用不会读取或保存你的密码与令牌</strong><p>OAuth 凭据始终由 Git Credential Manager 保管。Rust 后端只用账户名请求 GitHub 公开资料，并在本地下载、校验和转换头像；资料不会写入配置或日志。</p></div>
          </article>

          <article class="codex-panel other-hosts">
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
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'system' }" @click="setTheme('system')">
                <span class="theme-preview system-preview"><i></i><b></b><em></em></span><span>系统</span>
              </button>
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'light' }" @click="setTheme('light')">
                <span class="theme-preview light-preview"><i></i><b></b><em></em></span><span>浅色</span>
              </button>
              <button class="theme-choice" :class="{ selected: state.settings.themeMode === 'dark' }" @click="setTheme('dark')">
                <span class="theme-preview dark-preview"><i></i><b></b><em></em></span><span>深色</span>
              </button>
            </div>

            <article class="codex-panel appearance-panel">
              <div class="panel-title-row"><strong>{{ activeThemeName }}主题</strong><div class="theme-preset"><span>Aa</span><b>Codex</b><i>⌄</i></div></div>
              <div class="setting-row appearance-row"><div><strong>强调色</strong><small>按钮、开关与选中状态</small></div><label class="color-control accent-control" :style="{ background: state.settings.accentColor, color: 'var(--accent-text)' }"><input v-model="state.settings.accentColor" type="color" @change="persistSettings"/><input v-model="state.settings.accentColor" aria-label="强调色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>背景</strong><small>当前{{ activeThemeName }}主题背景</small></div><label class="color-control"><input v-model="activeBackground" type="color" @change="persistSettings"/><input v-model="activeBackground" aria-label="背景色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>前景</strong><small>文字与主要控件颜色</small></div><label class="color-control dark-control"><input v-model="activeForeground" type="color" @change="persistSettings"/><input v-model="activeForeground" aria-label="前景色" @change="persistSettings"/></label></div>
              <div class="setting-row appearance-row"><div><strong>UI 字体</strong></div><select v-model="state.settings.uiFont" class="font-select" @change="persistSettings"><option value="'Segoe UI Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif">Codex</option><option value="'Microsoft YaHei UI', 'Segoe UI', sans-serif">微软雅黑</option><option value="'Segoe UI', sans-serif">Segoe UI</option></select></div>
              <div class="setting-row appearance-row"><div><strong>代码字体</strong></div><select v-model="state.settings.codeFont" class="font-select code-select" @change="persistSettings"><option value="'Cascadia Mono', Consolas, monospace">Cascadia Mono</option><option value="Consolas, monospace">Consolas</option><option value="ui-monospace, monospace">系统等宽</option></select></div>
              <div class="setting-row appearance-row"><div><strong>半透明侧边栏</strong><small>让侧栏融入窗口背景</small></div><button class="switch" :class="{ on: state.settings.translucentSidebar }" role="switch" :aria-checked="state.settings.translucentSidebar" @click="state.settings.translucentSidebar = !state.settings.translucentSidebar; persistSettings()"><i></i></button></div>
              <div class="setting-row appearance-row"><div><strong>对比度</strong><small>调整界面层次与边界强度</small></div><div class="range-control"><input v-model.number="state.settings.contrast" type="range" min="0" max="100" @change="persistSettings"/><span>{{ state.settings.contrast }}</span></div></div>
            </article>
          </div>

          <div class="settings-group">
            <h2 class="settings-group-title">偏好设置</h2>
            <article class="codex-panel">
              <div class="setting-row appearance-row"><div><strong>使用指针光标</strong><small>悬停交互元素时切换为指针光标</small></div><button class="switch" :class="{ on: state.settings.pointerCursor }" role="switch" :aria-checked="state.settings.pointerCursor" @click="state.settings.pointerCursor = !state.settings.pointerCursor; persistSettings()"><i></i></button></div>
              <div class="setting-row appearance-row"><div><strong>减少动态效果</strong><small>减少动画效果或匹配系统设置</small></div><div class="segmented"><button :class="{ active: state.settings.motionPreference === 'system' }" @click="setMotion('system')">系统</button><button :class="{ active: state.settings.motionPreference === 'reduce' }" @click="setMotion('reduce')">开启</button><button :class="{ active: state.settings.motionPreference === 'full' }" @click="setMotion('full')">关闭</button></div></div>
              <div class="setting-row appearance-row"><div><strong>UI 字号</strong><small>调整应用界面使用的基准字号</small></div><div class="number-unit"><input v-model.number="state.settings.uiFontSize" type="number" min="11" max="20" @change="persistSettings"/><span>px</span></div></div>
              <div class="setting-row appearance-row"><div><strong>代码字体大小</strong><small>调整日志等宽文本的字号</small></div><div class="number-unit"><input v-model.number="state.settings.codeFontSize" type="number" min="10" max="20" @change="persistSettings"/><span>px</span></div></div>
            </article>
          </div>

          <div class="settings-group">
            <h2 class="settings-group-title">应用设置</h2>
            <div class="settings-grid compact-settings">
              <article class="settings-section codex-panel"><h3>网络与代理</h3><div class="setting-row"><div><strong>代理模式</strong></div><select v-model="state.settings.proxyMode" @change="persistSettings"><option value="system">跟随系统 / VPN</option><option value="disabled">禁用代理</option><option value="custom">自定义代理</option></select></div><div v-if="state.settings.proxyMode === 'custom'" class="setting-row"><div><strong>代理地址</strong></div><input v-model="state.settings.proxyAddress" placeholder="http://127.0.0.1:7890" @change="persistSettings"/></div></article>
              <article class="settings-section codex-panel"><h3>程序行为</h3><div class="setting-row"><div><strong>随 Windows 启动</strong></div><button class="switch" :class="{ on: state.settings.startWithWindows }" @click="state.settings.startWithWindows = !state.settings.startWithWindows; persistSettings()"><i></i></button></div><div class="setting-row"><div><strong>关闭窗口时</strong></div><select v-model="state.settings.closeBehavior" @change="persistSettings"><option value="background">后台运行</option><option value="exit">关闭程序</option></select></div></article>
              <article class="settings-section codex-panel maintenance-panel"><h3>软件维护</h3><div class="setting-row"><div><strong>自动维护日志</strong></div><button class="switch" :class="{ on: state.settings.autoMaintainLogs }" @click="state.settings.autoMaintainLogs = !state.settings.autoMaintainLogs; persistSettings()"><i></i></button></div><div class="setting-row"><div><strong>日志大小上限</strong></div><div class="inline-input"><input v-model.number="state.settings.maxLogSizeMb" type="number" min="1" max="100" @change="persistSettings"/><span>MB</span></div></div><div class="setting-row"><div><strong>自动检测软件更新</strong></div><button class="switch" :class="{ on: state.settings.autoCheckUpdates }" @click="state.settings.autoCheckUpdates = !state.settings.autoCheckUpdates; persistSettings()"><i></i></button></div><div class="setting-row endpoint-row"><div><strong>更新服务地址</strong></div><input v-model="state.settings.updateEndpoint" placeholder="https://…/latest.json" @change="persistSettings"/></div><div class="section-actions"><button class="button ghost" @click="clearLogs">清理日志</button><button class="button primary" @click="checkUpdate()">检测软件更新</button></div></article>
            </div>
          </div>
        </section>
      </template>

      <template v-else>
        <header class="page-header"><div><h1>运行日志</h1><p>最近的 Git 与后台任务记录</p></div><button class="button ghost" @click="clearLogs">清理日志</button></header>
        <section class="log-view surface"><div v-for="(line, index) in state.logs" :key="index">{{ line }}</div><div v-if="!state.logs.length" class="empty"><b>暂无日志</b></div></section>
      </template>
    </main>
    <Transition name="toast"><div v-if="toast" class="toast">{{ toast }}</div></Transition>
  </div>
</template>
