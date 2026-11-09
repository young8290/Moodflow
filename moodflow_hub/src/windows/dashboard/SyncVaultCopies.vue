<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
const props = defineProps<{ paused: boolean; busy: boolean; encrypted: boolean }>()
interface Backup {
  id: string
  name: string
  bytes: number
  revision: string
  conflict: boolean
}
const items = ref<Backup[]>([]),
  working = ref(false),
  phrase = ref(''),
  error = ref(''),
  notice = ref('')
let generation = 0,
  disposed = false
async function call<T>(method: string, params: unknown = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.syncRequest(method, JSON.stringify(params)))) as T
}
async function load(): Promise<void> {
  const mine = ++generation
  if (props.paused || disposed) return
  try {
    const [a, b] = await Promise.all([
      call<{ items: Backup[] }>('backup_list'),
      call<{ items: Backup[] }>('backup_list', { conflict: true }),
    ])
    if (mine === generation && !props.paused && !disposed) items.value = [...a.items, ...b.items]
  } catch (e) {
    if (mine === generation && !props.paused && !disposed) error.value = errorText(e)
  }
}
async function action(work: () => Promise<void>): Promise<void> {
  if (working.value || props.paused || props.busy || disposed) return
  const mine = ++generation
  working.value = true
  error.value = ''
  notice.value = ''
  try {
    await work()
  } catch (e) {
    if (mine === generation && !props.paused && !disposed) error.value = errorText(e)
  } finally {
    phrase.value = ''
    if (mine === generation) working.value = false
  }
}
async function receive(): Promise<void> {
  if (!props.encrypted || phrase.value.length < 24) return
  const mine = generation
  const path = await open({ multiple: false, filters: [{ name: 'Moodflow', extensions: ['mfvbackup'] }] })
  if (typeof path !== 'string' || mine !== generation || props.paused || props.busy || disposed) return
  await action(async () => {
    const current = generation
    await call('backup_import', { path, backup_recovery: phrase.value, confirm: true })
    if (current !== generation || props.paused || disposed) return
    notice.value = t('sync.backup_imported')
    const result = await call<{ items: Backup[] }>('backup_list')
    if (current === generation && !props.paused && !disposed)
      items.value = [...result.items, ...items.value.filter((i) => i.conflict)]
  })
}
async function exportBackup(item: Backup): Promise<void> {
  const mine = generation
  const path = await save({
    defaultPath: `${item.id}.mfvbackup`,
    filters: [{ name: 'Moodflow', extensions: ['mfvbackup'] }],
  })
  if (!path || mine !== generation || props.paused || props.busy || disposed) return
  await action(async () => {
    const current = generation
    await call('backup_export', { ...item, path })
    if (current === generation && !props.paused && !disposed) notice.value = t('sync.backup_exported')
  })
}
async function remove(item: Backup): Promise<void> {
  const mine = generation
  if (
    !(await confirm(t('sync.backup_delete_confirm'), { title: 'Moodflow', kind: 'warning' })) ||
    mine !== generation ||
    props.paused ||
    props.busy ||
    disposed
  )
    return
  await action(async () => {
    const current = generation
    await call('backup_delete', { ...item, confirm: true })
    if (current === generation && !props.paused && !disposed)
      items.value = items.value.filter((i) => i.name !== item.name || i.conflict !== item.conflict)
  })
}
watch(
  () => props.paused,
  () => {
    generation++
    phrase.value = ''
    items.value = []
    error.value = ''
    notice.value = ''
    working.value = false
    if (!props.paused) void load()
  },
)
watch(
  () => props.busy,
  () => {
    if (props.busy) phrase.value = ''
    else void load()
  },
)
onMounted(() => void load())
watch(
  () => props.encrypted,
  () => {
    phrase.value = ''
  },
)
onBeforeUnmount(() => {
  disposed = true
  generation++
  phrase.value = ''
})
</script>
<template>
  <section class="backups">
    <h3>{{ t('sync.backups_title') }}</h3>
    <p class="hint">{{ t('sync.backups_hint') }}</p>
    <template v-if="!paused"
      ><label v-if="encrypted"
        >{{ t('sync.backup_recovery')
        }}<input
          v-model="phrase"
          type="password"
          autocomplete="off"
          :disabled="busy || working"
          maxlength="2048"
      /></label>
      <div class="actions">
        <button :disabled="busy || working || !encrypted || phrase.length < 24" @click="receive">
          {{ t('sync.backup_import') }}</button
        ><button :disabled="busy || working" @click="load">{{ t('sync.refresh') }}</button>
      </div>
      <p v-if="!items.length">{{ t('sync.no_backups') }}</p>
      <ul>
        <li v-for="item in items" :key="`${item.conflict}:${item.name}`">
          <div>
            <strong>{{ t(item.conflict ? 'sync.backup_conflict' : 'sync.backup_label') }}</strong
            ><small
              >{{ item.id }} ·
              {{ t('sync.backup_bytes', { bytes: (item.bytes / 1048576).toFixed(2) }) }}</small
            >
          </div>
          <button :disabled="busy || working" @click="exportBackup(item)">
            {{ t('sync.backup_export') }}</button
          ><button :disabled="busy || working" @click="remove(item)">{{ t('sync.delete') }}</button>
        </li>
      </ul>
      <p v-if="error" role="alert" class="error">{{ error }}</p>
      <p v-if="notice" role="status">{{ notice }}</p>
    </template>
  </section>
</template>
<style scoped>
.backups {
  display: grid;
  gap: var(--mf-sp-3);
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}
h3,
p {
  margin: 0;
}
.hint,
small {
  font-size: var(--mf-fs-sm);
  color: var(--mf-text-2);
}
label {
  display: grid;
  gap: var(--mf-sp-2);
}
input {
  padding: var(--mf-sp-2);
  border: 1px solid var(--mf-border);
  background: var(--mf-surface);
  color: var(--mf-text-1);
  border-radius: var(--mf-radius-sm);
}
.actions,
li {
  display: flex;
  align-items: center;
  gap: var(--mf-sp-2);
  flex-wrap: wrap;
}
ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
li {
  padding: var(--mf-sp-2) 0;
}
li div {
  display: grid;
  flex: 1;
  gap: var(--mf-sp-1);
  overflow-wrap: anywhere;
}
button {
  padding: var(--mf-sp-2) var(--mf-sp-3);
}
.error {
  color: var(--mf-danger);
}
input:focus-visible,
button:focus-visible {
  outline: 2px solid var(--mf-focus);
  outline-offset: 2px;
}
</style>
