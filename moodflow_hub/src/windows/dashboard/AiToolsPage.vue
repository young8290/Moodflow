<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type AiConfigView } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import MathPreview from './MathPreview.vue'

type Action =
  'generate' | 'rewrite' | 'answer' | 'translate' | 'poem' | 'image_prompt' | 'latex' | 'reference'
const actions: Action[] = [
  'generate',
  'rewrite',
  'answer',
  'translate',
  'poem',
  'image_prompt',
  'latex',
  'reference',
]
interface Reply {
  text: string
  model: string
  mock: boolean
  sources: { reference: { title: string; doi: string; authors: string; year: string } }[]
}
const paused = useToolPrivacy()
const action = ref<Action>('generate')
const input = ref('')
const instructions = ref('')
const language = ref('')
const model = ref('')
const config = ref<AiConfigView | null>(null)
const consent = ref(false)
const searchConsent = ref(false)
const images = ref<string[]>([])
const output = ref<Reply | null>(null)
const error = ref('')
const notice = ref('')
const busy = ref(false)
const styles = [
  'gbt-compatible',
  'gb-7714-2015-numeric',
  'apa',
  'modern-language-association',
  'chicago-author-date',
  'vancouver',
  'ieee',
] as const
const style = ref('gbt-compatible')
let generation = 0
let disposed = false
let currentId: string | null = null
const models = computed(() => config.value?.llm?.models ?? [])
const ready = computed(
  () =>
    !paused.value &&
    !busy.value &&
    consent.value &&
    !!model.value &&
    !!config.value?.llm &&
    (input.value.trim().length > 0 || (action.value === 'latex' && images.value.length > 0)) &&
    (action.value !== 'reference' || searchConsent.value),
)
const label = (value: Action) => t(`ai_tools.action_${value}` as CopyKey)
async function request<T>(method: string, value: object): Promise<T> {
  return JSON.parse(await unwrap(commands.toolsRequest(method, JSON.stringify(value)))) as T
}
function cancel(): void {
  generation++
  if (currentId) void request('cancel', { id: currentId }).catch(() => {})
  currentId = null
  busy.value = false
}
function invalidate(): void {
  cancel()
  output.value = null
  error.value = ''
  notice.value = ''
  consent.value = false
  searchConsent.value = false
}
watch([input, instructions, language, model, style], invalidate)
watch(action, () => {
  invalidate()
  images.value = []
})
watch(paused, (value) => {
  if (value) {
    invalidate()
    images.value = []
    config.value = null
  } else void loadConfig()
})
async function loadConfig(): Promise<void> {
  const revision = generation
  try {
    const value = await unwrap(commands.aiConfigGet())
    if (!disposed && !paused.value && generation === revision) {
      config.value = value
      model.value = value.llm?.models[0] ?? ''
    }
  } catch (e) {
    if (!disposed && !paused.value) error.value = errorText(e)
  }
}
async function run(): Promise<void> {
  if (!ready.value) return
  const revision = ++generation
  const id = crypto.randomUUID()
  currentId = id
  busy.value = true
  output.value = null
  error.value = ''
  notice.value = ''
  const payload = {
    id,
    action: action.value,
    text: input.value,
    instructions: instructions.value,
    language: language.value,
    images: [...images.value],
    model: model.value,
    endpoint: config.value!.llm!.base_url,
    consent: consent.value,
    search_consent: searchConsent.value,
    style: style.value,
  }
  consent.value = false
  searchConsent.value = false
  try {
    const value = await request<Reply>('run', payload)
    if (revision === generation && !paused.value && !disposed) output.value = value
  } catch (e) {
    if (revision === generation && !paused.value && !disposed) error.value = errorText(e)
  } finally {
    if (revision === generation) {
      busy.value = false
      currentId = null
    }
  }
}
function removeImage(index: number): void {
  invalidate()
  images.value.splice(index, 1)
}
async function chooseImages(): Promise<void> {
  const revision = generation
  const selection = await open({
    multiple: true,
    filters: [{ name: 'PNG / JPEG', extensions: ['png', 'jpg', 'jpeg'] }],
  })
  if (!selection || revision !== generation || disposed || paused.value) return
  const chosen = Array.isArray(selection) ? selection : [selection]
  invalidate()
  if (chosen.length > 3) {
    error.value = t('ai_tools.image_limit')
    return
  }
  images.value = chosen
}
async function readBuffer(): Promise<void> {
  const revision = generation
  try {
    const value = JSON.parse(await unwrap(commands.imeTools('buffer.read', '{}'))) as { text: string }
    if (revision !== generation || disposed || paused.value) return
    if (value.text.length > 12000) {
      error.value = t('ai_tools.limit')
      return
    }
    input.value = value.text
  } catch (e) {
    if (!disposed && !paused.value) error.value = errorText(e)
  }
}
async function unicode(): Promise<void> {
  invalidate()
  const revision = generation
  try {
    const value = await request<{ text: string; unsupported: string[] }>('unicode', { text: input.value })
    if (revision !== generation || disposed || paused.value) return
    output.value = { text: value.text, model: '', mock: false, sources: [] }
    if (value.unsupported.length)
      notice.value = t('ai_tools.unsupported', { commands: value.unsupported.join('、') })
  } catch (e) {
    if (revision === generation && !paused.value) error.value = errorText(e)
  }
}
async function stage(): Promise<void> {
  if (!output.value || paused.value || busy.value) return
  const revision = generation
  try {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text: output.value.text })))
    if (revision === generation && !paused.value) notice.value = t('library.staged')
  } catch (e) {
    if (revision === generation && !paused.value) error.value = errorText(e)
  }
}
onMounted(() => {
  if (!paused.value) void loadConfig()
})
onBeforeUnmount(() => {
  disposed = true
  cancel()
  images.value = []
  output.value = null
})
</script>

<template>
  <section v-if="paused" class="empty">{{ t('ai_tools.privacy') }}</section>
  <section v-else class="tool-page">
    <header>
      <small>{{ t('ai_tools.eyebrow') }}</small>
      <h1>{{ t('ai_tools.title') }}</h1>
      <p>{{ t('ai_tools.description') }}</p>
    </header>
    <div class="action-tabs" :aria-label="t('ai_tools.action_label')">
      <button v-for="value in actions" :key="value" :aria-pressed="action === value" @click="action = value">
        {{ label(value) }}
      </button>
    </div>
    <label
      >{{ t('ai_tools.input')
      }}<textarea
        v-model="input"
        :maxlength="action === 'reference' ? 512 : 12000"
        rows="6"
        spellcheck="false"
      />
    </label>
    <div class="row">
      <button @click="readBuffer">{{ t('ai_tools.read_buffer') }}</button
      ><template v-if="action === 'latex'"
        ><button @click="chooseImages">{{ t('ai_tools.choose_images') }}</button
        ><button :disabled="!input" @click="unicode">{{ t('ai_tools.unicode') }}</button></template
      >
    </div>
    <ul v-if="images.length" class="images">
      <li v-for="(path, index) in images" :key="path">
        {{ path.split(/[\\/]/).pop() }}
        <button @click="removeImage(index)">
          {{ t('ai_tools.remove_image') }}
        </button>
      </li>
    </ul>
    <small v-if="action === 'latex'">{{ t('ai_tools.image_hint') }}</small>
    <MathPreview v-if="action === 'latex' && input && !images.length" :text="input" />
    <label v-if="action === 'translate'"
      >{{ t('ai_tools.language') }}<input v-model="language" maxlength="64"
    /></label>
    <label
      >{{ t('ai_tools.instructions') }}<textarea v-model="instructions" maxlength="1024" rows="2" />
    </label>
    <label v-if="action === 'reference'"
      >{{ t('ai_tools.citation_style')
      }}<select v-model="style">
        <option v-for="value in styles" :key="value" :value="value">
          {{ t(`citation.${value}` as CopyKey) }}
        </option>
      </select></label
    >
    <section class="connection">
      <p v-if="!config?.llm">{{ t('ai_tools.ai_unavailable') }}</p>
      <template v-else
        ><label
          >{{ t('ai_tools.model')
          }}<select v-model="model">
            <option v-for="value in models" :key="value" :value="value">{{ value }}</option>
          </select></label
        >
        <p class="endpoint">{{ config.llm.base_url }}</p>
        <p v-if="config.source === 'dev_mock'" class="mock">{{ t('ai_tools.mock') }}</p></template
      >
      <label class="check"
        ><input v-model="consent" type="checkbox" :disabled="busy" />{{ t('ai_tools.consent_label') }}</label
      >
      <label v-if="action === 'reference'" class="check"
        ><input v-model="searchConsent" type="checkbox" :disabled="busy" />{{
          t('ai_tools.search_consent_label')
        }}</label
      >
      <div class="row">
        <button :disabled="!ready" @click="run">{{ t('ai_tools.run') }}</button
        ><button v-if="busy" @click="cancel">{{ t('ai_tools.cancel') }}</button
        ><span v-if="busy" role="status">{{ t('ai_tools.working') }}</span>
      </div>
    </section>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <section v-if="output" class="result">
      <h2>{{ t('ai_tools.result') }}</h2>
      <p v-if="output.mock" class="mock">{{ t('ai_tools.mock') }}</p>
      <small v-if="output.model">{{ output.model }}</small>
      <textarea v-model="output.text" maxlength="16384" rows="9" spellcheck="false" />
      <MathPreview v-if="action === 'latex'" :text="output.text" />
      <ul v-if="output.sources.length">
        <li v-for="(source, index) in output.sources" :key="source.reference.doi">
          {{
            t('ai_tools.source_line', {
              number: index + 1,
              title: source.reference.title,
              authors: source.reference.authors,
              year: source.reference.year,
              doi: source.reference.doi,
            })
          }}
        </li>
      </ul>
      <p>{{ t('ai_tools.review') }}</p>
      <button :disabled="!output.text || busy" @click="stage">{{ t('ai_tools.stage') }}</button>
    </section>
  </section>
</template>

<style scoped>
.tool-page {
  display: grid;
  gap: var(--mf-sp-3);
  max-width: 920px;
  margin: 0 auto;
}
h1 {
  font-family: Georgia, 'Noto Serif SC', serif;
  margin: var(--mf-sp-2) 0;
}
h2 {
  font-size: var(--mf-text-body);
}
header p,
small,
.endpoint {
  color: var(--mf-text-2);
}
.endpoint {
  overflow-wrap: anywhere;
}
label {
  display: grid;
  gap: var(--mf-sp-2);
}
textarea,
input:not([type='checkbox']),
select {
  width: 100%;
  box-sizing: border-box;
  padding: var(--mf-sp-2);
  color: var(--mf-text-1);
  background: var(--mf-surface);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-sm);
}
textarea {
  resize: vertical;
}
.row,
.action-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: center;
}
.action-tabs button[aria-pressed='true'] {
  border-color: var(--mf-focus);
  background: var(--mf-surface-2);
}
.connection,
.result {
  display: grid;
  gap: var(--mf-sp-3);
  padding: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}
.check {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: baseline;
}
.mock {
  color: var(--mf-link);
}
.error {
  color: var(--mf-error);
}
li {
  overflow-wrap: anywhere;
  margin-bottom: var(--mf-sp-2);
}
</style>
