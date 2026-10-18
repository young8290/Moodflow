<script setup lang="ts">
// 看板 · 打字测速（XSIME 吸收计划 §3.3）：照着范文打一段，出有效速度、过程正确率、最终字准。
// 计分交给 Rust（practice_score，与输入法核心同一份口径）。这里打的字不保存、不计入日常统计
// （核心对 Moodflow 自己窗口里的输入不记日常统计）。
import { ref } from 'vue'
import { commands, type PracticeScore } from '@/api'
import { t } from '@/i18n'
import { nextPassage, PASSAGES, PracticeRecorder, reachedEnd } from './practice'

const target = ref<string>(PASSAGES[0]!)
const typed = ref('')
const score = ref<PracticeScore | null>(null)
let recorder = new PracticeRecorder()

function reset(passage: string): void {
  target.value = passage
  typed.value = ''
  score.value = null
  recorder = new PracticeRecorder()
}

async function finish(): Promise<void> {
  if (!recorder.started || score.value) return
  score.value = await commands.practiceScore(target.value, recorder.events, Math.round(recorder.elapsed()))
}

function onInput(e: Event): void {
  typed.value = (e.target as HTMLTextAreaElement).value
  recorder.record(typed.value)
  if (reachedEnd(typed.value, target.value)) void finish()
}

const pct = (v: number) => Math.round(v * 1000) / 10
</script>

<template>
  <section>
    <h1>{{ t('dashboard.nav.practice') }}</h1>
    <p class="muted d-small">{{ t('dashboard.practice.intro') }}</p>
    <blockquote class="passage">{{ target }}</blockquote>
    <textarea
      class="input"
      :value="typed"
      :placeholder="t('dashboard.practice.placeholder')"
      :disabled="score !== null"
      rows="4"
      @input="onInput"
    />
    <div class="d-row">
      <button v-if="!score" :disabled="!typed" @click="finish">{{ t('dashboard.practice.finish') }}</button>
      <button v-else @click="reset(target)">{{ t('dashboard.practice.again') }}</button>
      <button @click="reset(nextPassage(target))">{{ t('dashboard.practice.start') }}</button>
    </div>
    <ul v-if="score" class="d-list result" aria-live="polite">
      <li>{{ t('dashboard.practice.elapsed', { secs: (score.elapsed_ms / 1000).toFixed(1) }) }}</li>
      <li>{{ t('dashboard.practice.speed', { cpm: score.effective_cpm.toFixed(1) }) }}</li>
      <li>{{ t('dashboard.practice.process', { pct: pct(score.process_accuracy) }) }}</li>
      <li>{{ t('dashboard.practice.final', { pct: pct(score.final_accuracy) }) }}</li>
    </ul>
    <p v-if="score" class="muted d-small">{{ t('dashboard.practice.explain') }}</p>
  </section>
</template>

<style scoped>
.passage {
  margin: var(--mf-sp-3) 0;
  padding: var(--mf-sp-3);
  border-left: 3px solid var(--mf-link);
  background: var(--mf-surface-2);
  line-height: 1.8;
}

.input {
  box-sizing: border-box;
  width: 100%;
  font: inherit;
  line-height: 1.8;
}

.result li {
  font-size: var(--mf-fs-lg, 1.1em);
}
</style>
