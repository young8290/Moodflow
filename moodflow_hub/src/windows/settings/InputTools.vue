<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'

interface Mapping {
  keys: string[]
  output: string
  kind: 'syllable' | 'fragment'
}
interface Profile {
  version: number
  name: string
  left_keys: string[]
  right_keys: string[]
  interval_ms: number
  mappings: Mapping[]
}
interface ChordState {
  text: string
  revision: string
  activeRevision: string
  enabled: boolean
}
interface RimePreview {
  sha256: string
  schemes: { id: string; name: string; engine: string; preview: { willAdd: string[]; conflicts: string[] } }[]
  diagnostics: string[]
}
interface ImportResult {
  results: { id: string; error?: string; result?: { imported: string[]; conflicts: string[] } }[]
}

const profile = ref<Profile | null>(null)
const revision = ref('missing')
const activeRevision = ref('missing')
const unavailableStore = ref(false)
const busy = ref(false)
const error = ref('')
const notice = ref('')
const archive = ref('')
const preview = ref<RimePreview | null>(null)
const selected = ref<string[]>([])
const replace = ref(false)
const results = ref<ImportResult | null>(null)
const page = ref(0)
const rows = computed(() => profile.value?.mappings.slice(page.value * 25, (page.value + 1) * 25) ?? [])
const keyboard = ['q w e r t y u i o p', 'a s d f g h j k l semicolon', 'z x c v b n m comma period']

async function call<T>(method: string, payload: Record<string, unknown> = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.imeTools(method, JSON.stringify(payload)))) as T
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
function loadProfile(text: string): void {
  const value = JSON.parse(text) as Profile
  if (
    typeof value.name !== 'string' ||
    !Array.isArray(value.left_keys) ||
    !Array.isArray(value.right_keys) ||
    !Array.isArray(value.mappings)
  )
    throw new Error('invalid')
  profile.value = value
  page.value = 0
}
async function reload(): Promise<void> {
  await action(async () => {
    unavailableStore.value = !(await call<{ available: boolean }>('storage.health')).available
    const current = await call<ChordState>('chord.get')
    loadProfile(current.text)
    revision.value = current.revision
    activeRevision.value = current.activeRevision
  })
}
async function chooseArchive(): Promise<void> {
  const path = await open({ multiple: false, filters: [{ name: 'Rime ZIP', extensions: ['zip'] }] })
  if (typeof path !== 'string') return
  await action(async () => {
    preview.value = null
    results.value = null
    archive.value = path
    const value = await call<RimePreview>('scheme.rimePreview', { path })
    preview.value = value
    selected.value = value.schemes.map((s) => s.id)
  })
}
async function importSchemes(): Promise<void> {
  if (!preview.value) return
  await action(async () => {
    results.value = await call<ImportResult>('scheme.rimeImport', {
      path: archive.value,
      sha256: preview.value!.sha256,
      ids: selected.value,
      strategy: replace.value ? 'replace' : 'merge',
    })
  })
}
function list(value: string): string[] {
  return value.trim().split(/\s+/).filter(Boolean)
}
function cycleKey(key: string): void {
  const p = profile.value
  if (!p) return
  if (p.left_keys.includes(key)) {
    p.left_keys = p.left_keys.filter((k) => k !== key)
    p.right_keys.push(key)
  } else if (p.right_keys.includes(key)) p.right_keys = p.right_keys.filter((k) => k !== key)
  else p.left_keys.push(key)
}
async function saveProfile(apply: boolean): Promise<void> {
  if (!profile.value) return
  await action(async () => {
    const current = await call<ChordState>('chord.save', {
      text: JSON.stringify(profile.value),
      revision: revision.value,
      activeRevision: activeRevision.value,
      apply,
    })
    revision.value = current.revision
    activeRevision.value = current.activeRevision
    notice.value = t(apply ? 'tools.applied' : 'tools.saved')
  })
}
async function copyBuiltin(): Promise<void> {
  await action(async () => loadProfile((await call<{ text: string }>('chord.builtin')).text))
}
function newProfile(): void {
  loadProfile(
    JSON.stringify({ version: 1, name: '', left_keys: [], right_keys: [], interval_ms: 300, mappings: [] }),
  )
}
function addMapping(): void {
  profile.value?.mappings.unshift({ keys: [], output: '', kind: 'syllable' })
  page.value = 0
}
async function importProfile(): Promise<void> {
  const path = await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] })
  if (typeof path !== 'string') return
  await action(async () => loadProfile((await call<{ text: string }>('chord.readFile', { path })).text))
}
async function exportProfile(): Promise<void> {
  if (!profile.value) return
  const path = await save({ defaultPath: 'my-chord.json', filters: [{ name: 'JSON', extensions: ['json'] }] })
  if (!path) return
  await action(async () => {
    await call('chord.exportFile', { path, text: JSON.stringify(profile.value, null, 2) })
    notice.value = t('tools.saved')
  })
}
onMounted(() => void reload())
</script>

<template>
  <div class="input-tools" :aria-busy="busy">
    <p v-if="unavailableStore" class="fault" role="alert">{{ t('tools.storage_warning') }}</p>
    <p v-if="error" class="fault" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <div class="tool-card">
      <div class="tool-heading">
        <div>
          <h2>{{ t('tools.title') }}</h2>
          <p>{{ t('tools.intro') }}</p>
        </div>
        <button :disabled="busy" @click="reload">{{ t('tools.reload') }}</button>
      </div>
      <h3>{{ t('tools.rime') }}</h3>
      <button :disabled="busy" @click="chooseArchive">{{ t('tools.choose_zip') }}</button>
      <div v-if="preview" class="rime-preview">
        <h4>{{ t('tools.preview') }}</h4>
        <label v-for="scheme in preview.schemes" :key="scheme.id" class="scheme-row"
          ><input v-model="selected" type="checkbox" :value="scheme.id" :disabled="busy" />
          <strong>{{ scheme.name }}</strong
          ><span>{{
            t('tools.scheme_status', {
              engine: scheme.engine,
              added: scheme.preview.willAdd.length,
              conflicts: scheme.preview.conflicts.length,
            })
          }}</span></label
        >
        <details v-if="preview.diagnostics.length">
          <summary>{{ t('tools.diagnostics') }}</summary>
          <ul>
            <li v-for="d in preview.diagnostics" :key="d">{{ d }}</li>
          </ul>
        </details>
        <label><input v-model="replace" type="checkbox" :disabled="busy" /> {{ t('tools.replace') }}</label>
        <button :disabled="busy || !selected.length" @click="importSchemes">
          {{ t('tools.import_selected') }}
        </button>
      </div>
      <div v-if="results" role="status">
        <h4>{{ t('tools.import_done') }}</h4>
        <p v-for="result in results.results" :key="result.id">
          {{ result.id }} ·
          {{ result.error ?? `${result.result?.imported.length ?? 0} ${t('tools.files_imported')}`
          }}<span v-if="result.result?.conflicts.length">
            · {{ result.result.conflicts.length }} {{ t('tools.conflicts') }}</span
          >
        </p>
      </div>
    </div>
    <div class="tool-card">
      <h3>{{ t('tools.chord') }}</h3>
      <div class="actions">
        <button :disabled="busy" @click="copyBuiltin">{{ t('tools.copy_builtin') }}</button
        ><button :disabled="busy" @click="newProfile">{{ t('tools.new_profile') }}</button
        ><button :disabled="busy" @click="importProfile">{{ t('tools.import_json') }}</button
        ><button :disabled="busy || !profile" @click="exportProfile">{{ t('tools.export_json') }}</button>
      </div>
      <template v-if="profile">
        <label>{{ t('tools.name') }}<input v-model="profile.name" maxlength="128" :disabled="busy" /></label>
        <label
          >{{ t('tools.interval')
          }}<input v-model.number="profile.interval_ms" type="number" min="0" max="1000" :disabled="busy"
        /></label>
        <div class="keyboard" :aria-label="t('tools.keyboard_hint')">
          <div v-for="line in keyboard" :key="line">
            <button
              v-for="key in line.split(' ')"
              :key="key"
              :class="{ left: profile.left_keys.includes(key), right: profile.right_keys.includes(key) }"
              :disabled="busy"
              @click="cycleKey(key)"
            >
              {{
                key === 'semicolon' ? ';' : key === 'comma' ? ',' : key === 'period' ? '.' : key.toUpperCase()
              }}
            </button>
          </div>
        </div>
        <p>{{ t('tools.keyboard_hint') }}</p>
        <label
          >{{ t('tools.left')
          }}<input
            :value="profile.left_keys.join(' ')"
            :disabled="busy"
            @change="profile.left_keys = list(($event.target as HTMLInputElement).value)"
        /></label>
        <label
          >{{ t('tools.right')
          }}<input
            :value="profile.right_keys.join(' ')"
            :disabled="busy"
            @change="profile.right_keys = list(($event.target as HTMLInputElement).value)"
        /></label>
        <div class="mapping-header">
          <span>{{ t('tools.keys') }}</span
          ><span>{{ t('tools.output') }}</span
          ><span>{{ t('tools.kind') }}</span>
        </div>
        <div v-for="(row, index) in rows" :key="page * 25 + index" class="mapping-row">
          <input
            :aria-label="t('tools.keys')"
            :value="row.keys.join(' ')"
            :disabled="busy"
            @change="row.keys = list(($event.target as HTMLInputElement).value)"
          /><input v-model="row.output" :aria-label="t('tools.output')" :disabled="busy" /><select
            v-model="row.kind"
            :aria-label="t('tools.kind')"
            :disabled="busy"
          >
            <option value="syllable">{{ t('tools.syllable') }}</option>
            <option value="fragment">{{ t('tools.fragment') }}</option></select
          ><button
            :aria-label="t('tools.remove')"
            :disabled="busy"
            @click="profile.mappings.splice(page * 25 + index, 1)"
          >
            {{ t('tools.remove') }}
          </button>
        </div>
        <div class="actions">
          <button :disabled="busy || page === 0" @click="page--">{{ t('tools.previous') }}</button
          ><span>{{
            t('tools.page', {
              current: page + 1,
              total: Math.max(1, Math.ceil(profile.mappings.length / 25)),
            })
          }}</span
          ><button :disabled="busy || (page + 1) * 25 >= profile.mappings.length" @click="page++">
            {{ t('tools.next') }}</button
          ><button :disabled="busy" @click="addMapping">{{ t('tools.add_mapping') }}</button>
        </div>
        <div class="actions">
          <button :disabled="busy" @click="saveProfile(false)">{{ t('tools.save_draft') }}</button
          ><button class="primary" :disabled="busy" @click="saveProfile(true)">{{ t('tools.apply') }}</button>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.input-tools {
  display: grid;
  gap: var(--mf-sp-4);
  margin-block: var(--mf-sp-4);
}
.tool-card {
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface);
}
.tool-heading,
.actions,
.scheme-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--mf-sp-2);
}
.tool-heading {
  justify-content: space-between;
}
h2,
h3 {
  margin-top: 0;
}
p,
.scheme-row span {
  color: var(--mf-text-2);
}
label {
  display: grid;
  gap: var(--mf-sp-1);
  margin-block: var(--mf-sp-2);
}
.scheme-row {
  display: flex;
}
.actions {
  margin-block: var(--mf-sp-3);
}
.mapping-header,
.mapping-row {
  display: grid;
  grid-template-columns: 2fr 1fr 1.3fr 56px;
  gap: var(--mf-sp-2);
  margin-block: var(--mf-sp-1);
}
input,
select {
  min-width: 0;
}
.keyboard {
  margin-top: var(--mf-sp-4);
}
.keyboard > div {
  display: flex;
  gap: var(--mf-sp-1);
  margin-block: var(--mf-sp-1);
}
.keyboard button {
  min-width: 34px;
  padding: var(--mf-sp-1);
  color: var(--mf-text-3);
}
.keyboard .left {
  border-color: var(--mf-primary);
  color: var(--mf-link);
}
.keyboard .right {
  background: var(--mf-surface-2);
  color: var(--mf-text-1);
  border-style: dashed;
}
.fault {
  color: var(--mf-danger);
}
.primary {
  background: var(--mf-primary);
  color: var(--mf-on-primary);
}
@media (max-width: 680px) {
  .keyboard button {
    min-width: 24px;
  }
  .mapping-header,
  .mapping-row {
    grid-template-columns: 1.5fr 1fr 1fr 48px;
  }
}
</style>
