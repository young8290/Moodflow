<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { errorText, t, type CopyKey } from '@/i18n'
import { CAPTURE_BATCH_LIMIT } from './captureTypes'
import type {
  CaptureBatchReport,
  CaptureCatalogPage,
  CaptureOperation,
  CaptureOutcome,
  CaptureRow,
  CaptureScope,
  CaptureSelection,
} from './captureTypes'

const props = defineProps<{
  request: <T>(method: string, params?: object) => Promise<T>
  busy: boolean
  activeId?: string
  refresh: number
}>()
const emit = defineEmits<{
  choose: [id: string]
  changed: [ids: string[], operation: CaptureOperation | 'rename']
  working: [active: boolean]
}>()
const search = ref('')
const scope = ref<CaptureScope>('all')
const format = ref('')
const days = ref('')
const sort = ref('newest')
const offset = ref(0)
const page = ref<CaptureCatalogPage | null>(null)
const selected = ref<CaptureSelection[]>([])
const loading = ref(false)
const working = ref(false)
const error = ref('')
const pending = ref<{ operation: CaptureOperation; report: CaptureBatchReport; remaining: number } | null>(
  null,
)
const result = ref<CaptureBatchReport | null>(null)
const editing = ref<CaptureRow | null>(null)
const name = ref('')
const disabled = computed(() => props.busy || working.value || loading.value)
const pagePicked = computed(() => page.value?.rows.filter((row) => isSelected(row.asset.id)).length ?? 0)
const selectedCount = computed(() => selected.value.length)
const operation = ref<CaptureOperation>('keep')
const scopes: CaptureScope[] = ['all', 'temporary', 'kept', 'library', 'trash']
const actions = computed<CaptureOperation[]>(() =>
  scope.value === 'trash' ? ['restore', 'purge'] : ['keep', 'release', 'trash', 'export'],
)
const scopeLabels = {
  all: 'capture.catalog_all',
  temporary: 'capture.catalog_temporary',
  kept: 'capture.catalog_kept',
  library: 'capture.catalog_library',
  trash: 'capture.catalog_trash',
} satisfies Record<CaptureScope, CopyKey>
const actionLabels = {
  keep: 'capture.catalog_keep',
  release: 'capture.catalog_release',
  trash: 'capture.catalog_move_trash',
  restore: 'capture.catalog_restore',
  purge: 'capture.catalog_purge',
  export: 'capture.catalog_export',
} satisfies Record<CaptureOperation, CopyKey>
const outcomeLabels = {
  ready: 'capture.catalog_ready',
  done: 'capture.catalog_done',
  missing: 'capture.missing',
  stale: 'capture.catalog_stale',
  in_use: 'capture.in_use',
  wrong_state: 'capture.catalog_wrong_state',
  changed_file: 'capture.changed',
  failed: 'capture.catalog_failed',
  destination_exists: 'capture.catalog_destination_exists',
  export_limit: 'capture.catalog_export_limit',
  paused: 'capture.privacy',
} satisfies Record<CaptureOutcome, CopyKey>

let loadGeneration = 0
let alive = true
let searchTimer: ReturnType<typeof setTimeout> | undefined

function bytes(n: number): string {
  const exponent = n >= 1024 ** 3 ? 3 : n >= 1024 ** 2 ? 2 : n >= 1024 ? 1 : 0
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(n / 1024 ** exponent)} ${['B', 'KiB', 'MiB', 'GiB'][exponent]}`
}
function shortId(id: string) {
  return id.slice(0, 8)
}
function displayName(name: string | undefined, created: number) {
  return name || new Date(created * 1000).toLocaleString()
}
function rowTitle(row: CaptureRow) {
  return displayName(row.asset.name, row.asset.created_at)
}
function isSelected(id: string) {
  return selected.value.some((row) => row.id === id)
}
function selection(row: CaptureRow): CaptureSelection {
  return { id: row.asset.id, revision: row.revision }
}
function toggle(row: CaptureRow) {
  if (isSelected(row.asset.id)) selected.value = selected.value.filter((item) => item.id !== row.asset.id)
  else if (selectedCount.value < CAPTURE_BATCH_LIMIT) selected.value = [...selected.value, selection(row)]
}
function togglePage() {
  const rows = page.value?.rows ?? []
  if (pagePicked.value === rows.length) {
    const ids = new Set(rows.map((row) => row.asset.id))
    selected.value = selected.value.filter((item) => !ids.has(item.id))
  } else {
    selected.value = [
      ...selected.value,
      ...rows.filter((row) => !isSelected(row.asset.id)).map(selection),
    ].slice(0, CAPTURE_BATCH_LIMIT)
  }
}
async function load(next = offset.value, preserveError = false) {
  clearTimeout(searchTimer)
  const mine = ++loadGeneration
  loading.value = true
  if (!preserveError) error.value = ''
  try {
    const nextPage = await props.request<CaptureCatalogPage>('catalog_query', {
      search: search.value,
      scope: scope.value,
      format: format.value || null,
      days: Number(days.value) || null,
      sort: sort.value,
      offset: next,
    })
    if (alive && mine === loadGeneration) {
      page.value = nextPage
      offset.value = nextPage.offset
    }
  } catch (e) {
    if (alive && mine === loadGeneration) {
      page.value = null
      error.value = errorText(e)
    }
  } finally {
    if (alive && mine === loadGeneration) loading.value = false
  }
}
function resetScope() {
  selected.value = []
  pending.value = null
  editing.value = null
  result.value = null
  if (!actions.value.includes(operation.value)) operation.value = actions.value[0]!
}
watch(search, () => {
  clearTimeout(searchTimer)
  ++loadGeneration
  loading.value = true
  searchTimer = setTimeout(() => void load(0), 200)
})
watch([scope, format, days, sort], (_, before) => {
  if (scope.value !== before[0]) resetScope()
  void load(0)
})
watch(
  () => props.refresh,
  () => void load(),
)
onMounted(() => void load())
onBeforeUnmount(() => {
  alive = false
  clearTimeout(searchTimer)
  if (working.value) emit('working', false)
})

async function run(action: () => Promise<void>) {
  if (disabled.value) return
  working.value = true
  emit('working', true)
  error.value = ''
  try {
    await action()
  } catch (e) {
    if (alive) error.value = errorText(e)
  } finally {
    if (alive) {
      working.value = false
      emit('working', false)
    }
  }
}
function startRename(row: CaptureRow) {
  editing.value = row
  name.value = row.asset.name ?? ''
  pending.value = null
  result.value = null
}
async function rename() {
  const row = editing.value
  if (!row) return
  await run(async () => {
    try {
      await props.request('catalog_rename', { item: selection(row), name: name.value })
    } catch (e) {
      if (
        alive &&
        typeof e === 'object' &&
        e !== null &&
        'message_key' in e &&
        e.message_key === 'capture.catalog_stale'
      ) {
        editing.value = null
        await load()
      }
      throw e
    }
    if (!alive) return
    editing.value = null
    selected.value = selected.value.filter((item) => item.id !== row.asset.id)
    emit('changed', [row.asset.id], 'rename')
    await load()
  })
}
async function preview(expired = false) {
  editing.value = null
  result.value = null
  pending.value = null
  await run(async () => {
    if (expired) {
      const plan = await props.request<{ report: CaptureBatchReport; remaining: number }>('catalog_expired')
      if (alive) pending.value = { operation: 'trash', ...plan }
    } else {
      const op = operation.value
      const report = await props.request<CaptureBatchReport>('catalog_preview', {
        operation: op,
        items: selected.value,
      })
      if (alive) pending.value = { operation: op, report, remaining: 0 }
    }
  })
}
async function apply() {
  const plan = pending.value
  if (!plan?.report.ready) return
  await run(async () => {
    let directory: string | undefined
    if (plan.operation === 'export') {
      const destination = await open({ directory: true, multiple: false })
      if (!alive || typeof destination !== 'string') return
      directory = destination
    }
    // Keep the preview's exact revisions. The backend checks them again before applying.
    const items = plan.report.entries
      .filter((entry) => entry.status === 'ready')
      .map(({ id, revision }) => ({ id, revision }))
    const report = await props.request<CaptureBatchReport>('catalog_apply', {
      operation: plan.operation,
      items,
      ...(directory ? { directory } : {}),
    })
    if (!alive) return
    const outcomes = new Map(report.entries.map((entry) => [entry.id, entry]))
    const ids = report.entries.filter((entry) => entry.status === 'done').map((entry) => entry.id)
    result.value = { ...report, entries: plan.report.entries.map((entry) => outcomes.get(entry.id) ?? entry) }
    pending.value = null
    selected.value = []
    emit('changed', ids, plan.operation)
    await load()
  })
}
</script>

<template>
  <aside class="catalog" :aria-label="t('capture.history')">
    <h2>{{ t('capture.history') }}</h2>
    <p class="hint">{{ t('capture.retention') }}</p>
    <details v-if="page" class="usage">
      <summary>
        {{
          t('capture.catalog_usage', {
            used: bytes(page.storage.total.bytes),
            quota: bytes(page.storage.quota_bytes),
          })
        }}
      </summary>
      <progress
        :value="page.storage.total.bytes"
        :max="page.storage.quota_bytes"
        :aria-label="t('capture.catalog_progress')"
      />
      <dl>
        <template v-for="part in ['temporary', 'kept', 'library', 'trash'] as const" :key="part">
          <dt>{{ t(scopeLabels[part]) }}</dt>
          <dd>{{ page.storage[part].count }} · {{ bytes(page.storage[part].bytes) }}</dd>
        </template>
        <template v-for="part in ['png', 'gif', 'mp4'] as const" :key="part">
          <dt>{{ part.toUpperCase() }}</dt>
          <dd>{{ page.storage[part].count }} · {{ bytes(page.storage[part].bytes) }}</dd>
        </template>
      </dl>
      <button :disabled="disabled || page.storage.expired.count === 0" @click="preview(true)">
        {{ t('capture.catalog_expired', { count: page.storage.expired.count }) }}
      </button>
    </details>
    <div class="filters">
      <input
        v-model="search"
        :disabled="busy || working"
        type="search"
        :aria-label="t('capture.catalog_search')"
        :placeholder="t('capture.catalog_search')"
        maxlength="120"
      />
      <select v-model="scope" :disabled="busy || working" :aria-label="t('capture.catalog_scope')">
        <option v-for="key in scopes" :key="key" :value="key">{{ t(scopeLabels[key]) }}</option>
      </select>
      <select v-model="format" :disabled="busy || working" :aria-label="t('capture.catalog_format')">
        <option value="">{{ t('capture.catalog_all_formats') }}</option>
        <option v-for="key in ['png', 'gif', 'mp4']" :key="key" :value="key">{{ key.toUpperCase() }}</option>
      </select>
      <select v-model="days" :disabled="busy || working" :aria-label="t('capture.catalog_period')">
        <option value="">{{ t('capture.catalog_anytime') }}</option>
        <option value="7">{{ t('capture.catalog_days', { count: 7 }) }}</option>
        <option value="30">{{ t('capture.catalog_days', { count: 30 }) }}</option>
      </select>
      <select v-model="sort" :disabled="busy || working" :aria-label="t('capture.catalog_sort')">
        <option value="newest">{{ t('capture.catalog_newest') }}</option>
        <option value="oldest">{{ t('capture.catalog_oldest') }}</option>
        <option value="largest">{{ t('capture.catalog_largest') }}</option>
      </select>
      <button :disabled="disabled" @click="load()">{{ t('capture.catalog_refresh') }}</button>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="scope === 'trash'" class="hint">{{ t('capture.catalog_trash_hint') }}</p>

    <div v-if="page" class="toolbar">
      <button :disabled="disabled || !page.rows.length" @click="togglePage">
        {{
          t(
            pagePicked && pagePicked === page.rows.length
              ? 'capture.catalog_deselect_page'
              : 'capture.catalog_select_page',
          )
        }}
      </button>
      <button v-if="selectedCount" :disabled="disabled" @click="selected = []">
        {{ t('capture.catalog_clear_selection') }}
      </button>
      <span aria-live="polite">{{
        t('capture.catalog_selected', { count: selectedCount, limit: CAPTURE_BATCH_LIMIT })
      }}</span>
    </div>
    <div v-if="selectedCount" class="toolbar">
      <select v-model="operation" :disabled="disabled" :aria-label="t('capture.catalog_action')">
        <option v-for="op in actions" :key="op" :value="op">{{ t(actionLabels[op]) }}</option>
      </select>
      <button :disabled="disabled" @click="preview()">{{ t('capture.catalog_review') }}</button>
    </div>

    <form v-if="editing" class="review" @submit.prevent="rename">
      <label
        >{{ t('capture.catalog_name') }}<input v-model="name" :disabled="disabled" maxlength="80"
      /></label>
      <small>{{ t('capture.catalog_name_hint', { id: shortId(editing.asset.id) }) }}</small>
      <div class="toolbar">
        <button type="submit" :disabled="disabled">{{ t('capture.catalog_save_name') }}</button>
        <button type="button" :disabled="disabled" @click="editing = null">
          {{ t('capture.catalog_cancel') }}
        </button>
      </div>
    </form>

    <section v-if="pending || result" class="review" :aria-label="t('capture.catalog_review')">
      <strong v-if="pending"
        >{{ t(actionLabels[pending.operation]) }} ·
        {{
          t('capture.catalog_plan', { count: pending.report.ready, bytes: bytes(pending.report.ready_bytes) })
        }}</strong
      >
      <strong v-else-if="result" role="status">{{
        t('capture.catalog_result', { count: result.done, bytes: bytes(result.done_bytes) })
      }}</strong>
      <small v-if="pending?.remaining">{{
        t('capture.catalog_remaining', { count: pending.remaining })
      }}</small>
      <p v-if="pending?.operation === 'purge'" class="error">{{ t('capture.catalog_purge_warning') }}</p>
      <ul class="outcomes">
        <li v-for="entry in (pending?.report ?? result)?.entries" :key="entry.id">
          <strong v-if="entry.created_at !== undefined"
            >{{ displayName(entry.name, entry.created_at) }} ·
          </strong>
          {{ shortId(entry.id) }} · {{ bytes(entry.bytes) }} · {{ t(outcomeLabels[entry.status]) }}
          <small v-if="entry.filename">{{ entry.filename }}</small>
        </li>
      </ul>
      <div class="toolbar">
        <button v-if="pending" :disabled="disabled || !pending.report.ready" @click="apply">
          {{
            t(
              pending.operation === 'export'
                ? 'capture.catalog_pick_destination'
                : pending.operation === 'purge'
                  ? 'capture.catalog_confirm_purge'
                  : 'capture.catalog_confirm',
            )
          }}
        </button>
        <button :disabled="disabled" @click="pending = result = null">
          {{ t('capture.catalog_close') }}
        </button>
      </div>
    </section>

    <p v-if="loading" class="hint" role="status">{{ t('capture.catalog_loading') }}</p>
    <ul v-else-if="page?.rows.length" class="rows">
      <li v-for="row in page.rows" :key="row.asset.id">
        <input
          type="checkbox"
          :checked="isSelected(row.asset.id)"
          :disabled="disabled || (selectedCount >= CAPTURE_BATCH_LIMIT && !isSelected(row.asset.id))"
          :aria-label="t('capture.catalog_select_row', { name: rowTitle(row), id: shortId(row.asset.id) })"
          @change="toggle(row)"
        />
        <div class="row-detail">
          <button
            v-if="row.scope !== 'trash'"
            class="asset-title"
            :disabled="disabled"
            :aria-pressed="activeId === row.asset.id"
            @click="emit('choose', row.asset.id)"
          >
            {{ rowTitle(row) }}
          </button>
          <strong v-else>{{ rowTitle(row) }}</strong>
          <small
            >{{ shortId(row.asset.id) }} ·
            {{
              t('capture.asset_size', {
                format: row.asset.extension.toUpperCase(),
                width: row.asset.width,
                height: row.asset.height,
              })
            }}
            · {{ bytes(row.asset.bytes) }}</small
          >
          <small
            >{{ t(scopeLabels[row.scope])
            }}<template v-if="row.in_use || activeId === row.asset.id">
              · {{ t('capture.catalog_active') }}</template
            ></small
          >
        </div>
        <button
          v-if="row.scope !== 'trash'"
          :disabled="disabled"
          :aria-label="t('capture.catalog_rename_row', { id: shortId(row.asset.id) })"
          @click="startRename(row)"
        >
          {{ t('capture.catalog_rename') }}
        </button>
      </li>
    </ul>
    <p v-else-if="page" class="hint">
      {{ t(page.storage.total.count ? 'capture.catalog_no_match' : 'capture.no_assets') }}
    </p>
    <nav v-if="page?.matched.count" class="toolbar" :aria-label="t('capture.catalog_pages')">
      <button :disabled="disabled || page.offset === 0" @click="load(page.offset - page.limit)">
        {{ t('capture.catalog_prev') }}
      </button>
      <span>{{
        t('capture.catalog_range', {
          first: page.offset + 1,
          last: page.offset + page.rows.length,
          total: page.matched.count,
          bytes: bytes(page.matched.bytes),
        })
      }}</span>
      <button
        :disabled="disabled || page.offset + page.limit >= page.matched.count"
        @click="load(page.offset + page.limit)"
      >
        {{ t('capture.catalog_next') }}
      </button>
    </nav>
  </aside>
</template>

<style scoped>
.catalog,
.review,
.row-detail {
  display: grid;
  gap: var(--mf-sp-2);
  min-width: 0;
  align-content: start;
}
.catalog h2,
.catalog p {
  margin: 0;
}
.hint,
small {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.error {
  color: var(--mf-danger);
}
.filters {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--mf-sp-2);
}
.filters input {
  grid-column: 1 / -1;
  min-width: 0;
}
.filters select {
  min-width: 0;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--mf-sp-2);
  font-size: var(--mf-fs-sm);
}
.usage,
.review {
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-2);
}
.usage summary {
  cursor: pointer;
  overflow-wrap: anywhere;
}
.usage progress {
  width: 100%;
}
.usage dl {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: var(--mf-sp-1);
  margin: var(--mf-sp-2) 0;
}
.usage dd {
  margin: 0;
}
.rows,
.outcomes {
  padding: 0;
  margin: 0;
  list-style: none;
}
.rows {
  max-height: 48vh;
  overflow: auto;
}
.rows li {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: start;
  gap: var(--mf-sp-2);
  border-bottom: 1px solid var(--mf-border);
  padding: var(--mf-sp-2) 0;
}
.row-detail {
  gap: var(--mf-sp-1);
}
.asset-title {
  text-align: start;
}
.asset-title,
.row-detail strong,
.outcomes small {
  overflow-wrap: anywhere;
}
.outcomes {
  max-height: 11rem;
  overflow: auto;
}
.outcomes small {
  display: block;
}
.review label {
  display: grid;
  gap: var(--mf-sp-1);
}
</style>
