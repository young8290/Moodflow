<script setup lang="ts">
import { ref, watchEffect } from 'vue'
import katex from 'katex'
import 'katex/dist/katex.min.css'
import { t } from '@/i18n'

const props = defineProps<{ text: string }>()
const host = ref<HTMLElement | null>(null)
const failed = ref(false)
watchEffect(
  () => {
    if (!host.value) return
    host.value.replaceChildren()
    failed.value = false
    let text = props.text.trim()
    if (!text) return
    if (text.startsWith('$$') && text.endsWith('$$')) text = text.slice(2, -2)
    if (text.startsWith('\\[') && text.endsWith('\\]')) text = text.slice(2, -2)
    try {
      if (
        text.length > 16384 ||
        /<[a-z!/][^>]*>/i.test(text) ||
        /\\(?:includegraphics|href|url|html\w*|input|write|def|gdef|newcommand)\b/.test(text)
      )
        throw new Error('unsupported')
      let depth = 0
      for (const c of text) {
        if (c === '{' && ++depth > 64) throw new Error('limit')
        if (c === '}') depth--
      }
      katex.render(text, host.value, {
        trust: false,
        strict: 'error',
        throwOnError: true,
        maxExpand: 500,
        maxSize: 10,
        displayMode: true,
        output: 'htmlAndMathml',
        macros: {},
        globalGroup: false,
      })
    } catch {
      host.value.replaceChildren()
      failed.value = true
    }
  },
  { flush: 'post' },
)
</script>

<template>
  <div class="math-preview">
    <div ref="host" :aria-label="t('ai_tools.preview')" />
    <p v-if="failed" role="status">{{ t('ai_tools.preview_failed') }}</p>
  </div>
</template>

<style scoped>
.math-preview {
  overflow: auto;
  max-height: 400px;
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-sm);
  padding: var(--mf-sp-3);
  background: var(--mf-surface-2);
}
p {
  color: var(--mf-text-2);
}
</style>
