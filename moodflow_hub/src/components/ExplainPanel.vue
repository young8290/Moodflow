<script setup lang="ts">
// 状态解释的展示（FR-STA-09）：标题、每条说明一行、来源与附注弱化显示。拼句在 src/explain.ts。
// `actions` 插槽放在来源那一行的右侧，给“准 / 不准”（FR-STA-07：解释与反馈按钮放在一起）。
import type { ExplainLines } from '@/explain'

defineProps<{ lines: ExplainLines }>()
</script>

<template>
  <div class="explain">
    <p class="header">{{ lines.header }}</p>
    <ul v-if="lines.signals.length > 0" class="signals">
      <li v-for="s in lines.signals" :key="s">{{ s }}</li>
    </ul>
    <div v-if="lines.footer || $slots.actions" class="bottom">
      <p class="footer">{{ lines.footer }}</p>
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.explain {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-1);
}

.header {
  margin: 0;
  font-size: var(--mf-fs-sm);
  line-height: var(--mf-lh-sm);
  font-weight: 500;
}

.signals {
  margin: 0;
  padding: 0;
  list-style: none;
  color: var(--mf-text-1);
  font-size: var(--mf-fs-xs);
  line-height: var(--mf-lh-xs);
}

/* 每条说明前一个小圆点，不靠颜色区分（DS-COLOR-02） */
.signals li::before {
  content: '· ';
  color: var(--mf-text-2);
}

.bottom {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: center;
  margin-top: auto;
}

.footer {
  flex: 1;
  min-width: 0;
  margin: 0;
  color: var(--mf-text-3);
  font-size: var(--mf-fs-xs);
  line-height: var(--mf-lh-xs);
}
</style>
