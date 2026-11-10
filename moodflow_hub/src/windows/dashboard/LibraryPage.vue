<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { commands, events, unwrap } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
import CitationTools from './CitationTools.vue'

interface Metadata {
  capsule: string
  version: number
  id: string
  title: string
  tags: string[]
  updated_at: string
  [key: string]: unknown
}
interface Document {
  metadata: Metadata
  body: string
  revision: string
}
interface Summary {
  id: string
  title: string
  kind: string
  tags: string[]
  updated_at: string
  revision: string
}
interface Listing {
  items: Summary[]
  unreadable: string[]
}
const kinds = ['note', 'pdf', 'image', 'video', 'resource', 'skill', 'capture'] as const
const referenceFields = [
  'authors',
  'year',
  'container',
  'publisher',
  'volume',
  'issue',
  'pages',
  'doi',
  'url',
  'isbn',
] as const
const referenceTypes = ['book', 'journalArticle', 'conferencePaper', 'report'] as const
const query = ref('')
const kind = ref('')
const listing = ref<Listing>({ items: [], unreadable: [] })
const document = ref<Document | null>(null)
const baseline = ref('')
const dirty = computed(
  () => !!document.value && (!document.value.revision || JSON.stringify(document.value) !== baseline.value),
)
const tags = computed({
  get: () => document.value?.metadata.tags.join(', ') ?? '',
  set: (value: string) => {
    if (document.value)
      document.value.metadata.tags = value
        .split(/[,，]/)
        .map((s) => s.trim())
        .filter(Boolean)
  },
})
const busy = ref(false)
const error = ref('')
const notice = ref('')
let timer: ReturnType<typeof setTimeout> | undefined
let generation = 0
let unlisten: UnlistenFn | undefined
let disposed = false
let allowClose = false
const privacyPaused = ref(false)
let stopPrivacy: UnlistenFn | undefined

function label(value: string): string {
  return t(`library.${value}` as CopyKey)
}
async function call<T>(method: string, payload: Record<string, unknown> = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.libraryRequest(method, JSON.stringify(payload)))) as T
}
async function action(run: () => Promise<void>): Promise<void> {
  if (busy.value) return
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await run()
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}
async function refresh(reload = false): Promise<void> {
  const current = ++generation
  try {
    const value = await call<Listing>('list', { query: query.value, kind: kind.value, refresh: reload })
    if (current === generation && !disposed) listing.value = value
  } catch (e) {
    if (current === generation) error.value = errorText(e)
  }
}
function select(value: Document): void {
  document.value = value
  baseline.value = JSON.stringify(value)
}
async function canLeave(): Promise<boolean> {
  return !dirty.value || (await confirm(t('library.discard'), { title: 'Moodflow', kind: 'warning' }))
}
async function choose(id: string): Promise<void> {
  if (!(await canLeave())) return
  await action(async () => select(await call<Document>('get', { id })))
}
async function create(): Promise<void> {
  if (!(await canLeave())) return
  await action(async () =>
    select(await call<Document>('new', { kind: kind.value || 'note', title: t('library.untitled') })),
  )
}
async function persist(): Promise<void> {
  if (!document.value) return
  await action(async () => {
    const saved = await call<Document>('save', {
      document: document.value,
      expectedRevision: document.value!.revision || null,
    })
    select(saved)
    notice.value = t('library.saved')
    await refresh()
  })
}
async function duplicate(): Promise<void> {
  if (!document.value) return
  const old = structuredClone(JSON.parse(JSON.stringify(document.value)) as Document)
  await action(async () => {
    const fresh = await call<Document>('new', { kind: old.metadata.capsule, title: old.metadata.title })
    fresh.metadata = { ...old.metadata, id: fresh.metadata.id, updated_at: fresh.metadata.updated_at }
    fresh.body = old.body
    select(fresh)
  })
}
async function importMarkdown(): Promise<void> {
  if (!(await canLeave())) return
  const path = await open({
    multiple: false,
    filters: [{ name: 'Markdown', extensions: ['md', 'markdown'] }],
  })
  if (typeof path !== 'string') return
  await action(async () => select(await call<Document>('import', { path })))
}
async function exportMarkdown(): Promise<void> {
  if (!document.value || dirty.value) return
  const path = await save({
    defaultPath: `${document.value.metadata.id}.md`,
    filters: [{ name: 'Markdown', extensions: ['md'] }],
  })
  if (!path) return
  await action(async () => {
    await call('export', { id: document.value!.metadata.id, path })
    notice.value = t('library.saved')
  })
}
async function remove(): Promise<void> {
  if (
    !document.value?.revision ||
    !(await confirm(t('library.delete_question'), { title: 'Moodflow', kind: 'warning' }))
  )
    return
  await action(async () => {
    await call('delete', { id: document.value!.metadata.id, revision: document.value!.revision })
    document.value = null
    baseline.value = ''
    await refresh()
  })
}
async function stage(): Promise<void> {
  if (!document.value?.body) return
  await action(async () => {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text: document.value!.body })))
    notice.value = t('library.staged')
  })
}
async function chooseFile(): Promise<void> {
  const path = await open({ multiple: false })
  if (typeof path === 'string' && document.value) document.value.body = path
}
watch([query, kind], () => {
  clearTimeout(timer)
  timer = setTimeout(() => void refresh(), 200)
})
onMounted(async () => {
  try {
    let gotEvent = false
    const stop = await events.statusChanged.listen((event) => {
      gotEvent = true
      privacyPaused.value = event.payload.paused
      if (privacyPaused.value) generation++
    })
    if (disposed) {
      stop()
      return
    }
    stopPrivacy = stop
    const status = await unwrap(commands.getStatus())
    if (!gotEvent) privacyPaused.value = status.paused
  } catch {
    /* Preview has no native status feed. Native reads still enforce privacy. */
  }
  if (!privacyPaused.value) await refresh(true)
  try {
    const window = getCurrentWindow()
    const stop = await window.onCloseRequested(async (event) => {
      if (!dirty.value || allowClose) return
      event.preventDefault()
      if (await canLeave()) {
        allowClose = true
        await window.close()
      }
    })
    if (disposed) stop()
    else unlisten = stop
  } catch {
    /* Preview has no native window. */
  }
})
onBeforeUnmount(() => {
  disposed = true
  generation++
  clearTimeout(timer)
  unlisten?.()
  stopPrivacy?.()
})
defineExpose({ canLeave })
</script>

<template>
  <p v-if="privacyPaused" role="status">{{ t('library.privacy') }}</p>
  <section v-else class="library" :aria-busy="busy">
    <header class="library-head">
      <div>
        <h1>{{ t('library.title') }}</h1>
        <p>{{ t('library.intro') }}</p>
      </div>
      <div class="actions">
        <button :disabled="busy" @click="importMarkdown">{{ t('library.import') }}</button
        ><button :disabled="busy" @click="refresh(true)">{{ t('library.refresh') }}</button
        ><button class="primary" :disabled="busy" @click="create">{{ t('library.new') }}</button>
      </div>
    </header>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="listing.unreadable.length" class="error" role="status">
      {{ t('library.unreadable', { count: listing.unreadable.length }) }}
    </p>
    <nav class="filters" :aria-label="t('library.title')">
      <button :aria-pressed="kind === ''" @click="kind = ''">{{ t('library.all') }}</button
      ><button v-for="k in kinds" :key="k" :aria-pressed="kind === k" @click="kind = k">
        {{ label(k) }}
      </button>
    </nav>
    <input
      v-model="query"
      class="search"
      type="search"
      :placeholder="t('library.search')"
      :aria-label="t('library.search')"
      maxlength="1024"
    />
    <div class="workspace">
      <aside class="entries">
        <p v-if="!listing.items.length" class="empty">{{ t('library.empty') }}</p>
        <button
          v-for="item in listing.items"
          :key="item.id"
          class="entry"
          :class="{ selected: document?.metadata.id === item.id }"
          :disabled="busy"
          @click="choose(item.id)"
        >
          <small>{{ label(item.kind) }}</small
          ><strong>{{ item.title }}</strong
          ><span>{{ item.tags.join(' · ') }}</span>
        </button>
      </aside>
      <article v-if="document" class="editor">
        <div class="editor-toolbar">
          <select v-model="document.metadata.capsule" :aria-label="t('library.title')" :disabled="busy">
            <option v-for="k in kinds" :key="k" :value="k">{{ label(k) }}</option></select
          ><span v-if="dirty" class="dirty">{{ t('library.dirty') }}</span
          ><button :disabled="busy || !dirty" class="primary" @click="persist">
            {{ t('library.save') }}
          </button>
        </div>
        <input
          v-model="document.metadata.title"
          class="title"
          :aria-label="t('library.title_field')"
          maxlength="256"
          :disabled="busy"
        />
        <label>{{ t('library.tags') }}<input v-model="tags" :disabled="busy" /></label>
        <div v-if="document.metadata.capsule === 'pdf'" class="reference-fields">
          <label
            >{{ t('library.reference_type')
            }}<select v-model="document.metadata.reference_type" :disabled="busy">
              <option v-for="type in referenceTypes" :key="type" :value="type">{{ label(type) }}</option>
            </select></label
          ><label v-for="field in referenceFields" :key="field"
            >{{ label(`reference_${field}`)
            }}<input v-model="document.metadata[`reference_${field}`]" :disabled="busy"
          /></label>
        </div>
        <CitationTools
          v-if="document.metadata.capsule === 'pdf'"
          :key="document.metadata.id"
          :metadata="document.metadata"
          @apply="Object.assign(document!.metadata, $event)"
        />
        <label v-if="['note', 'skill'].includes(document.metadata.capsule)" class="body-label"
          >{{ t('library.body') }}<textarea v-model="document.body" spellcheck="false" :disabled="busy" />
        </label>
        <label v-else
          >{{ t('library.path') }}<input v-model="document.body" :disabled="busy" /><button
            :disabled="busy"
            @click="chooseFile"
          >
            {{ t('library.choose_file') }}
          </button></label
        >
        <footer class="actions">
          <button
            v-if="['note', 'skill'].includes(document.metadata.capsule)"
            :disabled="busy || !document.body"
            @click="stage"
          >
            {{ t('library.stage') }}</button
          ><button :disabled="busy" @click="duplicate">{{ t('library.duplicate') }}</button
          ><button :disabled="busy || dirty" @click="exportMarkdown">{{ t('library.export') }}</button
          ><button :disabled="busy || !document.revision" class="delete" @click="remove">
            {{ t('library.delete') }}
          </button>
        </footer>
      </article>
      <div v-else class="empty editor-empty">{{ t('library.select') }}</div>
    </div>
  </section>
</template>

<style scoped>
.library {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-3);
  min-height: 100%;
}
.library-head,
.actions,
.editor-toolbar,
.filters {
  display: flex;
  align-items: center;
  gap: var(--mf-sp-2);
  flex-wrap: wrap;
}
.library-head {
  justify-content: space-between;
}
h1 {
  margin: 0;
  font-family: Georgia, 'Noto Serif SC', 'SimSun', serif;
}
.library-head p {
  margin-bottom: 0;
  color: var(--mf-text-2);
}
.filters button {
  border-color: transparent;
  background: transparent;
}
.filters button[aria-pressed='true'] {
  border-color: var(--mf-primary);
  background: var(--mf-surface);
}
.search {
  width: 100%;
}
.workspace {
  display: grid;
  grid-template-columns: minmax(160px, 230px) minmax(0, 1fr);
  flex: 1;
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  overflow: hidden;
}
.entries {
  border-right: 1px solid var(--mf-border);
  background: var(--mf-surface-2);
  padding: var(--mf-sp-2);
  overflow: auto;
  max-height: 72vh;
}
.entry {
  display: grid;
  width: 100%;
  gap: var(--mf-sp-1);
  padding: var(--mf-sp-3);
  text-align: left;
  border-color: transparent;
  background: transparent;
}
.entry.selected {
  border-color: var(--mf-border);
  background: var(--mf-surface);
}
.entry small,
.entry span {
  color: var(--mf-text-2);
}
.editor {
  padding: var(--mf-sp-4);
  background: var(--mf-surface);
  min-width: 0;
}
.editor-toolbar {
  margin-bottom: var(--mf-sp-3);
}
.editor-toolbar .primary {
  margin-left: auto;
}
.title {
  font-family: Georgia, 'Noto Serif SC', 'SimSun', serif;
  font-size: var(--mf-fs-xl);
  width: 100%;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
  margin-block: var(--mf-sp-3);
}
textarea {
  min-height: 280px;
  resize: vertical;
  font-family: Consolas, monospace;
  line-height: 1.7;
}
.reference-fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0 var(--mf-sp-3);
}
.reference-fields input,
.reference-fields select {
  min-width: 0;
}
.empty {
  padding: var(--mf-sp-4);
  color: var(--mf-text-3);
}
.editor-empty {
  display: grid;
  place-items: center;
}
.primary {
  background: var(--mf-primary);
  color: var(--mf-on-primary);
}
.error,
.delete {
  color: var(--mf-danger);
}
.dirty {
  color: var(--mf-link);
  font-size: var(--mf-fs-xs);
}
@media (max-width: 760px) {
  .workspace {
    grid-template-columns: 150px minmax(0, 1fr);
  }
  .reference-fields {
    grid-template-columns: 1fr;
  }
}
</style>
