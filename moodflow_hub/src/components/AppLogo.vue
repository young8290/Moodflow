<script setup lang="ts">
// Moodflow logo：一笔起伏的波浪写成“M”，右上角一颗小太阳。源稿是 assets/brand/mark.svg，
// 应用图标由 scripts/moodflow/gen_icons.py 从 assets/brand/icon-1024.png 生成；改形状时两边一起改。
// 第一段笔画用 --mf-logo-ink（浅色深蓝、深色白色），波浪蓝和太阳黄是品牌色，不随主题变。
import { t } from '@/i18n'

// `decorative`：旁边已经写着“Moodflow”（引导页标题）时读屏不必再念一遍
withDefaults(defineProps<{ size?: number; decorative?: boolean }>(), { size: 64, decorative: false })

const WAVE =
  'M276 408 C 320 356, 360 298, 386 274 C 420 244, 450 268, 470 330 C 488 386, 502 414, 530 412 ' +
  'C 558 410, 586 342, 612 312 C 636 286, 662 288, 686 320 C 704 346, 720 372, 742 386'
const INK = 'M276 408 C 320 356, 360 298, 386 274'
const BRIDGE = 'M384 276 C 420 244, 450 268, 470 330'
</script>

<template>
  <svg
    class="logo"
    :width="size"
    :height="size"
    viewBox="240 60 540 540"
    :role="decorative ? undefined : 'img'"
    :aria-label="decorative ? undefined : t('window.widget')"
    :aria-hidden="decorative ? 'true' : undefined"
  >
    <path class="wave" :d="WAVE" stroke-linecap="round" stroke-linejoin="round" />
    <path class="ink" :d="INK" stroke-linecap="round" />
    <path class="wave" :d="BRIDGE" stroke-linecap="butt" />
    <circle class="sun" cx="712" cy="250" r="33" />
  </svg>
</template>

<style scoped>
.logo {
  display: block;
  flex: none;
}

.wave,
.ink {
  fill: none;
  stroke-width: 62;
}

.wave {
  stroke: var(--mf-logo-wave);
}

.ink {
  stroke: var(--mf-logo-ink);
}

.sun {
  fill: var(--mf-logo-sun);
}
</style>
