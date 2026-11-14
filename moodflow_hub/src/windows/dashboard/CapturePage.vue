<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { confirm, save } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import CaptureCatalog from './CaptureCatalog.vue'
import CaptureEditor from './CaptureEditor.vue'
import type { CaptureCrop, CaptureImage, CaptureOperation } from './captureTypes'
type Target = { kind: 'monitor'; index: number } | { kind: 'window'; hwnd: number; pid: number }
interface Sources {
  monitors: { index: number; name: string; width: number; height: number }[]
  windows: { hwnd: number; pid: number; title: string; width: number; height: number }[]
  ocr_available: boolean
}
const paused = useToolPrivacy()
const sources = ref<Sources>({ monitors: [], windows: [], ocr_available: false })
const selectedTarget = ref('')
const delay = ref(0)
const seconds = ref(10)
const format = ref('mp4')
const audio = ref(false)
const cursor = ref(false)
const catalogReady = ref(false)
const catalogRefresh = ref(0)
const catalogBusy = ref(false)
const current = ref<CaptureImage | null>(null)
const selection = ref<CaptureCrop | null>(null)
const frozenTarget = ref('')
const title = ref('')
const text = ref('')
const busy = ref(false)
const capturing = ref(false)
const available = ref(false)
const error = ref('')
const notice = ref('')
let generation = 0
let disposed = false
const target = computed<Target | null>(() => {
  const [kind, a, b] = selectedTarget.value.split(':')
  if (kind === 'monitor') return { kind, index: Number(a) }
  if (kind === 'window') return { kind, hwnd: Number(a), pid: Number(b) }
  return null
})
const imageUrl = computed(() => (current.value ? convertFileSrc(current.value.path) : ''))
async function call<T>(method: string, params: object = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.captureRequest(method, JSON.stringify(params)))) as T
}
function refreshCatalog(): void {
  catalogRefresh.value++
}
function clearPreview(): void {
  current.value = null
  selection.value = null
  frozenTarget.value = ''
  text.value = ''
  title.value = ''
}
function catalogChanged(ids: string[], operation: CaptureOperation | 'rename'): void {
  if (!current.value || !ids.includes(current.value.asset.id)) return
  if (operation === 'trash' || operation === 'purge') clearPreview()
}
async function refreshSources(): Promise<void> {
  const mine = generation
  const result = await call<Sources>('targets')
  if (mine !== generation || paused.value || disposed) return
  sources.value = result
  if (!selectedTarget.value && result.monitors[0])
    selectedTarget.value = `monitor:${result.monitors[0].index}`
}
async function action(run: () => Promise<void>): Promise<void> {
  if (busy.value || catalogBusy.value || paused.value) return
  const mine = generation
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await run()
  } catch (e) {
    if (mine === generation && !disposed) error.value = errorText(e)
  } finally {
    if (mine === generation) busy.value = false
  }
}
async function load(): Promise<void> {
  await action(async () => {
    const mine = generation
    const status = await call<{ supported: boolean; helper: boolean }>('status')
    if (mine !== generation || paused.value || disposed) return
    catalogReady.value = true
    available.value = status.supported && status.helper
    if (available.value) await refreshSources()
    else error.value = t('capture.missing_helper')
  })
}
async function choose(id: string): Promise<void> {
  await action(async () => {
    const mine = generation
    const image = await call<CaptureImage>('asset', { id })
    if (mine !== generation || paused.value || disposed) return
    current.value = image
    selection.value = null
    frozenTarget.value = ''
    text.value = ''
    title.value = ''
  })
}
async function capture(operation: 'screenshot' | 'record' | 'scroll'): Promise<void> {
  if (!target.value || busy.value || catalogBusy.value || paused.value || disposed) return
  const mine = generation
  const source = selectedTarget.value
  const region = operation !== 'screenshot' && frozenTarget.value === source ? selection.value : null
  capturing.value = true
  await action(async () => {
    const result = await call<CaptureImage>('start', {
      request: {
        operation,
        target: target.value,
        delay_ms: Math.round(delay.value * 1000),
        seconds: Math.round(seconds.value),
        cursor: cursor.value,
        audio: operation === 'record' && audio.value && !gif.value,
        gif: operation === 'record' && gif.value,
        crop: region,
      },
    })
    if (mine !== generation || paused.value || disposed) return
    current.value = result
    selection.value = null
    frozenTarget.value = operation === 'screenshot' ? source : ''
    text.value = ''
    title.value = ''
    refreshCatalog()
  })
  if (mine === generation) capturing.value = false
}
const gif = computed(() => format.value === 'gif')
watch(format, () => {
  if (gif.value) audio.value = false
})
watch(selectedTarget, () => {
  selection.value = null
  frozenTarget.value = ''
})
async function stop(abort: boolean): Promise<void> {
  try {
    await call(abort ? 'abort' : 'stop')
  } catch (e) {
    error.value = errorText(e)
  }
}
async function edited(png: string): Promise<void> {
  const mine = generation
  await action(async () => {
    const image = await call<CaptureImage>('annotate', { png })
    if (mine !== generation || paused.value || disposed) return
    current.value = image
    selection.value = null
    frozenTarget.value = ''
    refreshCatalog()
  })
}
async function read(operation: 'ocr' | 'qr'): Promise<void> {
  if (!current.value) return
  const id = current.value.asset.id
  const mine = generation
  await action(async () => {
    const result = await call<{ text?: string; codes?: string[] }>(operation, { id })
    if (mine !== generation || paused.value || disposed || current.value?.asset.id !== id) return
    text.value = result.text ?? result.codes?.join('\n\n') ?? ''
    if (operation === 'qr' && !text.value) notice.value = t('capture.empty_codes')
  })
}
async function simple(method: string, extra: object = {}): Promise<void> {
  if (!current.value) return
  const id = current.value.asset.id
  const mine = generation
  await action(async () => {
    await call(method, { id, ...extra })
    if (mine !== generation || paused.value || disposed || current.value?.asset.id !== id) return
    if (method === 'copy_image' || method === 'copy_text') notice.value = t('capture.copied')
  })
}
async function exportFile(): Promise<void> {
  const image = current.value
  const mine = generation
  if (!image || busy.value || catalogBusy.value || paused.value) return
  const path = await save({ defaultPath: `Moodflow-capture.${image.asset.extension}` })
  if (!path || mine !== generation || paused.value || disposed || current.value?.asset.id !== image.asset.id)
    return
  await action(async () => {
    await call('export', { id: image.asset.id, path })
  })
}
async function saveLibrary(): Promise<void> {
  const image = current.value
  const name = title.value.trim()
  const mine = generation
  if (!image || !name) return
  await action(async () => {
    await call('save_library', { id: image.asset.id, title: name })
    if (mine !== generation || paused.value || disposed) return
    if (current.value?.asset.id === image.asset.id) current.value.asset.permanent = true
    notice.value = t('capture.library_saved')
    refreshCatalog()
  })
}
async function stage(): Promise<void> {
  if (!text.value.trim()) return
  const mine = generation
  await action(async () => {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text: text.value })))
    if (mine !== generation || paused.value || disposed) return
    notice.value = t('library.staged')
  })
}
async function remove(): Promise<void> {
  const image = current.value
  const mine = generation
  if (!image?.asset.permanent || busy.value || catalogBusy.value) return
  if (!(await confirm(t('capture.delete_saved_confirm'), { title: 'Moodflow', kind: 'warning' }))) return
  if (mine !== generation || paused.value || disposed || current.value?.asset.id !== image.asset.id) return
  await action(async () => {
    await call('remove', { id: image.asset.id, confirm_saved: true })
    if (mine !== generation || paused.value || disposed) return
    clearPreview()
    refreshCatalog()
  })
}
watch(paused, (value) => {
  generation++
  busy.value = false
  capturing.value = false
  catalogBusy.value = false
  catalogReady.value = false
  clearPreview()
  sources.value = { monitors: [], windows: [], ocr_available: false }
  error.value = ''
  notice.value = ''
  if (value) {
    void call('abort').catch(() => {})
    void call('release').catch(() => {})
  } else void load()
})
const heartbeat = setInterval(() => {
  if (current.value && !paused.value) void call('lease', { id: current.value.asset.id }).catch(() => {})
}, 10000)
async function canLeave(): Promise<boolean> {
  if (capturing.value && !(await confirm(t('capture.leave'), { title: 'Moodflow', kind: 'warning' })))
    return false
  await call('abort').catch(() => {})
  return true
}
onBeforeUnmount(() => {
  disposed = true
  generation++
  clearInterval(heartbeat)
  clearPreview()
  void call('abort').catch(() => {})
  void call('release').catch(() => {})
})
defineExpose({ canLeave })
</script>
<template>
  <section v-if="paused" class="empty">{{ t('capture.privacy') }}</section>
  <section v-else class="capture">
    <header>
      <span class="eyebrow">{{ t('capture.local') }}</span>
      <h1>{{ t('capture.title') }}</h1>
      <p>{{ t('capture.description') }}</p>
    </header>
    <fieldset :disabled="catalogBusy || busy || !available">
      <label
        >{{ t('capture.source')
        }}<select v-model="selectedTarget">
          <optgroup :label="t('capture.source')">
            <option
              v-for="monitor in sources.monitors"
              :key="monitor.index"
              :value="`monitor:${monitor.index}`"
            >
              {{ t('capture.monitor', { n: monitor.index, width: monitor.width, height: monitor.height }) }}
            </option>
          </optgroup>
          <optgroup :label="t('capture.window')">
            <option
              v-for="window in sources.windows"
              :key="window.hwnd"
              :value="`window:${window.hwnd}:${window.pid}`"
            >
              {{ window.title }}
            </option>
          </optgroup>
        </select></label
      >
      <button @click="action(refreshSources)">{{ t('capture.refresh_sources') }}</button>
      <label>{{ t('capture.delay') }}<input v-model.number="delay" type="number" min="0" max="10" /></label>
      <label><input v-model="cursor" type="checkbox" />{{ t('capture.cursor') }}</label>
      <div class="actions">
        <button :disabled="!target" @click="capture('screenshot')">{{ t('capture.shot') }}</button
        ><button :disabled="target?.kind !== 'window'" @click="capture('scroll')">
          {{ t('capture.scroll') }}
        </button>
      </div>
      <p>{{ t('capture.scroll_hint') }}</p>
      <label
        >{{ t('capture.format')
        }}<select v-model="format">
          <option value="mp4">{{ t('capture.mp4') }}</option>
          <option value="gif">{{ t('capture.gif') }}</option>
        </select></label
      >
      <label
        >{{ t('capture.seconds') }}<input v-model.number="seconds" type="number" min="1" max="120"
      /></label>
      <label><input v-model="audio" type="checkbox" :disabled="gif" />{{ t('capture.audio') }}</label>
      <button :disabled="!target" @click="capture('record')">{{ t('capture.record') }}</button>
    </fieldset>
    <p class="hint">{{ t('capture.limits') }}</p>
    <div v-if="capturing" class="actions" role="status">
      <span>{{ t('capture.working') }}</span
      ><button @click="stop(false)">{{ t('capture.stop') }}</button
      ><button @click="stop(true)">{{ t('capture.abort') }}</button>
    </div>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <div class="assets">
      <CaptureCatalog
        v-if="catalogReady"
        :request="call"
        :busy="busy"
        :active-id="current?.asset.id"
        :refresh="catalogRefresh"
        @working="catalogBusy = $event"
        @choose="choose"
        @changed="catalogChanged"
      />
      <article v-if="current">
        <CaptureEditor
          v-if="current.asset.extension === 'png'"
          :key="current.asset.id"
          :image="current"
          :busy="catalogBusy || busy"
          @save="edited"
          @selection="selection = $event"
        />
        <video v-else-if="current.asset.extension === 'mp4'" :src="imageUrl" controls preload="metadata" />
        <img v-else :src="imageUrl" :alt="t('capture.title')" />
        <div class="actions">
          <button
            :disabled="catalogBusy || busy || current.asset.extension !== 'png'"
            @click="simple('copy_image')"
          >
            {{ t('capture.copy') }}</button
          ><button :disabled="catalogBusy || busy" @click="exportFile">{{ t('capture.export') }}</button
          ><button
            :disabled="catalogBusy || busy || current.asset.extension === 'mp4'"
            @click="simple('pin')"
          >
            {{ t('capture.pin') }}</button
          ><button :disabled="catalogBusy || busy" @click="simple('unpin')">{{ t('capture.unpin') }}</button
          ><button :disabled="catalogBusy || busy" @click="simple('pin_passthrough', { enabled: false })">
            {{ t('capture.unlock_pin') }}</button
          ><button v-if="current.asset.permanent" :disabled="catalogBusy || busy" @click="remove">
            {{ t('capture.delete') }}
          </button>
        </div>
        <div class="actions">
          <label>{{ t('capture.library_title') }}<input v-model="title" maxlength="256" /></label
          ><button :disabled="catalogBusy || busy || !title.trim()" @click="saveLibrary">
            {{ t('capture.library') }}
          </button>
        </div>
        <div class="actions">
          <button
            :disabled="catalogBusy || busy || current.asset.extension === 'mp4' || !sources.ocr_available"
            @click="read('ocr')"
          >
            {{ t('capture.ocr') }}</button
          ><button :disabled="catalogBusy || busy || current.asset.extension === 'mp4'" @click="read('qr')">
            {{ t('capture.qr') }}
          </button>
        </div>
        <p v-if="!sources.ocr_available" class="hint">{{ t('capture.ocr_missing') }}</p>
        <label
          >{{ t('capture.text_result') }}<textarea v-model="text" maxlength="65536" spellcheck="false" />
        </label>
        <div class="actions">
          <button :disabled="catalogBusy || busy || !text" @click="simple('copy_text', { text })">
            {{ t('capture.copy_text') }}</button
          ><button :disabled="catalogBusy || busy || !text.trim()" @click="stage">
            {{ t('capture.stage') }}
          </button>
        </div>
      </article>
    </div>
  </section>
</template>
<style scoped>
.capture {
  display: grid;
  gap: var(--mf-sp-4);
}
fieldset,
.actions {
  display: flex;
  flex-wrap: wrap;
  align-items: end;
  gap: var(--mf-sp-2);
}
fieldset {
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-3);
}
fieldset p {
  flex-basis: 100%;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
input[type='number'] {
  width: 80px;
}
select {
  max-width: 320px;
}
.hint,
header p {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.error {
  color: var(--mf-danger);
}
.assets {
  display: grid;
  grid-template-columns: minmax(270px, 35%) minmax(0, 1fr);
  gap: var(--mf-sp-4);
}
article {
  display: grid;
  gap: var(--mf-sp-3);
  align-content: start;
  min-width: 0;
}
video,
img {
  max-width: 100%;
  max-height: 60vh;
}
textarea {
  width: 100%;
  min-height: 140px;
  resize: vertical;
}
@media (max-width: 800px) {
  .assets {
    grid-template-columns: 1fr;
  }
}
</style>
