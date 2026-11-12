<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
interface Doc {
  metadata: { id: string; title: string; tags: string[]; capsule: string }
  body: string
  revision: string
}
interface Entry {
  name: string
  id: string
  title: string
  kind: string
  tags: string[]
  updated_at: string
  revision: string
}
interface Listing {
  items: Entry[]
  unreadable: string[]
  total: number
}
interface Review {
  name: string
  saved: Doc
  current: Doc | null
  current_state: string
  source_revision: string
  can_replace: boolean
  diff: {
    rows: { kind: string; left: number | null; right: number | null; text: string }[]
    added: number
    removed: number
    detailed: boolean
    truncated: boolean
    left_lines: number
    right_lines: number
  }
  fields: { key: string; current: string; saved: string }[]
  fields_truncated: boolean
}
const props = defineProps<{ disabled: boolean; refreshKey: number }>()
const query = ref('')
const backups = ref(false)
const listing = ref<Listing>({ items: [], unreadable: [], total: 0 })
const review = ref<Review | null>(null)
const offset = ref(0)
const working = ref(false)
const error = ref('')
const notice = ref('')
const editTitle = ref('')
const editBody = ref('')
const editTags = ref('')
const mode = ref<'copy' | 'replace'>('copy')
const baseline = ref('')
const completed = ref(false)
const expandedBody = ref(false)
let intent: { id: string; updated: string } | null = null
let generation = 0
let disposed = false
const locked = computed(() => props.disabled || working.value)
const editor = computed(() => JSON.stringify([editTitle.value, editBody.value, editTags.value]))
const dirty = computed(() => !!review.value && editor.value !== baseline.value && !completed.value)
const visible = computed(() => listing.value.items.slice(offset.value, offset.value + 20))
function copy(key: string): string {
  return t(`sync.review_${key}` as CopyKey)
}
const valid = (mine: number) => !disposed && mine === generation
async function call<T>(method: string, params: object = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.syncRequest(method, JSON.stringify(params)))) as T
}
async function action(work: (mine: number) => Promise<void>): Promise<void> {
  if (locked.value) return
  const mine = generation
  working.value = true
  error.value = ''
  notice.value = ''
  try {
    await work(mine)
  } catch (e) {
    if (valid(mine)) error.value = errorText(e)
  } finally {
    if (valid(mine)) working.value = false
  }
}
async function canLeave(): Promise<boolean> {
  return !dirty.value || (await confirm(copy('discard'), { title: 'Moodflow', kind: 'warning' }))
}
async function load(): Promise<void> {
  await action(async (mine) => {
    const value = await call<Listing>('review_list', { query: query.value, backups: backups.value })
    if (valid(mine)) {
      listing.value = {
        items: value.items,
        unreadable: value.unreadable ?? [],
        total: value.total ?? value.items.length,
      }
      offset.value = 0
    }
  })
}
async function changeSource(value: boolean): Promise<void> {
  const mine = generation
  if (locked.value || !(await canLeave()) || !valid(mine)) return
  backups.value = value
  review.value = null
  baseline.value = ''
  editBody.value = ''
  editTitle.value = ''
  editTags.value = ''
  completed.value = false
  intent = null
  await load()
}
async function choose(name: string): Promise<void> {
  const mine = generation
  if (!(await canLeave()) || !valid(mine)) return
  await action(async (current) => {
    const value = await call<Review>('review', { name, backups: backups.value })
    if (!valid(current)) return
    review.value = value
    editTitle.value = value.saved.metadata.title
    editBody.value = value.saved.body
    editTags.value = value.saved.metadata.tags.join(', ')
    mode.value = 'copy'
    completed.value = false
    intent = null
    baseline.value = editor.value
  })
}
async function resolve(): Promise<void> {
  if (!review.value || completed.value || !editTitle.value.trim() || locked.value) return
  const mine = generation
  if (
    !(await confirm(copy(mode.value === 'copy' ? 'copy_confirm' : 'replace_confirm'), {
      title: 'Moodflow',
      kind: 'warning',
    })) ||
    !valid(mine)
  )
    return
  if (!intent) intent = { id: crypto.randomUUID(), updated: new Date().toISOString() }
  const value = review.value
  const resolution = {
    name: value.name,
    source_revision: value.source_revision,
    current_revision: value.current?.revision ?? null,
    mode: mode.value,
    operation_id: intent.id,
    updated_at: intent.updated,
    title: editTitle.value.trim(),
    body: editBody.value,
    tags: editTags.value
      .split(/[,，]/)
      .map((s) => s.trim())
      .filter(Boolean),
  }
  await action(async (current) => {
    const result = await call<{ id: string; backup: string | null; repeated: boolean }>('review_resolve', {
      resolution,
      backups: backups.value,
      confirm: true,
    })
    if (!valid(current)) return
    completed.value = true
    baseline.value = editor.value
    notice.value = t('sync.review_saved', { id: result.id }) + (result.backup ? ` ${copy('backed_up')}` : '')
  })
}
async function remove(): Promise<void> {
  const value = review.value
  const mine = generation
  if (!value || locked.value || !(await canLeave()) || !valid(mine)) return
  if (!(await confirm(copy('delete_confirm'), { title: 'Moodflow', kind: 'warning' })) || !valid(mine)) return
  await action(async (current) => {
    await call('review_delete', {
      name: value.name,
      revision: value.source_revision,
      backups: backups.value,
      confirm: true,
    })
    if (!valid(current)) return
    review.value = null
    editBody.value = ''
    editTitle.value = ''
    editTags.value = ''
    baseline.value = ''
    intent = null
    const result = await call<Listing>('review_list', { query: query.value, backups: backups.value })
    if (valid(current)) {
      listing.value = result
      offset.value = 0
      notice.value = copy('deleted')
    }
  })
}
watch([editTitle, editBody, editTags, mode], () => {
  intent = null
  completed.value = false
})
watch(
  () => props.refreshKey,
  () => {
    if (!dirty.value && !working.value) void load()
  },
)
onMounted(() => void load())
onBeforeUnmount(() => {
  disposed = true
  generation++
  review.value = null
  editBody.value = ''
  editTitle.value = ''
  editTags.value = ''
  listing.value = { items: [], unreadable: [], total: 0 }
  intent = null
})
defineExpose({ canLeave })
</script>
<template>
  <section class="recovery" :aria-busy="working">
    <header>
      <h3>{{ copy('title') }}</h3>
      <p>{{ copy('intro') }}</p>
    </header>
    <nav class="actions" :aria-label="copy('source')">
      <button :disabled="locked" :aria-pressed="!backups" @click="changeSource(false)">
        {{ copy('conflicts') }}</button
      ><button :disabled="locked" :aria-pressed="backups" @click="changeSource(true)">
        {{ copy('backups') }}
      </button>
    </nav>
    <form class="actions" @submit.prevent="load">
      <label class="query"
        >{{ copy('query') }}<input v-model="query" maxlength="256" :disabled="locked" type="search" /></label
      ><button type="submit" :disabled="locked">{{ copy('search') }}</button>
    </form>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="listing.unreadable.length" role="status" class="error">
      {{ t('sync.review_unreadable', { count: listing.unreadable.length }) }}
    </p>
    <p>{{ t('sync.review_total', { count: listing.total }) }}</p>
    <div class="versions">
      <button
        v-for="entry in visible"
        :key="entry.name"
        :disabled="locked"
        :aria-pressed="review?.name === entry.name"
        @click="choose(entry.name)"
      >
        <strong>{{ entry.title }}</strong
        ><small>{{ entry.id }} · {{ entry.updated_at }}</small
        ><span>{{ entry.tags.join(' · ') }}</span>
      </button>
    </div>
    <p v-if="!listing.total">{{ copy('empty') }}</p>
    <nav class="actions" :aria-label="copy('pages')">
      <button :disabled="locked || !offset" @click="offset = Math.max(0, offset - 20)">
        {{ copy('previous') }}</button
      ><span>{{ t('sync.review_page', { page: Math.floor(offset / 20) + 1 }) }}</span
      ><button :disabled="locked || offset + 20 >= listing.total" @click="offset += 20">
        {{ copy('next') }}
      </button>
    </nav>
    <article v-if="review" class="review">
      <h4>{{ review.saved.metadata.title }}</h4>
      <button :disabled="locked" @click="choose(review.name)">{{ copy('reload') }}</button>
      <p role="status">{{ copy(`state_${review.current_state}`) }}</p>
      <p class="scope">{{ copy('semantics') }}</p>
      <details>
        <summary>{{ copy('metadata') }}</summary>
        <p>{{ copy('metadata_hint') }}</p>
        <table>
          <thead>
            <tr>
              <th>{{ copy('field') }}</th>
              <th>{{ copy('current') }}</th>
              <th>{{ copy('saved_copy') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="field in review.fields" :key="field.key">
              <th>{{ field.key }}</th>
              <td>{{ field.current }}</td>
              <td>{{ field.saved }}</td>
            </tr>
          </tbody>
        </table>
        <p v-if="review.fields_truncated">{{ copy('limited') }}</p>
      </details>
      <h4>{{ copy('difference') }}</h4>
      <p v-if="review.diff.rows.length">
        {{ t('sync.review_counts', { added: review.diff.added, removed: review.diff.removed }) }}
      </p>
      <p v-if="!review.diff.detailed || review.diff.truncated" role="status">{{ copy('limited') }}</p>
      <div class="diff" :aria-label="copy('difference')">
        <div v-for="(row, index) in review.diff.rows" :key="index" :class="['diff-row', row.kind]">
          <span>{{
            row.kind === 'added' ? '+' : row.kind === 'removed' ? '−' : row.kind === 'gap' ? '…' : ' '
          }}</span
          ><span>{{ row.left ?? '' }}</span
          ><span>{{ row.right ?? '' }}</span
          ><code>{{ row.text }}</code>
        </div>
      </div>
      <button :aria-expanded="expandedBody" @click="expandedBody = !expandedBody">
        {{ copy('full_text') }}
      </button>
      <div v-if="expandedBody" class="side-by-side">
        <label>{{ copy('current') }}<textarea :value="review.current?.body ?? ''" readonly /></label
        ><label>{{ copy('saved_copy') }}<textarea :value="review.saved.body" readonly /></label>
      </div>
      <button :disabled="locked" @click="remove">{{ copy('delete') }}</button>
      <h4>{{ copy('edit') }}</h4>
      <label>{{ copy('name') }}<input v-model="editTitle" maxlength="256" :disabled="locked" /></label>
      <label>{{ copy('tags') }}<input v-model="editTags" :disabled="locked" /></label>
      <template v-if="review.saved.metadata.capsule !== 'pdf'"
        ><div class="actions">
          <button :disabled="locked || !review.current" @click="editBody = review.current?.body ?? ''">
            {{ copy('use_current') }}</button
          ><button :disabled="locked" @click="editBody = review.saved.body">{{ copy('use_saved') }}</button>
        </div>
        <label
          >{{ copy('merged_body')
          }}<textarea v-model="editBody" maxlength="2097152" spellcheck="false" :disabled="locked" /></label
      ></template>
      <p v-else>{{ copy('bibliography') }}</p>
      <label
        >{{ copy('mode')
        }}<select v-model="mode" :disabled="locked">
          <option value="copy">{{ copy('copy') }}</option>
          <option v-if="review.can_replace" value="replace">{{ copy('replace') }}</option>
        </select></label
      >
      <p v-if="mode === 'replace'" class="warning">{{ copy('replace_hint') }}</p>
      <button class="primary" :disabled="locked || completed || !editTitle.trim()" @click="resolve">
        {{ copy('apply') }}
      </button>
    </article>
  </section>
</template>
<style scoped>
.recovery,
.review {
  display: grid;
  gap: var(--mf-sp-3);
}
.recovery {
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}
h3,
h4,
p {
  margin: 0;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: end;
}
.query {
  flex: 1;
  min-width: 180px;
}
label {
  display: grid;
  gap: var(--mf-sp-2);
}
.versions {
  display: grid;
  gap: var(--mf-sp-2);
  max-height: 320px;
  overflow: auto;
}
.versions button {
  text-align: left;
  display: grid;
  gap: var(--mf-sp-1);
  overflow-wrap: anywhere;
}
small,
p {
  color: var(--mf-text-2);
}
input,
select,
textarea {
  width: 100%;
  box-sizing: border-box;
  color: var(--mf-text-1);
  background: var(--mf-surface);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-sm);
  padding: var(--mf-sp-2);
}
textarea {
  min-height: 200px;
  resize: vertical;
}
.side-by-side {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--mf-sp-3);
}
.diff {
  max-height: 400px;
  overflow: auto;
  border: 1px solid var(--mf-border);
}
.diff-row {
  display: grid;
  grid-template-columns: 24px 48px 48px minmax(0, 1fr);
  padding: var(--mf-sp-1);
  gap: var(--mf-sp-1);
}
.diff-row code {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.added {
  border-left: 3px solid var(--mf-focus);
  background: var(--mf-surface-2);
}
.removed {
  border-left: 3px solid var(--mf-danger);
}
.gap {
  color: var(--mf-text-3);
}
details {
  overflow: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th,
td {
  text-align: left;
  vertical-align: top;
  padding: var(--mf-sp-2);
  border: 1px solid var(--mf-border);
  overflow-wrap: anywhere;
  max-width: 260px;
  white-space: pre-wrap;
}
.scope {
  padding: var(--mf-sp-3);
  border-left: 3px solid var(--mf-focus);
}
.warning,
.error {
  color: var(--mf-danger);
}
@media (max-width: 700px) {
  .side-by-side {
    grid-template-columns: 1fr;
  }
}
</style>
