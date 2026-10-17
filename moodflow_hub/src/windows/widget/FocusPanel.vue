<script setup lang="ts">
// 选专注时长（06 FR-RST-10）：25 / 45 / 60 分钟，或自定义 10–120 分钟。Esc 关闭。
import { onMounted, ref } from 'vue'
import { t } from '@/i18n'
import { MAX_MINUTES, MIN_MINUTES, PRESETS, customMinutes } from './focus'

const emit = defineEmits<{ start: [minutes: number]; close: [] }>()
const root = ref<HTMLElement | null>(null)
const custom = ref<string | number>('')
const invalid = ref(false)

function startCustom(): void {
  const m = customMinutes(custom.value)
  invalid.value = m === null
  if (m !== null) emit('start', m)
}

onMounted(() => root.value?.querySelector<HTMLButtonElement>('.preset')?.focus())
</script>

<template>
  <div
    ref="root"
    class="focus-panel"
    role="dialog"
    :aria-label="t('focus.panel_title')"
    @pointerdown.stop
    @keydown.esc.stop="emit('close')"
  >
    <p class="title">{{ t('focus.panel_title') }}</p>
    <div class="presets">
      <button v-for="m in PRESETS" :key="m" class="preset compact" @click.stop="emit('start', m)">
        {{ t('focus.minutes', { n: m }) }}
      </button>
    </div>
    <form class="custom" @submit.prevent.stop="startCustom">
      <label>
        {{ t('focus.custom') }}
        <input
          v-model="custom"
          type="number"
          :min="MIN_MINUTES"
          :max="MAX_MINUTES"
          step="5"
          :placeholder="t('focus.custom_hint')"
          @keydown.enter.stop
        />
      </label>
      <button type="submit" class="compact">{{ t('focus.start') }}</button>
    </form>
    <p v-if="invalid" class="error" role="alert">{{ t('focus.custom_invalid') }}</p>
    <p class="hint">{{ t('focus.panel_hint') }}</p>
    <button class="compact cancel" @click.stop="emit('close')">{{ t('focus.cancel') }}</button>
  </div>
</template>

<style scoped>
.focus-panel {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-2);
  padding: var(--mf-sp-3);
  background: var(--mf-surface);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}

.title {
  margin: 0;
  font-weight: 600;
}

.presets,
.custom {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: center;
}

.custom input {
  width: 5em;
  margin-left: var(--mf-sp-1);
}

.hint {
  margin: 0;
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.error {
  margin: 0;
  color: var(--mf-danger);
  font-size: var(--mf-fs-xs);
}

.cancel {
  align-self: flex-end;
}
</style>
