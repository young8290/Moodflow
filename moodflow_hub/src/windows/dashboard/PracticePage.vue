<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap, type PracticeCategory, type PracticeMode, type PracticeScore } from '@/api'
import { errorText, t } from '@/i18n'
import PracticeHistory from './PracticeHistory.vue'
import PracticeReview from './PracticeReview.vue'
import {
  clipText,
  MAX_PRACTICE_CHARS,
  MAX_PRACTICE_EVENTS,
  passageChoices,
  PRACTICE_CATEGORIES,
  PRACTICE_MODES,
  PracticeRecorder,
  reachedEnd,
  scoredTarget,
  timeLimit,
  type TrainingRound,
} from './practice'

const category = ref<PracticeCategory>('chinese')
const mode = ref<PracticeMode>('passage')
const index = ref(0)
const custom = ref('')
const round = ref<TrainingRound | null>(null)
const typed = ref('')
const elapsed = ref(0)
const input = ref<HTMLTextAreaElement>()
const active = ref(false)
const composing = ref(false)
const finishing = ref(false)
const ended = ref(false)
const error = ref('')
const notice = ref('')
const result = ref<{ id: string; score: PracticeScore; target: string; typed: string } | null>(null)
const history = ref<InstanceType<typeof PracticeHistory>>()
let recorder = new PracticeRecorder()
let generation = 0
let timer: ReturnType<typeof setInterval> | undefined

const choices = computed(() =>
  category.value === 'custom' ? [] : passageChoices(category.value, mode.value),
)
const target = computed(() =>
  category.value === 'custom' ? clipText(custom.value.trim()) : (choices.value[index.value] ?? ''),
)
const passageSize = computed(() => [...(round.value?.target ?? target.value)].length)
const typedSize = computed(() => [...typed.value].length)
const locked = computed(() => active.value || finishing.value)
const duration = computed(() => timeLimit(round.value?.mode ?? mode.value))
const progress = computed(() => Math.min(typedSize.value, passageSize.value))
const seconds = computed(() =>
  (round.value?.mode ?? mode.value) === 'passage'
    ? elapsed.value / 1000
    : Math.ceil(Math.max(0, duration.value - elapsed.value) / 1000),
)
const saveInput = computed(() =>
  round.value && result.value && result.value.score.elapsed_ms >= 1000 && result.value.score.typed_chars > 0
    ? {
        id: result.value.id,
        category: round.value.category,
        mode: round.value.mode,
        score: result.value.score,
      }
    : null,
)

watch([category, mode], () => {
  index.value = 0
})
function reset() {
  generation++
  clearInterval(timer)
  recorder = new PracticeRecorder()
  active.value = false
  composing.value = false
  finishing.value = false
  ended.value = false
  typed.value = ''
  elapsed.value = 0
  round.value = null
  result.value = null
  error.value = ''
  notice.value = ''
  if (input.value) input.value.value = ''
}
function applyCustom(event: Event) {
  const field = event.target as HTMLTextAreaElement
  custom.value = clipText(field.value)
  if (field.value !== custom.value) field.value = custom.value
}
function tick() {
  if (!recorder.started || finishing.value || !round.value) return
  elapsed.value = Math.min(recorder.elapsed(), duration.value)
  if (elapsed.value >= duration.value) void finish()
}
async function finish() {
  const session = round.value
  if (!session || !recorder.started || finishing.value || result.value) return
  clearInterval(timer)
  finishing.value = true
  active.value = false
  composing.value = false
  error.value = ''
  if (!ended.value) elapsed.value = Math.min(recorder.elapsed(), timeLimit(session.mode))
  ended.value = true
  const finalTyped = recorder.text
  typed.value = finalTyped
  if (input.value) input.value.value = finalTyped
  const reference = scoredTarget(session, finalTyped)
  const mine = generation
  try {
    const score = await unwrap(commands.practiceScore(reference, [...recorder.events], elapsed.value))
    if (mine === generation)
      result.value = { id: crypto.randomUUID(), score, target: reference, typed: finalTyped }
  } catch (e) {
    if (mine === generation) error.value = errorText(e)
  } finally {
    if (mine === generation) finishing.value = false
  }
}
function commit(event: Event) {
  const field = event.target as HTMLTextAreaElement
  if (composing.value || (event as InputEvent).isComposing) return
  if (ended.value || !target.value) {
    field.value = typed.value
    return
  }
  const timestamp = recorder.elapsed()
  if (round.value && timestamp >= duration.value) {
    void finish()
    return
  }
  const value = clipText(field.value)
  if (field.value !== value) field.value = value
  if (!recorder.started && !value) return
  if (!round.value) {
    round.value = { target: target.value, category: category.value, mode: mode.value }
    active.value = true
    notice.value = ''
    timer = setInterval(tick, 200)
  }
  recorder.record(value, timestamp)
  typed.value = recorder.text
  if (
    !composing.value &&
    (reachedEnd(typed.value, round.value.target) || recorder.events.length >= MAX_PRACTICE_EVENTS)
  )
    void finish()
}
function compositionEnd(event: CompositionEvent) {
  composing.value = false
  commit(event)
}
function blockPaste(event: Event) {
  event.preventDefault()
  notice.value = t('dashboard.practice.no_paste')
}
function tryAgain() {
  reset()
  requestAnimationFrame(() => input.value?.focus())
}
function next() {
  if (choices.value.length > 1) index.value = (index.value + 1) % choices.value.length
  reset()
}
onBeforeUnmount(() => {
  generation++
  clearInterval(timer)
})
const pct = (n: number) => Math.round(n * 1000) / 10
</script>

<template>
  <section class="training">
    <h1>{{ t('dashboard.nav.practice') }}</h1>
    <p class="hint">{{ t('dashboard.practice.intro') }}</p>
    <fieldset :disabled="locked" class="options">
      <label
        >{{ t('dashboard.practice.category') }}
        <select v-model="category" @change="reset">
          <option v-for="key in PRACTICE_CATEGORIES" :key="key" :value="key">
            {{ t(`dashboard.practice.category_${key}`) }}
          </option>
        </select>
      </label>
      <label
        >{{ t('dashboard.practice.mode') }}
        <select v-model="mode" @change="reset">
          <option v-for="key in PRACTICE_MODES" :key="key" :value="key">
            {{ t(`dashboard.practice.mode_${key}`) }}
          </option>
        </select>
      </label>
      <label v-if="category !== 'custom'"
        >{{ t('dashboard.practice.passage') }}
        <select v-model.number="index" @change="reset">
          <option v-for="(_, n) in choices" :key="n" :value="n">
            {{ t('dashboard.practice.passage_number', { n: n + 1 }) }}
          </option>
        </select>
      </label>
      <label v-else class="custom"
        >{{ t('dashboard.practice.custom_prompt') }}
        <textarea
          :value="custom"
          :maxlength="MAX_PRACTICE_CHARS * 2"
          rows="3"
          spellcheck="false"
          @input="applyCustom"
          @change="reset"
        />
      </label>
    </fieldset>
    <p v-if="mode !== 'passage'" class="hint">{{ t('dashboard.practice.timed_hint') }}</p>
    <blockquote v-if="target" class="passage">
      <span class="reached">{{ [...(round?.target ?? target)].slice(0, progress).join('') }}</span
      >{{ [...(round?.target ?? target)].slice(progress).join('') }}
    </blockquote>
    <div v-if="target" class="progress">
      <progress :value="progress" :max="passageSize" :aria-label="t('dashboard.practice.progress')" />
      <span>{{ t('dashboard.practice.char_count', { done: typedSize, total: passageSize }) }}</span>
      <span>{{
        t(mode === 'passage' ? 'dashboard.practice.elapsed' : 'dashboard.practice.remaining', {
          secs: mode === 'passage' ? seconds.toFixed(1) : seconds,
        })
      }}</span>
    </div>
    <label class="input-label"
      >{{ t('dashboard.practice.placeholder') }}
      <textarea
        ref="input"
        class="input"
        :disabled="!target || ended"
        :placeholder="t('dashboard.practice.placeholder')"
        rows="4"
        spellcheck="false"
        autocomplete="off"
        @input="commit"
        @compositionstart="composing = true"
        @compositionend="compositionEnd"
        @paste="blockPaste"
        @drop="blockPaste"
      />
    </label>
    <div class="d-row">
      <button v-if="active" :disabled="composing" @click="finish">
        {{ t('dashboard.practice.finish') }}
      </button>
      <button v-if="ended && error && !result" @click="finish">
        {{ t('dashboard.practice.retry_score') }}
      </button>
      <button v-if="active || ended" @click="tryAgain">{{ t('dashboard.practice.again') }}</button>
      <button v-if="choices.length > 1" :disabled="locked" @click="next">
        {{ t('dashboard.practice.start') }}
      </button>
    </div>
    <p v-if="finishing" class="hint" role="status">{{ t('dashboard.practice.scoring') }}</p>
    <p v-if="notice" class="hint" role="status">{{ notice }}</p>
    <p v-if="error" class="d-error" role="alert">{{ error }}</p>
    <section v-if="result" class="d-card">
      <h2>{{ t('dashboard.practice.result') }}</h2>
      <ul class="d-list result" aria-live="polite">
        <li>{{ t('dashboard.practice.elapsed', { secs: (result.score.elapsed_ms / 1000).toFixed(1) }) }}</li>
        <li>{{ t('dashboard.practice.speed', { cpm: result.score.effective_cpm.toFixed(1) }) }}</li>
        <li>{{ t('dashboard.practice.process', { pct: pct(result.score.process_accuracy) }) }}</li>
        <li>{{ t('dashboard.practice.final', { pct: pct(result.score.final_accuracy) }) }}</li>
      </ul>
      <p class="hint">{{ t('dashboard.practice.explain') }}</p>
      <PracticeReview :target="result.target" :typed="result.typed" />
      <button v-if="saveInput" class="save" @click="history?.save(saveInput)">
        {{ t('dashboard.practice.history_save') }}
      </button>
      <p v-else class="hint">{{ t('dashboard.practice.history_minimum') }}</p>
    </section>
    <PracticeHistory ref="history" />
  </section>
</template>

<style scoped>
.training {
  display: grid;
  gap: var(--mf-sp-3);
  max-width: 920px;
}
.training h1,
.training p,
.training ul {
  margin-block: 0;
}
.hint {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
.options {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  padding: var(--mf-sp-3);
}
.options label,
.input-label {
  display: grid;
  gap: var(--mf-sp-1);
}
.custom {
  flex-basis: 100%;
}
.passage {
  margin: 0;
  padding: var(--mf-sp-3);
  border-inline-start: 3px solid var(--mf-link);
  background: var(--mf-surface-2);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.8;
}
.reached {
  color: var(--mf-link);
  font-weight: 600;
}
.progress {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--mf-sp-2);
  font-variant-numeric: tabular-nums;
}
.progress progress {
  flex: 1;
  min-width: 5rem;
}
.input {
  box-sizing: border-box;
  width: 100%;
  font: inherit;
  line-height: 1.8;
}
.result {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
  gap: var(--mf-sp-2);
}
.result li {
  font-weight: 600;
}
.save {
  margin-top: var(--mf-sp-3);
}
</style>
