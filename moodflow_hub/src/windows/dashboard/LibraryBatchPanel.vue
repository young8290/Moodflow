<script setup lang="ts">
import { computed, ref } from 'vue'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'

interface Entry {
  id: string
  title: string
  revision: string
}
interface ImportItem {
  index: number
  source: number
  id: string
  title: string
  kind: string
  tags: string[]
  status: string
  expected_revision: string | null
}
interface Preview {
  sources: { path: string; digest: string }[]
  items: ImportItem[]
  bytes: number
}
type Mode = 'skip' | 'copy' | 'create' | 'replace'
interface Outcome {
  index: number
  id: string | null
  status: string
}
const props = defineProps<{
  items: Entry[]
  disabled: boolean
  beforeChange: () => Promise<boolean>
}>()
const emit = defineEmits<{ changed: []; busy: [value: boolean] }>()
const expanded = ref(false)
const working = ref(false)
const preview = ref<Preview | null>(null)
const modes = ref<Record<number, Mode>>({})
const results = ref<Outcome[]>([])
const selection = ref<Record<string, string>>({})
const error = ref('')
const notice = ref('')
const selectedCount = computed(() => Object.keys(selection.value).length)
const pendingCount = computed(
  () =>
    preview.value?.items.filter((i) => modes.value[i.index] !== 'skip' && !succeeded(i.index)).length ?? 0,
)
const imported = computed(() => results.value.filter((r) => r.status === 'imported').length)
const failed = computed(
  () => results.value.filter((r) => r.status === 'conflict' || r.status === 'failed').length,
)
const locked = computed(() => props.disabled || working.value)
function copy(key: string): string {
  return t(`library.batch_${key}` as CopyKey)
}
function succeeded(index: number): boolean {
  return results.value.some((r) => r.index === index && r.status === 'imported')
}
function available(item: ImportItem): Mode[] {
  if (item.status === 'new') return ['create', 'copy', 'skip']
  if (item.status === 'conflict' || item.status === 'identical') return ['skip', 'copy', 'replace']
  return ['skip', 'copy']
}
async function call<T>(method: string, payload: Record<string, unknown>): Promise<T> {
  return JSON.parse(await unwrap(commands.libraryRequest(method, JSON.stringify(payload)))) as T
}
async function run(work: () => Promise<void>): Promise<void> {
  if (locked.value) return
  working.value = true
  emit('busy', true)
  error.value = ''
  notice.value = ''
  try {
    await work()
  } catch (e) {
    error.value = errorText(e)
  } finally {
    working.value = false
    emit('busy', false)
  }
}
async function browse(): Promise<void> {
  await run(async () => {
    const paths = await open({
      multiple: true,
      filters: [{ name: copy('files'), extensions: ['md', 'markdown', 'mflibrary'] }],
    })
    if (!paths || (Array.isArray(paths) && !paths.length)) return
    const value = await call<Preview>('batch_preview', { paths: typeof paths === 'string' ? [paths] : paths })
    preview.value = value
    results.value = []
    modes.value = Object.fromEntries(
      value.items.map((i) => [i.index, i.status === 'new' ? 'create' : 'skip']),
    )
  })
}
function clearPreview(): void {
  preview.value = null
  results.value = []
}
function setAll(mode: 'skip' | 'copy'): void {
  if (!preview.value) return
  for (const item of preview.value.items) if (!succeeded(item.index)) modes.value[item.index] = mode
}
async function apply(): Promise<void> {
  if (!preview.value || !pendingCount.value || locked.value) return
  if (!(await props.beforeChange())) return
  const replaces = preview.value.items.filter(
    (i) => modes.value[i.index] === 'replace' && !succeeded(i.index),
  ).length
  if (
    !(await confirm(t('library.batch_confirm', { count: pendingCount.value, replace: replaces }), {
      title: 'Moodflow',
      kind: 'warning',
    }))
  )
    return
  await run(async () => {
    const decisions = preview
      .value!.items.filter((i) => !succeeded(i.index))
      .map((i) => ({
        index: i.index,
        mode: modes.value[i.index],
        expected_revision: modes.value[i.index] === 'replace' ? i.expected_revision : null,
      }))
    const value = await call<Outcome[]>('batch_apply', { sources: preview.value!.sources, decisions })
    results.value = [...results.value.filter((r) => r.status === 'imported'), ...value]
    // Successful copies must not be retried; failed rows require a new preview.
    for (const result of value) modes.value[result.index] = 'skip'
    emit('changed')
  })
}
function toggle(entry: Entry, checked: boolean): void {
  if (checked && selectedCount.value >= 200) {
    notice.value = copy('limit')
    return
  }
  if (checked) selection.value[entry.id] = entry.revision
  else delete selection.value[entry.id]
}
function selectVisible(): void {
  selection.value = Object.fromEntries(props.items.slice(0, 200).map((i) => [i.id, i.revision]))
  if (props.items.length > 200) notice.value = copy('limit')
}
async function exportSelected(): Promise<void> {
  if (!selectedCount.value) return
  await run(async () => {
    const path = await save({
      defaultPath: 'Moodflow-library.mflibrary',
      filters: [{ name: copy('bundle'), extensions: ['mflibrary'] }],
    })
    if (!path) return
    const value = await call<{ exported: number }>('batch_export', {
      path,
      selection: Object.entries(selection.value).map(([id, revision]) => ({ id, revision })),
    })
    notice.value = t('library.batch_exported', { count: value.exported })
  })
}
</script>

<template>
  <section class="batch" :aria-busy="working">
    <button :aria-expanded="expanded" :disabled="locked" @click="expanded = !expanded">
      {{ copy('title') }}
    </button>
    <div v-if="expanded" class="batch-content">
      <p>{{ copy('intro') }}</p>
      <p role="note">{{ copy('references') }}</p>
      <p v-if="error" role="alert" class="error">{{ error }}</p>
      <p v-if="notice" role="status">{{ notice }}</p>
      <div class="actions">
        <button :disabled="locked" @click="browse">{{ copy('browse') }}</button>
        <button :disabled="locked || !preview" @click="clearPreview">
          {{ copy('clear_preview') }}
        </button>
      </div>
      <div v-if="preview" class="preview">
        <h2>{{ copy('preview') }}</h2>
        <p>{{ t('library.batch_summary', { count: preview.items.length, bytes: preview.bytes }) }}</p>
        <div class="actions">
          <button :disabled="locked" @click="setAll('skip')">{{ copy('skip_all') }}</button>
          <button :disabled="locked" @click="setAll('copy')">{{ copy('copy_all') }}</button>
        </div>
        <div class="table-scroll">
          <table>
            <thead>
              <tr>
                <th>{{ copy('entry') }}</th>
                <th>{{ copy('status') }}</th>
                <th>{{ copy('action') }}</th>
                <th>{{ copy('result') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in preview.items" :key="item.index">
                <td>
                  <strong>{{ item.title }}</strong
                  ><small>{{ item.tags.join(' · ') }}</small>
                </td>
                <td>{{ copy(item.status) }}</td>
                <td>
                  <select
                    v-model="modes[item.index]"
                    :aria-label="t('library.batch_row_action', { title: item.title })"
                    :disabled="locked || succeeded(item.index)"
                  >
                    <option v-for="mode in available(item)" :key="mode" :value="mode">
                      {{ copy(mode) }}
                    </option>
                  </select>
                </td>
                <td>
                  {{
                    results.find((r) => r.index === item.index)
                      ? copy(results.find((r) => r.index === item.index)!.status)
                      : ''
                  }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <p v-if="results.length" role="status">{{ t('library.batch_results', { imported, failed }) }}</p>
        <p>{{ copy('retry') }}</p>
        <button class="primary" :disabled="locked || !pendingCount" @click="apply">
          {{ t('library.batch_apply', { count: pendingCount }) }}
        </button>
      </div>
      <h2>{{ copy('export_title') }}</h2>
      <p>{{ copy('export_help') }}</p>
      <div class="actions">
        <button :disabled="locked" @click="selectVisible">{{ copy('select_visible') }}</button>
        <button :disabled="locked || !selectedCount" @click="selection = {}">
          {{ copy('clear_selection') }}
        </button>
        <button :disabled="locked || !selectedCount" @click="exportSelected">
          {{ t('library.batch_export', { count: selectedCount }) }}
        </button>
      </div>
      <div class="export-list">
        <label v-for="item in items" :key="item.id">
          <input
            type="checkbox"
            :checked="item.id in selection"
            :disabled="locked"
            @change="toggle(item, ($event.target as HTMLInputElement).checked)"
          />
          {{ item.title }}
        </label>
      </div>
    </div>
  </section>
</template>

<style scoped>
.batch {
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-3);
}
.batch-content {
  display: grid;
  gap: var(--mf-sp-3);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
}
h2 {
  font-size: var(--mf-fs-md);
  margin-bottom: 0;
}
p {
  color: var(--mf-text-2);
}
.table-scroll,
.export-list {
  max-height: 320px;
  overflow: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th,
td {
  text-align: left;
  padding: var(--mf-sp-2);
  border-bottom: 1px solid var(--mf-border);
  overflow-wrap: anywhere;
}
small {
  display: block;
  color: var(--mf-text-2);
}
.export-list {
  display: grid;
  gap: var(--mf-sp-2);
}
.export-list label {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: center;
}
.error {
  color: var(--mf-danger);
}
</style>
