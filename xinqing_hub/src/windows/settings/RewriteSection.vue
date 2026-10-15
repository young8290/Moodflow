<script setup lang="ts">
// 设置中心 · 温柔改写（07 FR-SET-10）：功能开关（同意 ⑥）、改写快捷键（输入法配置 xinqing.rewrite_hotkey）、今日改写次数。
// 默认风格与文本来源开关还没有登记设置键（核心按 Tab 轮换风格、先取最近上屏再取剪贴板），这里先照实说明。
import { onMounted, ref } from 'vue'
import { commands, unwrap, type UsageRow } from '@/api'
import { t } from '@/i18n'
import { useSettingsStore } from '@/stores/settings'
import HotkeyRecorder from './HotkeyRecorder.vue'
import SectionHead from './SectionHead.vue'
import { HUB_HOTKEYS, hotkeyOwner } from './hotkey'
import { useConsents } from './useConsents'
import { useNotice } from './useHubSettings'
import { useImeKeys } from './useImeKeys'

const HOTKEY = 'xinqing.rewrite_hotkey'

const settings = useSettingsStore()
const ime = useImeKeys([HOTKEY])
const { available: imeReady, error: imeError, get: imeGet } = ime
const consents = useConsents()
const { granted, withdrawn, error: consentError } = consents
const usage = ref<UsageRow | null>(null)

const { notice, flash } = useNotice()

onMounted(async () => {
  try {
    await settings.init(HUB_HOTKEYS.map((h) => h.key))
    usage.value = (await unwrap(commands.aiUsageToday())).find((u) => u.kind === 'rewrite') ?? null
  } catch {
    // 读不到用量就不显示那一行
  }
})

async function setConsent(on: boolean): Promise<void> {
  if (await consents.set('rewrite', on)) flash(t('settings.saved'))
}

async function saveHotkey(v: string): Promise<void> {
  if (await ime.set(HOTKEY, v)) flash(t('settings.saved'))
}

async function restore(): Promise<void> {
  if (await ime.restore()) flash(t('settings.restored'))
}

const takenBy = (combo: string) => hotkeyOwner(HOTKEY, combo, imeGet, (k) => settings.values[k])
</script>

<template>
  <section>
    <SectionHead :title="t('settings.rewrite_nav')" :restorable="imeReady === true" @restore="restore" />
    <p v-if="consentError" class="s-error" role="alert">{{ consentError }}</p>

    <fieldset class="s-group" data-group="consent">
      <legend>{{ t('settings.rewrite.consent') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="granted('rewrite') === true"
          :disabled="granted('rewrite') === null"
          @change="setConsent(($event.target as HTMLInputElement).checked)"
        />
        {{ t('consent.rewrite') }}
      </label>
      <p class="s-hint">{{ t('settings.rewrite.consent_hint') }}</p>
      <p v-if="withdrawn.has('rewrite')" class="s-note" role="status">{{ t('error.withdraw_notice') }}</p>
      <p v-if="usage" class="usage">
        {{ t('settings.rewrite.usage', { used: usage.used, cap: usage.cap }) }}
      </p>
    </fieldset>

    <fieldset class="s-group" data-group="hotkey" :disabled="imeReady !== true">
      <legend>{{ t('settings.rewrite.hotkey') }}</legend>
      <HotkeyRecorder
        id="hk-rewrite"
        :name="t('settings.rewrite.hotkey')"
        :value="imeGet(HOTKEY)"
        :taken-by="takenBy"
        @save="saveHotkey"
      />
      <p v-if="imeReady === false" class="s-note">{{ t('settings.sense.ime_needed') }}</p>
      <p v-if="imeError" class="s-error" role="alert">{{ imeError }}</p>
      <p class="s-hint">{{ t('settings.rewrite.style_hint') }}</p>
      <p class="s-hint">{{ t('settings.rewrite.sources_hint') }}</p>
    </fieldset>

    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.usage {
  margin: 0;
}
</style>
