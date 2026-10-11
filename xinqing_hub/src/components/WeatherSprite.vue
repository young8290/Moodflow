<script setup lang="ts">
// 天气小精灵（16 D-03、07 DS-MOTION-02）：lottie 动画（lottie-web 的 SVG 精简播放器，03 第 5 节），
// 素材由 tools/gen_sprite_lottie.py 生成（sprite.ts），每种天气一个文件，按标记分段播放：
// - loop：4 秒循环——呼吸、眨眼，加上各自配饰的动作；
// - play('shake')“晃一下”（打错字，0.5 秒）、play('approach')“靠近”（出现暖心话，0.6 秒），播完回到 loop；
// - eyesClosed 变真时播 close（闭眼）再循环 sleep，变假时倒着播 close（睁眼）再回到 loop；
// - 系统关闭动画时不播，只停在 loop / sleep 的第一帧（DS-MOTION-03）；窗口不可见时暂停（DS-MOTION-05）；
//   闪电 4 秒暗一下，远低于每秒 3 次（DS-MOTION-04）。
// 颜色不在素材里：每个图层只有一种填充或描边、带一个 class，由下面的 CSS 按设计令牌上色，深浅色与高对比度跟着令牌走。
// 状态除颜色外还有各自的配饰形状（DS-COLOR-02）。
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web/build/player/lottie_light'
import type { Weather } from '@/api'
import { t } from '@/i18n'
import { weatherColor, weatherName } from '@/weather'
import { SPRITES, segment, type SpriteSegment } from './sprite'

const props = withDefaults(defineProps<{ weather: Weather; eyesClosed?: boolean; size?: number }>(), {
  eyesClosed: false,
  size: 96,
})

export type SpriteAction = 'shake' | 'approach'

const label = computed(() => t('widget.sprite_label', { weather: weatherName(props.weather) }))
const host = ref<HTMLDivElement | null>(null)
let anim: AnimationItem | null = null
/** 正在播一次性的段（晃一下、靠近、闭眼 / 睁眼），播完回到循环 */
let oneShot = false
const motion = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-reduced-motion: reduce)') : null

const reduced = () => motion?.matches ?? false
const range = (name: SpriteSegment) => segment(SPRITES[props.weather], name)

/** 回到循环：睁眼是 loop，闭眼是 sleep；关闭动画时只停在那一段的第一帧 */
function rest(): void {
  if (!anim) return
  oneShot = false
  const [from, to] = range(props.eyesClosed ? 'sleep' : 'loop')
  if (reduced()) {
    anim.goToAndStop(from, true)
    return
  }
  anim.loop = true
  anim.playSegments([from, to], true)
  if (document.hidden) anim.pause()
}

/** 播一段一次（`to < from` 时倒着播），播完回到循环 */
function once(from: number, to: number): void {
  if (!anim) return
  if (reduced()) return rest()
  oneShot = true
  anim.loop = false
  anim.playSegments([from, to], true)
  if (document.hidden) anim.pause()
}

function play(a: SpriteAction): void {
  // 闭着眼（无痕、暂停）时不晃也不凑近
  if (props.eyesClosed) return
  const [from, to] = range(a)
  once(from, to)
}

function load(): void {
  anim?.destroy()
  anim = null
  if (!host.value) return
  anim = lottie.loadAnimation({
    container: host.value,
    renderer: 'svg',
    loop: true,
    autoplay: false,
    // lottie 会改写传进去的数据，每只小精灵用一份拷贝
    animationData: structuredClone(SPRITES[props.weather]),
    rendererSettings: { preserveAspectRatio: 'xMidYMid meet', focusable: false },
  })
  anim.addEventListener('complete', () => {
    if (oneShot) rest()
  })
  rest()
}

function onVisibility(): void {
  if (!anim) return
  if (document.hidden) anim.pause()
  else if (!reduced()) anim.play()
}

watch(
  () => props.eyesClosed,
  (closed) => {
    const [from, to] = range('close')
    if (closed) once(from, to)
    else once(to, from)
  },
)
watch(() => props.weather, load)

onMounted(() => {
  load()
  document.addEventListener('visibilitychange', onVisibility)
  motion?.addEventListener('change', rest)
})

onBeforeUnmount(() => {
  document.removeEventListener('visibilitychange', onVisibility)
  motion?.removeEventListener('change', rest)
  anim?.destroy()
  anim = null
})

defineExpose({ play })
</script>

<template>
  <div
    class="sprite"
    role="img"
    :aria-label="label"
    :data-weather="weather"
    :style="{ width: `${size}px`, height: `${size}px`, '--sprite-w': weatherColor(weather) }"
  >
    <div ref="host" class="host" aria-hidden="true" />
  </div>
</template>

<style scoped>
.sprite {
  flex: none;
}

.host {
  width: 100%;
  height: 100%;
}

/* 晃一下、凑近时会稍微超出画布 */
.host :deep(svg) {
  overflow: visible;
}

/* 按图层的 class 上色（tools/gen_sprite_lottie.py）：xq-fill-* 是填充，xq-line-* 是描边 */
.host :deep(.xq-fill-w path) {
  fill: var(--sprite-w);
}

.host :deep(.xq-fill-on path) {
  fill: var(--xq-on-weather);
}

.host :deep(.xq-line-on path) {
  stroke: var(--xq-on-weather);
}

/* 光芒和风线画在身体外面、直接落在窗口底色上，要跟随深浅色 */
.host :deep(.xq-line-text path) {
  stroke: var(--xq-text-2);
}
</style>
