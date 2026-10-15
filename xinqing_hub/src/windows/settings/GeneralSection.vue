<script setup lang="ts">
// 设置中心 · 常规（07 FR-SET-03）：心晴功能总开关（输入法配置 xinqing.enabled）、小组件显示 / 置顶 / 贴边收起 / 不透明度
// （FR-WGT-01）、颜色模式（DS-COLOR-03）、全局快捷键（FR-ENT-04，ADR 0036）。
// “晴天收集”是 P2（FR-REV-04），还没有设置键，暂不显示。
import { computed, onMounted, reactive, ref } from 'vue'
import { commands, unwrap, type HotkeyAction, type HotkeyStatus } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
import HotkeyRecorder from './HotkeyRecorder.vue'
import SectionHead from './SectionHead.vue'
import { HUB_HOTKEYS, UNSET, hotkeyOwner, hubHotkeyOk } from './hotkey'
import { useHubSettings } from './useHubSettings'
import { useImeKeys } from './useImeKeys'

const WIDGET = [
  { key: 'widget.visible', label: 'settings.general.widget_visible', hint: null },
  { key: 'widget.topmost', label: 'settings.general.widget_topmost', hint: null },
  {
    key: 'widget.autohide',
    label: 'settings.general.widget_autohide',
    hint: 'settings.general.widget_autohide_hint',
  },
] as const satisfies readonly { key: string; label: CopyKey; hint: CopyKey | null }[]
const THEME_LABEL: Record<string, CopyKey> = {
  system: 'settings.theme_system',
  light: 'settings.theme_light',
  dark: 'settings.theme_dark',
}
const HOTKEYS = HUB_HOTKEYS

const { error, notice, ready, flash, bool, num, text, range, options, save, restore } = useHubSettings([
  ...WIDGET.map((w) => w.key),
  'widget.opacity',
  'ui.theme',
  ...HOTKEYS.map((h) => h.key),
])
// 总开关不随“恢复默认”改：关着时恢复默认不该悄悄重新打开感知
const { available: imeReady, error: imeError, get: imeGet, set: imeSet } = useImeKeys([])

const status = ref<HotkeyStatus[]>([])
const hotkeyError = reactive<Record<string, string | null>>({})

/** 不透明度按百分比显示；范围取自 schema，读到前不出滑块 */
const opacityRange = computed(() => {
  const r = range('widget.opacity')
  return r ? { min: Math.round(r.min * 100), max: Math.round(r.max * 100) } : null
})
const opacity = computed(() => Math.round((num('widget.opacity') ?? 1) * 100))

async function loadStatus(): Promise<void> {
  try {
    status.value = await commands.hotkeyStatus()
  } catch {
    // 读不到注册结果就不提示冲突，快捷键照样能改
  }
}
onMounted(loadStatus)

/** 登录时后台启动（FR-OPS-03）：以系统启动项为准，读不到时不显示开关 */
const loginStart = ref<boolean | null>(null)
onMounted(async () => {
  try {
    loginStart.value = await unwrap(commands.autostartGet())
  } catch {
    loginStart.value = null
  }
})

async function setLoginStart(on: boolean): Promise<void> {
  error.value = null
  try {
    loginStart.value = await unwrap(commands.autostartSet(on))
    flash(t('settings.saved'))
  } catch (e) {
    error.value = errorText(e)
  }
}

const conflict = (action: HotkeyAction) => status.value.some((s) => s.action === action && s.conflict)

const takenBy = (self: string, combo: string) => hotkeyOwner(self, combo, imeGet, text)

async function saveHotkey(key: string, recorded: string): Promise<void> {
  // 录制框清除时给的是输入法的写法 `none`，Hub 的设置键用空串表示不设
  const value = recorded === UNSET ? '' : recorded
  if (value !== '' && !hubHotkeyOk(value)) {
    hotkeyError[key] = t('settings.general.hotkey_invalid')
    return
  }
  hotkeyError[key] = null
  if (await save(key, value)) await loadStatus()
}

async function setEnabled(on: boolean): Promise<void> {
  if (await imeSet('xinqing.enabled', on)) flash(t('settings.saved'))
}

async function restoreAll(): Promise<void> {
  for (const k of Object.keys(hotkeyError)) hotkeyError[k] = null
  await restore()
  await loadStatus()
  // 出厂是关着的
  if (loginStart.value) await setLoginStart(false)
}
</script>

<template>
  <section>
    <SectionHead :title="t('settings.general_nav')" @restore="restoreAll" />
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>

    <fieldset class="s-group" data-group="enabled">
      <legend>{{ t('settings.general.enabled') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="imeGet('xinqing.enabled') !== false"
          :disabled="imeReady !== true"
          @change="setEnabled(($event.target as HTMLInputElement).checked)"
        />
        {{ imeGet('xinqing.enabled') === false ? t('settings.off') : t('settings.on') }}
      </label>
      <p class="s-hint">{{ t('settings.general.enabled_hint') }}</p>
      <p v-if="imeReady === false" class="s-note">{{ t('settings.sense.ime_needed') }}</p>
      <p v-if="imeError" class="s-error" role="alert">{{ imeError }}</p>
    </fieldset>

    <fieldset v-if="loginStart !== null" class="s-group" data-group="login-start">
      <legend>{{ t('settings.general.login_start') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="loginStart"
          @change="setLoginStart(($event.target as HTMLInputElement).checked)"
        />
        {{ loginStart ? t('settings.on') : t('settings.off') }}
      </label>
      <p class="s-hint">{{ t('settings.general.login_start_hint') }}</p>
    </fieldset>

    <fieldset class="s-group" data-group="widget">
      <legend>{{ t('settings.general.widget') }}</legend>
      <template v-for="w in WIDGET" :key="w.key">
        <label class="s-choice">
          <input
            type="checkbox"
            role="switch"
            :checked="bool(w.key)"
            :disabled="!ready"
            @change="save(w.key, ($event.target as HTMLInputElement).checked)"
          />
          {{ t(w.label) }}
        </label>
        <p v-if="w.hint" class="s-hint">{{ t(w.hint) }}</p>
      </template>
      <label v-if="opacityRange" class="s-row">
        <span>{{ t('settings.widget_opacity') }}</span>
        <input
          type="range"
          :min="opacityRange.min"
          :max="opacityRange.max"
          step="5"
          :value="opacity"
          @change="save('widget.opacity', Number(($event.target as HTMLInputElement).value) / 100)"
        />
        <output>{{ t('settings.percent', { n: opacity }) }}</output>
      </label>
    </fieldset>

    <fieldset class="s-group" data-group="theme">
      <legend>{{ t('settings.theme') }}</legend>
      <div class="s-choices">
        <label v-for="o in options('ui.theme')" :key="o" class="s-choice">
          <input type="radio" name="theme" :checked="text('ui.theme') === o" @change="save('ui.theme', o)" />
          {{ THEME_LABEL[o] ? t(THEME_LABEL[o]) : o }}
        </label>
      </div>
    </fieldset>

    <fieldset class="s-group" data-group="hotkeys">
      <legend>{{ t('settings.general.hotkeys') }}</legend>
      <p class="s-hint">{{ t('settings.general.hotkeys_hint') }}</p>
      <div v-for="h in HOTKEYS" :key="h.key" class="hotkey-row" :data-key="h.key">
        <label :for="`hk-${h.action}`">{{ t(h.label) }}</label>
        <HotkeyRecorder
          :id="`hk-${h.action}`"
          :name="t(h.label)"
          :value="text(h.key)"
          :taken-by="(combo) => takenBy(h.key, combo)"
          @save="(v) => saveHotkey(h.key, v)"
        />
        <p v-if="hotkeyError[h.key]" class="s-error" role="alert">{{ hotkeyError[h.key] }}</p>
        <p v-else-if="conflict(h.action)" class="s-error" role="alert">{{ t('error.hotkey_conflict') }}</p>
      </div>
    </fieldset>

    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.hotkey-row {
  display: grid;
  grid-template-columns: 140px 1fr;
  gap: var(--xq-sp-1) var(--xq-sp-3);
  align-items: center;
}

.hotkey-row .s-error {
  grid-column: 2;
}
</style>
