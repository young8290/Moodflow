<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { commands, unwrap } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'

const props = defineProps<{ metadata: Record<string, unknown> }>()
const emit = defineEmits<{ apply: [fields: Record<string, unknown>] }>()
interface Reference {
  title: string
  kind: string
  csl: unknown
  [key: string]: unknown
}
const fields = ['authors', 'year', 'container', 'publisher', 'volume', 'issue', 'pages', 'doi', 'url', 'isbn']
const reference = computed<Record<string, unknown>>(() => ({
  title: props.metadata.title ?? '',
  kind: props.metadata.reference_type || 'book',
  csl: props.metadata.reference_csl ?? null,
  ...Object.fromEntries(fields.map((f) => [f, props.metadata[`reference_${f}`] ?? ''])),
}))
const styles = [
  'gbt-compatible',
  'gb-7714-2015-numeric',
  'apa',
  'modern-language-association',
  'chicago-author-date',
  'vancouver',
  'ieee',
]
const style = ref('gbt-compatible')
const output = ref('')
const preview = ref<Reference[]>([])
const selected = ref(0)
const busy = ref(false)
const error = ref('')
const notice = ref('')
let generation = 0
watch([reference, style], () => {
  generation++
  output.value = ''
})
async function call<T>(method: string, payload: Record<string, unknown>): Promise<T> {
  return JSON.parse(await unwrap(commands.citationRequest(method, JSON.stringify(payload)))) as T
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
async function format(): Promise<void> {
  const current = ++generation
  await action(async () => {
    const value = await call<{ missing: string[]; text: string }>('format', {
      reference: reference.value,
      style: style.value,
    })
    if (current !== generation) return
    if (value.missing.length)
      error.value = t('citation.missing', {
        fields: value.missing
          .map((f) => t(`library.${f === 'title' ? 'title_field' : `reference_${f}`}` as CopyKey))
          .join('、'),
      })
    else output.value = value.text
  })
}
async function importFile(): Promise<void> {
  const path = await open({
    multiple: false,
    filters: [{ name: 'CSL JSON / BibTeX', extensions: ['json', 'bib'] }],
  })
  if (typeof path !== 'string') return
  await action(async () => {
    preview.value = (await call<{ references: Reference[] }>('import', { path })).references
    selected.value = 0
  })
}
async function lookupDoi(): Promise<void> {
  const doi = String(reference.value.doi ?? '').trim()
  if (!doi || !(await confirm(t('citation.doi_question', { doi }), { title: 'Moodflow', kind: 'info' })))
    return
  await action(async () => {
    preview.value = [(await call<{ reference: Reference }>('doi', { doi, consent: true })).reference]
    selected.value = 0
  })
}
function apply(): void {
  const item = preview.value[selected.value]
  if (!item) return
  emit('apply', {
    title: item.title,
    reference_type: item.kind,
    reference_csl: item.csl,
    ...Object.fromEntries(fields.map((f) => [`reference_${f}`, item[f] ?? ''])),
  })
  preview.value = []
  notice.value = t('citation.applied')
}
async function exportFile(format: 'bibtex' | 'csl'): Promise<void> {
  const extension = format === 'bibtex' ? 'bib' : 'json'
  const path = await save({
    defaultPath: `reference.${extension}`,
    filters: [{ name: 'CSL JSON / BibTeX', extensions: [extension] }],
  })
  if (!path) return
  await action(async () => {
    await call('export', { path, format, reference: reference.value })
    notice.value = t('citation.exported')
  })
}
async function stage(): Promise<void> {
  await action(async () => {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text: output.value })))
    notice.value = t('library.staged')
  })
}
</script>

<template>
  <section class="citation-tools">
    <h3>{{ t('citation.title') }}</h3>
    <p>{{ t('citation.description') }}</p>
    <div class="actions">
      <button :disabled="busy" @click="importFile">{{ t('citation.import') }}</button
      ><button :disabled="busy || !reference.doi" @click="lookupDoi">{{ t('citation.doi') }}</button
      ><button :disabled="busy" @click="exportFile('bibtex')">{{ t('citation.export_bib') }}</button
      ><button :disabled="busy" @click="exportFile('csl')">{{ t('citation.export_csl') }}</button>
    </div>
    <div v-if="preview.length" class="preview">
      <label
        >{{ t('citation.preview')
        }}<select v-model="selected">
          <option v-for="(item, index) in preview" :key="index" :value="index">{{ item.title }}</option>
        </select></label
      >
      <p>{{ preview[selected]?.authors }}</p>
      <button :disabled="busy" @click="apply">{{ t('citation.apply') }}</button>
    </div>
    <div class="actions">
      <select v-model="style" :aria-label="t('citation.style')" :disabled="busy">
        <option v-for="item in styles" :key="item" :value="item">
          {{ t(`citation.${item}` as CopyKey) }}
        </option></select
      ><button :disabled="busy" @click="format">{{ t('citation.format') }}</button>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <template v-if="output"
      ><blockquote>{{ output }}</blockquote>
      <button :disabled="busy" @click="stage">{{ t('library.stage') }}</button></template
    >
  </section>
</template>

<style scoped>
.citation-tools {
  border-top: 1px solid var(--mf-border);
  margin-top: var(--mf-sp-3);
  padding-top: var(--mf-sp-3);
}
.citation-tools p {
  color: var(--mf-text-2);
}
.actions {
  display: flex;
  gap: var(--mf-sp-2);
  flex-wrap: wrap;
  margin-block: var(--mf-sp-2);
}
.preview {
  padding: var(--mf-sp-3);
  background: var(--mf-surface-2);
  border-radius: var(--mf-radius-card);
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
blockquote {
  margin: var(--mf-sp-3) 0;
  padding: var(--mf-sp-3);
  border-left: 3px solid var(--mf-primary);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.citation-tools .error {
  color: var(--mf-danger);
}
</style>
