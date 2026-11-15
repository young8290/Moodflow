<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import {
  advance,
  drillPool,
  physicalKey,
  scoreAttempt,
  shortKey,
  startDrill,
  type ChordError,
  type ChordOutcome,
  type ChordProfile,
  type ChordStroke,
  type RehearsalResponse,
  type Scope,
} from './chordPractice'

const props = defineProps<{ profile: ChordProfile }>()
const emit = defineEmits<{ close: [] }>()
const source = computed(() => JSON.stringify(props.profile))
const loaded = ref<Extract<RehearsalResponse, { physical: object }> | null>(null)
const physical = computed(() => loaded.value?.physical ?? {})
const capture = ref<HTMLElement | null>(null)
const summary = ref<HTMLElement | null>(null)
const heading = ref<HTMLElement | null>(null)
const focused = ref(false)
const running = ref(false)
const checking = ref(false)
const error = ref('')
const feedback = ref('')
const drills = ref(false)
const scope = ref<Scope>('all')
const showHint = ref(true)
const round = ref(0)
const drill = ref(startDrill([], 0))
const target = computed(() => drill.value.targets[drill.value.index])
const choices = computed(() => drillPool(props.profile, scope.value))
const held = ref(new Set<string>())
const trace = ref<{ id: number; outcomes: ChordOutcome[]; span: number }[]>([])
const waiting = ref(false)
let generation = 0
let sequence = 0
let started = 0
let pending: ChordStroke[] = []
let timer: ReturnType<typeof setTimeout> | undefined

const groups = computed(() => [
  { title: 'rehearsal.left' as const, keys: props.profile.left_keys },
  { title: 'rehearsal.right' as const, keys: props.profile.right_keys },
])
const errorKeys = {
  profile_limit: 'rehearsal.profile_limit',
  invalid_profile: 'rehearsal.invalid_profile',
  empty_profile: 'rehearsal.empty_profile',
  unsupported_key: 'rehearsal.unsupported_key',
  duplicate_key: 'rehearsal.duplicate_key',
  trace_limit: 'rehearsal.trace_limit',
  invalid_trace: 'rehearsal.invalid_trace',
} as const satisfies Record<ChordError, string>

async function replay(text: string, strokes: ChordStroke[]) {
  const response = await unwrap(commands.imeTools('chord.rehearse', JSON.stringify({ text, strokes })))
  return JSON.parse(response) as RehearsalResponse
}

function clearPending(): void {
  clearTimeout(timer)
  held.value = new Set()
  pending = []
  waiting.value = false
}

function resetRound(): void {
  drill.value = startDrill(choices.value, round.value)
  feedback.value = ''
}

async function checkProfile(): Promise<void> {
  const attempt = ++generation
  checking.value = true
  running.value = false
  loaded.value = null
  error.value = ''
  trace.value = []
  round.value = 0
  clearPending()
  resetRound()
  try {
    const result = await replay(source.value, [])
    if (generation !== attempt) return
    if ('error' in result) error.value = t(errorKeys[result.error], { key: result.key ?? '' })
    else loaded.value = result
  } catch (e) {
    if (generation === attempt) error.value = errorText(e)
  } finally {
    if (generation === attempt) checking.value = false
  }
}

async function finish(): Promise<void> {
  if (!pending.length || held.value.size || !loaded.value) return
  const strokes = pending
  clearPending()
  const attempt = generation
  running.value = true
  error.value = ''
  feedback.value = ''
  try {
    const result = await replay(source.value, strokes)
    if (generation !== attempt) return
    if ('error' in result) {
      error.value = t(errorKeys[result.error], { key: result.key ?? '' })
      return
    }
    trace.value = [
      { id: sequence++, outcomes: result.outcomes, span: strokes.at(-1)?.atMs ?? 0 },
      ...trace.value,
    ].slice(0, 12)
    if (drills.value && target.value) {
      feedback.value = t(scoreAttempt(drill.value, result.outcomes) ? 'rehearsal.correct' : 'rehearsal.retry')
      if (!target.value && focused.value) {
        await nextTick()
        summary.value?.focus()
      }
    }
  } catch (e) {
    if (generation === attempt) error.value = errorText(e)
  } finally {
    if (generation === attempt) running.value = false
  }
}

function stroke(event: KeyboardEvent, down: boolean): void {
  if (!loaded.value || (down && (event.isComposing || event.ctrlKey || event.altKey || event.metaKey))) return
  const position = physicalKey(event.code)
  const key = position && physical.value[position]
  if (!key) return
  event.preventDefault()
  if (running.value || (drills.value && !target.value) || event.repeat || held.value.has(key) === down) return
  if (!pending.length) {
    started = event.timeStamp
    error.value = ''
  }
  const atMs = Math.max(pending.at(-1)?.atMs ?? 0, Math.round(event.timeStamp - started))
  if (pending.length >= loaded.value.maxStrokes || atMs > 120_000) {
    clearPending()
    error.value = t('rehearsal.trace_limit')
    return
  }
  clearTimeout(timer)
  waiting.value = false
  pending.push({ key, down, atMs })
  if (down) held.value.add(key)
  else {
    held.value.delete(key)
    if (!held.value.size) {
      waiting.value = true
      timer = setTimeout(() => void finish(), loaded.value.intervalMs)
    }
  }
}

function keyDown(event: KeyboardEvent): void {
  focused.value = true
  stroke(event, true)
}

function leaveCapture(): void {
  const interrupted = pending.length > 0
  focused.value = false
  clearPending()
  if (interrupted) feedback.value = t('rehearsal.interrupted')
}

async function focusCapture(): Promise<void> {
  await nextTick()
  capture.value?.focus()
}

function changeMode(guided: boolean): void {
  clearPending()
  drills.value = guided
  round.value = 0
  resetRound()
  void focusCapture()
}

function skip(): void {
  clearPending()
  advance(drill.value)
  feedback.value = t('rehearsal.skipped')
  void nextTick(() => (target.value ? capture.value : summary.value)?.focus())
}

function newRound(): void {
  clearPending()
  round.value++
  resetRound()
  void focusCapture()
}

watch(source, () => void checkProfile())
watch(scope, () => {
  clearPending()
  round.value = 0
  resetRound()
})
onMounted(() => {
  heading.value?.focus()
  window.addEventListener('blur', leaveCapture)
  void checkProfile()
})
onBeforeUnmount(() => {
  window.removeEventListener('blur', leaveCapture)
  generation++
  clearPending()
})
</script>

<template>
  <section class="rehearsal" :aria-label="t('rehearsal.title')" :aria-busy="checking || running">
    <header>
      <div>
        <h4 ref="heading" tabindex="-1">{{ t('rehearsal.title') }}</h4>
        <p>{{ t('rehearsal.description') }}</p>
      </div>
      <button @click="emit('close')">{{ t('rehearsal.close') }}</button>
    </header>

    <p v-if="checking" role="status">{{ t('rehearsal.checking') }}</p>
    <div v-if="error" class="actions">
      <p class="fault" role="alert">{{ error }}</p>
      <button v-if="!loaded && !checking" @click="checkProfile">{{ t('tools.reload') }}</button>
    </div>

    <template v-if="loaded">
      <div class="actions">
        <button :aria-pressed="!drills" :disabled="running" @click="changeMode(false)">
          {{ t('rehearsal.free') }}
        </button>
        <button :aria-pressed="drills" :disabled="running" @click="changeMode(true)">
          {{ t('rehearsal.drill') }}
        </button>
        <span class="muted">{{ t('rehearsal.interval', { ms: loaded.intervalMs }) }}</span>
      </div>

      <section v-if="drills" class="drill">
        <div class="actions">
          <label
            >{{ t('rehearsal.scope') }}
            <select v-model="scope" :disabled="running">
              <option value="all">{{ t('rehearsal.all') }}</option>
              <option value="single">{{ t('rehearsal.single') }}</option>
              <option value="both">{{ t('rehearsal.both') }}</option>
            </select>
          </label>
          <label><input v-model="showHint" type="checkbox" /> {{ t('rehearsal.hint') }}</label>
          <button v-if="target" :disabled="running" @click="skip">{{ t('rehearsal.skip') }}</button>
        </div>
        <p v-if="!drill.targets.length">{{ t('rehearsal.no_targets') }}</p>
        <div v-else-if="target" class="target">
          <span>{{ t('rehearsal.step', { current: drill.index + 1, count: drill.targets.length }) }}</span>
          <strong>{{ target.output }}</strong>
          <span>{{ t(target.kind === 'fragment' ? 'tools.fragment' : 'tools.syllable') }}</span>
          <div v-if="showHint" :aria-label="t('rehearsal.keys_hint')">
            <kbd v-for="key in [...new Set(target.keys)]" :key="key">{{ shortKey(key, physical) }}</kbd>
          </div>
          <p v-else>{{ t('rehearsal.hidden_hint') }}</p>
        </div>
        <div v-else ref="summary" class="summary" tabindex="-1">
          <h5>{{ t('rehearsal.complete') }}</h5>
          <dl>
            <div>
              <dt>{{ t('rehearsal.solved') }}</dt>
              <dd>{{ drill.solved }}/{{ drill.targets.length }}</dd>
            </div>
            <div>
              <dt>{{ t('rehearsal.first_hits') }}</dt>
              <dd>{{ drill.firstHits }}/{{ drill.targets.length }}</dd>
            </div>
            <div>
              <dt>{{ t('rehearsal.attempts') }}</dt>
              <dd>{{ drill.attempts }}</dd>
            </div>
          </dl>
          <button :disabled="running" @click="newRound">{{ t('rehearsal.next_round') }}</button>
        </div>
      </section>

      <div
        v-if="!drills || target"
        ref="capture"
        class="capture"
        tabindex="0"
        role="group"
        :aria-label="t('rehearsal.capture')"
        @focus="focused = true"
        @blur="leaveCapture"
        @keydown="keyDown"
        @keyup="stroke($event, false)"
      >
        <strong>{{ t(focused ? 'rehearsal.focused' : 'rehearsal.capture') }}</strong>
        <p>{{ t('rehearsal.capture_help') }}</p>
        <div class="hands">
          <div v-for="group in groups" :key="group.title">
            <small>{{ t(group.title) }}</small>
            <div class="key-list">
              <kbd
                v-for="key in group.keys"
                :key="key"
                :class="{ held: held.has(key), hint: drills && showHint && target?.keys.includes(key) }"
                >{{ shortKey(key, physical) }}</kbd
              >
            </div>
          </div>
        </div>
        <p v-if="waiting" class="muted">{{ t('rehearsal.waiting') }}</p>
        <p v-if="running" class="muted">{{ t('rehearsal.evaluating') }}</p>
      </div>
      <p v-if="feedback" role="status" class="feedback">{{ feedback }}</p>

      <section class="history">
        <div class="actions">
          <h5>{{ t('rehearsal.history') }}</h5>
          <button :disabled="!trace.length || running" @click="trace = []">{{ t('rehearsal.clear') }}</button>
        </div>
        <p v-if="!trace.length">{{ t('rehearsal.no_results') }}</p>
        <ol v-else>
          <li v-for="entry in trace" :key="entry.id">
            <p class="muted">{{ t('rehearsal.span', { ms: entry.span }) }}</p>
            <ul>
              <li v-for="(outcome, i) in entry.outcomes" :key="i" class="outcome">
                <span>{{ outcome.keys.map((key) => shortKey(key, physical)).join(' + ') }}</span>
                <strong v-if="outcome.output"
                  >{{ outcome.output }}
                  <small>{{ t(outcome.kind === 'fragment' ? 'tools.fragment' : 'tools.syllable') }}</small>
                </strong>
                <strong v-else class="fault">{{ t('rehearsal.no_match') }}</strong>
                <small class="muted">
                  {{ t('rehearsal.settlement', { ms: outcome.atMs }) }}
                  {{ outcome.idle ? t('rehearsal.after_wait') : '' }}
                </small>
              </li>
            </ul>
          </li>
        </ol>
      </section>
    </template>
  </section>
</template>

<style scoped>
.rehearsal,
.drill,
.history {
  display: grid;
  gap: var(--mf-sp-2);
}
.rehearsal {
  margin-block: var(--mf-sp-4);
  padding: var(--mf-sp-4);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface-2);
}
header,
.actions,
.hands,
.outcome,
dl {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--mf-sp-2);
}
header {
  justify-content: space-between;
}
header > div {
  flex: 1 1 260px;
}
h4,
h5,
p,
dl,
dd {
  margin: 0;
}
.muted,
header p,
.capture p,
small {
  color: var(--mf-text-2);
}
button[aria-pressed='true'] {
  border-color: var(--mf-primary);
  color: var(--mf-link);
}
.actions h5 {
  flex: 1;
}
.capture,
.target,
.summary {
  padding: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
  background: var(--mf-surface);
}
.capture:focus {
  outline: 2px solid var(--mf-focus);
  outline-offset: 2px;
}
.hands {
  align-items: flex-start;
  margin-block: var(--mf-sp-2);
}
.hands > div {
  flex: 1 1 170px;
}
.key-list {
  display: flex;
  flex-wrap: wrap;
}
kbd {
  display: inline-block;
  min-inline-size: 2.1em;
  margin: 2px;
  padding: 2px var(--mf-sp-1);
  border: 1px solid var(--mf-border);
  border-radius: 4px;
  text-align: center;
}
kbd.hint {
  border-color: var(--mf-link);
}
kbd.held {
  background: var(--mf-primary);
  color: var(--mf-on-primary);
}
.target {
  display: grid;
  gap: var(--mf-sp-1);
}
.target strong {
  font-size: 1.5em;
  overflow-wrap: anywhere;
}
.summary dl {
  gap: var(--mf-sp-4);
  margin-block: var(--mf-sp-2);
}
dd {
  font-weight: 600;
}
.history ol {
  padding-inline-start: 1.5em;
  margin: 0;
}
.history ol > li {
  padding: var(--mf-sp-2);
  border-top: 1px solid var(--mf-border);
}
.history ul {
  padding: 0;
  list-style: none;
}
.outcome {
  overflow-wrap: anywhere;
}
.fault {
  color: var(--mf-danger);
}
.feedback {
  font-weight: 600;
}
</style>
