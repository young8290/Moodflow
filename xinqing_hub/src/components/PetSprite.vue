<script setup lang="ts">
// 桌宠的一帧（ADR 0044）：从 Codex 格式的精灵图里取第 `row` 行第 `col` 格。内置的晴晴有身体蒙版与线条两层，
// 身体按当前天气的设计令牌上色（--xq-w-*），和原来的小精灵一样“看颜色知道心情天气”；导入的宠物照原图画。
// 纯装饰，读屏不念：状态由气泡里的文字说明。
import { computed } from 'vue'
import type { PetSheet, Weather } from '@/api'
import { weatherColor } from '@/weather'
import { CELL_H, CELL_W, COLUMNS } from '@/windows/widget/pet'

const props = defineProps<{
  sheet: PetSheet
  col: number
  row: number
  /** 精灵图缩放（0.5 / 0.75 / 1） */
  scale: number
  weather: Weather
}>()

const w = computed(() => Math.round(CELL_W * props.scale))
const h = computed(() => Math.round(CELL_H * props.scale))
const rows = computed(() => (props.sheet.version === 2 ? 11 : 9))
const layer = (url: string) => ({
  size: `${w.value * COLUMNS}px ${h.value * rows.value}px`,
  pos: `${-props.col * w.value}px ${-props.row * h.value}px`,
  url: `url("${url}")`,
})
const tinted = computed(() => props.sheet.builtin && props.sheet.body !== null && props.sheet.lines !== null)
const full = computed(() => {
  const l = layer(props.sheet.sheet)
  return { backgroundImage: l.url, backgroundSize: l.size, backgroundPosition: l.pos }
})
const body = computed(() => {
  const l = layer(props.sheet.body ?? '')
  return {
    backgroundColor: weatherColor(props.weather),
    maskImage: l.url,
    maskSize: l.size,
    maskPosition: l.pos,
    maskRepeat: 'no-repeat',
  }
})
const lines = computed(() => {
  const l = layer(props.sheet.lines ?? '')
  return { backgroundImage: l.url, backgroundSize: l.size, backgroundPosition: l.pos }
})
</script>

<template>
  <div
    class="pet"
    :style="{ width: `${w}px`, height: `${h}px` }"
    aria-hidden="true"
    :data-row="row"
    :data-col="col"
  >
    <template v-if="tinted">
      <div class="layer" :style="body" />
      <div class="layer" :style="lines" />
    </template>
    <div v-else class="layer" :style="full" />
  </div>
</template>

<style scoped>
.pet {
  position: relative;
  pointer-events: none;
}

.layer {
  position: absolute;
  inset: 0;
  background-repeat: no-repeat;
}
</style>
