<script setup lang="ts">
// 对话窗口（05 FR-CHT-01/02/04/09，16 D-06）：顶部常驻 AI 说明，危机时固定求助卡片（FR-SAF-02），
// 左侧抽屉是按日期分组的历史会话，回复逐字显示并带 `AI 生成` 标签，可停止、重试、复制。
// 快捷指令（FR-CHT-06）：吐槽 / 理一理切换会话的对话方式（ADR 0022），呼吸是本地动画，“写成情绪日记”把这段对话交给日记起草。
// 记忆（FR-CHT-07）：对话里明确说“记住”或点消息上的“让晴晴记住”，都先确认再写入。
// 历史抽屉（FR-CHT-08）：本地搜索（chat_search）、删除、保留期限（chat.retention_days）。可切换置顶（FR-CHT-01），记在本机。
// 看板“对话与日记”点开某段对话时，经 xq:chat-open（刚打开窗口时从 localStorage 取）跳到那一段。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { commands, unwrap, type ChatSearchItem, type DiarySaved } from '@/api'
import { errorText, isCopyKey, t } from '@/i18n'
import { useSettingsStore } from '@/stores/settings'
import DiaryEditor from '../shared/DiaryEditor.vue'
import { onChatOpen, takeChatOpen } from '../shared/bus'
import BreathingGuide from './BreathingGuide.vue'
import SafetyCard from './SafetyCard.vue'
import { MAX_INPUT, dayGroup, useChat, type ChatLine } from './useChat'

const chat = useChat()
const draft = ref('')
const notice = ref<string | null>(null)
/** 不是错误的一句提示（“记住啦”），几秒后消失 */
const info = ref<string | null>(null)
const breathing = ref(false)
const drawer = ref(false)
const confirmAll = ref(false)
const copiedId = ref<ChatLine | null>(null)
const logEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLTextAreaElement | null>(null)
const historyBtn = ref<HTMLButtonElement | null>(null)
const drawerEl = ref<HTMLElement | null>(null)
const diary = ref(false)
const pinned = ref(false)
const query = ref('')
/** 搜索结果；null 表示没在搜，抽屉显示按日期分组的会话 */
const hits = ref<ChatSearchItem[] | null>(null)
const settings = useSettingsStore()
const RETENTION_KEY = 'chat.retention_days'
const retentionOptions = ref<string[]>([])
/** 置顶记在本机（每台电脑各自的习惯），下次打开窗口照旧 */
const PIN_STORAGE_KEY = 'xq.chat.pinned'
let unlistenOpen: (() => void) | undefined
let disposed = false

const groups = computed(() => {
  const now = Date.now()
  const out = { today: [], yesterday: [], earlier: [] } as Record<
    'today' | 'yesterday' | 'earlier',
    typeof chat.sessions.value
  >
  for (const s of chat.sessions.value) out[dayGroup(s.last_ts, now)].push(s)
  return (['today', 'yesterday', 'earlier'] as const)
    .map((k) => ({ key: k, items: out[k] }))
    .filter((g) => g.items.length > 0)
})

const failureText = computed(() =>
  chat.failure.value === 'daily_cap' ? t('chat.daily_cap') : chat.failure.value ? t('chat.stuck') : null,
)
const showSafety = computed(() => chat.safeMode.value !== 'off')
const modeText = computed(() =>
  chat.mode.value === 'vent'
    ? t('chat.mode_vent')
    : chat.mode.value === 'organize'
      ? t('chat.mode_organize')
      : null,
)

async function run(action: () => Promise<unknown>): Promise<void> {
  notice.value = null
  try {
    await action()
  } catch (e) {
    notice.value = errorText(e)
  }
}

onMounted(async () => {
  await run(async () => {
    await chat.init()
    const id = takeChatOpen()
    if (id !== null) await chat.open(id)
    inputEl.value?.focus()
  })
  try {
    const off = await onChatOpen((id) => void openSession(id))
    if (disposed) off()
    else unlistenOpen = off
  } catch {
    // 浏览器预览里没有 Tauri 事件
  }
  if (readPinned()) void setPinned(true)
  void loadRetention()
})
onBeforeUnmount(() => {
  disposed = true
  unlistenOpen?.()
})

function readPinned(): boolean {
  try {
    return localStorage.getItem(PIN_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

async function setPinned(on: boolean): Promise<void> {
  await run(async () => {
    await getCurrentWindow().setAlwaysOnTop(on)
    pinned.value = on
    try {
      localStorage.setItem(PIN_STORAGE_KEY, on ? '1' : '0')
    } catch {
      // 记不住也照样置顶，只是下次打开要再点一次
    }
  })
}

async function loadRetention(): Promise<void> {
  try {
    const [schema] = await Promise.all([commands.settingsSchema(), settings.init([RETENTION_KEY])])
    const f = schema.find((x) => x.key === RETENTION_KEY)
    retentionOptions.value = f?.kind.type === 'choice' ? f.kind.options : []
  } catch {
    // 读不到就不显示这一项，聊天照常
  }
}

function retentionLabel(v: string): string {
  const k = `chat.retention_${v}`
  return isCopyKey(k) ? t(k) : v
}

async function setRetention(v: string): Promise<void> {
  await run(() => settings.set(RETENTION_KEY, v))
}

async function search(): Promise<void> {
  const q = query.value.trim()
  if (!q) {
    hits.value = null
    return
  }
  await run(async () => {
    hits.value = await unwrap(commands.chatSearch(q))
  })
}

function clearSearch(): void {
  query.value = ''
  hits.value = null
}

const hitDay = (ts: number) => new Date(ts).toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })

function openDiary(): void {
  breathing.value = false
  diary.value = true
}

function onDiarySaved(r: DiarySaved): void {
  diary.value = false
  // 命中危机词表时后端已经在这个窗口打开求助卡片，不再另外提示
  if (!r.safety) {
    info.value = t('diary.saved')
    setTimeout(() => (info.value = null), 3000)
  }
  inputEl.value?.focus()
}

// 有新内容时滚到底
watch(
  () => [chat.lines.value.length, chat.lines.value[chat.lines.value.length - 1]?.content],
  async () => {
    await nextTick()
    if (logEl.value) logEl.value.scrollTop = logEl.value.scrollHeight
  },
)

async function submit(): Promise<void> {
  const text = draft.value
  if (!text.trim() || chat.busy.value) return
  draft.value = ''
  await run(() => chat.send(text))
  if (notice.value) draft.value = text
}

function onInputKeydown(e: KeyboardEvent): void {
  // Enter 发送、Shift+Enter 换行（FR-CHT-09）；输入法正在组字时的 Enter 是上屏，不发送
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing) return
  e.preventDefault()
  void submit()
}

function onKeydown(e: KeyboardEvent): void {
  // Esc 关闭窗口，不结束会话（FR-CHT-01）；呼吸引导、抽屉开着时先关它们（DS-A11Y-01）
  if (e.key !== 'Escape') return
  if (breathing.value) closeBreathing()
  else if (diary.value) diary.value = false
  else if (drawer.value) drawer.value = false
  else void getCurrentWindow().hide()
}

function closeBreathing(): void {
  breathing.value = false
  inputEl.value?.focus()
}

// 抽屉打开时焦点移进去，关上时回到“历史”按钮，键盘用户不会迷路（DS-A11Y-01）
watch(drawer, async (open) => {
  await nextTick()
  // 打开时焦点落在搜索框：可以直接打字搜，也可以 Tab 到会话列表
  if (open) drawerEl.value?.querySelector<HTMLElement>('input, button')?.focus()
  else if (drawerEl.value === null && document.activeElement === document.body) historyBtn.value?.focus()
})

async function openSession(id: number): Promise<void> {
  drawer.value = false
  await run(() => chat.open(id))
}

function newChat(): void {
  drawer.value = false
  chat.newChat()
  inputEl.value?.focus()
}

async function copy(line: ChatLine): Promise<void> {
  await run(() => chat.copy(line))
  if (!notice.value) copiedId.value = line
}

async function remember(): Promise<void> {
  await run(chat.confirmRemember)
  if (!notice.value) {
    info.value = t('chat.remember_done')
    setTimeout(() => (info.value = null), 3000)
  }
}

async function removeAll(): Promise<void> {
  confirmAll.value = false
  drawer.value = false
  await run(() => chat.removeAll())
}
</script>

<template>
  <main class="chat" @keydown="onKeydown">
    <p class="notice" role="note">{{ t('chat.header_notice') }}</p>
    <header class="bar">
      <button
        ref="historyBtn"
        class="icon"
        :aria-label="t('chat.history')"
        :title="t('chat.history')"
        :aria-expanded="drawer"
        @click="drawer = !drawer"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M2 4h12M2 8h12M2 12h12" />
        </svg>
      </button>
      <h1>{{ t('window.chat') }}</h1>
      <button
        class="icon"
        :aria-label="pinned ? t('chat.unpin') : t('chat.pin')"
        :title="pinned ? t('chat.unpin') : t('chat.pin')"
        :aria-pressed="pinned"
        :class="{ on: pinned }"
        @click="setPinned(!pinned)"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M6 2h4M7 2v4L4.5 9h7L9 6V2M8 9v5" />
        </svg>
      </button>
      <button class="compact" :disabled="chat.busy.value" @click="newChat">{{ t('chat.new_chat') }}</button>
    </header>

    <SafetyCard
      v-if="showSafety"
      :collapsed="chat.collapsed.value"
      :dismissed="chat.safeMode.value === 'dismissed'"
      @safe="chat.collapsed.value = true"
      @expand="chat.collapsed.value = false"
      @misread="run(chat.dismiss)"
    />

    <div ref="logEl" class="log" role="log" aria-live="polite">
      <p v-if="chat.lines.value.length === 0" class="empty">{{ t('chat.empty') }}</p>
      <div v-for="(l, i) in chat.lines.value" :key="l.id ?? `p${i}`" class="row" :class="l.role">
        <p v-if="l.pending && !l.content" class="typing">
          {{ t('chat.typing') }}<span class="dots" aria-hidden="true"><span /><span /><span /></span>
        </p>
        <p v-else class="bubble">{{ l.content }}</p>
        <div v-if="l.role === 'assistant' && !l.pending" class="meta">
          <span v-if="l.aiGenerated" class="tag">{{ t('chat.ai_tag') }}</span>
          <button class="link" @click="copy(l)">
            {{ copiedId === l ? t('chat.copied') : t('chat.copy') }}
          </button>
        </div>
        <div v-else-if="l.role === 'user' && l.id !== null" class="meta">
          <button class="link remember" @click="chat.askRemember(l.content)">
            {{ t('chat.remember_this') }}
          </button>
        </div>
      </div>
      <div v-if="failureText" class="failure" role="status">
        <span>{{ failureText }}</span>
        <button class="compact" @click="run(chat.retry)">{{ t('chat.btn_retry') }}</button>
      </div>
    </div>

    <div v-if="chat.memoryAsk.value" class="ask" role="group" :aria-label="t('chat.remember_ask')">
      <p role="status">{{ t('chat.remember_ask') }}</p>
      <blockquote>{{ chat.memoryAsk.value }}</blockquote>
      <button class="compact primary" @click="remember">{{ t('chat.remember_yes') }}</button>
      <button class="compact" @click="chat.declineRemember">{{ t('chat.remember_no') }}</button>
    </div>
    <p v-if="info" class="info" role="status">{{ info }}</p>
    <p v-if="notice" class="error" role="alert">{{ notice }}</p>
    <div v-if="modeText" class="mode" role="status">
      <span>{{ modeText }}</span>
      <button class="link" :disabled="chat.busy.value" @click="run(() => chat.setMode('normal'))">
        {{ t('chat.mode_back') }}
      </button>
    </div>
    <div class="shortcuts" role="group" :aria-label="t('chat.shortcuts')">
      <button
        class="compact"
        :disabled="chat.busy.value || showSafety"
        :aria-pressed="chat.mode.value === 'vent'"
        @click="run(() => chat.setMode('vent'))"
      >
        {{ t('chat.shortcut_vent') }}
      </button>
      <button
        class="compact"
        :disabled="chat.busy.value || showSafety"
        :aria-pressed="chat.mode.value === 'organize'"
        @click="run(() => chat.setMode('organize'))"
      >
        {{ t('chat.shortcut_organize') }}
      </button>
      <button class="compact" @click="breathing = true">{{ t('chat.shortcut_breathe') }}</button>
      <button class="compact" :disabled="chat.busy.value || chat.sessionId.value === null" @click="openDiary">
        {{ t('chat.shortcut_diary') }}
      </button>
    </div>
    <form class="composer" @submit.prevent="submit">
      <textarea
        ref="inputEl"
        v-model="draft"
        rows="2"
        :maxlength="MAX_INPUT"
        :placeholder="t('chat.placeholder')"
        :aria-label="t('chat.placeholder')"
        @keydown="onInputKeydown"
      />
      <button v-if="chat.busy.value" type="button" class="compact" @click="run(chat.stop)">
        {{ t('chat.btn_stop') }}
      </button>
      <button v-else type="submit" class="compact primary" :disabled="!draft.trim()">
        {{ t('chat.send') }}
      </button>
    </form>

    <BreathingGuide v-if="breathing" @close="closeBreathing" />

    <div v-if="diary" class="diary" role="dialog" aria-modal="true" :aria-label="t('diary.editor')">
      <DiaryEditor :session-id="chat.sessionId.value" @saved="onDiarySaved" @cancel="diary = false" />
    </div>

    <nav v-if="drawer" ref="drawerEl" class="drawer" :aria-label="t('chat.history')">
      <form class="search" role="search" @submit.prevent="search">
        <input
          v-model="query"
          type="search"
          :aria-label="t('chat.search')"
          :placeholder="t('chat.search')"
          @keydown.esc.stop="clearSearch"
        />
        <button type="submit" class="compact">{{ t('chat.search_go') }}</button>
      </form>
      <template v-if="hits !== null">
        <div class="hits-head">
          <span class="muted" role="status">{{
            hits.length ? t('chat.search_count', { n: hits.length }) : t('chat.search_empty')
          }}</span>
          <button class="link" @click="clearSearch">{{ t('chat.search_clear') }}</button>
        </div>
        <ul class="hits">
          <li v-for="h in hits" :key="h.id">
            <button class="hit" @click="openSession(h.session_id)">
              <span class="hit-title"
                >{{ h.title }}<span class="hit-day">{{ hitDay(h.ts) }}</span></span
              >
              <span class="hit-text">{{ h.content }}</span>
            </button>
          </li>
        </ul>
      </template>
      <template v-else>
        <p v-if="groups.length === 0" class="muted">{{ t('chat.history_empty') }}</p>
        <section v-for="g in groups" :key="g.key">
          <h2>{{ t(`chat.${g.key}`) }}</h2>
          <ul>
            <li v-for="s in g.items" :key="s.id" :class="{ current: s.id === chat.sessionId.value }">
              <button class="item" @click="openSession(s.id)">{{ s.title }}</button>
              <button
                class="icon"
                :aria-label="t('chat.delete')"
                :title="t('chat.delete')"
                @click="run(() => chat.remove(s.id))"
              >
                <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
                  <path d="M4 4l8 8M12 4l-8 8" />
                </svg>
              </button>
            </li>
          </ul>
        </section>
        <div v-if="groups.length > 0" class="danger">
          <template v-if="confirmAll">
            <p>{{ t('chat.delete_all_confirm') }}</p>
            <button class="compact" @click="removeAll">{{ t('chat.confirm') }}</button>
            <button class="compact" @click="confirmAll = false">{{ t('chat.cancel') }}</button>
          </template>
          <button v-else class="compact" @click="confirmAll = true">{{ t('chat.delete_all') }}</button>
        </div>
      </template>
      <label v-if="retentionOptions.length" class="retention">
        <span>{{ t('chat.retention') }}</span>
        <select
          :value="settings.values[RETENTION_KEY]"
          @change="setRetention(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="o in retentionOptions" :key="o" :value="o">{{ retentionLabel(o) }}</option>
        </select>
        <span class="muted">{{ t('chat.retention_hint') }}</span>
      </label>
    </nav>
  </main>
</template>

<style scoped>
.chat {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/* 顶部常驻 AI 说明（FR-CHT-04 第 4 条、DS-COPY-05） */
.notice {
  margin: 0;
  padding: var(--xq-sp-2) var(--xq-sp-4);
  background: var(--xq-surface-2);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-xs);
  line-height: var(--xq-lh-xs);
}

.bar {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: center;
  padding: var(--xq-sp-2) var(--xq-sp-3);
}

.bar h1 {
  flex: 1;
  margin: 0;
  font-size: var(--xq-fs-lg);
  line-height: var(--xq-lh-lg);
}

.icon {
  flex: none;
  padding: 0;
  border-color: transparent;
  background: transparent;
  color: var(--xq-text-2);
}

.icon path {
  fill: none;
  stroke: currentColor;
  stroke-width: 1.5;
  stroke-linecap: round;
}

.compact {
  padding: 0 var(--xq-sp-2);
  font-size: var(--xq-fs-sm);
}

.log {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: var(--xq-sp-3);
  padding: var(--xq-sp-2) var(--xq-sp-4);
  overflow-y: auto;
}

.empty {
  margin: auto;
  color: var(--xq-text-2);
}

.row {
  display: flex;
  flex-direction: column;
  max-width: 85%;
}

.row.user {
  align-self: flex-end;
  align-items: flex-end;
}

.row.assistant {
  align-self: flex-start;
}

.bubble,
.typing {
  margin: 0;
  padding: var(--xq-sp-2) var(--xq-sp-3);
  /* 透明边框：高对比度主题下背景色被换掉，气泡靠边框分开（DS-A11Y-04） */
  border: 1px solid transparent;
  border-radius: var(--xq-radius-card);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}

.user .bubble {
  background: var(--xq-primary);
  color: var(--xq-on-primary);
}

.assistant .bubble {
  background: var(--xq-surface-2);
}

.typing {
  color: var(--xq-text-2);
}

/* “晴晴正在输入…”的三个点（FR-CHT-04 第 1 条）：每点 1.2 秒一明一暗，远低于每秒 3 次（DS-MOTION-04）；
   系统关闭动画时 base.css 把动画停在第一帧 */
.dots {
  display: inline-flex;
  gap: 3px;
  margin-left: var(--xq-sp-1);
  vertical-align: middle;
}

.dots span {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: currentColor;
  animation: dot 1.2s ease-in-out infinite;
}

.dots span:nth-child(2) {
  animation-delay: 0.2s;
}

.dots span:nth-child(3) {
  animation-delay: 0.4s;
}

@keyframes dot {
  0%,
  100% {
    opacity: 0.25;
  }

  50% {
    opacity: 1;
  }
}

.meta {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: center;
  color: var(--xq-text-3);
  font-size: var(--xq-fs-xs);
  line-height: var(--xq-lh-xs);
}

/* `AI 生成` 标签（DS-COPY-05）：--xq-text-2 文字、--xq-surface-2 底色、圆角；透明边框给高对比度主题用 */
.tag {
  padding: 0 var(--xq-sp-2);
  border: 1px solid transparent;
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface-2);
  color: var(--xq-text-2);
}

/* 文字样式的小按钮：看起来是链接，点击目标仍守 32 × 32（DS-A11Y-03，base.css 的最小尺寸不覆盖） */
.link {
  padding: 0 var(--xq-sp-1);
  border-color: transparent;
  background: transparent;
  color: var(--xq-link);
  font-size: var(--xq-fs-xs);
}

/* “让晴晴记住”每条用户消息都有：平时隐去，鼠标移到这条消息上或键盘聚焦时才出现，免得满屏都是 */
.remember {
  opacity: 0;
  transition: opacity 150ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.row:hover .remember,
.remember:focus-visible {
  opacity: 1;
}

.failure {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: center;
  color: var(--xq-text-2);
  font-size: var(--xq-fs-sm);
}

.error {
  margin: 0 var(--xq-sp-4);
  color: var(--xq-danger);
  font-size: var(--xq-fs-sm);
}

.info {
  margin: 0 var(--xq-sp-4);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-sm);
}

/* 记忆确认条（FR-CHT-07 第 1 条） */
.ask {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-2);
  align-items: center;
  margin: 0 var(--xq-sp-4) var(--xq-sp-2);
  padding: var(--xq-sp-2) var(--xq-sp-3);
  border-radius: var(--xq-radius-card);
  background: var(--xq-surface-2);
  font-size: var(--xq-fs-sm);
}

.ask p {
  margin: 0;
}

.ask blockquote {
  width: 100%;
  margin: 0;
  color: var(--xq-text-2);
  overflow-wrap: anywhere;
}

/* 快捷指令与当前对话方式（FR-CHT-06） */
.mode {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: center;
  margin: 0 var(--xq-sp-4);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-xs);
}

.shortcuts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-1);
  padding: var(--xq-sp-2) var(--xq-sp-4) 0;
}

.shortcuts [aria-pressed='true'] {
  border-color: var(--xq-primary);
}

.composer {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: flex-end;
  padding: var(--xq-sp-3) var(--xq-sp-4) var(--xq-sp-4);
  border-top: 1px solid var(--xq-border);
}

textarea {
  flex: 1;
  min-height: 48px;
  max-height: 160px;
  padding: var(--xq-sp-2);
  border: 1px solid var(--xq-border);
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface);
  color: var(--xq-text-1);
  font: inherit;
  resize: vertical;
}

.drawer {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  z-index: 2;
  width: 260px;
  padding: var(--xq-sp-4) var(--xq-sp-3);
  overflow-y: auto;
  background: var(--xq-surface);
  box-shadow: var(--xq-shadow-float);
}

.drawer h2 {
  margin: var(--xq-sp-3) 0 var(--xq-sp-1);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-xs);
  line-height: var(--xq-lh-xs);
}

.drawer ul {
  margin: 0;
  padding: 0;
  list-style: none;
}

.drawer li {
  display: flex;
  align-items: center;
  border-radius: var(--xq-radius-sm);
}

.drawer li.current {
  background: var(--xq-surface-2);
}

.item {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  border-color: transparent;
  background: transparent;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.danger {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-1);
  margin-top: var(--xq-sp-4);
  font-size: var(--xq-fs-sm);
}

.danger p {
  width: 100%;
  margin: 0;
}

.icon.on {
  color: var(--xq-primary);
}

.search {
  display: flex;
  gap: var(--xq-sp-1);
}

.search input,
.retention select {
  min-width: 0;
  min-height: 32px;
  padding: 0 var(--xq-sp-2);
  border: 1px solid var(--xq-border);
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface);
  color: var(--xq-text-1);
  font: inherit;
}

.search input {
  flex: 1;
}

.hits-head {
  display: flex;
  justify-content: space-between;
  margin: var(--xq-sp-2) 0 var(--xq-sp-1);
  font-size: var(--xq-fs-sm);
}

.hit {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  width: 100%;
  min-height: 0;
  padding: var(--xq-sp-2);
  border-color: transparent;
  background: transparent;
  text-align: left;
}

.hit:hover {
  background: var(--xq-surface-2);
}

.hit-title {
  display: flex;
  justify-content: space-between;
  font-size: var(--xq-fs-sm);
  font-weight: 500;
}

.hit-day {
  flex: none;
  margin-left: var(--xq-sp-2);
  color: var(--xq-text-3);
  font-weight: 400;
}

/* 命中的那句最多两行 */
.hit-text {
  display: -webkit-box;
  overflow: hidden;
  color: var(--xq-text-2);
  font-size: var(--xq-fs-xs);
  line-height: var(--xq-lh-xs);
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.retention {
  display: grid;
  gap: var(--xq-sp-1);
  margin-top: var(--xq-sp-4);
  padding-top: var(--xq-sp-3);
  border-top: 1px solid var(--xq-border);
  font-size: var(--xq-fs-sm);
}

.retention .muted {
  font-size: var(--xq-fs-xs);
}

/* 写日记的面板盖在对话上，关掉回到对话 */
.diary {
  position: absolute;
  inset: 0;
  z-index: 3;
  padding: var(--xq-sp-4);
  overflow-y: auto;
  background: var(--xq-surface);
}
</style>
