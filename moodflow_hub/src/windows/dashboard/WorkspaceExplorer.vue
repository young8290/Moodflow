<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { confirm, save } from '@tauri-apps/plugin-dialog'
import { errorText, t, type CopyKey } from '@/i18n'
import { workspaceCall as call, type WorkspaceKind } from './workspaceTypes'
interface Hit {
  id: string
  kind: WorkspaceKind
  title: string
  revision: string
  archived: boolean
  unread: boolean
  count: number
  matched_messages: number
  matches: { id: string; role: string; snippet: string }[]
}
interface Page {
  items: Hit[]
  total: number
  offset: number
  limit: number
  unreadable: string[]
}
interface Outcome {
  id: string
  status: string
  revision: string | null
}
const props = defineProps<{ disabled: boolean; beforeChange: () => Promise<boolean> }>()
const emit = defineEmits<{ open: [hit: Hit]; changed: []; busy: [value: boolean] }>()
const expanded = ref(false)
const query = ref('')
const kind = ref('')
const archive = ref('active')
const scope = ref('all')
const sort = ref('updated')
const unreadOnly = ref(false)
const page = ref<Page | null>(null)
const selection = ref<Record<string, string>>({})
const working = ref(false)
const error = ref('')
const notice = ref('')
const results = ref<Outcome[]>([])
const renameId = ref<string | null>(null)
const renameTitle = ref('')
const format = ref('markdown')
const includeInstructions = ref(false)
let generation = 0
let disposed = false
const locked = computed(() => props.disabled || working.value)
const count = computed(() => Object.keys(selection.value).length)
const selected = computed(() => Object.entries(selection.value).map(([id, revision]) => ({ id, revision })))
const valid = (mine: number) => !disposed && mine === generation
function copy(key: string): string {
  return t(`workspace.explorer_${key}` as CopyKey)
}
async function run(work: (mine: number) => Promise<void>): Promise<void> {
  if (locked.value) return
  const mine = generation
  working.value = true
  emit('busy', true)
  error.value = ''
  notice.value = ''
  try {
    await work(mine)
  } catch (e) {
    if (valid(mine)) error.value = errorText(e)
  } finally {
    if (valid(mine)) {
      working.value = false
      emit('busy', false)
    }
  }
}
async function requestPage(offset: number, mine: number): Promise<void> {
  const value = await call<Page>('search', {
    query: query.value,
    kind: kind.value || null,
    archive: archive.value,
    scope: scope.value,
    sort: sort.value,
    unread_only: unreadOnly.value,
    offset,
    limit: 20,
  })
  if (valid(mine)) {
    page.value = value
    selection.value = {}
    renameId.value = null
  }
}
async function search(offset = 0): Promise<void> {
  await run(async (mine) => {
    results.value = []
    await requestPage(offset, mine)
  })
}
async function expand(): Promise<void> {
  expanded.value = !expanded.value
  if (expanded.value && !page.value) await search()
}
function selectPage(): void {
  selection.value = Object.fromEntries((page.value?.items ?? []).map((i) => [i.id, i.revision]))
}
function toggle(hit: Hit, checked: boolean): void {
  if (checked) selection.value[hit.id] = hit.revision
  else delete selection.value[hit.id]
}
async function change(type: 'archive' | 'unread', value: boolean): Promise<void> {
  if (!count.value || locked.value) return
  const mine = generation
  if (!(await props.beforeChange()) || !valid(mine)) return
  const confirmed = await confirm(
    t('workspace.explorer_confirm', { count: count.value, action: copy(`${type}_${value}`) }),
    { title: 'Moodflow', kind: 'warning' },
  )
  if (!confirmed || !valid(mine)) return
  await run(async (current) => {
    const valueResult = await call<Outcome[]>('manage', {
      selection: selected.value,
      change: { type, value },
    })
    if (!valid(current)) return
    results.value = valueResult
    emit('changed')
    await requestPage(page.value?.offset ?? 0, current)
  })
}
function beginRename(hit: Hit): void {
  renameId.value = hit.id
  renameTitle.value = hit.title
}
async function rename(): Promise<void> {
  const hit = page.value?.items.find((i) => i.id === renameId.value)
  if (!hit || !renameTitle.value.trim() || locked.value) return
  const mine = generation
  if (!(await props.beforeChange()) || !valid(mine)) return
  await run(async (current) => {
    await call('rename', {
      selection: { id: hit.id, revision: hit.revision },
      title: renameTitle.value.trim(),
    })
    if (!valid(current)) return
    emit('changed')
    notice.value = copy('renamed')
    await requestPage(page.value?.offset ?? 0, current)
  })
}
async function exportTranscript(hit: Hit): Promise<void> {
  await run(async (mine) => {
    const path = await save({
      defaultPath: `${hit.id}.${format.value === 'markdown' ? 'md' : 'json'}`,
      filters: [{ name: copy('transcript'), extensions: [format.value === 'markdown' ? 'md' : 'json'] }],
    })
    if (!path || !valid(mine)) return
    await call('transcript', {
      selection: { id: hit.id, revision: hit.revision },
      format: format.value,
      include_instructions: includeInstructions.value,
      path,
    })
    if (valid(mine)) notice.value = copy('exported')
  })
}
onBeforeUnmount(() => {
  disposed = true
  generation++
  page.value = null
  query.value = ''
  selection.value = {}
  renameTitle.value = ''
})
</script>
<template>
  <section class="explorer" :aria-busy="working">
    <button :disabled="locked" :aria-expanded="expanded" @click="expand">{{ copy('title') }}</button>
    <div v-if="expanded" class="explorer-content">
      <p>{{ copy('intro') }}</p>
      <form class="filters" @submit.prevent="search(0)">
        <label class="query"
          >{{ copy('query') }}<input v-model="query" maxlength="256" type="search" :disabled="locked"
        /></label>
        <label
          >{{ copy('kind')
          }}<select v-model="kind" :disabled="locked">
            <option value="">{{ copy('all') }}</option>
            <option v-for="k in ['chat', 'inbox', 'terminal']" :key="k" :value="k">
              {{ t(`workspace.${k}` as CopyKey) }}
            </option>
          </select></label
        >
        <label
          >{{ copy('archive')
          }}<select v-model="archive" :disabled="locked">
            <option v-for="v in ['active', 'archived', 'all']" :key="v" :value="v">{{ copy(v) }}</option>
          </select></label
        >
        <label
          >{{ copy('scope')
          }}<select v-model="scope" :disabled="locked">
            <option v-for="v in ['all', 'title', 'messages']" :key="v" :value="v">
              {{ copy(`scope_${v}`) }}
            </option>
          </select></label
        >
        <label
          >{{ copy('sort')
          }}<select v-model="sort" :disabled="locked">
            <option v-for="v in ['updated', 'created', 'title']" :key="v" :value="v">
              {{ copy(`sort_${v}`) }}
            </option>
          </select></label
        >
        <label class="inline"
          ><input v-model="unreadOnly" type="checkbox" :disabled="locked" />{{ copy('unread_only') }}</label
        >
        <button type="submit" :disabled="locked">{{ copy('search') }}</button>
      </form>
      <p v-if="error" role="alert" class="error">{{ error }}</p>
      <p v-if="notice" role="status">{{ notice }}</p>
      <p v-if="page?.unreadable.length" role="status" class="error">
        {{ t('workspace.explorer_unreadable', { count: page.unreadable.length }) }}
      </p>
      <template v-if="page">
        <p role="status">{{ t('workspace.explorer_total', { count: page.total }) }}</p>
        <div class="actions">
          <button :disabled="locked || !page.items.length" @click="selectPage">
            {{ copy('select_page') }}
          </button>
          <button :disabled="locked || !count" @click="selection = {}">{{ copy('clear') }}</button>
          <button
            v-for="value in [true, false]"
            :key="`archive-${value}`"
            :disabled="locked || !count"
            @click="change('archive', value)"
          >
            {{ copy(`archive_${value}`) }}
          </button>
          <button
            v-for="value in [false, true]"
            :key="`unread-${value}`"
            :disabled="locked || !count"
            @click="change('unread', value)"
          >
            {{ copy(`unread_${value}`) }}
          </button>
        </div>
        <p>{{ t('workspace.explorer_selected', { count }) }}</p>
        <ul v-if="results.length" class="outcomes" role="status">
          <li v-for="result in results" :key="result.id">{{ result.id }} · {{ copy(result.status) }}</li>
        </ul>
        <div class="export-options">
          <label
            >{{ copy('format')
            }}<select v-model="format" :disabled="locked">
              <option value="markdown">{{ copy('markdown') }}</option>
              <option value="json">{{ copy('json') }}</option>
            </select></label
          >
          <label class="inline"
            ><input v-model="includeInstructions" type="checkbox" :disabled="locked" />{{
              copy('instructions')
            }}</label
          >
          <p>{{ copy('export_notice') }}</p>
        </div>
        <article v-for="hit in page.items" :key="hit.id" class="search-hit">
          <header>
            <label class="inline"
              ><input
                type="checkbox"
                :checked="hit.id in selection"
                :disabled="locked"
                @change="toggle(hit, ($event.target as HTMLInputElement).checked)"
              /><strong>{{ hit.title }}</strong></label
            ><span
              >{{ copy(hit.kind) }} · {{ hit.archived ? copy('archived') : copy('active') }} ·
              {{ hit.unread ? copy('unread') : copy('read') }}</span
            >
          </header>
          <p>{{ t('workspace.explorer_counts', { count: hit.count, matches: hit.matched_messages }) }}</p>
          <blockquote v-for="match in hit.matches" :key="match.id">
            <small>{{ t(`workspace.role_${match.role}` as CopyKey) }}</small>
            <p>{{ match.snippet }}</p>
          </blockquote>
          <form v-if="renameId === hit.id" @submit.prevent="rename">
            <label>{{ copy('name') }}<input v-model="renameTitle" maxlength="256" :disabled="locked" /></label
            ><button type="submit" :disabled="locked || !renameTitle.trim()">{{ copy('save_name') }}</button
            ><button type="button" :disabled="locked" @click="renameId = null">{{ copy('cancel') }}</button>
          </form>
          <div class="actions">
            <button :disabled="locked" @click="emit('open', hit)">{{ copy('open') }}</button
            ><button :disabled="locked" @click="beginRename(hit)">{{ copy('rename') }}</button
            ><button :disabled="locked" @click="exportTranscript(hit)">{{ copy('export') }}</button>
          </div>
        </article>
        <p v-if="!page.items.length">{{ copy('empty') }}</p>
        <nav class="actions" :aria-label="copy('pages')">
          <button
            :disabled="locked || page.offset === 0"
            @click="search(Math.max(0, page.offset - page.limit))"
          >
            {{ copy('previous') }}</button
          ><span>{{ t('workspace.explorer_page', { page: Math.floor(page.offset / page.limit) + 1 }) }}</span
          ><button
            :disabled="locked || page.offset + page.limit >= page.total"
            @click="search(page.offset + page.limit)"
          >
            {{ copy('next') }}
          </button>
        </nav>
      </template>
    </div>
  </section>
</template>
<style scoped>
.explorer {
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-3);
}
.explorer-content {
  display: grid;
  gap: var(--mf-sp-3);
}
.filters,
.actions,
.export-options {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: end;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
.inline {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: center;
}
.query {
  flex: 1;
  min-width: 160px;
}
.search-hit {
  padding: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-sm);
  overflow-wrap: anywhere;
}
.search-hit header {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: var(--mf-sp-2);
}
blockquote {
  margin: var(--mf-sp-2) 0;
  padding: var(--mf-sp-2);
  border-left: 2px solid var(--mf-border);
  white-space: pre-wrap;
}
small,
p,
header span {
  color: var(--mf-text-2);
}
.outcomes {
  max-height: 180px;
  overflow: auto;
}
.error {
  color: var(--mf-danger);
}
</style>
