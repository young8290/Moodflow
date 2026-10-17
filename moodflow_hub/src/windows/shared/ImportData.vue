<script setup lang="ts">
// 从备份导入（09 FR-DAT-07，ADR 0037）：选“导出我的数据”生成的 zip，合并或覆盖、是否导入设置，导入后请用户重启 Hub。
// 设置页“隐私”和首次引导最后一页（“从备份恢复”）共用。
import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type ImportMode, type ImportReport } from '@/api'
import { errorText, t } from '@/i18n'

const importPath = ref<string | null>(null)
const importMode = ref<ImportMode>('merge')
const importSettings = ref(false)
const importing = ref(false)
const importReport = ref<ImportReport | null>(null)
const importError = ref<string | null>(null)

async function pickImport(): Promise<void> {
  importError.value = null
  importReport.value = null
  try {
    const p = await open({
      multiple: false,
      directory: false,
      filters: [{ name: t('privacy.import_filter'), extensions: ['zip'] }],
    })
    if (typeof p === 'string') importPath.value = p
  } catch (e) {
    importError.value = errorText(e)
  }
}

async function runImport(): Promise<void> {
  if (!importPath.value) return
  importError.value = null
  importing.value = true
  try {
    importReport.value = await unwrap(
      commands.dataImport(importPath.value, importMode.value, importSettings.value),
    )
  } catch (e) {
    importError.value = errorText(e)
  } finally {
    importing.value = false
  }
}

async function restart(): Promise<void> {
  try {
    await commands.appRestart()
  } catch (e) {
    importError.value = errorText(e)
  }
}
</script>

<template>
  <div class="import" data-group="import">
    <div class="row">
      <button :disabled="importing" @click="pickImport">{{ t('privacy.import_pick') }}</button>
      <span v-if="importPath" class="path">{{ importPath }}</span>
    </div>
    <template v-if="importPath && !importReport">
      <label class="check">
        <input v-model="importMode" type="radio" name="import-mode" value="merge" :disabled="importing" />
        <span>{{ t('privacy.import_merge') }}</span>
      </label>
      <label class="check">
        <input v-model="importMode" type="radio" name="import-mode" value="overwrite" :disabled="importing" />
        <span>{{ t('privacy.import_overwrite') }}</span>
      </label>
      <label class="check">
        <input v-model="importSettings" type="checkbox" :disabled="importing" />
        <span>{{ t('privacy.import_settings') }}</span>
      </label>
      <div class="row">
        <button class="primary" :disabled="importing" @click="runImport">
          {{ t('privacy.import_btn') }}
        </button>
        <span v-if="importing" role="status">{{ t('privacy.importing') }}</span>
      </div>
    </template>
    <div v-if="importReport" class="row">
      <span role="status">{{
        t('privacy.import_done', { added: importReport.added, skipped: importReport.skipped })
      }}</span>
      <button @click="restart">{{ t('privacy.import_restart') }}</button>
    </div>
    <p v-if="importError" class="error" role="alert">{{ importError }}</p>
  </div>
</template>

<style scoped>
.import {
  display: grid;
  gap: var(--mf-sp-2);
  max-width: 560px;
}

.row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-3);
  align-items: center;
}

/* 整行都能点，点击目标不小于 32 px 高（DS-A11Y-03） */
.check {
  display: flex;
  gap: var(--mf-sp-2);
  align-items: flex-start;
  min-height: 32px;
  cursor: pointer;
}

.path {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
  overflow-wrap: anywhere;
}

.error {
  margin: 0;
  color: var(--mf-danger);
}
</style>
