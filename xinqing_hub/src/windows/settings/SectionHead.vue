<script setup lang="ts">
// 分类标题与“恢复默认”（07 FR-SET-01）。恢复会一次改掉这一页的很多项，先在标题下问一句，点“恢复默认”才改。
import { nextTick, ref } from 'vue'
import { t } from '@/i18n'

withDefaults(defineProps<{ title: string; restorable?: boolean }>(), { restorable: true })
const emit = defineEmits<{ restore: [] }>()

const confirming = ref(false)
const noBtn = ref<HTMLButtonElement | null>(null)

async function ask(): Promise<void> {
  confirming.value = true
  await nextTick()
  // 焦点落在“先不了”：误按回车不会把设置全改掉
  noBtn.value?.focus()
}

function yes(): void {
  confirming.value = false
  emit('restore')
}
</script>

<template>
  <div class="s-head">
    <h1>{{ title }}</h1>
    <button v-if="restorable && !confirming" type="button" class="s-compact" @click="ask">
      {{ t('settings.restore') }}
    </button>
  </div>
  <div v-if="confirming" class="s-confirm" role="group" :aria-label="t('settings.restore')">
    <p>{{ t('settings.restore_confirm') }}</p>
    <div class="s-row">
      <button type="button" class="s-compact s-danger" @click="yes">{{ t('settings.restore_yes') }}</button>
      <button ref="noBtn" type="button" class="s-compact" @click="confirming = false">
        {{ t('settings.restore_no') }}
      </button>
    </div>
  </div>
</template>
