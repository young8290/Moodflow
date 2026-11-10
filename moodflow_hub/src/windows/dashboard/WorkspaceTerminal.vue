<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open, confirm } from '@tauri-apps/plugin-dialog'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { errorText, t } from '@/i18n'
import { workspaceCall as call } from './workspaceTypes'
const props = defineProps<{ id: string; paused: boolean; archived?: boolean }>()
const host = ref<HTMLDivElement | null>(null)
const directory = ref('')
const exists = ref(false)
const running = ref(false)
const busy = ref(false)
const error = ref('')
const gap = ref(false)
let terminal: Terminal | null = null
let fit: FitAddon | null = null
let observer: ResizeObserver | null = null
let timer: ReturnType<typeof setTimeout> | undefined
let resizeTimer: ReturnType<typeof setTimeout> | undefined
let generation = 0
let after = 0
let disposed = false
let sending = Promise.resolve()
let queuedBytes = 0
type Poll = { data: string; after: number; gap: boolean; running: boolean; exit_code: number | null }
const base64 = (bytes: Uint8Array): string => btoa(String.fromCharCode(...bytes))
async function send(bytes: Uint8Array, mine: number, id: string): Promise<void> {
  if (bytes.length > 65536) {
    error.value = t('workspace.limit')
    return
  }
  for (let offset = 0; offset < bytes.length; offset += 4096) {
    if (mine !== generation || props.id !== id || props.paused || disposed || !running.value) return
    await call('terminal_input', { id, data: base64(bytes.subarray(offset, offset + 4096)) })
  }
}
function queue(bytes: Uint8Array): void {
  if (props.paused || disposed || queuedBytes + bytes.length > 65536) {
    error.value = t('workspace.limit')
    return
  }
  const mine = generation
  const id = props.id
  queuedBytes += bytes.length
  sending = sending
    .then(() => send(bytes, mine, id))
    .catch((e: unknown) => {
      if (mine === generation && !props.paused && !disposed) error.value = errorText(e)
    })
    .finally(() => {
      queuedBytes -= bytes.length
    })
}
async function poll(mine: number): Promise<void> {
  if (mine !== generation || props.paused || disposed) return
  try {
    const result = await call<Poll>('terminal_read', { id: props.id, after })
    if (mine !== generation || props.paused || disposed) return
    exists.value = true
    running.value = result.running
    after = result.after
    if (result.gap) {
      gap.value = true
      terminal?.reset()
    }
    if (result.data) terminal?.write(Uint8Array.from(atob(result.data), (c) => c.charCodeAt(0)))
  } catch (e) {
    if (mine !== generation || props.paused || disposed) return
    if ((e as { code?: string })?.code === 'workspace.no_terminal') {
      exists.value = false
      running.value = false
    } else error.value = errorText(e)
  }
  if (mine === generation && !props.paused && !disposed)
    timer = setTimeout(() => void poll(mine), running.value ? 100 : 1000)
}
function detach(): void {
  generation++
  clearTimeout(timer)
  clearTimeout(resizeTimer)
  observer?.disconnect()
  observer = null
  terminal?.dispose()
  terminal = null
  fit = null
  after = 0
  gap.value = false
  error.value = ''
}
async function attach(): Promise<void> {
  detach()
  const mine = generation
  if (props.paused || disposed) return
  await nextTick()
  if (mine !== generation || !host.value) return
  terminal = new Terminal({
    cols: 96,
    rows: 24,
    scrollback: 1000,
    fontSize: 14,
    convertEol: false,
    linkHandler: { activate: () => {} },
    windowOptions: {},
    theme: { background: '#17191b', foreground: '#e7e3dc', cursor: '#e9bc83' },
  })
  fit = new FitAddon()
  terminal.loadAddon(fit)
  terminal.open(host.value)
  // Untrusted process output cannot read/write the system clipboard or open hyperlinks.
  terminal.parser.registerOscHandler(52, () => true)
  terminal.onData((data) => queue(new TextEncoder().encode(data)))
  terminal.onBinary((data) => queue(Uint8Array.from(data, (c) => c.charCodeAt(0))))
  observer = new ResizeObserver(() => {
    clearTimeout(resizeTimer)
    resizeTimer = setTimeout(() => {
      if (mine !== generation || !terminal || !fit) return
      fit.fit()
      terminal.resize(Math.max(20, Math.min(240, terminal.cols)), Math.max(8, Math.min(100, terminal.rows)))
      if (running.value)
        void call('terminal_resize', { id: props.id, cols: terminal.cols, rows: terminal.rows }).catch(
          () => {},
        )
    }, 120)
  })
  observer.observe(host.value)
  void poll(mine)
}
async function choose(): Promise<void> {
  if (props.archived || props.paused) return
  const mine = generation
  const path = await open({ directory: true, multiple: false })
  if (typeof path === 'string' && mine === generation && !props.paused && !disposed) directory.value = path
}
async function start(): Promise<void> {
  if (busy.value || !directory.value || props.paused || props.archived) return
  const mine = generation
  busy.value = true
  error.value = ''
  try {
    await call('terminal_start', {
      id: props.id,
      directory: directory.value,
      cols: terminal?.cols ?? 96,
      rows: terminal?.rows ?? 24,
    })
    if (mine !== generation || props.paused || disposed) return
    exists.value = true
    running.value = true
    after = 0
    terminal?.reset()
    terminal?.focus()
  } catch (e) {
    if (mine === generation && !disposed) error.value = errorText(e)
  } finally {
    if (mine === generation) busy.value = false
  }
}
async function end(): Promise<void> {
  const mine = generation
  if (
    !(await confirm(t('workspace.end_confirm'), { title: 'Moodflow', kind: 'warning' })) ||
    mine !== generation ||
    props.paused ||
    disposed
  )
    return
  busy.value = true
  try {
    await call('terminal_end', { id: props.id })
    if (mine === generation) {
      exists.value = false
      running.value = false
    }
  } catch (e) {
    if (mine === generation) error.value = errorText(e)
  } finally {
    if (mine === generation) busy.value = false
  }
}
watch(
  () => props.paused,
  () => {
    if (props.paused) {
      detach()
      busy.value = false
    } else void attach()
  },
)
watch(
  () => props.id,
  () => {
    directory.value = ''
    exists.value = false
    running.value = false
    void attach()
  },
)
onMounted(() => void attach())
onBeforeUnmount(() => {
  disposed = true
  detach()
})
</script>
<template>
  <section class="terminal">
    <p class="hint">{{ t('workspace.terminal_hint') }}</p>
    <div class="actions">
      <button :disabled="busy || running || paused || archived" @click="choose">
        {{ t('workspace.directory') }}
      </button>
      <span class="directory">{{ directory }}</span>
      <button v-if="!exists" :disabled="busy || !directory || paused || archived" @click="start">
        {{ t('workspace.start_terminal') }}
      </button>
      <button v-else :disabled="busy || paused" @click="end">{{ t('workspace.end_terminal') }}</button>
      <span>{{
        t(
          running
            ? 'workspace.terminal_running'
            : exists
              ? 'workspace.terminal_exited'
              : 'workspace.terminal_idle',
        )
      }}</span>
    </div>
    <p v-if="gap" role="status">{{ t('workspace.output_gap') }}</p>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <div v-show="!paused" ref="host" class="surface" :aria-label="t('workspace.terminal')" />
  </section>
</template>
<style scoped>
.terminal {
  display: grid;
  gap: var(--mf-sp-3);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--mf-sp-2);
}
.directory {
  overflow-wrap: anywhere;
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.surface {
  height: 380px;
  padding: 12px;
  background: #17191b;
  border-radius: var(--mf-radius-card);
  overflow: hidden;
}
.hint {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.error {
  color: var(--mf-danger);
}
</style>
