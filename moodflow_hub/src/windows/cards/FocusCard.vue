<script setup lang="ts">
// 专注结束卡片（06 FR-RST-10）：“专注 25 分钟 · 按键 1,832 次 · 上屏约 960 字”，建议休息 5 分钟。
// “开始休息”在卡片上倒数 5 分钟（附护眼提示），结束后显示“休息好啦”2 秒再收起。提前结束也一样，不做任何评价。
import { onBeforeUnmount, ref } from 'vue'
import type { FocusSummary } from '@/api'
import { t } from '@/i18n'
import CardShell from './CardShell.vue'
import { THANKS_MS } from './rest'
import { REST_SECONDS, clock, doneLine } from '../widget/focus'

const props = defineProps<{ summary: FocusSummary }>()
const emit = defineEmits<{ close: [] }>()
const ICON = '🎯'

/** 休息倒计时剩余秒数；没开始为 null，0 表示休息结束、正在显示“休息好啦” */
const left = ref<number | null>(null)
let timer: ReturnType<typeof setInterval> | undefined
let bye: ReturnType<typeof setTimeout> | undefined

function rest(): void {
  if (left.value !== null) return
  left.value = REST_SECONDS
  timer = setInterval(() => {
    left.value = (left.value ?? 1) - 1
    if (left.value > 0) return
    clearInterval(timer)
    bye = setTimeout(() => emit('close'), THANKS_MS)
  }, 1000)
}

onBeforeUnmount(() => {
  clearInterval(timer)
  clearTimeout(bye)
})
</script>

<template>
  <CardShell class="focus" :label="t('focus.card_label')" @primary="rest" @dismiss="emit('close')">
    <template #title>
      <span aria-hidden="true">{{ ICON }}</span>
      {{ doneLine(props.summary) }}
    </template>
    <p v-if="left === null">{{ t('focus.rest_hint') }}</p>
    <p v-else class="countdown" role="timer" aria-live="off">
      {{ left > 0 ? t('focus.resting', { time: clock(left) }) : t('focus.rest_done') }}
    </p>
    <p v-if="left !== null && left > 0" class="tip">{{ t('rest.eye') }}</p>
    <template v-if="left === null" #actions>
      <button class="primary" @click="rest">{{ t('focus.rest_start') }}</button>
      <button @click="emit('close')">{{ t('focus.ok') }}</button>
    </template>
  </CardShell>
</template>

<style scoped>
.countdown {
  font-size: var(--mf-fs-lg);
  font-variant-numeric: tabular-nums;
}

.tip {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}
</style>
