<script setup lang="ts">
// 设置中心 · 提醒（07 FR-SET-06）：四类休息提醒的开关和间隔（FR-RST-09，范围取自 settings_schema）、深夜提醒从几点开始、
// 无痕时是否继续计时（FR-SEN-06 第 4 条）。专注时段与专注计时是 P2（FR-RST-10），还没有设置键。
import { reactive } from 'vue'
import { t, type CopyKey } from '@/i18n'
import SectionHead from './SectionHead.vue'
import { useHubSettings } from './useHubSettings'

const KINDS = [
  { id: 'eye', label: 'settings.rest.eye', hint: 'settings.rest.eye_hint' },
  { id: 'water', label: 'settings.rest.water', hint: 'settings.rest.water_hint' },
  { id: 'move', label: 'settings.rest.move', hint: 'settings.rest.move_hint' },
] as const satisfies readonly { id: string; label: CopyKey; hint: CopyKey }[]

const { error, notice, ready, bool, num, text, range, options, save, restore } = useHubSettings([
  ...KINDS.flatMap((k) => [`rest.${k.id}.enabled`, `rest.${k.id}.interval`]),
  'rest.night.enabled',
  'rest.night.start',
  'rest.count_when_paused',
])
const intervalError = reactive<Record<string, string | null>>({})

async function saveInterval(key: string, input: HTMLInputElement): Promise<void> {
  const r = range(key)
  const n = Math.round(Number(input.value))
  if (!r || input.value.trim() === '' || !Number.isFinite(n) || n < r.min || n > r.max) {
    if (r) intervalError[key] = t('settings.rest.interval_invalid', { min: r.min, max: r.max })
    input.value = String(num(key) ?? '')
    return
  }
  intervalError[key] = null
  await save(key, n)
}
</script>

<template>
  <section>
    <SectionHead :title="t('settings.rest_nav')" @restore="restore" />
    <p class="s-hint intro">{{ t('settings.rest.intro') }}</p>
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>

    <fieldset v-for="k in KINDS" :key="k.id" class="s-group" :data-group="k.id">
      <legend>{{ t(k.label) }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="bool(`rest.${k.id}.enabled`)"
          :disabled="!ready"
          @change="save(`rest.${k.id}.enabled`, ($event.target as HTMLInputElement).checked)"
        />
        {{ bool(`rest.${k.id}.enabled`) ? t('settings.on') : t('settings.off') }}
      </label>
      <div v-if="range(`rest.${k.id}.interval`)" class="s-row">
        <label :for="`iv-${k.id}`">{{ t('settings.rest.interval') }}</label>
        <input
          :id="`iv-${k.id}`"
          type="number"
          step="5"
          :min="range(`rest.${k.id}.interval`)!.min"
          :max="range(`rest.${k.id}.interval`)!.max"
          :value="num(`rest.${k.id}.interval`)"
          :disabled="!bool(`rest.${k.id}.enabled`)"
          @change="saveInterval(`rest.${k.id}.interval`, $event.target as HTMLInputElement)"
        />
        <span class="s-hint">{{ t('settings.rest.interval_hint', range(`rest.${k.id}.interval`)!) }}</span>
      </div>
      <p class="s-hint">{{ t(k.hint) }}</p>
      <p v-if="intervalError[`rest.${k.id}.interval`]" class="s-error" role="alert">
        {{ intervalError[`rest.${k.id}.interval`] }}
      </p>
    </fieldset>

    <fieldset class="s-group" data-group="night">
      <legend>{{ t('settings.rest.night') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="bool('rest.night.enabled')"
          :disabled="!ready"
          @change="save('rest.night.enabled', ($event.target as HTMLInputElement).checked)"
        />
        {{ bool('rest.night.enabled') ? t('settings.on') : t('settings.off') }}
      </label>
      <div class="s-row">
        <label for="night-start">{{ t('settings.rest.night_start') }}</label>
        <select
          id="night-start"
          :value="text('rest.night.start')"
          :disabled="!bool('rest.night.enabled')"
          @change="save('rest.night.start', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="o in options('rest.night.start')" :key="o" :value="o">{{ o }}</option>
        </select>
      </div>
      <p class="s-hint">{{ t('settings.rest.night_hint') }}</p>
    </fieldset>

    <fieldset class="s-group" data-group="paused">
      <legend>{{ t('settings.rest.count_when_paused') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="bool('rest.count_when_paused')"
          :disabled="!ready"
          @change="save('rest.count_when_paused', ($event.target as HTMLInputElement).checked)"
        />
        {{ bool('rest.count_when_paused') ? t('settings.on') : t('settings.off') }}
      </label>
      <p class="s-hint">{{ t('settings.rest.count_when_paused_hint') }}</p>
    </fieldset>

    <p class="s-hint">{{ t('settings.rest.focus_pending') }}</p>
    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.intro {
  max-width: 560px;
  margin-bottom: var(--xq-sp-4);
}
</style>
