<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { t } from '@/i18n'
import { mappingRow, type ImportPlan, type ImportPolicy, type MappingChange } from './mappingTypes'

const props = defineProps<{
  request: <T>(method: string, params?: object) => Promise<T>
  apply: (edits: MappingChange[]) => Promise<boolean>
  message: (error: unknown) => string
}>()
const emit = defineEmits<{ close: [] }>()
const busy = ref(false)
const error = ref('')
const input = ref('')
const source = ref('')
const policy = ref<ImportPolicy>('skip')
const plan = ref<ImportPlan | null>(null)
const offset = ref(0)
const limit = 20
const maxBytes = 768 * 1024
const shown = computed(() => plan.value?.edits.slice(offset.value, offset.value + limit) ?? [])
let alive = true

function invalidate() {
  plan.value = null
  offset.value = 0
  error.value = ''
}
async function run(action: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  try {
    await action()
  } catch (e) {
    if (alive) error.value = props.message(e)
  } finally {
    if (alive) busy.value = false
  }
}
function readFile(event: Event) {
  const picker = event.target as HTMLInputElement
  const file = picker.files?.[0]
  picker.value = ''
  if (!file) return
  void run(async () => {
    invalidate()
    source.value = ''
    input.value = ''
    if (file.size > maxBytes) {
      error.value = t('mapping.limit')
      return
    }
    const bytes = await file.arrayBuffer()
    if (!alive) return
    try {
      input.value = new TextDecoder('utf-8', { fatal: true }).decode(bytes)
      source.value = file.name
    } catch {
      error.value = t('mapping.encoding')
    }
  })
}
function inspect() {
  if (!input.value.trim()) return
  void run(async () => {
    invalidate()
    if (new TextEncoder().encode(input.value).length > maxBytes) {
      error.value = t('mapping.limit')
      return
    }
    const result = await props.request<ImportPlan>('importPreview', {
      text: input.value,
      policy: policy.value,
    })
    if (!alive) return
    plan.value = {
      ...result,
      edits: result.edits.map((edit) => ({
        before: mappingRow(edit.before),
        after: mappingRow(edit.after),
      })),
    }
  })
}
function confirmImport() {
  const edits = plan.value?.edits
  if (!edits?.length) return
  void run(async () => {
    try {
      await props.apply(edits)
    } catch (e) {
      if (alive) plan.value = null
      throw e
    }
  })
}
onBeforeUnmount(() => {
  alive = false
})
</script>

<template>
  <section class="d-card import" :aria-label="t('mapping.import')">
    <h2>{{ t('mapping.import') }}</h2>
    <p class="hint">{{ t('mapping.import_hint') }}</p>
    <div class="d-row">
      <label
        >{{ t('mapping.import_file')
        }}<input type="file" accept=".jsonl,.txt" :disabled="busy" @change="readFile"
      /></label>
      <span v-if="source" class="hint">{{ source }}</span>
    </div>
    <label
      >{{ t('mapping.import_text') }}
      <textarea
        v-model="input"
        :maxlength="maxBytes"
        rows="5"
        :disabled="busy"
        spellcheck="false"
        @input="invalidate"
      />
    </label>
    <label
      >{{ t('mapping.policy') }}
      <select v-model="policy" :disabled="busy" @change="invalidate">
        <option value="skip">{{ t('mapping.policy_skip') }}</option>
        <option value="merge">{{ t('mapping.policy_merge') }}</option>
        <option value="replace">{{ t('mapping.policy_replace') }}</option>
      </select>
    </label>
    <p v-if="policy === 'replace'" class="hint">{{ t('mapping.replace_hint') }}</p>
    <p v-if="error" class="d-error" role="alert">{{ error }}</p>
    <p v-if="busy" class="hint" role="status">{{ t('mapping.working') }}</p>
    <template v-if="plan">
      <h3>{{ t('mapping.import_review') }}</h3>
      <p role="status">
        {{
          t('mapping.import_counts', {
            new: plan.new_count,
            conflicts: plan.conflict_count,
            skipped: plan.skipped_count,
            unchanged: plan.unchanged_count,
            changes: plan.edits.length,
          })
        }}
      </p>
      <div v-if="shown.length" class="table-wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">{{ t('mapping.word') }}</th>
              <th scope="col">{{ t('mapping.before') }}</th>
              <th scope="col">{{ t('mapping.after') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="change in shown" :key="change.after.word">
              <th scope="row">{{ change.after.word }}</th>
              <td>
                <small>{{ t('mapping.added') }}</small>
                {{ change.before.added.join('\u3000') || t('mapping.none') }} <br /><small>{{
                  t('mapping.hidden')
                }}</small>
                {{ change.before.hidden.join('\u3000') || t('mapping.none') }}
              </td>
              <td>
                <small>{{ t('mapping.added') }}</small>
                {{ change.after.added.join('\u3000') || t('mapping.none') }} <br /><small>{{
                  t('mapping.hidden')
                }}</small>
                {{ change.after.hidden.join('\u3000') || t('mapping.none') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <nav v-if="plan.edits.length > limit" class="d-row" :aria-label="t('mapping.pages')">
        <button :disabled="busy || !offset" @click="offset -= limit">{{ t('mapping.previous') }}</button>
        <span>{{
          t('mapping.range', { first: offset + 1, last: offset + shown.length, total: plan.edits.length })
        }}</span>
        <button :disabled="busy || offset + shown.length >= plan.edits.length" @click="offset += limit">
          {{ t('mapping.next') }}
        </button>
      </nav>
    </template>
    <div class="d-row">
      <button :disabled="busy || !input.trim()" @click="inspect">{{ t('mapping.import_preview') }}</button>
      <button v-if="plan" :disabled="busy || !plan.edits.length" @click="confirmImport">
        {{ t('mapping.import_apply') }}
      </button>
      <button :disabled="busy" @click="emit('close')">{{ t('mapping.close') }}</button>
    </div>
  </section>
</template>

<style scoped>
.import {
  display: grid;
  gap: var(--mf-sp-3);
}
h2,
h3,
p {
  margin: 0;
}
.hint,
small {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}
label {
  display: grid;
  gap: var(--mf-sp-1);
}
textarea {
  font-family: monospace;
  min-width: 0;
}
select {
  justify-self: start;
}
.table-wrap {
  overflow-x: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
  text-align: start;
}
th,
td {
  border-bottom: 1px solid var(--mf-border);
  padding: var(--mf-sp-2);
  vertical-align: top;
  overflow-wrap: anywhere;
}
td {
  min-width: 7rem;
}
</style>
