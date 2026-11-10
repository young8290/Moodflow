<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type PluginPreview } from '@/api'
import { errorText, t } from '@/i18n'
import { capabilityText, contributesText } from './plugins'

const props = defineProps<{ community: boolean }>()
const emit = defineEmits<{ installed: [result: { id: string; fresh: boolean }]; changed: [] }>()
type Source = { label: string; url: string; publicKey: string }
type Release = { version: string; url: string; size: number; sha256: string }
type Item = {
  id: string
  name: string
  author: string
  description: string
  repo: string
  release: Release | null
  installed: string | null
  update: boolean
}
type Catalog = {
  source: Source | null
  revision: string
  items: Item[]
  revoked: { id: string; version: string; reason: string }[]
  fingerprint?: string
  generatedAt?: string
  expired: boolean
}
type Preview = { token: string; preview: PluginPreview; source: Source; fingerprint: string; sha256: string }
const view = ref<Catalog | null>(null)
const label = ref(''),
  url = ref(''),
  key = ref(''),
  query = ref('')
const trust = ref(false),
  consent = ref(false),
  accepted = ref(false),
  busy = ref(false)
const error = ref(''),
  notice = ref('')
const pending = ref<Preview | null>(null)
let generation = 0,
  disposed = false
const rows = computed(
  () =>
    view.value?.items.filter((item) =>
      `${item.name} ${item.id} ${item.description}`
        .toLocaleLowerCase()
        .includes(query.value.toLocaleLowerCase()),
    ) ?? [],
)
async function api<T>(method: string, payload: object = {}): Promise<T> {
  return JSON.parse(await unwrap(commands.marketRequest(method, JSON.stringify(payload)))) as T
}
async function load(): Promise<void> {
  const gen = ++generation
  try {
    const value = await api<Catalog>('catalog')
    if (disposed || gen !== generation) return
    view.value = value
    label.value = value.source?.label ?? ''
    url.value = value.source?.url ?? ''
    key.value = value.source?.publicKey ?? ''
  } catch (e) {
    if (!disposed && gen === generation) error.value = errorText(e)
  }
}
function cancel(): void {
  generation++
  busy.value = false
  pending.value = null
  accepted.value = false
  void api('cancel').catch(() => {})
}
async function act<T>(work: () => Promise<T>, done: (value: T) => void): Promise<void> {
  const gen = ++generation
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    const value = await work()
    if (!disposed && gen === generation) done(value)
  } catch (e) {
    if (!disposed && gen === generation) error.value = errorText(e)
  } finally {
    if (gen === generation) busy.value = false
  }
}
async function configure(): Promise<void> {
  if (!view.value || !trust.value || busy.value) return
  const revision = view.value.revision
  await act(
    () =>
      api<Catalog>('configure', {
        revision,
        confirmed: true,
        source: {
          label: label.value.trim(),
          url: url.value.trim(),
          publicKey: key.value.trim().toLowerCase(),
        },
      }),
    (value) => {
      view.value = value
      pending.value = null
      trust.value = false
      consent.value = false
      notice.value = t('market.source_saved')
      emit('changed')
    },
  )
}
async function clearSource(): Promise<void> {
  if (!view.value || busy.value) return
  const revision = view.value.revision
  if (
    !(await confirm(t('market.clear_confirm'), { title: t('market.source_title'), kind: 'warning' })) ||
    disposed
  )
    return
  await act(
    () => api<Catalog>('configure', { revision, confirmed: true, source: null }),
    (value) => {
      view.value = value
      pending.value = null
      trust.value = false
      consent.value = false
      label.value = ''
      url.value = ''
      key.value = ''
      emit('changed')
    },
  )
}
async function refresh(): Promise<void> {
  if (!consent.value || busy.value) return
  pending.value = null
  accepted.value = false
  await act(
    () => api<Catalog>('refresh', { consent: true }),
    (value) => {
      view.value = value
      emit('changed')
    },
  )
  consent.value = false
}
async function preview(item: Item): Promise<void> {
  if (busy.value || !item.release || view.value?.expired) return
  pending.value = null
  accepted.value = false
  await act(
    () => api<Preview>('preview', { id: item.id, consent: true }),
    (value) => {
      pending.value = value
    },
  )
}
async function install(): Promise<void> {
  if (busy.value || !pending.value || !accepted.value) return
  const selected = pending.value
  await act(
    () =>
      api<{ id: string; fresh: boolean }>('install', {
        token: selected.token,
        accept: selected.preview.added,
        confirmed: true,
      }),
    (value) => {
      pending.value = null
      accepted.value = false
      notice.value = t('market.installed_done')
      emit('installed', value)
    },
  )
  if (!error.value && !disposed) await load()
}
async function communityLink(target: string): Promise<void> {
  error.value = ''
  try {
    await api('community', { target })
  } catch (e) {
    error.value = errorText(e)
  }
}
watch([label, url, key], () => {
  trust.value = false
  pending.value = null
  accepted.value = false
})
watch(
  () => props.community,
  () => {
    cancel()
    error.value = ''
    if (!props.community) void load()
  },
)
onMounted(() => {
  if (!props.community) void load()
})
onBeforeUnmount(() => {
  disposed = true
  cancel()
})
</script>

<template>
  <div class="market" data-market>
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>
    <template v-if="community">
      <h3>{{ t('market.community') }}</h3>
      <p class="s-hint">{{ t('market.community_hint') }}</p>
      <div class="s-row">
        <button type="button" @click="communityLink('issues')">{{ t('market.issues') }}</button>
        <button type="button" @click="communityLink('submit')">{{ t('market.submit') }}</button>
        <button type="button" @click="communityLink('format')">{{ t('market.format') }}</button>
      </div>
      <p class="s-hint">{{ t('market.project_url') }}</p>
    </template>
    <template v-else>
      <p class="s-hint">{{ t('market.intro') }}</p>
      <fieldset class="s-group">
        <legend>{{ t('market.source_title') }}</legend>
        <p v-if="!view?.source" class="s-note">{{ t('market.no_official') }}</p>
        <label>{{ t('market.label') }}<input v-model="label" maxlength="64" :disabled="busy" /></label>
        <label
          >{{ t('market.url')
          }}<input v-model="url" maxlength="2048" :disabled="busy" :placeholder="t('market.url_example')"
        /></label>
        <label
          >{{ t('market.key') }}<input v-model="key" maxlength="64" :disabled="busy" spellcheck="false"
        /></label>
        <p class="s-hint">{{ t('market.pin_hint') }}</p>
        <label class="s-choice"
          ><input v-model="trust" type="checkbox" :disabled="busy" />{{ t('market.trust') }}</label
        >
        <div class="s-row">
          <button
            type="button"
            :disabled="busy || !trust || !view || !label.trim() || !url.trim() || key.trim().length !== 64"
            @click="configure"
          >
            {{ t('market.save_source') }}
          </button>
          <button v-if="view?.source" type="button" class="s-compact" :disabled="busy" @click="clearSource">
            {{ t('market.clear_source') }}
          </button>
        </div>
        <p v-if="view?.fingerprint" class="fingerprint">
          {{ t('market.fingerprint') }} {{ view.fingerprint }}
        </p>
      </fieldset>
      <fieldset v-if="view?.source" class="s-group">
        <legend>{{ view.source.label }}</legend>
        <p class="s-hint">{{ view.source.url }}</p>
        <p class="s-hint">{{ t('market.signature_hint') }}</p>
        <label class="s-choice"
          ><input v-model="consent" type="checkbox" :disabled="busy" />{{
            t('market.refresh_consent')
          }}</label
        >
        <div class="s-row">
          <button type="button" :disabled="busy || !consent" @click="refresh">
            {{ t('market.refresh') }}
          </button>
          <button v-if="busy || pending" type="button" class="s-compact" @click="cancel">
            {{ t('plugins.cancel') }}
          </button>
        </div>
        <p v-if="view.generatedAt" class="s-hint">{{ t('market.generated') }} {{ view.generatedAt }}</p>
        <p v-if="view.expired" class="s-error">{{ t('market.expired') }}</p>
        <div v-for="withdrawn in view.revoked" :key="withdrawn.id" class="s-error" role="status">
          {{ t('market.withdrawn_item', { id: withdrawn.id, version: withdrawn.version }) }}
          <p>{{ withdrawn.reason }}</p>
        </div>
        <label>{{ t('market.search') }}<input v-model="query" maxlength="256" type="search" /></label>
        <p v-if="!rows.length" class="s-note">{{ t('market.empty') }}</p>
        <article v-for="item in rows" :key="item.id" class="item">
          <h3>
            {{ item.name }} <small>{{ item.release?.version }}</small>
          </h3>
          <p>{{ item.description }}</p>
          <p class="s-hint">{{ item.id }} · {{ item.author }}</p>
          <p class="source">{{ item.repo }}</p>
          <p v-if="item.release" class="source">
            {{ t('market.package_source', { url: item.release.url }) }}
          </p>
          <p v-if="item.installed" class="s-hint">
            {{ t('market.installed_version', { version: item.installed }) }}
          </p>
          <button
            type="button"
            :disabled="busy || view.expired || !item.release || (!!item.installed && !item.update)"
            @click="preview(item)"
          >
            {{ item.update ? t('market.download_update') : t('market.download') }}
          </button>
          <p v-if="!item.release" class="s-hint">{{ t('market.incompatible') }}</p>
        </article>
      </fieldset>
      <section v-if="pending" class="s-confirm" data-market-preview>
        <h3>{{ pending.preview.name }} {{ pending.preview.version }}</h3>
        <p>{{ pending.preview.description }}</p>
        <p class="s-hint">{{ pending.source.label }} · {{ pending.source.url }}</p>
        <p class="fingerprint">{{ t('market.package_hash', { hash: pending.sha256 }) }}</p>
        <ul>
          <li v-for="line in contributesText(pending.preview.contributes)" :key="line">{{ line }}</li>
        </ul>
        <p>{{ pending.preview.upgrade_from ? t('plugins.consent_new_caps') : t('plugins.consent_caps') }}</p>
        <ul v-if="pending.preview.added.length">
          <li v-for="cap in pending.preview.added" :key="cap">{{ capabilityText(cap) }}</li>
        </ul>
        <p v-else>{{ t('plugins.consent_no_new_caps') }}</p>
        <p class="s-hint">{{ t('plugins.privacy_line') }}</p>
        <label class="s-choice"
          ><input v-model="accepted" type="checkbox" :disabled="busy" />{{ t('market.accept') }}</label
        >
        <div class="s-row">
          <button type="button" :disabled="busy || !accepted" @click="install">
            {{ t('market.install') }}
          </button>
          <button type="button" class="s-compact" @click="cancel">{{ t('plugins.cancel') }}</button>
        </div>
      </section>
    </template>
    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </div>
</template>

<style scoped>
.market {
  margin-top: var(--mf-sp-4);
}
.s-group {
  display: grid;
  gap: var(--mf-sp-3);
}
label:not(.s-choice) {
  display: grid;
  gap: var(--mf-sp-1);
}
input:not([type='checkbox']) {
  width: 100%;
  box-sizing: border-box;
}
.item {
  border-top: 1px solid var(--mf-line);
  padding: var(--mf-sp-3) 0;
}
.item h3 {
  margin: 0;
}
small {
  font-weight: normal;
}
.fingerprint,
.source {
  overflow-wrap: anywhere;
  font-size: 12px;
}
.fingerprint {
  font-family: monospace;
}
</style>
