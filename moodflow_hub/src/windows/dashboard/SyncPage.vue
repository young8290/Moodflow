<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open, confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import SyncVaultCopies from './SyncVaultCopies.vue'
type Target = { kind: 'folder'; path: string } | { kind: 'webdav'; url: string; username: string }
interface Profile {
  target: Target
  encrypted: boolean
}
interface Status {
  profile: Profile | null
  revision: string | null
  busy: boolean
}
interface Conflict {
  name: string
  title: string
  kind: string
  updated_at: string
}
interface Preview {
  document: { metadata: { title: string }; body: string }
  revision: string
}
interface Report {
  uploaded: number
  downloaded: number
  deleted: number
  conflicts: number
  deferred: number
}
const paused = useToolPrivacy()
const kind = ref<'folder' | 'webdav'>('folder')
const path = ref(''),
  url = ref(''),
  username = ref(''),
  password = ref(''),
  recovery = ref('')
const encrypted = ref(true),
  consent = ref(false),
  plaintext = ref(false)
const includeVault = ref(false)
const revision = ref<string | null>(null),
  saved = ref<Profile | null>(null)
const busy = ref(false),
  nativeBusy = ref(false),
  error = ref(''),
  notice = ref('')
const conflicts = ref<Conflict[]>([]),
  selected = ref<string | null>(null),
  preview = ref<Preview | null>(null)
const report = ref<(Report & { skipped: number }) | null>(null)
let generation = 0,
  disposed = false
const profile = computed<Profile>(() => ({
  target:
    kind.value === 'folder'
      ? { kind: 'folder', path: path.value }
      : { kind: 'webdav', url: url.value, username: username.value },
  encrypted: encrypted.value,
}))
const dirty = computed(() => JSON.stringify(profile.value) !== JSON.stringify(saved.value))
const ready = computed(
  () =>
    !busy.value &&
    !nativeBusy.value &&
    !paused.value &&
    !dirty.value &&
    saved.value &&
    consent.value &&
    (encrypted.value ? recovery.value.length >= 24 : plaintext.value),
)
async function call<T>(method: string, params: unknown = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.syncRequest(method, JSON.stringify(params)))) as T
}
function applyStatus(status: Status): void {
  saved.value = status.profile
  revision.value = status.revision
  nativeBusy.value = status.busy
  if (status.profile) {
    const target = status.profile.target
    kind.value = target.kind
    encrypted.value = status.profile.encrypted
    if (target.kind === 'folder') path.value = target.path
    else {
      url.value = target.url
      username.value = target.username
    }
  }
}
async function load(): Promise<void> {
  const mine = ++generation
  if (paused.value || disposed) return
  try {
    const [status, list] = await Promise.all([
      call<Status>('status'),
      call<{ items: Conflict[] }>('conflicts'),
    ])
    if (mine !== generation || paused.value || disposed) return
    applyStatus(status)
    conflicts.value = list.items
  } catch (e) {
    if (mine === generation && !paused.value && !disposed) error.value = errorText(e)
  }
}
async function action(work: () => Promise<void>, secrets = false): Promise<void> {
  if (busy.value || paused.value || disposed) return
  const mine = ++generation
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await work()
  } catch (e) {
    if (mine === generation && !paused.value && !disposed) error.value = errorText(e)
  } finally {
    if (secrets) {
      password.value = ''
      recovery.value = ''
      consent.value = false
      includeVault.value = false
    }
    if (mine === generation) busy.value = false
  }
}
async function choose(): Promise<void> {
  const mine = generation
  const result = await open({ directory: true, multiple: false })
  if (typeof result === 'string' && mine === generation && !paused.value && !disposed) path.value = result
}
async function saveProfile(): Promise<void> {
  await action(async () => {
    const mine = generation
    await call('save', {
      profile: profile.value,
      revision: revision.value,
      confirm_plaintext: plaintext.value,
    })
    const status = await call<Status>('status')
    if (mine === generation && !paused.value && !disposed) {
      applyStatus(status)
      notice.value = t('sync.saved')
    }
  })
}
async function run(method: 'initialize' | 'run' | 'repair'): Promise<void> {
  if (!ready.value) return
  const mine = generation
  if (
    method !== 'run' &&
    (!(await confirm(t(method === 'initialize' ? 'sync.initialize_confirm' : 'sync.repair_confirm'), {
      title: 'Moodflow',
      kind: 'warning',
    })) ||
      mine !== generation ||
      paused.value ||
      disposed)
  )
    return
  await action(async () => {
    const current = generation
    report.value = null
    const value = await call<{ report?: Report; skipped?: number }>(method, {
      consent: consent.value,
      recovery: recovery.value,
      password: password.value,
      confirm_plaintext: plaintext.value,
      confirm: true,
      include_vault: includeVault.value,
    })
    if (current !== generation || paused.value || disposed) return
    notice.value = t(
      method === 'initialize' ? 'sync.initialized' : method === 'repair' ? 'sync.repaired' : 'sync.completed',
    )
    if (value.report) report.value = { ...value.report, skipped: value.skipped ?? 0 }
    const list = await call<{ items: Conflict[] }>('conflicts')
    if (current === generation && !paused.value && !disposed) {
      conflicts.value = list.items
      nativeBusy.value = false
    }
  }, true)
}
async function stop(): Promise<void> {
  try {
    await call('cancel')
    notice.value = t('sync.cancel_requested')
  } catch (e) {
    error.value = errorText(e)
  }
}
async function inspect(name: string): Promise<void> {
  await action(async () => {
    const mine = generation
    const result = await call<Preview>('conflict', { name })
    if (mine === generation && !paused.value && !disposed) {
      selected.value = name
      preview.value = result
    }
  })
}
async function resolve(method: 'restore_conflict' | 'delete_conflict'): Promise<void> {
  if (!preview.value || !selected.value) return
  const mine = generation,
    name = selected.value,
    expected = preview.value.revision
  if (
    !(await confirm(t(method === 'restore_conflict' ? 'sync.restore_confirm' : 'sync.delete_confirm'), {
      title: 'Moodflow',
      kind: 'warning',
    })) ||
    mine !== generation ||
    paused.value ||
    disposed
  )
    return
  await action(async () => {
    const current = generation
    await call(method, { name, revision: expected, confirm: true })
    if (current !== generation || paused.value || disposed) return
    notice.value = t(method === 'restore_conflict' ? 'sync.restored' : 'sync.conflict_deleted')
    preview.value = null
    selected.value = null
    const list = await call<{ items: Conflict[] }>('conflicts')
    if (current === generation && !paused.value && !disposed) conflicts.value = list.items
  })
}
watch(paused, () => {
  includeVault.value = false
  generation++
  password.value = ''
  recovery.value = ''
  consent.value = false
  plaintext.value = false
  preview.value = null
  selected.value = null
  report.value = null
  conflicts.value = []
  error.value = ''
  notice.value = ''
  busy.value = false
  if (paused.value) void call('cancel').catch(() => {})
  else void load()
})
watch(encrypted, () => {
  includeVault.value = false
  recovery.value = ''
  plaintext.value = false
  consent.value = false
})
watch([kind, path, url, username], () => {
  password.value = ''
  consent.value = false
})
onMounted(() => void load())
onBeforeUnmount(() => {
  disposed = true
  generation++
  password.value = ''
  recovery.value = ''
})
</script>
<template>
  <section class="sync-page">
    <header>
      <div>
        <h2>{{ t('sync.title') }}</h2>
        <p class="hint">{{ t('sync.description') }}</p>
      </div>
      <button :disabled="busy || paused" @click="load">{{ t('sync.refresh') }}</button>
    </header>
    <p v-if="paused" role="status">{{ t('sync.privacy') }}</p>
    <template v-else>
      <p class="scope">{{ t('sync.scope') }}</p>
      <form class="settings" @submit.prevent="saveProfile">
        <label
          >{{ t('sync.target')
          }}<select v-model="kind" :disabled="busy || nativeBusy">
            <option value="folder">{{ t('sync.folder') }}</option>
            <option value="webdav">{{ t('sync.webdav') }}</option>
          </select></label
        >
        <label v-if="kind === 'folder'"
          >{{ t('sync.directory') }}
          <div class="row">
            <input v-model="path" :disabled="busy || nativeBusy" /><button
              type="button"
              :disabled="busy || nativeBusy"
              @click="choose"
            >
              {{ t('sync.choose') }}
            </button>
          </div></label
        >
        <template v-else
          ><label
            >{{ t('sync.url')
            }}<input
              v-model="url"
              type="url"
              :placeholder="t('sync.url_placeholder')"
              :disabled="busy || nativeBusy" /></label
          ><label
            >{{ t('sync.username')
            }}<input v-model="username" autocomplete="off" :disabled="busy || nativeBusy" /></label
        ></template>
        <label class="check"
          ><input v-model="encrypted" type="checkbox" :disabled="busy || nativeBusy" />{{
            t('sync.encrypted')
          }}</label
        >
        <p class="hint">{{ t('sync.encryption_hint') }}</p>
        <label v-if="!encrypted" class="check warning"
          ><input v-model="plaintext" type="checkbox" :disabled="busy || nativeBusy" />{{
            t('sync.plaintext_confirm')
          }}</label
        >
        <button type="submit" :disabled="busy || nativeBusy || !dirty || (!encrypted && !plaintext)">
          {{ t('sync.save') }}
        </button>
      </form>
      <section class="run-panel">
        <label v-if="kind === 'webdav'"
          >{{ t('sync.password')
          }}<input v-model="password" type="password" autocomplete="off" :disabled="busy || nativeBusy"
        /></label>
        <label v-if="encrypted"
          >{{ t('sync.recovery_label')
          }}<input
            v-model="recovery"
            type="password"
            autocomplete="off"
            :disabled="busy || nativeBusy"
            maxlength="2048"
        /></label>
        <p class="hint">{{ t('sync.secrets_hint') }}</p>
        <label v-if="encrypted" class="check"
          ><input v-model="includeVault" type="checkbox" :disabled="busy || nativeBusy" />{{
            t('sync.include_backups')
          }}</label
        >
        <label class="check"
          ><input v-model="consent" type="checkbox" :disabled="busy || nativeBusy" />{{
            t('sync.consent_label')
          }}</label
        >
        <div class="row">
          <button :disabled="!ready" @click="run('run')">{{ t('sync.run') }}</button
          ><button :disabled="!ready" @click="run('initialize')">{{ t('sync.initialize') }}</button
          ><button v-if="kind === 'folder'" :disabled="!ready" @click="run('repair')">
            {{ t('sync.repair') }}</button
          ><button v-if="busy || nativeBusy" @click="stop">{{ t('sync.cancel') }}</button>
        </div>
        <p v-if="dirty" class="hint">{{ t('sync.unsaved') }}</p>
        <p v-if="busy || nativeBusy" role="status">{{ t('sync.running') }}</p>
      </section>
      <SyncVaultCopies :paused="paused" :busy="busy || nativeBusy" :encrypted="encrypted" />
      <p v-if="error" role="alert" class="error">{{ error }}</p>
      <p v-if="notice" role="status">{{ notice }}</p>
      <p v-if="report" class="scope">
        {{
          t('sync.report', {
            uploaded: report.uploaded,
            downloaded: report.downloaded,
            deleted: report.deleted,
            conflicts: report.conflicts,
            deferred: report.deferred + report.skipped,
          })
        }}
      </p>
      <section class="conflicts">
        <h3>{{ t('sync.conflicts') }}</h3>
        <p class="hint">{{ t('sync.conflicts_hint') }}</p>
        <p v-if="!conflicts.length">{{ t('sync.no_conflicts') }}</p>
        <ul>
          <li v-for="item in conflicts" :key="item.name">
            <button :disabled="busy" @click="inspect(item.name)">{{ item.title }}</button
            ><small>{{ item.updated_at }}</small>
          </li>
        </ul>
        <article v-if="preview">
          <h4>{{ preview.document.metadata.title }}</h4>
          <textarea :value="preview.document.body" readonly :aria-label="t('sync.conflict_body')" />
          <div class="row">
            <button :disabled="busy" @click="resolve('restore_conflict')">{{ t('sync.restore') }}</button
            ><button :disabled="busy" @click="resolve('delete_conflict')">{{ t('sync.delete') }}</button>
          </div>
        </article>
      </section>
    </template>
  </section>
</template>
<style scoped>
.sync-page {
  display: grid;
  gap: var(--mf-sp-4);
  max-width: 960px;
  margin: 0 auto;
  width: 100%;
  padding: var(--mf-sp-5);
}
header,
.row {
  display: flex;
  align-items: center;
  gap: var(--mf-sp-3);
  flex-wrap: wrap;
}
header {
  justify-content: space-between;
  align-items: flex-start;
}
h2,
h3,
h4,
p {
  margin: 0;
}
.hint,
small {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.settings,
.run-panel,
.conflicts {
  display: grid;
  gap: var(--mf-sp-3);
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}
label {
  display: grid;
  gap: var(--mf-sp-2);
}
.check {
  display: flex;
  align-items: center;
  gap: var(--mf-sp-2);
}
input:not([type='checkbox']),
select,
textarea {
  width: 100%;
  box-sizing: border-box;
  padding: var(--mf-sp-2);
  background: var(--mf-surface);
  color: var(--mf-text-1);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-sm);
}
.row input {
  flex: 1;
  min-width: 160px;
}
.check input {
  width: auto;
}
button {
  padding: var(--mf-sp-2) var(--mf-sp-3);
}
button:focus-visible,
input:focus-visible,
select:focus-visible,
textarea:focus-visible {
  outline: 2px solid var(--mf-focus);
  outline-offset: 2px;
}
.scope {
  padding: var(--mf-sp-3);
  border-left: 3px solid var(--mf-focus);
  color: var(--mf-text-2);
}
.error,
.warning {
  color: var(--mf-danger);
}
ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
li {
  display: flex;
  gap: var(--mf-sp-3);
  align-items: center;
  padding: var(--mf-sp-2) 0;
}
article {
  display: grid;
  gap: var(--mf-sp-3);
}
textarea {
  min-height: 180px;
  resize: vertical;
}
</style>
