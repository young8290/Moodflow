<script setup lang="ts">
// Moodflow logo：直接用品牌原图里的标志（assets/brand/mark.png 的副本），不在代码里重画。
// 换 logo 时替换 assets/brand/ 的原图，重新裁出 mark.png 复制到这里，并重跑 scripts/moodflow/gen_icons.py。
import { t } from '@/i18n'
import mark from '@/assets/brand-mark.png'

// `decorative`：旁边已经写着“Moodflow”（引导页标题）时读屏不必再念一遍
withDefaults(defineProps<{ size?: number; decorative?: boolean }>(), { size: 64, decorative: false })
</script>

<template>
  <!-- 标志是横向的（约 2.1:1），size 指高度的参照：宽度取 size × 1.5，高度按比例 -->
  <img
    class="logo"
    :src="mark"
    :width="Math.round(size * 1.5)"
    :alt="decorative ? '' : t('window.widget')"
    :aria-hidden="decorative ? 'true' : undefined"
  />
</template>

<style scoped>
.logo {
  display: block;
  flex: none;
  height: auto;
}
</style>
