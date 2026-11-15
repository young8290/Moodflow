<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { confirm, save } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { t, errorText } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import MappingImport from './MappingImport.vue'
import {
  mappingKey,
  mappingRow,
  type MappingChange,
  type MappingPage,
  type MappingPreview,
  type MappingRow,
} from './mappingTypes'

const paused = useToolPrivacy()
const busy = ref(false)
const error = ref('')
const status = ref('')
const search = ref('')
const kind = ref('all')
const data = ref<MappingPage | null>(null)
const lookup = ref('')
const symbol = ref('')
const before = ref<MappingRow | null>(null)
const draft = ref<MappingRow | null>(null)
const preview = ref<MappingPreview | null>(null)
const checked = ref(false)
const showImport = ref(false)
let generation = 0
let alive = true

const dirty = computed(() => !!draft.value && JSON.stringify(draft.value) !== JSON.stringify(before.value))
const hiddenKeys = computed(() => new Set(draft.value?.hidden.map(mappingKey)))
const builtins = computed(() => preview.value?.builtin ?? [])
const blocked = computed(() => busy.value || paused.value)
function valid(mine: number) {
  return alive && !paused.value && mine === generation
}
async function request<T>(method: string, params: object = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.imeTools(`mapping.${method}`, JSON.stringify(params)))) as T
}
function message(e: unknown) {
  if (typeof e === 'object' && e && 'ui' in e) {
    const code = (e.ui as { code?: string })?.code
    if (code?.startsWith('mapping.import_line:'))
      return t('mapping.import_line', { line: code.split(':')[1] ?? '' })
  }
  return errorText(e)
}
async function run(action: (mine: number) => Promise<void>) {
  if (blocked.value) return
  busy.value = true
  error.value = ''
  status.value = ''
  const mine = generation
  try {
    await action(mine)
  } catch (e) {
    if (valid(mine)) error.value = message(e)
  } finally {
    if (valid(mine)) busy.value = false
  }
}
async function load(mine: number, offset = data.value?.offset ?? 0) {
  const page = await request<MappingPage>('list', { search: search.value.trim(), kind: kind.value, offset })
  if (valid(mine)) data.value = { ...page, rows: page.rows.map(mappingRow) }
}
function browse(offset: number) {
  void run((mine) => load(mine, offset))
}
async function openRow(mine: number, word: string) {
  const reply = await request<{ row: MappingRow; preview: MappingPreview }>('get', { word })
  if (!valid(mine)) return
  before.value = mappingRow(reply.row)
  draft.value = mappingRow(reply.row)
  lookup.value = reply.row.word
  symbol.value = ''
  preview.value = reply.preview
  checked.value = true
}
function open(word = lookup.value.trim()) {
  if (dirty.value || !word) return
  void run((mine) => openRow(mine, word))
}
function reset() {
  before.value = null
  draft.value = null
  preview.value = null
  checked.value = false
  symbol.value = ''
}
function discard() {
  if (!before.value) return
  draft.value = mappingRow(before.value)
  checked.value = false
  error.value = ''
}
function edit(action: (row: MappingRow) => void) {
  if (!draft.value || blocked.value) return
  action(draft.value)
  checked.value = false
  error.value = ''
  status.value = ''
}
function add() {
  const value = symbol.value.trim()
  if (!value || !draft.value) return
  const key = mappingKey(value)
  if (!key || [...value].length > 16 || /[\s\p{Cc}]/u.test(value)) {
    error.value = t('mapping.invalid')
    return
  }
  if (key === mappingKey(draft.value.word) || draft.value.added.some((item) => mappingKey(item) === key)) {
    error.value = t(key === mappingKey(draft.value.word) ? 'mapping.same_as_host' : 'mapping.duplicate')
    return
  }
  if (draft.value.added.length >= 8) {
    error.value = t('mapping.limit')
    return
  }
  edit((row) => row.added.push(value))
  symbol.value = ''
}
function shift(index: number, delta: number) {
  edit((row) => {
    const to = index + delta
    if (to < 0 || to >= row.added.length) return
    const item = row.added.splice(index, 1)[0]!
    row.added.splice(to, 0, item)
  })
}
function toggleHidden(value: string) {
  const key = mappingKey(value)
  if (!hiddenKeys.value.has(key) && (draft.value?.hidden.length ?? 0) >= 16) {
    error.value = t('mapping.limit')
    return
  }
  edit((row) => {
    if (hiddenKeys.value.has(key)) row.hidden = row.hidden.filter((item) => mappingKey(item) !== key)
    else row.hidden.push(value)
  })
}
function checkDraft() {
  if (!draft.value) return
  const row = mappingRow(draft.value)
  void run(async (mine) => {
    const reply = await request<MappingPreview>('preview', row)
    if (valid(mine)) {
      preview.value = reply
      checked.value = true
    }
  })
}
function enableSuggestions() {
  if (!draft.value || !preview.value) return
  const row = mappingRow(draft.value)
  const items: { key: string; value: boolean | string }[] = [{ key: 'input.emoji.enabled', value: true }]
  if (preview.value.scope === 'off' && preview.value.show_as !== 'comment') {
    items.push({ key: 'input.emoji.scope', value: 'exact' })
  }
  void run(async (mine) => {
    const result = await unwrap(commands.imeConfigSet(items))
    if (!valid(mine)) return
    const reply = await request<MappingPreview>('preview', row)
    if (!valid(mine)) return
    preview.value = reply
    checked.value = true
    if (result.skipped.length || !reply.enabled || (reply.scope === 'off' && reply.show_as !== 'comment')) {
      error.value = t('mapping.enable_failed')
    }
  })
}
function saveDraft() {
  if (!draft.value || !before.value || !checked.value || !dirty.value) return
  const edits = [{ before: mappingRow(before.value), after: mappingRow(draft.value) }]
  void run(async (mine) => {
    await request('apply', { edits })
    if (!valid(mine)) return
    before.value = mappingRow(edits[0]!.after)
    status.value = t('mapping.saved')
    await load(mine)
  })
}
async function importChanges(edits: MappingChange[]) {
  // MappingImport owns the import dialog state, but shares the same privacy generation.
  const mine = generation
  if (!valid(mine) || busy.value || dirty.value) return false
  await request('apply', { edits })
  if (!valid(mine)) return false
  reset()
  showImport.value = false
  status.value = t('mapping.imported', { count: edits.length })
  try {
    await load(mine, 0)
  } catch (e) {
    if (valid(mine)) error.value = message(e)
  }
  return true
}
function exportFile() {
  void run(async (mine) => {
    const path = await save({
      defaultPath: 'Moodflow-mappings.jsonl',
      filters: [{ name: 'JSONL', extensions: ['jsonl'] }],
    })
    if (!path || !valid(mine)) return
    await request('exportFile', { path })
    if (valid(mine)) status.value = t('mapping.exported')
  })
}
async function canLeave() {
  return !dirty.value || confirm(t('mapping.discard_confirm'), { title: 'Moodflow', kind: 'warning' })
}
watch(paused, (closed) => {
  generation++
  busy.value = false
  error.value = ''
  status.value = ''
  if (closed) {
    data.value = null
    search.value = ''
    lookup.value = ''
    showImport.value = false
    reset()
  } else browse(0)
})
onBeforeUnmount(() => {
  alive = false
  generation++
})
defineExpose({ canLeave })
</script>

<template>
  <p v-if="paused" role="status">{{ t('library.privacy') }}</p>
  <section v-else class="mapping">
    <h1>{{ t('mapping.title') }}</h1>
    <p class="hint">{{ t('mapping.intro') }}</p>
    <p v-if="error" class="d-error" role="alert">{{ error }}</p>
    <p v-if="status" role="status">{{ status }}</p>
    <div v-if="!showImport" class="d-row">
      <button :disabled="blocked || dirty" :aria-expanded="showImport" @click="showImport = true">
        {{ t('mapping.import') }}
      </button>
      <button :disabled="blocked" @click="exportFile">{{ t('mapping.export') }}</button>
    </div>
    <MappingImport
      v-if="showImport"
      :request="request"
      :apply="importChanges"
      :message="message"
      @close="showImport = false"
    />
    <div v-else class="layout">
      <section class="d-card list">
        <h2>{{ t('mapping.list') }}</h2>
        <form class="stack" @submit.prevent="browse(0)">
          <label
            >{{ t('mapping.search')
            }}<input v-model="search" type="search" maxlength="32" :disabled="blocked"
          /></label>
          <label
            >{{ t('mapping.filter') }}
            <select v-model="kind" :disabled="blocked">
              <option value="all">{{ t('mapping.filter_all') }}</option>
              <option value="added">{{ t('mapping.added') }}</option>
              <option value="hidden">{{ t('mapping.hidden') }}</option>
            </select>
          </label>
          <button :disabled="blocked">{{ t('mapping.find') }}</button>
        </form>
        <p v-if="data && !data.total" class="hint">{{ t('mapping.no_rows') }}</p>
        <ul v-if="data?.total" class="rows">
          <li v-for="row in data.rows" :key="row.word">
            <button
              :disabled="blocked || dirty"
              :aria-pressed="draft?.word === row.word"
              @click="open(row.word)"
            >
              <strong>{{ row.word }}</strong>
              <small>{{
                t('mapping.row_counts', { added: row.added.length, hidden: row.hidden.length })
              }}</small>
            </button>
          </li>
        </ul>
        <nav v-if="data && data.total > data.limit" class="d-row" :aria-label="t('mapping.pages')">
          <button :disabled="blocked || !data.offset" @click="browse(data.offset - data.limit)">
            {{ t('mapping.previous') }}
          </button>
          <span>{{
            t('mapping.range', {
              first: data.offset + 1,
              last: data.offset + data.rows.length,
              total: data.total,
            })
          }}</span>
          <button
            :disabled="blocked || data.offset + data.rows.length >= data.total"
            @click="browse(data.offset + data.limit)"
          >
            {{ t('mapping.next') }}
          </button>
        </nav>
      </section>
      <section class="d-card stack">
        <h2>{{ t('mapping.edit') }}</h2>
        <form class="d-row" @submit.prevent="open()">
          <label
            >{{ t('mapping.word') }}<input v-model="lookup" maxlength="64" :disabled="blocked || dirty"
          /></label>
          <button :disabled="blocked || dirty || !lookup.trim()">{{ t('mapping.open') }}</button>
        </form>
        <template v-if="draft">
          <h3>{{ draft.word }}</h3>
          <form class="d-row" @submit.prevent="add">
            <label
              >{{ t('mapping.symbol') }}<input v-model="symbol" maxlength="32" :disabled="blocked"
            /></label>
            <button type="submit" :disabled="blocked || !symbol.trim()">{{ t('mapping.add') }}</button>
          </form>
          <h3>{{ t('mapping.added') }}</h3>
          <ol v-if="draft.added.length" class="symbols">
            <li v-for="(item, index) in draft.added" :key="index" class="d-row">
              <strong>{{ item }}</strong>
              <button
                :disabled="blocked || !index"
                :aria-label="t('mapping.move_up_for', { item })"
                @click="shift(index, -1)"
              >
                {{ t('mapping.up') }}
              </button>
              <button
                :disabled="blocked || index + 1 >= draft.added.length"
                :aria-label="t('mapping.move_down_for', { item })"
                @click="shift(index, 1)"
              >
                {{ t('mapping.down') }}
              </button>
              <button :disabled="blocked" @click="toggleHidden(item)">
                {{ t(hiddenKeys.has(mappingKey(item)) ? 'mapping.show' : 'mapping.hide') }}
              </button>
              <button
                :disabled="blocked"
                :aria-label="t('mapping.remove_for', { item })"
                @click="edit((row) => row.added.splice(index, 1))"
              >
                {{ t('mapping.remove') }}
              </button>
            </li>
          </ol>
          <p v-else class="hint">{{ t('mapping.no_symbols') }}</p>

          <div v-if="builtins.length" class="stack">
            <h3>{{ t('mapping.builtin') }}</h3>
            <ul class="chips">
              <li v-for="item in builtins" :key="item">
                <button
                  :disabled="blocked"
                  :aria-pressed="hiddenKeys.has(mappingKey(item))"
                  :aria-label="
                    t(hiddenKeys.has(mappingKey(item)) ? 'mapping.show_for' : 'mapping.hide_for', { item })
                  "
                  @click="toggleHidden(item)"
                >
                  {{ item }}
                </button>
              </li>
            </ul>
          </div>
          <div v-if="draft.hidden.length" class="stack">
            <h3>{{ t('mapping.hidden') }}</h3>
            <ul class="chips">
              <li v-for="item in draft.hidden" :key="item">
                <button
                  :disabled="blocked"
                  :aria-label="t('mapping.show_for', { item })"
                  @click="toggleHidden(item)"
                >
                  {{ item }}
                </button>
              </li>
            </ul>
          </div>

          <h3>{{ t('mapping.effective') }}</h3>
          <p v-if="!checked" class="hint">{{ t('mapping.needs_preview') }}</p>
          <p
            v-else-if="
              preview && (!preview.enabled || (preview.scope === 'off' && preview.show_as !== 'comment'))
            "
            class="hint"
          >
            {{ t('mapping.disabled') }}
          </p>
          <p v-else-if="preview?.effective.length" class="effective">
            {{ preview.effective.join('\u3000') }}
          </p>
          <p v-else class="hint">{{ t('mapping.no_symbols') }}</p>
          <button
            v-if="
              checked &&
              preview &&
              (!preview.enabled || (preview.scope === 'off' && preview.show_as !== 'comment'))
            "
            :disabled="blocked"
            @click="enableSuggestions"
          >
            {{
              t(
                preview.scope === 'off' && preview.show_as !== 'comment'
                  ? 'mapping.enable_scope'
                  : 'mapping.enable',
              )
            }}
          </button>
          <p v-if="preview?.show_as === 'comment'" class="hint">{{ t('mapping.comment_mode') }}</p>
          <div class="d-row">
            <button :disabled="blocked" @click="checkDraft">{{ t('mapping.preview') }}</button>
            <button :disabled="blocked || !checked || !dirty" @click="saveDraft">
              {{ t('mapping.save') }}
            </button>
            <button v-if="dirty" :disabled="blocked" @click="discard">{{ t('mapping.discard') }}</button>
          </div>
        </template>
      </section>
    </div>
  </section>
</template>

<style scoped>
.mapping,
.stack,
.list {
  display: grid;
  gap: var(--mf-sp-3);
  align-content: start;
}
.mapping h1,
.mapping h2,
.mapping h3,
.mapping p {
  margin: 0;
}
.hint,
small {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.layout {
  display: grid;
  grid-template-columns: minmax(12rem, 1fr) minmax(0, 2.5fr);
  gap: var(--mf-sp-3);
  align-items: start;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
input,
select {
  min-width: 0;
}
.rows,
.chips {
  margin: 0;
  padding: 0;
  list-style: none;
}
.rows button {
  display: grid;
  text-align: start;
  width: 100%;
  overflow-wrap: anywhere;
}
.rows li + li {
  margin-top: var(--mf-sp-1);
}
.symbols {
  margin: 0;
  padding-inline-start: var(--mf-sp-4);
}
.symbols li {
  display: list-item;
  padding-block: var(--mf-sp-1);
}
.symbols button {
  margin-inline-start: var(--mf-sp-1);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-1);
}
.effective {
  overflow-wrap: anywhere;
  font-size: var(--mf-fs-lg);
}
[aria-pressed='true'] {
  border-color: var(--mf-link);
}
@media (max-width: 730px) {
  .layout {
    grid-template-columns: 1fr;
  }
}
</style>
