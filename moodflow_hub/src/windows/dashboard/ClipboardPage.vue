<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'

interface Entry {
  id: number
  text: string
  pinned: boolean
  copied_at: number
  kind: 'text' | 'link'
}
const paused = useToolPrivacy()
const entries = ref<Entry[]>([])
const query = ref('')
const selectedId = ref<number | null>(null)
const error = ref('')
const notice = ref('')
const busy = ref(false)
let generation = 0
let disposed = false
const visible = computed(() => {
  const needle = query.value.trim().toLocaleLowerCase()
  return entries.value.filter((entry) => entry.text.toLocaleLowerCase().includes(needle))
})
const selected = computed(() => visible.value.find((entry) => entry.id === selectedId.value))
async function request(method: string, params: object) {
  return JSON.parse(await unwrap(commands.imeTools(method, JSON.stringify(params))))
}
async function refresh(): Promise<void> {
  if (paused.value || busy.value) return
  const current = ++generation
  busy.value = true
  error.value = ''
  try {
    const result = (await request('clipboard.list', {})) as { entries: Entry[] }
    if (current === generation && !paused.value && !disposed) entries.value = result.entries
  } catch (e) {
    if (current === generation && !disposed) {
      entries.value = []
      error.value = errorText(e)
    }
  } finally {
    if (current === generation) busy.value = false
  }
}
async function act(action: string): Promise<void> {
  if (busy.value || paused.value) return
  const entry = selected.value
  if (action !== 'clear' && !entry) return
  if (action === 'clear' && !window.confirm(t('clipboard.clear_confirm'))) return
  if (action === 'remove' && !window.confirm(t('clipboard.remove_confirm'))) return
  const current = ++generation
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    const result = (await request('clipboard.action', {
      action,
      id: entry?.id ?? 0,
      pinned: !entry?.pinned,
    })) as { changed: boolean }
    if (current !== generation || disposed || paused.value) return
    notice.value = !result.changed
      ? t('clipboard.missing')
      : action === 'copy'
        ? t('clipboard.copied')
        : action === 'stage'
          ? t('library.staged')
          : t('clipboard.changed')
  } catch (e) {
    if (current === generation && !disposed) error.value = errorText(e)
  } finally {
    if (current === generation) {
      busy.value = false
      void refresh()
    }
  }
}
watch(paused, (value) => {
  generation++
  busy.value = false
  entries.value = []
  selectedId.value = null
  query.value = ''
  notice.value = ''
  error.value = ''
  if (!value) void refresh()
})
const timer = setInterval(() => void refresh(), 3000)
onBeforeUnmount(() => {
  disposed = true
  generation++
  clearInterval(timer)
  entries.value = []
})
</script>

<template>
  <section v-if="paused" class="empty">{{ t('library.privacy') }}</section>
  <section v-else class="clipboard">
    <header>
      <h1>{{ t('clipboard.title') }}</h1>
      <p>{{ t('clipboard.description') }}</p>
    </header>
    <div class="actions">
      <input
        v-model="query"
        type="search"
        maxlength="256"
        :aria-label="t('clipboard.search')"
        :placeholder="t('clipboard.search')"
      />
      <button :disabled="busy" @click="refresh">{{ t('clipboard.refresh') }}</button>
      <button :disabled="busy || !entries.some((entry) => !entry.pinned)" @click="act('clear')">
        {{ t('clipboard.clear') }}
      </button>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <div class="panes">
      <aside :aria-label="t('clipboard.title')">
        <p v-if="!visible.length">{{ t('clipboard.empty') }}</p>
        <button
          v-for="entry in visible"
          :key="entry.id"
          class="item"
          :class="{ selected: selectedId === entry.id }"
          :aria-pressed="selectedId === entry.id"
          @click="selectedId = entry.id"
        >
          <small v-if="entry.pinned">{{ t('clipboard.pin') }}</small> {{ entry.text.slice(0, 100) }}
        </button>
      </aside>
      <article v-if="selected">
        <time>{{ new Date(selected.copied_at * 1000).toLocaleString() }}</time>
        <pre tabindex="0">{{ selected.text }}</pre>
        <div class="actions">
          <button :disabled="busy" @click="act('copy')">{{ t('clipboard.copy') }}</button>
          <button :disabled="busy" @click="act('stage')">{{ t('clipboard.stage') }}</button>
          <button :disabled="busy" @click="act('pin')">
            {{ t(selected.pinned ? 'clipboard.unpin' : 'clipboard.pin') }}
          </button>
          <button :disabled="busy" @click="act('remove')">{{ t('clipboard.remove') }}</button>
        </div>
      </article>
      <p v-else>{{ t('clipboard.select') }}</p>
    </div>
  </section>
</template>

<style scoped>
.clipboard {
  display: grid;
  gap: var(--mf-sp-4);
}
header p,
time {
  color: var(--mf-text-2);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
}
input {
  flex: 1;
  min-width: 160px;
}
.panes {
  display: grid;
  grid-template-columns: minmax(160px, 35%) 1fr;
  gap: var(--mf-sp-4);
}
aside {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-2);
  max-height: 65vh;
  overflow: auto;
}
.item {
  display: block;
  text-align: left;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.selected {
  border-color: var(--mf-focus);
}
article {
  min-width: 0;
}
pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 50vh;
  overflow: auto;
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface-2);
  font: inherit;
}
.error {
  color: var(--mf-danger);
}
@media (max-width: 700px) {
  .panes {
    grid-template-columns: 1fr;
  }
  aside {
    max-height: 25vh;
  }
}
</style>
