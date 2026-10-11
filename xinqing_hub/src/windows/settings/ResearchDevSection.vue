<script setup lang="ts">
// 设置中心 · 隐私与关于里的“参加研究”（FR-DMO-04，ADR 0025）与“开发者选项”（演示模式 FR-DMO-03，ADR 0023）。
// 研究模式先填研究编号再开；研究结束后可删掉研究期间问来的心情记录（只删 `source = esm` 的，先确认）。
// 演示模式改了要重启 Hub 才生效，当前是否演示模式以 demo_status 为准。
import { onMounted, ref } from 'vue'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { RESEARCH_ID_MAX_CHARS, validResearchId } from './care'
import { useHubSettings } from './useHubSettings'

const { error, notice, ready, flash, bool, text, save } = useHubSettings([
  'research.id',
  'research.enabled',
  'dev.demo',
])
const idError = ref<string | null>(null)
const clearAsk = ref(false)
const clearError = ref<string | null>(null)
const demoOn = ref(false)

onMounted(async () => {
  try {
    demoOn.value = (await unwrap(commands.demoStatus())).enabled
  } catch {
    // 读不到就不显示“现在是演示模式”
  }
})

async function saveId(raw: string): Promise<void> {
  const id = raw.trim()
  if (!validResearchId(id)) return void (idError.value = t('privacy.research.id_invalid'))
  idError.value = null
  if (id === text('research.id')) return
  // 清空编号时一并关掉研究模式：编号为空时开关本来就不起作用，别让界面显示“开着”
  if (await save('research.id', id)) {
    if (id === '' && bool('research.enabled')) await save('research.enabled', false)
  }
}

async function clearEsm(): Promise<void> {
  clearAsk.value = false
  clearError.value = null
  try {
    const n = await unwrap(commands.researchClear())
    flash(t('privacy.research.cleared', { n }))
  } catch (e) {
    clearError.value = errorText(e)
  }
}
</script>

<template>
  <h3>{{ t('privacy.research.title') }}</h3>
  <p class="muted small">{{ t('privacy.research.hint') }}</p>
  <p v-if="error" class="s-error" role="alert">{{ error }}</p>
  <div class="s-group" data-group="research">
    <div class="s-row">
      <label for="research-id">{{ t('privacy.research.id') }}</label>
      <input
        id="research-id"
        type="text"
        spellcheck="false"
        autocomplete="off"
        :maxlength="RESEARCH_ID_MAX_CHARS"
        :placeholder="t('privacy.research.id_placeholder')"
        :value="text('research.id')"
        :disabled="!ready"
        @change="saveId(($event.target as HTMLInputElement).value)"
      />
    </div>
    <p v-if="idError" class="s-error" role="alert">{{ idError }}</p>
    <label class="s-choice">
      <input
        type="checkbox"
        role="switch"
        :checked="bool('research.enabled')"
        :disabled="!ready || text('research.id') === ''"
        @change="save('research.enabled', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('privacy.research.enabled') }}
    </label>
    <p v-if="text('research.id') === ''" class="s-hint">{{ t('privacy.research.enabled_needs_id') }}</p>
    <div v-if="clearAsk" class="s-confirm" role="group" :aria-label="t('privacy.research.clear')">
      <p>{{ t('privacy.research.clear_confirm') }}</p>
      <div class="s-row">
        <button type="button" class="s-compact s-danger" @click="clearEsm">
          {{ t('privacy.research.clear_yes') }}
        </button>
        <button type="button" class="s-compact" @click="clearAsk = false">
          {{ t('privacy.research.clear_no') }}
        </button>
      </div>
    </div>
    <div v-else>
      <button type="button" class="s-compact" @click="clearAsk = true">
        {{ t('privacy.research.clear') }}
      </button>
    </div>
    <p class="s-hint">{{ t('privacy.research.clear_hint') }}</p>
    <p v-if="clearError" class="s-error" role="alert">{{ clearError }}</p>
  </div>

  <h3>{{ t('privacy.dev.title') }}</h3>
  <div class="s-group" data-group="dev">
    <label class="s-choice">
      <input
        type="checkbox"
        role="switch"
        :checked="bool('dev.demo')"
        :disabled="!ready"
        @change="save('dev.demo', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('privacy.dev.demo') }}
    </label>
    <p class="s-hint">{{ t('privacy.dev.demo_hint') }}</p>
    <p v-if="demoOn" class="s-note" role="status">{{ t('privacy.dev.demo_on') }}</p>
  </div>

  <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
</template>

<style scoped>
/* 与“隐私”的小标题同级 */
h3 {
  margin: var(--xq-sp-5) 0 var(--xq-sp-2);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-md);
  font-weight: 500;
}

.small {
  margin: 0 0 var(--xq-sp-2);
  font-size: var(--xq-fs-sm);
}

.muted {
  color: var(--xq-text-2);
}
</style>
