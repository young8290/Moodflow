<script setup lang="ts">
// 设置中心 · 感知（07 FR-SET-04）：云端判断（同意 ②，FR-STA-08）、增强模式（同意 ⑤，开启前单独说明）、
// 不感知 / 只感知的程序与记住无痕状态、无痕模式快捷键（输入法配置 moodflow.*，ADR 0016）、个人基线进度与“重新熟悉”（FR-STA-03）。
// “从正在运行的程序中选择”需要外壳列出进程的命令，还没有，先手填程序名。
import { onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { commands, events, unwrap, type ConsentItem } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
import { useSettingsStore } from '@/stores/settings'
import HotkeyRecorder from './HotkeyRecorder.vue'
import SectionHead from './SectionHead.vue'
import { APP_MAX_CHARS, hasApp, validApp } from './care'
import { useRunningApps } from './useRunningApps'
import { HUB_HOTKEYS, hotkeyOwner } from './hotkey'
import { useConsents } from './useConsents'
import { useNotice } from './useHubSettings'
import { useImeKeys } from './useImeKeys'

const LISTS = [
  { key: 'moodflow.app_blocklist', label: 'settings.sense.blocklist', hint: 'settings.sense.blocklist_hint' },
  { key: 'moodflow.app_allowlist', label: 'settings.sense.allowlist', hint: 'settings.sense.allowlist_hint' },
] as const satisfies readonly { key: string; label: CopyKey; hint: CopyKey }[]
const PAUSE_HOTKEY = 'moodflow.pause_hotkey'

const settings = useSettingsStore()
const ime = useImeKeys([...LISTS.map((l) => l.key), 'moodflow.remember_pause', PAUSE_HOTKEY])
const { available: imeReady, error: imeError, get: imeGet } = ime
const consents = useConsents()
const { granted, withdrawn, error: consentError } = consents

const { notice, flash } = useNotice()

// ── 增强模式：开启前先把“会发送上屏文字”说清楚，用户点“我明白”才记同意 ──
const askEnhanced = ref(false)

async function setConsent(item: ConsentItem, on: boolean): Promise<void> {
  if (item === 'enhanced' && on) {
    askEnhanced.value = true
    return
  }
  if (await consents.set(item, on)) flash(t('settings.saved'))
}

async function confirmEnhanced(): Promise<void> {
  askEnhanced.value = false
  if (await consents.set('enhanced', true)) flash(t('settings.saved'))
}

// ── 程序名单 ──
const running = useRunningApps()
const drafts = reactive<Record<string, string>>({})
const listError = reactive<Record<string, string | null>>({})
const listOf = (key: string): string[] => {
  const v = imeGet(key)
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []
}

async function addApp(key: string): Promise<void> {
  const a = (drafts[key] ?? '').trim()
  if (!validApp(a)) return void (listError[key] = t('settings.sense.app_invalid'))
  if (hasApp(listOf(key), a)) return void (listError[key] = t('settings.sense.app_duplicate'))
  listError[key] = null
  if (await saveIme(key, [...listOf(key), a])) drafts[key] = ''
}

async function saveIme(key: string, value: unknown): Promise<boolean> {
  const ok = await ime.set(key, value)
  if (ok) flash(t('settings.saved'))
  return ok
}

const takenBy = (combo: string) => hotkeyOwner(PAUSE_HOTKEY, combo, imeGet, (k) => settings.values[k])

// ── 个人基线 ──
const progress = ref<number | null>(null)
const resetAsk = ref(false)
const baselineError = ref<string | null>(null)
let unlisten: (() => void) | undefined
let disposed = false

onMounted(async () => {
  try {
    progress.value = (await unwrap(commands.getStatus())).baseline_progress
    const off = await events.statusChanged.listen((e) => (progress.value = e.payload.baseline_progress))
    if (disposed) off()
    else unlisten = off
    await settings.init(HUB_HOTKEYS.map((h) => h.key))
  } catch (e) {
    baselineError.value = errorText(e)
  }
})
onBeforeUnmount(() => {
  disposed = true
  unlisten?.()
})

async function resetBaseline(): Promise<void> {
  resetAsk.value = false
  baselineError.value = null
  try {
    await unwrap(commands.baselineReset())
    progress.value = 0
    flash(t('settings.sense.baseline_reset_done'))
  } catch (e) {
    baselineError.value = errorText(e)
  }
}

async function restore(): Promise<void> {
  if (await ime.restore()) flash(t('settings.restored'))
}
</script>

<template>
  <section>
    <SectionHead :title="t('settings.sense_nav')" :restorable="imeReady === true" @restore="restore" />
    <p v-if="consentError" class="s-error" role="alert">{{ consentError }}</p>

    <fieldset class="s-group" data-group="cloud">
      <legend>{{ t('settings.sense.cloud') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="granted('jev_features') === true"
          :disabled="granted('jev_features') === null"
          @change="setConsent('jev_features', ($event.target as HTMLInputElement).checked)"
        />
        {{ t('consent.jev_features') }}
      </label>
      <p class="s-hint">{{ t('settings.sense.cloud_hint') }}</p>
      <p v-if="withdrawn.has('jev_features')" class="s-note" role="status">
        {{ t('error.withdraw_notice') }}
      </p>
    </fieldset>

    <fieldset class="s-group" data-group="enhanced">
      <legend>{{ t('settings.sense.enhanced') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="granted('enhanced') === true || askEnhanced"
          :disabled="granted('enhanced') === null"
          @change="setConsent('enhanced', ($event.target as HTMLInputElement).checked)"
        />
        {{ t('consent.enhanced') }}
      </label>
      <div v-if="askEnhanced" class="s-confirm" role="group" :aria-label="t('settings.sense.enhanced')">
        <p>{{ t('settings.sense.enhanced_warning') }}</p>
        <div class="s-row">
          <button type="button" class="s-compact" @click="confirmEnhanced">
            {{ t('settings.sense.enhanced_on') }}
          </button>
          <button type="button" class="s-compact" @click="askEnhanced = false">
            {{ t('settings.restore_no') }}
          </button>
        </div>
      </div>
      <p v-if="withdrawn.has('enhanced')" class="s-note" role="status">{{ t('error.withdraw_notice') }}</p>
    </fieldset>

    <p v-if="imeReady === false" class="s-note ime-needed">{{ t('settings.sense.ime_needed') }}</p>
    <p v-if="imeError" class="s-error" role="alert">{{ imeError }}</p>

    <fieldset
      v-for="l in LISTS"
      :key="l.key"
      class="s-group"
      :data-group="l.key"
      :disabled="imeReady !== true"
    >
      <legend>{{ t(l.label) }}</legend>
      <p class="s-hint">{{ t(l.hint) }}</p>
      <ul v-if="listOf(l.key).length" class="s-items">
        <li v-for="a in listOf(l.key)" :key="a" class="s-item">
          <span class="s-name">{{ a }}</span>
          <button
            type="button"
            class="s-icon"
            :aria-label="t('settings.sense.remove', { app: a })"
            @click="
              saveIme(
                l.key,
                listOf(l.key).filter((x) => x !== a),
              )
            "
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        </li>
      </ul>
      <form class="s-row" @submit.prevent="addApp(l.key)">
        <input
          v-model="drafts[l.key]"
          type="text"
          class="app-input"
          spellcheck="false"
          autocomplete="off"
          :maxlength="APP_MAX_CHARS"
          list="mf-running-apps"
          :aria-label="`${t(l.label)} ${t('settings.sense.app_placeholder')}`"
          :placeholder="t('settings.sense.app_placeholder')"
          @focus="running.load()"
        />
        <button type="submit" class="s-compact">{{ t('settings.sense.add') }}</button>
      </form>
      <p v-if="listError[l.key]" class="s-error" role="alert">{{ listError[l.key] }}</p>
    </fieldset>

    <datalist id="mf-running-apps">
      <option v-for="a in running.apps.value" :key="a" :value="a" />
    </datalist>

    <fieldset class="s-group" data-group="pause" :disabled="imeReady !== true">
      <legend>{{ t('settings.sense.pause_group') }}</legend>
      <label class="s-choice">
        <input
          type="checkbox"
          role="switch"
          :checked="imeGet('moodflow.remember_pause') === true"
          @change="saveIme('moodflow.remember_pause', ($event.target as HTMLInputElement).checked)"
        />
        {{ t('settings.sense.remember_pause') }}
      </label>
      <p class="s-hint">{{ t('settings.sense.remember_pause_hint') }}</p>
      <div class="s-row">
        <label for="hk-pause">{{ t('settings.sense.pause_hotkey') }}</label>
        <HotkeyRecorder
          id="hk-pause"
          :name="t('settings.sense.pause_hotkey')"
          :value="imeGet(PAUSE_HOTKEY)"
          :taken-by="takenBy"
          @save="(v) => saveIme(PAUSE_HOTKEY, v)"
        />
      </div>
    </fieldset>

    <fieldset class="s-group" data-group="baseline">
      <legend>{{ t('settings.sense.baseline') }}</legend>
      <template v-if="progress !== null">
        <p class="baseline">
          {{
            progress >= 100
              ? t('settings.sense.baseline_done')
              : t('settings.sense.baseline_progress', { pct: progress })
          }}
        </p>
        <progress max="100" :value="progress" :aria-label="t('settings.sense.baseline')" />
      </template>
      <p class="s-hint">{{ t('settings.sense.baseline_hint') }}</p>
      <div v-if="resetAsk" class="s-confirm" role="group" :aria-label="t('settings.sense.baseline_reset')">
        <p>{{ t('settings.sense.baseline_confirm') }}</p>
        <div class="s-row">
          <button type="button" class="s-compact s-danger" @click="resetBaseline">
            {{ t('settings.sense.baseline_yes') }}
          </button>
          <button type="button" class="s-compact" @click="resetAsk = false">
            {{ t('settings.sense.baseline_no') }}
          </button>
        </div>
      </div>
      <div v-else>
        <button type="button" class="s-compact" @click="resetAsk = true">
          {{ t('settings.sense.baseline_reset') }}
        </button>
      </div>
      <p v-if="baselineError" class="s-error" role="alert">{{ baselineError }}</p>
    </fieldset>

    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.ime-needed {
  max-width: 560px;
  margin-bottom: var(--mf-sp-4);
}

.app-input {
  flex: 1;
}

.baseline {
  margin: 0;
}

/* 进度条：浅底、主色填充，圆头（与 AI 服务的用量条一致） */
progress {
  width: 100%;
  max-width: 320px;
  height: 8px;
  overflow: hidden;
  border: 0;
  border-radius: 4px;
  background: var(--mf-surface-2);
  appearance: none;
}

progress::-webkit-progress-bar {
  border-radius: 4px;
  background: var(--mf-surface-2);
}

progress::-webkit-progress-value {
  border-radius: 4px;
  background: var(--mf-primary);
}

progress::-moz-progress-bar {
  border-radius: 4px;
  background: var(--mf-primary);
}
</style>
