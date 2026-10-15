<script setup lang="ts">
// 设置中心 · 关于 · 导出运行信息（11 FR-OPS-04）：先列出会打包的文件，用户确认后选保存位置再导出。
// 界面不叫“诊断信息”：“诊断”在禁用词表里（DS-COPY-03，ADR 0039）。
import { ref } from 'vue'
import { save } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type DiagFile } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'

const files = ref<DiagFile[] | null>(null)
const busy = ref(false)
const done = ref<string | null>(null)
const error = ref<string | null>(null)

const what = (f: DiagFile) => t(`about.file_${f.what}` as CopyKey)

async function show(): Promise<void> {
  done.value = null
  error.value = null
  try {
    files.value = await commands.diagnosticsPreview()
  } catch (e) {
    error.value = errorText(e)
  }
}

function defaultName(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `xinqing-run-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}.zip`
}

async function exportNow(): Promise<void> {
  error.value = null
  let path: string | null
  try {
    path = await save({
      defaultPath: defaultName(),
      filters: [{ name: t('about.run_info_filter'), extensions: ['zip'] }],
    })
  } catch (e) {
    error.value = errorText(e)
    return
  }
  if (!path) return
  busy.value = true
  try {
    await unwrap(commands.diagnosticsExport(path))
    done.value = t('about.run_info_done', { path })
    files.value = null
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="run-info" data-group="run-info">
    <h2>{{ t('about.run_info') }}</h2>
    <p class="muted">{{ t('about.run_info_hint') }}</p>
    <button v-if="!files" type="button" @click="show">{{ t('about.run_info_show') }}</button>
    <template v-else>
      <p>{{ t('about.run_info_list') }}</p>
      <ul class="files">
        <li v-for="f in files" :key="f.name">
          <code>{{ f.name }}</code> {{ what(f) }}
        </li>
      </ul>
      <div class="actions">
        <button type="button" class="primary" :disabled="busy" @click="exportNow">
          {{ busy ? t('about.run_info_saving') : t('about.run_info_save') }}
        </button>
        <button type="button" :disabled="busy" @click="files = null">{{ t('about.run_info_cancel') }}</button>
      </div>
    </template>
    <p v-if="done" role="status">{{ done }}</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.files {
  padding-left: var(--xq-sp-4);
}

.files li {
  margin-bottom: var(--xq-sp-1);
}

.actions {
  display: flex;
  gap: var(--xq-sp-2);
}

.error {
  color: var(--xq-danger);
}
</style>
