<script setup lang="ts">
// Moodflow logo（07 DS-BRAND-02）：一朵圆润的云后面露出半个太阳。界面内用这个矢量版；
// 应用图标由 tools/gen_hub_icons.py 按同一套几何生成（坐标都以画布的 1/100 为单位），改形状时两边一起改。
// 不使用 Moodflow 输入法的任何 logo 元素（C-LAW-08）。
import { t } from '@/i18n'

// `decorative`：旁边已经写着“Moodflow”（引导页标题）时读屏不必再念一遍
withDefaults(defineProps<{ size?: number; decorative?: boolean }>(), { size: 64, decorative: false })

// 云：三个圆加一个圆角底座（与 gen_hub_icons.py 的 cloud_shapes / base 对应）
const CIRCLES = [
  { cx: 32, cy: 68, r: 18 },
  { cx: 52, cy: 56, r: 22 },
  { cx: 71, cy: 67, r: 17 },
]
const BASE = { x: 14, y: 62, width: 74, height: 24, rx: 12 }
</script>

<template>
  <svg
    class="logo"
    :width="size"
    :height="size"
    viewBox="0 0 100 100"
    :role="decorative ? undefined : 'img'"
    :aria-label="decorative ? undefined : t('window.widget')"
    :aria-hidden="decorative ? 'true' : undefined"
  >
    <circle class="sun" cx="68" cy="32" r="24" />
    <!-- 先整体描一圈粗边，再在上面铺无边的填充：几个形状并起来只留外轮廓 -->
    <g class="outline">
      <circle v-for="(c, i) in CIRCLES" :key="i" v-bind="c" />
      <rect v-bind="BASE" />
    </g>
    <g class="cloud">
      <circle v-for="(c, i) in CIRCLES" :key="i" v-bind="c" />
      <rect v-bind="BASE" />
    </g>
  </svg>
</template>

<style scoped>
.logo {
  display: block;
  flex: none;
}

.sun {
  fill: var(--mf-w-sunny);
}

.outline > * {
  fill: var(--mf-text-2);
  stroke: var(--mf-text-2);
  stroke-width: 8;
}

.cloud > * {
  fill: var(--mf-surface);
}
</style>
