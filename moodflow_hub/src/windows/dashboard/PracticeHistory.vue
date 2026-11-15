<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import {
  commands,
  unwrap,
  type PracticeCategory,
  type PracticeMode,
  type PracticeRecord,
  type PracticeRecordInput,
} from '@/api'
import { errorText, t } from '@/i18n'
import { PRACTICE_CATEGORIES, PRACTICE_MODES } from './practice'

const records = ref<PracticeRecord[]>([])
const opened = ref(false)
const loaded = ref(false)
const busy = ref(false)
const error = ref('')
const status = ref('')
const category = ref<PracticeCategory | 'all'>('all')
const mode = ref<PracticeMode | 'all'>('all')
const pending = ref<{ id?: string } | null>(null)
const page = ref(0)
const pageSize = 12
let alive = true

const filtered = computed(() =>
  records.value.filter(
    ({ data }) =>
      (category.value === 'all' || data.category === category.value) &&
      (mode.value === 'all' || data.mode === mode.value),
  ),
)
const shown = computed(() => filtered.value.slice(page.value * pageSize, (page.value + 1) * pageSize))
const recent = computed(() => filtered.value.slice(0, 8).reverse())
const top = computed(() => Math.max(1, ...recent.value.map(({ data }) => data.score.effective_cpm)))
const average = computed(
  () => recent.value.reduce((sum, r) => sum + r.data.score.effective_cpm, 0) / recent.value.length,
)
const datetime = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
const pct = (v: number) => (v * 100).toFixed(1)
function date(iso: string) {
  return datetime.format(new Date(iso))
}
function filtersChanged() {
  page.value = 0
  pending.value = null
}
function toggle(event: Event) {
  opened.value = (event.target as HTMLDetailsElement).open
  if (opened.value && !loaded.value && !busy.value) void run(reload)
}
async function run(action: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  status.value = ''
  try {
    await action()
  } catch (e) {
    if (alive) error.value = errorText(e)
  } finally {
    if (alive) busy.value = false
  }
}
async function reload() {
  const data = await unwrap(commands.practiceHistoryList())
  if (!alive) return
  records.value = data
  loaded.value = true
  page.value = Math.min(page.value, Math.floor(Math.max(0, filtered.value.length - 1) / pageSize))
}
async function save(input: PracticeRecordInput) {
  opened.value = true
  pending.value = null
  await run(async () => {
    await unwrap(commands.practiceHistorySave(input))
    if (!alive) return
    category.value = input.category
    mode.value = input.mode
    page.value = 0
    status.value = t('dashboard.practice.history_saved')
    await reload()
  })
}
async function remove() {
  const selected = pending.value
  if (!selected) return
  await run(async () => {
    await unwrap(commands.practiceHistoryDelete(selected.id ?? null))
    if (!alive) return
    pending.value = null
    status.value = t('dashboard.practice.history_deleted')
    await reload()
  })
}
onBeforeUnmount(() => {
  alive = false
})
defineExpose({ save })
</script>

<template>
  <details class="d-card history" :open="opened" @toggle="toggle">
    <summary>{{ t('dashboard.practice.history') }}</summary>
    <div class="history-body">
      <p class="hint">{{ t('dashboard.practice.history_hint') }}</p>
      <p v-if="error" role="alert" class="d-error">{{ error }}</p>
      <p v-if="status" role="status">{{ status }}</p>
      <p v-if="busy" class="hint" role="status">{{ t('dashboard.practice.history_loading') }}</p>
      <div class="d-row">
        <label
          >{{ t('dashboard.practice.category') }}
          <select v-model="category" :disabled="busy" @change="filtersChanged">
            <option value="all">{{ t('dashboard.practice.history_all') }}</option>
            <option v-for="key in PRACTICE_CATEGORIES" :key="key" :value="key">
              {{ t(`dashboard.practice.category_${key}`) }}
            </option>
          </select>
        </label>
        <label
          >{{ t('dashboard.practice.mode') }}
          <select v-model="mode" :disabled="busy" @change="filtersChanged">
            <option value="all">{{ t('dashboard.practice.history_all') }}</option>
            <option v-for="key in PRACTICE_MODES" :key="key" :value="key">
              {{ t(`dashboard.practice.mode_${key}`) }}
            </option>
          </select>
        </label>
        <button :disabled="busy" @click="run(reload)">{{ t('dashboard.practice.history_refresh') }}</button>
        <button v-if="records.length" :disabled="busy" @click="pending = {}">
          {{ t('dashboard.practice.history_clear') }}
        </button>
      </div>
      <div v-if="pending" class="d-row confirm">
        <span>{{
          t(pending.id ? 'dashboard.practice.history_confirm_one' : 'dashboard.practice.history_confirm_all')
        }}</span>
        <button :disabled="busy" @click="remove">{{ t('dashboard.practice.history_confirm') }}</button>
        <button :disabled="busy" @click="pending = null">{{ t('dashboard.practice.history_cancel') }}</button>
      </div>
      <template v-if="filtered.length">
        <section v-if="category !== 'all' && mode !== 'all'">
          <h3>{{ t('dashboard.practice.trend') }}</h3>
          <p class="hint">
            {{ t('dashboard.practice.trend_summary', { n: recent.length, avg: average.toFixed(1) }) }}
          </p>
          <ol class="bars">
            <li v-for="record in recent" :key="record.data.id">
              <time :datetime="record.date">{{ date(record.date) }}</time>
              <meter
                :value="record.data.score.effective_cpm"
                :max="top"
                :aria-label="
                  t('dashboard.practice.speed', { cpm: record.data.score.effective_cpm.toFixed(1) })
                "
              />
              <span>{{ record.data.score.effective_cpm.toFixed(1) }}</span>
            </li>
          </ol>
        </section>
        <p v-else class="hint">{{ t('dashboard.practice.trend_hint') }}</p>
        <div class="table-wrap">
          <table>
            <caption>
              {{
                t('dashboard.practice.history')
              }}
            </caption>
            <thead>
              <tr>
                <th scope="col">{{ t('dashboard.practice.history_date') }}</th>
                <th scope="col">{{ t('dashboard.practice.category') }}</th>
                <th scope="col">{{ t('dashboard.practice.mode') }}</th>
                <th scope="col">{{ t('dashboard.practice.history_cpm') }}</th>
                <th scope="col">{{ t('dashboard.practice.history_accuracy') }}</th>
                <th scope="col">{{ t('dashboard.practice.history_process') }}</th>
                <th scope="col">{{ t('dashboard.practice.history_actions') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="{ date: savedAt, data } in shown" :key="data.id">
                <td>
                  <time :datetime="savedAt">{{ date(savedAt) }}</time>
                </td>
                <td>{{ t(`dashboard.practice.category_${data.category}`) }}</td>
                <td>{{ t(`dashboard.practice.mode_${data.mode}`) }}</td>
                <td>{{ data.score.effective_cpm.toFixed(1) }}</td>
                <td>{{ pct(data.score.final_accuracy) }}{{ '%' }}</td>
                <td>{{ pct(data.score.process_accuracy) }}{{ '%' }}</td>
                <td>
                  <button
                    :disabled="busy"
                    :aria-label="t('dashboard.practice.history_delete_at', { date: date(savedAt) })"
                    @click="pending = { id: data.id }"
                  >
                    {{ t('dashboard.practice.history_delete') }}
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <nav
          v-if="filtered.length > pageSize"
          class="d-row"
          :aria-label="t('dashboard.practice.history_pages')"
        >
          <button :disabled="page === 0 || busy" @click="page--">
            {{ t('dashboard.practice.history_previous') }}
          </button>
          <span>{{
            t('dashboard.practice.history_range', {
              start: page * pageSize + 1,
              end: page * pageSize + shown.length,
              total: filtered.length,
            })
          }}</span>
          <button :disabled="(page + 1) * pageSize >= filtered.length || busy" @click="page++">
            {{ t('dashboard.practice.history_next') }}
          </button>
        </nav>
      </template>
      <p v-else-if="loaded" class="hint">
        {{ t(records.length ? 'dashboard.practice.history_no_match' : 'dashboard.practice.history_empty') }}
      </p>
    </div>
  </details>
</template>

<style scoped>
summary {
  cursor: pointer;
  font-weight: 600;
}
.history-body {
  display: grid;
  gap: var(--mf-sp-3);
  margin-block-start: var(--mf-sp-3);
}
.history-body p {
  margin: 0;
}
.hint {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.confirm {
  border-block: 1px solid var(--mf-border);
  padding-block: var(--mf-sp-2);
}
.table-wrap {
  overflow-x: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
  text-align: start;
  font-variant-numeric: tabular-nums;
}
caption {
  text-align: start;
}
th,
td {
  text-align: start;
  padding: var(--mf-sp-2);
  border-bottom: 1px solid var(--mf-border);
}
.bars {
  list-style: none;
  margin: var(--mf-sp-2) 0 0;
  padding: 0;
  display: grid;
  gap: var(--mf-sp-1);
}
.bars li {
  display: grid;
  grid-template-columns: minmax(7rem, max-content) 1fr 3.5rem;
  align-items: center;
  gap: var(--mf-sp-2);
  font-variant-numeric: tabular-nums;
}
.bars meter {
  width: 100%;
}
</style>
