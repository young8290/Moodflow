<script setup lang="ts">
// 设置中心 · 温柔改写（07 FR-SET-10）：功能开关（同意 ⑥）、改写快捷键、起始风格与剪贴板来源（都是输入法配置
// xinqing.*，核心开始改写时读，ADR 0041）、今日改写次数。
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
const STYLE = 'xinqing.rewrite_style'
const CLIPBOARD = 'xinqing.rewrite_clipboard'
const STYLES = ['gentle', 'polite', 'concise', 'structured'] as const

const settings = useSettingsStore()
const ime = useImeKeys([HOTKEY, STYLE, CLIPBOARD])
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

async function saveIme(key: string, v: unknown): Promise<void> {
  if (await ime.set(key, v)) flash(t('settings.saved'))
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
      <!-- FR-RWR-06：开着时告知（ADR 0041） -->
      <p v-if="granted('rewrite') === true" class="s-note rewrite-notice" role="note">
        {{ t('rewrite.first_use_notice') }}
      </p>
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
    </fieldset>

    <fieldset class="s-group" data-group="style">
      <legend>{{ t('settings.rewrite.style') }}</legend>
      <label v-for="s in STYLES" :key="s" class="s-choice">
        <input
          type="radio"
          name="rewrite-style"
          :value="s"
          :checked="(imeGet(STYLE) ?? 'gentle') === s"
          :disabled="imeReady !== true"
          @change="saveIme(STYLE, s)"
        />
        {{ t(`settings.rewrite.style_${s}`) }}
      </label>
      <p class="s-hint">{{ t('settings.rewrite.style_hint') }}</p>
    </fieldset>

    <fieldset class="s-group" data-group="sources">
      <legend>{{ t('settings.rewrite.sources') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="imeGet(CLIPBOARD) !== false"
          :disabled="imeReady !== true"
          @change="saveIme(CLIPBOARD, ($event.target as HTMLInputElement).checked)"
        />
        {{ t('settings.rewrite.clipboard') }}
      </label>
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
