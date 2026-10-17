<script setup lang="ts">
// 设置中心 · 日程与待办（07 FR-SET-07）：识别开关（同意 ③）、有时刻的日程默认提前多久提醒（FR-SCH-07，ADR 0032）、
// 待办每日汇总（FR-SCH-14，默认关）、导出方式（看板“日程与待办”里生成 .ics，FR-SCH-10）。
import { isCopyKey, t } from '@/i18n'
import { openDashboard } from '../shared/bus'
import SectionHead from './SectionHead.vue'
import { useConsents } from './useConsents'
import { useHubSettings } from './useHubSettings'

const { error, notice, ready, flash, bool, text, options, save, restore } = useHubSettings([
  'sch.default_offsets',
  'todo.daily_digest',
])
const consents = useConsents()
const { granted, withdrawn, error: consentError } = consents

function offsetLabel(v: string): string {
  const k = `settings.agenda.offset_${v}`
  return isCopyKey(k) ? t(k) : v
}

async function setRecognize(on: boolean): Promise<void> {
  if (await consents.set('schedule', on)) flash(t('settings.saved'))
}

async function openAgenda(): Promise<void> {
  try {
    await openDashboard({ page: 'schedule' })
  } catch {
    // 打不开看板时什么也不做：这只是个快捷入口
  }
}
</script>

<template>
  <section>
    <SectionHead :title="t('settings.agenda_nav')" @restore="restore" />
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>
    <p v-if="consentError" class="s-error" role="alert">{{ consentError }}</p>

    <fieldset class="s-group" data-group="recognize">
      <legend>{{ t('settings.agenda.recognize') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="granted('schedule') === true"
          :disabled="granted('schedule') === null"
          @change="setRecognize(($event.target as HTMLInputElement).checked)"
        />
        {{ t('consent.schedule') }}
      </label>
      <p class="s-hint">{{ t('settings.agenda.recognize_hint') }}</p>
      <p v-if="withdrawn.has('schedule')" class="s-note" role="status">{{ t('error.withdraw_notice') }}</p>
    </fieldset>

    <fieldset class="s-group" data-group="offset">
      <legend>{{ t('settings.agenda.offset') }}</legend>
      <select
        :aria-label="t('settings.agenda.offset')"
        :value="text('sch.default_offsets')"
        :disabled="!ready"
        @change="save('sch.default_offsets', ($event.target as HTMLSelectElement).value)"
      >
        <option v-for="o in options('sch.default_offsets')" :key="o" :value="o">{{ offsetLabel(o) }}</option>
      </select>
      <p class="s-hint">{{ t('settings.agenda.offset_hint') }}</p>
    </fieldset>

    <fieldset class="s-group" data-group="digest">
      <legend>{{ t('settings.agenda.digest') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="bool('todo.daily_digest')"
          :disabled="!ready"
          @change="save('todo.daily_digest', ($event.target as HTMLInputElement).checked)"
        />
        {{ bool('todo.daily_digest') ? t('settings.on') : t('settings.off') }}
      </label>
    </fieldset>

    <fieldset class="s-group" data-group="export">
      <legend>{{ t('settings.agenda.export') }}</legend>
      <p class="s-hint">{{ t('settings.agenda.export_hint') }}</p>
      <div>
        <button type="button" class="s-compact" @click="openAgenda">{{ t('settings.agenda.open') }}</button>
      </div>
    </fieldset>

    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>
