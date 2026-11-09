<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'

interface Entry {
  simplified: string
  traditional: string
  pinyin: string
  definitions: string[]
}
interface Result {
  entries: Entry[]
  segments: { input: string; output: string; covered: boolean }[]
  complete: boolean
  matched: boolean
  entry_count: number
}
const paused = useToolPrivacy()
const query = ref('')
const result = ref<Result | null>(null)
const error = ref('')
const notice = ref('')
const busy = ref(false)
let timer: ReturnType<typeof setTimeout> | undefined
let generation = 0
let disposed = false
async function lookup(): Promise<void> {
  const current = ++generation
  const text = query.value.trim()
  result.value = null
  error.value = ''
  notice.value = ''
  if (!text || paused.value) return
  busy.value = true
  try {
    const value = JSON.parse(
      await unwrap(commands.imeTools('translate.lookup', JSON.stringify({ query: text }))),
    ) as Result
    if (current === generation && !paused.value && !disposed) result.value = value
  } catch (e) {
    if (current === generation && !paused.value) error.value = errorText(e)
  } finally {
    if (current === generation) busy.value = false
  }
}
watch([query, paused], () => {
  clearTimeout(timer)
  generation++
  result.value = null
  error.value = ''
  notice.value = ''
  busy.value = false
  if (!paused.value) timer = setTimeout(() => void lookup(), 400)
})
async function readBuffer(): Promise<void> {
  try {
    const value = JSON.parse(await unwrap(commands.imeTools('buffer.read', '{}'))) as { text: string }
    if (value.text.length > 512) {
      error.value = t('translate.too_long')
      return
    }
    if (!paused.value) query.value = value.text
  } catch (e) {
    error.value = errorText(e)
  }
}
async function stage(text: string): Promise<void> {
  try {
    await unwrap(commands.imeTools('buffer.stage', JSON.stringify({ text })))
    notice.value = t('library.staged')
  } catch (e) {
    error.value = errorText(e)
  }
}
onBeforeUnmount(() => {
  disposed = true
  generation++
  clearTimeout(timer)
})
</script>

<template>
  <section v-if="paused" class="empty">{{ t('library.privacy') }}</section>
  <section v-else class="translation">
    <header>
      <span class="eyebrow">{{ t('translate.offline') }}</span>
      <h1>{{ t('translate.title') }}</h1>
      <p>{{ t('translate.description') }}</p>
    </header>
    <label
      >{{ t('translate.input')
      }}<textarea
        v-model="query"
        maxlength="512"
        :placeholder="t('translate.placeholder')"
        spellcheck="false"
      />
    </label>
    <div class="actions">
      <button @click="readBuffer">{{ t('translate.read_buffer') }}</button
      ><span v-if="busy" role="status">{{ t('translate.searching') }}</span>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <template v-if="result">
      <p class="coverage">
        {{
          t(
            result.complete
              ? 'translate.covered'
              : result.matched
                ? 'translate.partial'
                : 'translate.no_match',
          )
        }}
      </p>
      <div v-if="result.segments.length && !result.entries.length" class="segments">
        <div v-for="(segment, index) in result.segments" :key="index" :class="{ unknown: !segment.covered }">
          <span>{{ segment.input }}</span
          ><strong>{{ segment.output }}</strong
          ><small v-if="!segment.covered">{{ t('translate.uncovered') }}</small>
        </div>
      </div>
      <article v-for="(entry, index) in result.entries" :key="index" class="entry">
        <div class="headword">
          <h2>{{ entry.simplified }}</h2>
          <span v-if="entry.simplified !== entry.traditional">{{ entry.traditional }}</span
          ><span class="pinyin">{{ entry.pinyin }}</span>
        </div>
        <ol>
          <li v-for="(definition, i) in entry.definitions" :key="i">{{ definition }}</li>
        </ol>
        <div class="actions">
          <button @click="stage(entry.simplified)">{{ t('translate.stage_zh') }}</button
          ><button @click="stage(entry.definitions[0]!)">{{ t('translate.stage_en') }}</button>
        </div>
      </article>
      <small class="attribution">{{ t('translate.attribution', { count: result.entry_count }) }}</small>
    </template>
  </section>
</template>

<style scoped>
.translation {
  max-width: 900px;
  margin: 0 auto;
  display: grid;
  gap: var(--mf-sp-3);
}
h1 {
  font-family: Georgia, 'Noto Serif SC', 'SimSun', serif;
  margin: var(--mf-sp-2) 0;
}
header p,
.attribution,
.pinyin {
  color: var(--mf-text-2);
}
.eyebrow {
  color: var(--mf-link);
  font-size: var(--mf-text-small);
}
label {
  display: grid;
  gap: var(--mf-sp-2);
}
textarea {
  min-height: 110px;
  resize: vertical;
}
.actions,
.headword {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: center;
  flex-wrap: wrap;
}
.headword h2 {
  margin: 0;
}
.entry {
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface);
}
.entry li {
  padding-block: var(--mf-sp-1);
}
.segments {
  display: grid;
  gap: var(--mf-sp-2);
}
.segments > div {
  padding: var(--mf-sp-3);
  display: flex;
  gap: var(--mf-sp-3);
  flex-wrap: wrap;
  border-left: 3px solid var(--mf-primary);
}
.segments .unknown {
  border-color: var(--mf-danger);
}
.segments strong {
  font-weight: 500;
  white-space: pre-wrap;
}
.unknown small,
.error {
  color: var(--mf-danger);
}
</style>
