<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import type { AppState, Repository, Settings } from './types'

type Page = 'repositories' | 'automation' | 'settings' | 'logs'
const page = ref<Page>('repositories')
const loading = ref(true)
const busy = ref(false)
const toast = ref('')
const state = reactive<AppState>({ version: '3.0.1', settings: { repositories: [], startWithWindows: false, closeBehavior: 'background', proxyMode: 'system', proxyAddress: '', autoMaintainLogs: true, maxLogSizeMb: 5, autoCheckUpdates: true, updateEndpoint: 'https://github.com/sleep-into-a-coma/Git-Myself-Pull/releases/latest/download/latest.json' }, logs: [] })
const selectedId = ref<string | null>(null)
const blank = (): Repository => ({ id: '', name: '', url: '', localPath: '', branch: '', autoPull: false, intervalMinutes: 30, lastStatus: '尚未更新', isRunning: false })
const form = reactive<Repository>(blank())
const selected = computed(() => state.settings.repositories.find(r => r.id === selectedId.value))

function apply(next: AppState) {
  state.version = next.version
  state.settings = next.settings
  state.logs = next.logs
  const current = state.settings.repositories.find(r => r.id === selectedId.value)
  if (current) Object.assign(form, current)
}

async function refresh() { apply(await api.state()) }
function notify(message: string) { toast.value = message; window.setTimeout(() => toast.value = '', 2400) }
function select(repo?: Repository) { selectedId.value = repo?.id || null; Object.assign(form, repo ? structuredClone(repo) : blank()) }

async function saveRepository() {
  if (!form.url.trim() || !form.localPath.trim()) return notify('请填写 Git 地址和本地目录')
  busy.value = true
  try { const saved = await api.saveRepository({ ...form }); selectedId.value = saved.id; await refresh(); notify('项目已保存') }
  catch (e) { notify(String(e)) } finally { busy.value = false }
}

async function removeRepository() {
  if (!selected.value || !confirm(`删除“${selected.value.name}”？不会删除本地文件。`)) return
  await api.deleteRepository(selected.value.id); select(); await refresh(); notify('项目已删除')
}

async function browse() { const path = await api.chooseFolder(); if (path) form.localPath = path }
async function detectBranch() { form.branch = await api.detectBranch(form.localPath); if (!form.branch) notify('未检测到分支') }
async function run(id: string) { busy.value = true; try { await api.updateRepository(id); await refresh(); notify('检查完成') } catch (e) { notify(String(e)) } finally { busy.value = false } }
async function runAll() { busy.value = true; try { await api.updateAll(); await refresh(); notify('全部检查完成') } catch (e) { notify(String(e)) } finally { busy.value = false } }
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
  try {
    await refresh()
    await listen<AppState>('state-changed', event => apply(event.payload))
    if (state.settings.autoCheckUpdates && state.settings.updateEndpoint.trim()) window.setTimeout(() => void checkUpdate(true), 1200)
  } catch (e) { notify(String(e)) } finally { loading.value = false }
})
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <div class="brand"><div class="brand-mark">G</div><div><strong>Git Auto Pull</strong><small>v{{ state.version }}</small></div></div>
      <nav>
        <button :class="{ active: page === 'repositories' }" @click="page = 'repositories'"><span>⌂</span>仓库管理</button>
        <button :class="{ active: page === 'automation' }" @click="page = 'automation'"><span>↻</span>自动更新</button>
        <button :class="{ active: page === 'settings' }" @click="page = 'settings'"><span>⚙</span>设置</button>
        <button :class="{ active: page === 'logs' }" @click="page = 'logs'"><span>≡</span>运行日志</button>
      </nav>
      <button class="exit-button" @click="api.exit()"><span>×</span>退出程序</button>
    </aside>

    <main class="main" :aria-busy="loading || busy">
      <template v-if="page === 'repositories'">
        <header class="page-header"><div><h1>仓库管理</h1><p>管理本地 Git 项目与远程更新</p></div><div class="header-actions"><button class="button ghost" @click="runAll">全部更新</button><button class="button primary" @click="select()">注册新项目</button></div></header>
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
            <label>本地目录<div class="field-action"><input v-model="form.localPath" placeholder="选择 clone 位置" /><button type="button" @click="browse">浏览</button></div></label>
            <label>分支<div class="field-action"><input v-model="form.branch" placeholder="留空使用当前分支" /><button type="button" @click="detectBranch">检测</button></div></label>
            <div class="form-actions"><button type="submit" class="button primary" :disabled="busy">保存项目</button><button v-if="form.id" type="button" class="button ghost" @click="run(form.id)">立即更新</button><button v-if="form.id" type="button" class="button ghost" @click="api.openFolder(form.localPath)">打开目录</button><button v-if="form.id" type="button" class="button danger" @click="removeRepository">删除</button></div>
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

      <template v-else-if="page === 'settings'">
        <header class="page-header"><div><h1>设置</h1><p>网络、程序行为与软件维护</p></div></header>
        <section class="settings-grid">
          <article class="settings-section surface"><h2>网络与代理</h2><div class="setting-row"><div><strong>代理模式</strong></div><select v-model="state.settings.proxyMode" @change="persistSettings"><option value="system">跟随系统 / VPN</option><option value="disabled">禁用代理</option><option value="custom">自定义代理</option></select></div><div v-if="state.settings.proxyMode === 'custom'" class="setting-row"><div><strong>代理地址</strong></div><input v-model="state.settings.proxyAddress" placeholder="http://127.0.0.1:7890" @change="persistSettings"/></div></article>
          <article class="settings-section surface"><h2>程序行为</h2><div class="setting-row"><div><strong>随 Windows 启动</strong></div><button class="switch" :class="{ on: state.settings.startWithWindows }" @click="state.settings.startWithWindows = !state.settings.startWithWindows; persistSettings()"><i></i></button></div><div class="setting-row"><div><strong>关闭窗口时</strong></div><select v-model="state.settings.closeBehavior" @change="persistSettings"><option value="background">后台运行</option><option value="exit">关闭程序</option></select></div></article>
          <article class="settings-section surface"><h2>软件维护</h2><div class="setting-row"><div><strong>自动维护日志</strong></div><button class="switch" :class="{ on: state.settings.autoMaintainLogs }" @click="state.settings.autoMaintainLogs = !state.settings.autoMaintainLogs; persistSettings()"><i></i></button></div><div class="setting-row"><div><strong>日志大小上限</strong></div><div class="inline-input"><input v-model.number="state.settings.maxLogSizeMb" type="number" min="1" max="100" @change="persistSettings"/><span>MB</span></div></div><div class="setting-row"><div><strong>自动检测软件更新</strong></div><button class="switch" :class="{ on: state.settings.autoCheckUpdates }" @click="state.settings.autoCheckUpdates = !state.settings.autoCheckUpdates; persistSettings()"><i></i></button></div><div class="setting-row"><div><strong>更新服务地址</strong></div><input v-model="state.settings.updateEndpoint" placeholder="https://…/latest.json" @change="persistSettings"/></div><div class="section-actions"><button class="button ghost" @click="clearLogs">清理日志</button><button class="button primary" @click="checkUpdate()">检测软件更新</button></div></article>
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
