<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'

interface Entry {
  id: string
  title: string
  account: string
  password: string
  url: string
  notes: string
  totp: { secret: string; algorithm: string; digits: number; period: number } | null
}
interface View {
  entry: Entry
  revision: string
  has_password: boolean
  has_totp: boolean
  totp_settings?: { algorithm: string; digits: number; period: number }
}
interface Status {
  supported: boolean
  exists: boolean
  unlocked: boolean
  blocked: boolean
  remaining_ms: number
}
const paused = useToolPrivacy()
const status = ref<Status>({
  supported: true,
  exists: false,
  unlocked: false,
  blocked: false,
  remaining_ms: 0,
})
const ready = ref(false)
const busy = ref(false)
const master = ref('')
const masterConfirm = ref('')
const changingMaster = ref(false)
const items = ref<{ id: string; title: string }[]>([])
const revision = ref('')
const view = ref<View | null>(null)
const baseline = ref('')
const clearPassword = ref(false)
const clearTotp = ref(false)
const totpSecret = ref('')
const totpAlgorithm = ref('SHA1')
const totpDigits = ref(6)
const totpPeriod = ref(30)
const totpBaseline = ref('')
const revealed = ref<{ value: string; field: string } | null>(null)
const error = ref('')
const notice = ref('')
const dirty = computed(
  () =>
    !!view.value &&
    (JSON.stringify(view.value.entry) !== baseline.value ||
      !!totpSecret.value ||
      clearPassword.value ||
      clearTotp.value ||
      (view.value.has_totp &&
        JSON.stringify([totpAlgorithm.value, totpDigits.value, totpPeriod.value]) !== totpBaseline.value)),
)
let generation = 0
let disposed = false
let revealUntil = 0
let leaseUntil = 0
let poll: ReturnType<typeof setInterval> | undefined
let closeStop: UnlistenFn | undefined
let allowClose = false
async function call<T>(method: string, payload: Record<string, unknown> = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.vaultRequest(method, JSON.stringify(payload)))) as T
}
function scrubEditor(): void {
  if (view.value) {
    view.value.entry.password = ''
    view.value.entry.account = ''
    view.value.entry.notes = ''
    if (view.value.entry.totp) view.value.entry.totp.secret = ''
  }
  view.value = null
  baseline.value = ''
  totpSecret.value = ''
  clearPassword.value = false
  clearTotp.value = false
  revealed.value = null
  revealUntil = 0
}
function scrub(): void {
  generation++
  scrubEditor()
  items.value = []
  revision.value = ''
  master.value = ''
  masterConfirm.value = ''
  changingMaster.value = false
  leaseUntil = 0
}
async function loadList(): Promise<void> {
  const current = generation
  const result = await call<{ items: { id: string; title: string }[]; revision: string }>('list')
  if (disposed || paused.value || current !== generation) return
  items.value = result.items
  revision.value = result.revision
}
async function refresh(): Promise<void> {
  if (paused.value || disposed) return
  const current = generation
  const next = await call<Status>('status')
  if (disposed || paused.value || current !== generation) return
  const wasUnlocked = status.value.unlocked
  status.value = next
  ready.value = true
  leaseUntil = performance.now() + next.remaining_ms
  if (!next.unlocked) {
    if (wasUnlocked || view.value) scrub()
  } else if (!wasUnlocked) await loadList()
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
    if (typeof e === 'object' && e && 'message_key' in e && e.message_key === 'vault.locked') {
      scrub()
      status.value.unlocked = false
    }
  } finally {
    master.value = ''
    masterConfirm.value = ''
    busy.value = false
  }
}
async function unlock(): Promise<void> {
  if (!status.value.exists && master.value !== masterConfirm.value) {
    error.value = t('vault.mismatch')
    return
  }
  const current = generation
  await action(async () => {
    await call(status.value.exists ? 'unlock' : 'create', { master: master.value })
    if (current !== generation || paused.value || disposed) {
      await call('lock')
      return
    }
    await refresh()
  })
}
async function lock(): Promise<void> {
  scrub()
  status.value.unlocked = false
  try {
    await call('lock')
  } catch {
    error.value = t('vault.failed')
  }
}
function select(next: View): void {
  scrubEditor()
  view.value = next
  baseline.value = JSON.stringify(next.entry)
  totpAlgorithm.value = next.totp_settings?.algorithm ?? 'SHA1'
  totpDigits.value = next.totp_settings?.digits ?? 6
  totpPeriod.value = next.totp_settings?.period ?? 30
  totpBaseline.value = JSON.stringify([totpAlgorithm.value, totpDigits.value, totpPeriod.value])
}
async function canLeave(): Promise<boolean> {
  if (dirty.value && !(await confirm(t('vault.discard'), { title: 'Moodflow', kind: 'warning' })))
    return false
  await lock()
  return true
}
async function choose(id: string): Promise<void> {
  if (dirty.value && !(await confirm(t('vault.discard'), { title: 'Moodflow', kind: 'warning' }))) return
  const current = generation
  await action(async () => {
    const next = await call<View>('get', { id, revision: revision.value })
    if (current === generation && !paused.value && !disposed) select(next)
  })
}
async function create(): Promise<void> {
  if (dirty.value && !(await confirm(t('vault.discard'), { title: 'Moodflow', kind: 'warning' }))) return
  const current = generation
  await action(async () => {
    const next = await call<View>('new')
    if (current === generation && !paused.value && !disposed) {
      select(next)
      baseline.value = ''
    }
  })
}
async function persist(): Promise<void> {
  if (!view.value) return
  const current = generation
  await action(async () => {
    const entry = JSON.parse(JSON.stringify(view.value!.entry)) as Entry
    const secret = totpSecret.value.trim()
    const uri = secret.startsWith('otpauth://') ? secret : ''
    entry.totp =
      secret && !uri
        ? { secret, algorithm: totpAlgorithm.value, digits: totpDigits.value, period: totpPeriod.value }
        : null
    const result = await call<{ id: string; revision: string }>('save', {
      entry,
      revision: view.value!.revision,
      keep_password: view.value!.has_password && !entry.password && !clearPassword.value,
      keep_totp: view.value!.has_totp && !secret && !clearTotp.value,
      totp_uri: uri,
      totp_settings: { algorithm: totpAlgorithm.value, digits: totpDigits.value, period: totpPeriod.value },
    })
    entry.password = ''
    if (entry.totp) entry.totp.secret = ''
    if (current !== generation || paused.value || disposed) return
    notice.value = t('vault.saved')
    scrubEditor()
    await loadList()
    const saved = await call<View>('get', { id: result.id, revision: result.revision })
    if (current === generation && !paused.value && !disposed) {
      select(saved)
      notice.value = t('vault.saved')
    }
  })
}
async function remove(): Promise<void> {
  if (!view.value || !(await confirm(t('vault.delete_question'), { title: 'Moodflow', kind: 'warning' })))
    return
  await action(async () => {
    await call('delete', { id: view.value!.entry.id, revision: view.value!.revision })
    scrubEditor()
    await loadList()
    notice.value = t('vault.deleted')
  })
}
async function reveal(field: string): Promise<void> {
  if (!view.value) return
  const current = generation
  await action(async () => {
    const result = await call<{ value: string; reveal_ms: number; totp_remaining?: number }>('reveal', {
      id: view.value!.entry.id,
      revision: view.value!.revision,
      field,
    })
    if (current !== generation || paused.value || disposed) {
      result.value = ''
      return
    }
    revealed.value = { value: result.value, field }
    result.value = ''
    revealUntil = performance.now() + Math.min(result.reveal_ms, (result.totp_remaining ?? 15) * 1000)
  })
}
async function copy(field: string): Promise<void> {
  if (!view.value) return
  await action(async () => {
    await call('copy', { id: view.value!.entry.id, revision: view.value!.revision, field })
    notice.value = t('vault.copied')
  })
}
async function generate(): Promise<void> {
  const current = generation
  await action(async () => {
    const result = await call<{ value: string }>('generate', { length: 24, symbols: true })
    if (view.value && current === generation && !paused.value && !disposed)
      view.value.entry.password = result.value
    result.value = ''
    notice.value = t('vault.generated')
  })
}
async function copySelection(event: ClipboardEvent, cut = false): Promise<void> {
  event.preventDefault()
  if (!status.value.unlocked || paused.value) return
  const target = event.target
  const input = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement ? target : null
  if (input?.type === 'password') return
  const original = input?.value ?? ''
  const start = input?.selectionStart ?? 0
  const end = input?.selectionEnd ?? 0
  const text = input ? original.slice(start, end) : (window.getSelection()?.toString() ?? '')
  if (!text) return
  const current = generation
  await action(async () => {
    await call('copy_draft', { text })
    if (cut && input && input.value === original && current === generation) {
      input.setRangeText('', start, end, 'end')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    }
    notice.value = t('vault.copied')
  })
}
async function changeMaster(): Promise<void> {
  if (master.value !== masterConfirm.value) {
    error.value = t('vault.mismatch')
    return
  }
  await action(async () => {
    await call('change_master', { master: master.value, revision: revision.value })
    await lock()
    notice.value = t('vault.master_changed')
  })
}
watch(paused, (value) => {
  if (value) void lock()
  else
    void refresh().catch((e) => {
      error.value = errorText(e)
    })
})
onMounted(async () => {
  poll = setInterval(() => {
    if (revealed.value && performance.now() >= revealUntil) revealed.value = null
    if (status.value.unlocked && performance.now() >= leaseUntil) {
      scrub()
      status.value.unlocked = false
      void call('lock').catch(() => {})
    }
    if (!busy.value && !paused.value)
      void refresh().catch((e) => {
        error.value = errorText(e)
      })
  }, 500)
  const stop = await getCurrentWindow().onCloseRequested(async (event) => {
    if (allowClose) return
    event.preventDefault()
    if (await canLeave()) {
      allowClose = true
      await getCurrentWindow().close()
    }
  })
  if (disposed) stop()
  else closeStop = stop
})
onBeforeUnmount(() => {
  disposed = true
  clearInterval(poll)
  closeStop?.()
  scrub()
  void call('lock').catch(() => {})
})
defineExpose({ canLeave })
</script>

<template>
  <section v-if="paused || status.blocked" class="empty">{{ t('library.privacy') }}</section>
  <section
    v-else
    class="vault"
    @copy.capture="copySelection($event)"
    @cut.capture="copySelection($event, true)"
  >
    <header>
      <div>
        <span class="eyebrow">{{ t('vault.local') }}</span>
        <h1>{{ t('vault.title') }}</h1>
        <p>{{ t('vault.description') }}</p>
      </div>
      <button v-if="status.unlocked" :disabled="busy" @click="lock">{{ t('vault.lock') }}</button>
    </header>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="!ready">{{ t('vault.loading') }}</p>
    <p v-else-if="!status.supported">{{ t('vault.unsupported') }}</p>
    <form v-else-if="!status.unlocked" class="unlock" @submit.prevent="unlock">
      <h2>{{ t(status.exists ? 'vault.unlock_title' : 'vault.create_title') }}</h2>
      <p>{{ t('vault.key_notice') }}</p>
      <label
        >{{ t('vault.master')
        }}<input v-model="master" type="password" autocomplete="off" maxlength="1024" :disabled="busy"
      /></label>
      <label v-if="!status.exists"
        >{{ t('vault.confirm_master')
        }}<input v-model="masterConfirm" type="password" autocomplete="off" maxlength="1024" :disabled="busy"
      /></label>
      <button class="primary" type="submit" :disabled="busy || !master">
        {{ t(status.exists ? 'vault.unlock' : 'vault.create') }}
      </button>
    </form>
    <template v-else>
      <div class="toolbar">
        <button :disabled="busy" @click="create">{{ t('vault.new') }}</button
        ><button :disabled="busy || dirty" @click="changingMaster = !changingMaster">
          {{ t('vault.change_master') }}</button
        ><span>{{ t('vault.lease') }}</span>
      </div>
      <form v-if="changingMaster" class="master-change" @submit.prevent="changeMaster">
        <label
          >{{ t('vault.new_master')
          }}<input
            v-model="master"
            type="password"
            maxlength="1024"
            autocomplete="off"
            :disabled="busy" /></label
        ><label
          >{{ t('vault.confirm_master')
          }}<input
            v-model="masterConfirm"
            type="password"
            maxlength="1024"
            autocomplete="off"
            :disabled="busy" /></label
        ><button :disabled="busy || !master" type="submit">{{ t('vault.change_master') }}</button>
      </form>
      <div class="workspace">
        <aside>
          <button
            v-for="item in items"
            :key="item.id"
            :disabled="busy"
            :class="{ selected: view?.entry.id === item.id }"
            @click="choose(item.id)"
          >
            {{ item.title }}
          </button>
          <p v-if="!items.length">{{ t('vault.empty') }}</p>
        </aside>
        <form v-if="view" class="editor" autocomplete="off" @submit.prevent="persist">
          <label
            >{{ t('vault.entry_title')
            }}<input v-model="view.entry.title" maxlength="256" :disabled="busy" /></label
          ><label
            >{{ t('vault.account')
            }}<input v-model="view.entry.account" maxlength="4096" autocomplete="off" :disabled="busy"
          /></label>
          <label
            >{{ t('vault.password')
            }}<input
              v-model="view.entry.password"
              type="password"
              autocomplete="new-password"
              maxlength="4096"
              :placeholder="view.has_password ? t('vault.keep_password') : ''"
              :disabled="busy"
          /></label>
          <div class="actions">
            <button type="button" :disabled="busy" @click="generate">{{ t('vault.generate') }}</button
            ><label v-if="view.has_password" class="check"
              ><input v-model="clearPassword" type="checkbox" :disabled="busy" />{{
                t('vault.clear_password')
              }}</label
            >
          </div>
          <label
            >{{ t('vault.url') }}<input v-model="view.entry.url" maxlength="4096" :disabled="busy" /></label
          ><label
            >{{ t('vault.notes')
            }}<textarea v-model="view.entry.notes" maxlength="8192" spellcheck="false" :disabled="busy" />
          </label>
          <details>
            <summary>{{ t('vault.totp') }}</summary>
            <p>{{ t('vault.totp_help') }}</p>
            <label
              >{{ t('vault.totp_secret')
              }}<input
                v-model="totpSecret"
                type="password"
                maxlength="2048"
                autocomplete="off"
                :placeholder="view.has_totp ? t('vault.keep_totp') : ''"
                :disabled="busy"
            /></label>
            <div class="actions">
              <select v-model="totpAlgorithm" :disabled="busy">
                <option v-for="algorithm in ['SHA1', 'SHA256', 'SHA512']" :key="algorithm" :value="algorithm">
                  {{ algorithm }}
                </option></select
              ><select v-model="totpDigits" :disabled="busy">
                <option :value="6">{{ t('vault.six_digits') }}</option>
                <option :value="8">{{ t('vault.eight_digits') }}</option></select
              ><label
                >{{ t('vault.period')
                }}<input v-model.number="totpPeriod" type="number" min="15" max="120" :disabled="busy"
              /></label>
            </div>
            <label v-if="view.has_totp" class="check"
              ><input v-model="clearTotp" type="checkbox" :disabled="busy" />{{
                t('vault.clear_totp')
              }}</label
            >
          </details>
          <div class="actions">
            <button class="primary" type="submit" :disabled="busy || !dirty || !view.entry.title.trim()">
              {{ t('library.save') }}</button
            ><button
              type="button"
              :disabled="busy || !items.some((i) => i.id === view!.entry.id)"
              @click="remove"
            >
              {{ t('library.delete') }}
            </button>
          </div>
          <div v-if="!dirty" class="actions">
            <button v-if="view.has_password" type="button" :disabled="busy" @click="reveal('password')">
              {{ t('vault.reveal_password') }}</button
            ><button v-if="view.has_password" type="button" :disabled="busy" @click="copy('password')">
              {{ t('vault.copy_password') }}</button
            ><button type="button" :disabled="busy || !view.entry.account" @click="copy('account')">
              {{ t('vault.copy_account') }}</button
            ><button v-if="view.has_totp" type="button" :disabled="busy" @click="reveal('totp')">
              {{ t('vault.reveal_totp') }}</button
            ><button v-if="view.has_totp" type="button" :disabled="busy" @click="copy('totp')">
              {{ t('vault.copy_totp') }}
            </button>
          </div>
          <output v-if="revealed" class="revealed" aria-live="polite">{{ revealed.value }}</output
          ><small v-if="revealed">{{ t('vault.reveal_notice') }}</small>
        </form>
        <p v-else class="empty">{{ t('vault.select') }}</p>
      </div>
    </template>
  </section>
</template>

<style scoped>
.vault {
  display: grid;
  gap: var(--mf-sp-3);
  max-width: 1000px;
  margin: auto;
}
header,
.toolbar,
.actions {
  display: flex;
  align-items: center;
  gap: var(--mf-sp-2);
  flex-wrap: wrap;
}
header {
  justify-content: space-between;
}
h1 {
  font-family: Georgia, 'Noto Serif SC', 'SimSun', serif;
  margin: var(--mf-sp-2) 0;
}
header p,
small,
.toolbar span {
  color: var(--mf-text-2);
}
.eyebrow {
  color: var(--mf-link);
}
.unlock,
.master-change {
  max-width: 480px;
  padding: var(--mf-sp-5);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface);
}
label {
  display: grid;
  gap: var(--mf-sp-1);
  margin-block: var(--mf-sp-3);
}
.workspace {
  display: grid;
  grid-template-columns: 200px minmax(0, 1fr);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  overflow: hidden;
}
aside {
  padding: var(--mf-sp-3);
  background: var(--mf-surface-2);
}
aside button {
  display: block;
  width: 100%;
  text-align: left;
  margin-bottom: var(--mf-sp-2);
  overflow-wrap: anywhere;
}
aside .selected {
  border-color: var(--mf-primary);
}
.editor {
  padding: var(--mf-sp-4);
  min-width: 0;
}
input,
textarea {
  min-width: 0;
}
textarea {
  min-height: 100px;
  resize: vertical;
}
.check {
  display: flex;
  align-items: center;
}
.primary {
  background: var(--mf-primary);
  color: var(--mf-on-primary);
}
.revealed {
  display: block;
  font-family: Consolas, monospace;
  padding: var(--mf-sp-4);
  background: var(--mf-surface-2);
  overflow-wrap: anywhere;
  user-select: none;
}
.error {
  color: var(--mf-danger);
}
@media (max-width: 700px) {
  .workspace {
    grid-template-columns: 1fr;
  }
  aside {
    max-height: 180px;
    overflow: auto;
  }
}
</style>
