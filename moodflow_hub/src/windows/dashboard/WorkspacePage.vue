<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { confirm, open, save } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import WorkspaceTerminal from './WorkspaceTerminal.vue'
import WorkspaceExplorer from './WorkspaceExplorer.vue'
import {
  workspaceCall as call,
  type WorkspaceKind,
  type WorkspaceSummary,
  type WorkspaceReply,
  type WorkspaceMessage,
  type WorkspaceFile,
} from './workspaceTypes'
const paused = useToolPrivacy()
const kind = ref<WorkspaceKind>('chat')
const labels = { chat: 'workspace.chat', inbox: 'workspace.inbox', terminal: 'workspace.terminal' } as const
const sessions = ref<WorkspaceSummary[]>([])
const selected = ref<WorkspaceReply | null>(null)
const archived = ref(false)
const creating = ref(false)
const title = ref('')
const instructions = ref('')
const text = ref('')
const review = ref('')
const consent = ref(false)
const history = ref(true)
const busy = ref(false)
const loading = ref(false)
const error = ref('')
const notice = ref('')
const port = ref(18771)
const gateway = ref<{ enabled: boolean; address: string | null; terminal_supported: boolean }>({
  enabled: false,
  address: null,
  terminal_supported: false,
})
const pendingId = ref<string | null>(null)
let generation = 0
let operation = 0
let refreshing = false
let disposed = false
const visible = computed(() =>
  sessions.value.filter((s) => s.kind === kind.value && (archived.value || !s.archived)),
)
const current = computed(() => selected.value?.session ?? null)
async function action(run: () => Promise<void>): Promise<void> {
  if (busy.value || paused.value || disposed) return
  const mine = generation
  operation++
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    await run()
  } catch (e) {
    if (mine === generation && !disposed) error.value = errorText(e)
  } finally {
    if (mine === generation) busy.value = false
  }
}
const valid = (mine: number) => mine === generation && !disposed && !paused.value
async function list(): Promise<void> {
  const mine = generation
  const version = operation
  const result = await call<{ sessions: WorkspaceSummary[] }>('list')
  if (valid(mine) && version === operation) sessions.value = result.sessions
}
async function status(): Promise<void> {
  const mine = generation
  const result = await call<typeof gateway.value>('gateway_status')
  if (valid(mine)) gateway.value = result
}
async function load(): Promise<void> {
  if (paused.value || disposed) return
  const mine = generation
  loading.value = true
  try {
    await list()
    if (valid(mine)) await status()
  } catch (e) {
    if (valid(mine)) error.value = errorText(e)
  } finally {
    if (valid(mine)) loading.value = false
  }
}
async function canLeave(): Promise<boolean> {
  return (
    !(
      text.value.trim() ||
      review.value.trim() ||
      (creating.value && (title.value.trim() || instructions.value.trim()))
    ) || (await confirm(t('workspace.leave'), { title: 'Moodflow', kind: 'warning' }))
  )
}
async function switchKind(next: WorkspaceKind): Promise<void> {
  const mine = generation
  if (!(await canLeave()) || !valid(mine)) return
  generation++
  busy.value = false
  loading.value = false
  kind.value = next
  selected.value = null
  text.value = ''
  review.value = ''
  consent.value = false
  pendingId.value = null
  creating.value = false
  title.value = ''
  instructions.value = ''
  error.value = ''
  notice.value = ''
  void load()
}
async function choose(id: string): Promise<void> {
  const before = generation
  if (!(await canLeave()) || !valid(before)) return
  await action(async () => {
    const mine = generation
    const result = await call<WorkspaceReply>('get', { id })
    if (!valid(mine)) return
    selected.value = result
    review.value = ''
    text.value = ''
    consent.value = false
    pendingId.value = null
    creating.value = false
    title.value = ''
    instructions.value = ''
  })
}
async function openSearchResult(hit: { id: string; kind: WorkspaceKind }): Promise<void> {
  await choose(hit.id)
  if (current.value?.id === hit.id) kind.value = hit.kind
}
async function explorerChanged(): Promise<void> {
  title.value = ''
  instructions.value = ''
  selected.value = null
  text.value = ''
  review.value = ''
  consent.value = false
  pendingId.value = null
  creating.value = false
  await load()
}
async function beginCreate(): Promise<void> {
  const mine = generation
  if (!(await canLeave()) || !valid(mine)) return
  text.value = ''
  review.value = ''
  title.value = ''
  instructions.value = ''
  consent.value = false
  creating.value = true
}
async function create(): Promise<void> {
  await action(async () => {
    const mine = generation
    const result = await call<WorkspaceReply>('create', {
      kind: kind.value,
      title: title.value.trim(),
      instructions: kind.value === 'chat' ? instructions.value : '',
    })
    if (!valid(mine)) return
    selected.value = result
    creating.value = false
    title.value = ''
    instructions.value = ''
    text.value = ''
    review.value = ''
    consent.value = false
    pendingId.value = null
    await list()
  })
}
async function send(): Promise<void> {
  const session = current.value
  if (!session || !text.value.trim()) return
  const mine = generation
  const body = text.value.trim()
  const id = session.id
  if (session.kind === 'chat' && !consent.value) return
  const requestId = pendingId.value ?? crypto.randomUUID()
  pendingId.value = requestId
  await action(async () => {
    const result = await call<WorkspaceReply>(session.kind === 'chat' ? 'chat' : 'note', {
      id,
      text: body,
      message_id: requestId,
      revision: selected.value!.revision,
      consent: consent.value,
      include_history: history.value,
    })
    if (!valid(mine) || current.value?.id !== id) return
    selected.value = result
    text.value = ''
    pendingId.value = null
    notice.value = t(session.kind === 'chat' ? 'workspace.reply_saved' : 'workspace.note_saved')
    await list()
  })
}
async function stopChat(): Promise<void> {
  const id = current.value?.id
  const mine = generation
  if (!id || current.value?.kind !== 'chat') return
  try {
    await call('chat_cancel', { id })
  } catch (e) {
    if (valid(mine)) error.value = errorText(e)
  }
}
watch(text, () => {
  pendingId.value = null
})
async function update(values: { archived?: boolean; unread?: boolean }): Promise<void> {
  const session = current.value
  if (!session) return
  const mine = generation
  await action(async () => {
    const result = await call<WorkspaceReply>('update', {
      id: session.id,
      revision: selected.value!.revision,
      title: session.title,
      archived: values.archived ?? session.archived,
      unread: values.unread ?? session.unread,
    })
    if (valid(mine) && current.value?.id === session.id) {
      selected.value = result
      await list()
    }
  })
}
async function remove(): Promise<void> {
  const session = current.value
  const mine = generation
  if (!session) return
  if (
    !(await confirm(t('workspace.delete_confirm'), { title: 'Moodflow', kind: 'warning' })) ||
    !valid(mine) ||
    current.value?.id !== session.id
  )
    return
  await action(async () => {
    await call('remove', { id: session.id, revision: selected.value!.revision })
    if (valid(mine)) {
      selected.value = null
      text.value = ''
      review.value = ''
      await list()
    }
  })
}
function inspect(message: WorkspaceMessage): void {
  review.value = message.body
  notice.value = ''
}
async function stage(): Promise<void> {
  if (!review.value.trim()) return
  const mine = generation
  await action(async () => {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text: review.value })))
    if (valid(mine)) notice.value = t('library.staged')
  })
}
async function copy(value: string): Promise<void> {
  const mine = generation
  await action(async () => {
    await call('copy_text', { text: value })
    if (valid(mine)) notice.value = t('workspace.copied')
  })
}
async function receiveFile(): Promise<void> {
  const session = current.value
  const mine = generation
  if (!session) return
  const path = await open({ multiple: false })
  if (typeof path !== 'string' || !valid(mine) || current.value?.id !== session.id) return
  await action(async () => {
    const result = await call<WorkspaceReply>('receive_file', {
      id: session.id,
      revision: selected.value!.revision,
      path,
      text: text.value,
    })
    if (valid(mine)) {
      selected.value = result
      text.value = ''
      await list()
    }
  })
}
async function exportFile(file: WorkspaceFile): Promise<void> {
  const session = current.value
  const mine = generation
  if (!session) return
  const path = await save({ defaultPath: file.name })
  if (!path || !valid(mine) || current.value?.id !== session.id) return
  await action(async () => {
    await call('export_file', { id: session.id, revision: selected.value!.revision, file_id: file.id, path })
    if (valid(mine)) notice.value = t('workspace.file_saved')
  })
}
async function gatewayAction(
  method: 'gateway_start' | 'gateway_stop' | 'gateway_token' | 'gateway_rotate',
): Promise<void> {
  const mine = generation
  if (
    method === 'gateway_rotate' &&
    !(await confirm(t('workspace.rotate_confirm'), { title: 'Moodflow', kind: 'warning' }))
  )
    return
  if (!valid(mine)) return
  await action(async () => {
    await call(method, { port: port.value, confirm: method === 'gateway_rotate' })
    if (valid(mine)) {
      await status()
      if (method === 'gateway_token') notice.value = t('workspace.token_copied')
    }
  })
}
watch(paused, (value) => {
  generation++
  busy.value = false
  loading.value = false
  selected.value = null
  sessions.value = []
  text.value = ''
  review.value = ''
  consent.value = false
  title.value = ''
  instructions.value = ''
  creating.value = false
  error.value = ''
  notice.value = ''
  pendingId.value = null
  if (!value) void load()
})
const refresh = setInterval(() => {
  if (paused.value || busy.value || loading.value || disposed || refreshing) return
  refreshing = true
  const version = operation
  const id = current.value?.kind === 'inbox' ? current.value.id : null
  const mine = generation
  void (async () => {
    await list()
    if (id && valid(mine) && version === operation && !busy.value) {
      const result = await call<WorkspaceReply>('get', { id })
      if (valid(mine) && current.value?.id === id && version === operation && !busy.value)
        selected.value = result
    }
  })()
    .catch(() => {})
    .finally(() => {
      refreshing = false
    })
}, 2000)
onBeforeUnmount(() => {
  disposed = true
  generation++
  clearInterval(refresh)
  selected.value = null
  text.value = ''
  review.value = ''
  instructions.value = ''
})
defineExpose({ canLeave })
</script>
<template>
  <section v-if="paused" class="empty">{{ t('workspace.privacy') }}</section>
  <section v-else class="workspace">
    <header>
      <span class="eyebrow">{{ t('workspace.local') }}</span>
      <h1>{{ t('workspace.title') }}</h1>
      <p>{{ t('workspace.description') }}</p>
    </header>
    <WorkspaceExplorer
      :disabled="busy"
      :before-change="canLeave"
      @busy="busy = $event"
      @open="openSearchResult"
      @changed="explorerChanged"
    />
    <div class="tabs" role="tablist" :aria-label="t('workspace.title')">
      <button
        v-for="item in ['chat', 'inbox', 'terminal'] as const"
        :key="item"
        role="tab"
        :aria-selected="kind === item"
        @click="switchKind(item)"
      >
        {{ t(labels[item]) }}
      </button>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <div v-if="kind === 'inbox'" class="gateway">
      <h2>{{ t('workspace.gateway') }}</h2>
      <p class="hint">{{ t('workspace.gateway_hint') }}</p>
      <div class="actions">
        <label
          >{{ t('workspace.port')
          }}<input
            v-model.number="port"
            type="number"
            min="1024"
            max="65535"
            :disabled="gateway.enabled || busy"
        /></label>
        <button v-if="!gateway.enabled" :disabled="busy" @click="gatewayAction('gateway_start')">
          {{ t('workspace.gateway_start') }}
        </button>
        <button v-else :disabled="busy" @click="gatewayAction('gateway_stop')">
          {{ t('workspace.gateway_stop') }}
        </button>
        <button :disabled="busy" @click="gatewayAction('gateway_token')">
          {{ t('workspace.copy_token') }}
        </button>
        <button :disabled="busy" @click="gatewayAction('gateway_rotate')">
          {{ t('workspace.rotate_token') }}
        </button>
      </div>
      <label v-if="gateway.address"
        >{{ t('workspace.mcp_url')
        }}<input
          :value="`${gateway.address}/mcp`"
          readonly
          @focus="($event.target as HTMLInputElement).select()"
      /></label>
    </div>
    <div class="sessions">
      <aside>
        <button :disabled="busy" @click="beginCreate">{{ t('workspace.new') }}</button>
        <label><input v-model="archived" type="checkbox" />{{ t('workspace.show_archived') }}</label>
        <button :disabled="busy || loading" @click="load">{{ t('workspace.refresh') }}</button>
        <button
          v-for="session in visible"
          :key="session.id"
          :disabled="busy"
          :aria-pressed="current?.id === session.id"
          @click="choose(session.id)"
        >
          <span>{{ session.unread ? '● ' : '' }}{{ session.title }}</span
          ><small>{{ t('workspace.message_count', { n: session.count }) }}</small>
        </button>
        <p v-if="!visible.length" class="hint">{{ t('workspace.empty') }}</p>
      </aside>
      <article v-if="creating">
        <h2>{{ t('workspace.new') }}</h2>
        <label>{{ t('workspace.name') }}<input v-model="title" maxlength="256" :disabled="busy" /></label>
        <label v-if="kind === 'chat'"
          >{{ t('workspace.instructions')
          }}<textarea v-model="instructions" maxlength="8192" :disabled="busy" />
        </label>
        <p class="hint">{{ t('workspace.instructions_hint') }}</p>
        <div class="actions">
          <button :disabled="busy || !title.trim()" @click="create">{{ t('workspace.create') }}</button
          ><button :disabled="busy" @click="creating = false">{{ t('workspace.cancel') }}</button>
        </div>
      </article>
      <article v-else-if="current">
        <div class="actions">
          <h2>{{ current.title }}</h2>
          <button :disabled="busy" @click="update({ archived: !current.archived })">
            {{ t(current.archived ? 'workspace.unarchive' : 'workspace.archive') }}</button
          ><button v-if="current.unread" :disabled="busy" @click="update({ unread: false })">
            {{ t('workspace.mark_read') }}</button
          ><button :disabled="busy" @click="remove">{{ t('workspace.delete') }}</button>
        </div>
        <p v-if="current.model" class="hint">
          {{ t('workspace.frozen_model', { model: current.model.model }) }}
        </p>
        <label v-if="current.kind === 'inbox'"
          >{{ t('workspace.session_id')
          }}<input :value="current.id" readonly @focus="($event.target as HTMLInputElement).select()"
        /></label>
        <WorkspaceTerminal
          v-if="current.kind === 'terminal'"
          :id="current.id"
          :key="current.id"
          :paused="paused"
          :archived="current.archived"
        />
        <div class="messages">
          <section v-for="message in current.messages" :key="message.id" class="message">
            <span class="message-role"
              >{{ t(`workspace.role_${message.role}`)
              }}<small v-if="message.mock"> · {{ t('workspace.mock') }}</small></span
            >
            <p class="body">{{ message.body }}</p>
            <button v-for="file in message.files" :key="file.id" :disabled="busy" @click="exportFile(file)">
              {{ t('workspace.export_file', { name: file.name }) }}
            </button>
            <button v-if="message.body" :disabled="busy" @click="inspect(message)">
              {{ t('workspace.review') }}
            </button>
          </section>
        </div>
        <label
          >{{ t(current.kind === 'chat' ? 'workspace.message' : 'workspace.local_note')
          }}<textarea v-model="text" maxlength="16384" spellcheck="false" :disabled="busy" />
        </label>
        <template v-if="current.kind === 'chat'">
          <p class="hint">{{ t('workspace.network_notice') }}</p>
          <label
            ><input v-model="history" type="checkbox" :disabled="busy" />{{
              t('workspace.include_history')
            }}</label
          >
          <label
            ><input v-model="consent" type="checkbox" :disabled="busy" />{{
              t('workspace.consent_label')
            }}</label
          >
        </template>
        <div class="actions">
          <button
            :disabled="busy || !text.trim() || (current.kind === 'chat' && (!consent || current.archived))"
            @click="send"
          >
            {{ t(current.kind === 'chat' ? 'workspace.send' : 'workspace.save_note') }}</button
          ><button v-if="busy && current.kind === 'chat'" @click="stopChat">{{ t('workspace.stop') }}</button
          ><button v-if="current.kind !== 'chat'" :disabled="busy" @click="receiveFile">
            {{ t('workspace.receive_file') }}
          </button>
        </div>
        <p v-if="current.kind === 'inbox'" class="hint">{{ t('workspace.note_hint') }}</p>
        <div v-if="review" class="review">
          <h3>{{ t('workspace.review') }}</h3>
          <textarea v-model="review" maxlength="16384" spellcheck="false" :disabled="busy" />
          <div class="actions">
            <button :disabled="busy || !review.trim()" @click="stage">{{ t('workspace.stage') }}</button
            ><button :disabled="busy || !review" @click="copy(review)">{{ t('workspace.copy_text') }}</button
            ><button @click="review = ''">{{ t('workspace.cancel') }}</button>
          </div>
        </div>
      </article>
      <p v-else class="hint">{{ t('workspace.select') }}</p>
    </div>
    <p class="hint">{{ t('workspace.limits') }}</p>
  </section>
</template>
<style scoped>
.workspace,
article {
  display: grid;
  gap: var(--mf-sp-3);
}
header p,
.hint,
small {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.tabs,
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: center;
}
.tabs [aria-selected='true'] {
  background: var(--mf-focus);
  color: var(--mf-text);
}
.gateway,
.review {
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-3);
  display: grid;
  gap: var(--mf-sp-2);
}
.sessions {
  display: grid;
  grid-template-columns: 190px minmax(0, 1fr);
  gap: var(--mf-sp-4);
}
aside {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-2);
  max-height: 65vh;
  overflow: auto;
}
aside button {
  display: grid;
  text-align: left;
  overflow-wrap: anywhere;
}
article {
  min-width: 0;
  align-content: start;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
label:has(input[type='checkbox']) {
  display: flex;
  align-items: center;
}
input:not([type='checkbox']),
textarea {
  width: 100%;
}
input[type='number'] {
  width: 100px;
}
textarea {
  min-height: 100px;
  resize: vertical;
}
.messages {
  display: grid;
  gap: var(--mf-sp-2);
  max-height: 48vh;
  overflow: auto;
}
.message {
  padding: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}
.message-role {
  font-size: var(--mf-fs-sm);
  color: var(--mf-text-2);
}
.body {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
.error {
  color: var(--mf-danger);
}
@media (max-width: 760px) {
  .sessions {
    grid-template-columns: 1fr;
  }
  aside {
    max-height: 200px;
  }
}
</style>
