<script setup lang="ts">
// 插件设置（07 FR-SET-12 第 4 条，plugin-system.md P3）：插件不带自己的界面，这里按它的设置声明渲染
// text / toggle / choice / number 四种控件；密钥类（secure_text）的存储还没做，只说明暂时不能填。
// 先在界面上校验长度与范围，核心保存前按同一份声明再校验一次；存完核心重载插件，新值马上生效。
import { onMounted, reactive, ref } from 'vue'
import { commands, unwrap, type PluginSettingField } from '@/api'
import { errorText, t } from '@/i18n'
import { fieldProblem } from './plugins'

const props = defineProps<{ id: string; name: string }>()
const emit = defineEmits<{ saved: []; close: [] }>()

const fields = ref<PluginSettingField[] | null>(null)
const draft = reactive<Record<string, unknown>>({})
const error = ref<string | null>(null)
const busy = ref(false)

onMounted(async () => {
  try {
    const s = await unwrap(commands.pluginsSettings(props.id))
    fields.value = s.fields
    Object.assign(draft, s.values as Record<string, unknown>)
  } catch (e) {
    error.value = errorText(e)
  }
})

function problem(f: PluginSettingField): string | null {
  return fieldProblem(f, draft[f.id])
}

function setNumber(f: PluginSettingField, raw: string): void {
  draft[f.id] = raw.trim() === '' ? null : Number(raw)
}

async function save(): Promise<void> {
  if (!fields.value) return
  error.value = null
  if (fields.value.some((f) => problem(f) !== null)) return
  busy.value = true
  try {
    const values = fields.value
      .filter((f) => f.type !== 'secure_text' && draft[f.id] !== undefined)
      .map((f) => ({ key: f.id, value: draft[f.id] }))
    await unwrap(commands.pluginsSaveSettings(props.id, values))
    emit('saved')
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="settings" role="group" :aria-label="t('plugins.settings_title', { name })" data-form="settings">
    <p class="title">{{ t('plugins.settings_title', { name }) }}</p>
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>
    <p v-if="fields === null && !error" class="s-note">{{ t('plugins.settings_loading') }}</p>
    <template v-if="fields">
      <div v-for="f in fields" :key="f.id" class="field" :data-field="f.id">
        <template v-if="f.type === 'toggle'">
          <label class="s-choice">
            <input v-model="draft[f.id]" type="checkbox" role="switch" />
            {{ f.label }}
          </label>
        </template>
        <template v-else>
          <label :for="`pf-${id}-${f.id}`">{{ f.label }}</label>
          <input
            v-if="f.type === 'text'"
            :id="`pf-${id}-${f.id}`"
            v-model="draft[f.id]"
            type="text"
            :maxlength="f.max_len ?? undefined"
          />
          <select v-else-if="f.type === 'choice'" :id="`pf-${id}-${f.id}`" v-model="draft[f.id]">
            <option v-for="o in f.options ?? []" :key="o" :value="o">{{ o }}</option>
          </select>
          <input
            v-else-if="f.type === 'number'"
            :id="`pf-${id}-${f.id}`"
            type="number"
            :min="f.min ?? undefined"
            :max="f.max ?? undefined"
            step="any"
            :value="draft[f.id] ?? ''"
            @input="setNumber(f, ($event.target as HTMLInputElement).value)"
          />
          <p v-else-if="f.type === 'secure_text'" :id="`pf-${id}-${f.id}`" class="s-note">
            {{ t('plugins.settings_secret') }}
          </p>
        </template>
        <p class="s-hint">{{ f.help }}</p>
        <p v-if="problem(f)" class="s-error" role="alert">{{ problem(f) }}</p>
      </div>
      <p v-if="fields.length === 0" class="s-note">{{ t('error.plugin_no_settings') }}</p>
      <div class="s-row">
        <button v-if="fields.length > 0" type="button" class="s-compact" :disabled="busy" @click="save">
          {{ t('plugins.settings_save') }}
        </button>
        <button type="button" class="s-compact" @click="emit('close')">
          {{ t('plugins.settings_close') }}
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-2);
  margin-top: var(--mf-sp-2);
  padding: var(--mf-sp-3);
  border-radius: var(--mf-radius-sm);
  background: var(--mf-surface-2);
}

.title {
  margin: 0;
  font-weight: 600;
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-1);
}

.field .s-hint,
.field .s-error {
  margin: 0;
}
</style>
