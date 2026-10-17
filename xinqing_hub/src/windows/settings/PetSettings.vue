<script setup lang="ts">
// 设置中心 · 常规 · 桌宠（ADR 0044）：用哪只宠物、多大、要不要在屏幕底边散步；导入 Codex 桌宠（选 pet.json、
// zip 包或一张精灵图，或从 Codex / petdex 装过的宠物里一键导入），删除导入的宠物。文件读写都在 Rust 端（pets.rs）。
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type PetCandidate, type PetInfo } from '@/api'
import { errorText, t, type CopyKey } from '@/i18n'
import { useHubSettings } from './useHubSettings'

const SIZE_LABEL: Record<string, CopyKey> = {
  small: 'settings.general.pet_size_small',
  medium: 'settings.general.pet_size_medium',
  large: 'settings.general.pet_size_large',
}
const SOURCE_LABEL: Record<string, CopyKey> = {
  codex: 'settings.general.pet_source_codex',
  petdex: 'settings.general.pet_source_petdex',
}

const { error, notice, ready, flash, bool, text, options, save, restore } = useHubSettings([
  'pet.id',
  'pet.size',
  'pet.roam',
])
const pets = ref<PetInfo[]>([])
const found = ref<PetCandidate[] | null>(null)
const busy = ref(false)

function sizeName(s: string): string {
  const key = SIZE_LABEL[s]
  return key ? t(key) : s
}

function sourceName(s: string): string {
  const key = SOURCE_LABEL[s]
  return key ? t(key) : s
}

async function refresh(): Promise<void> {
  try {
    pets.value = await unwrap(commands.petList())
  } catch (e) {
    error.value = errorText(e)
  }
  try {
    found.value = await unwrap(commands.petDiscover())
  } catch {
    found.value = []
  }
}
onMounted(refresh)

/** 当前用的宠物：设置里的编号不在列表里时就是晴晴 */
function current(): string {
  const id = text('pet.id')
  return pets.value.some((p) => p.id === id) ? id : 'qingqing'
}

async function importFrom(path: string): Promise<void> {
  error.value = null
  busy.value = true
  try {
    const info = await unwrap(commands.petImport(path))
    await refresh()
    if (await save('pet.id', info.id)) flash(t('settings.general.pet_imported', { name: info.name }))
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}

async function pick(): Promise<void> {
  error.value = null
  try {
    const p = await open({
      multiple: false,
      directory: false,
      title: t('settings.general.pet_dialog_title'),
      filters: [{ name: t('settings.general.pet'), extensions: ['json', 'zip', 'png', 'webp'] }],
    })
    if (typeof p === 'string') await importFrom(p)
  } catch (e) {
    error.value = errorText(e)
  }
}

async function remove(p: PetInfo): Promise<void> {
  error.value = null
  try {
    await unwrap(commands.petRemove(p.id))
    await refresh()
    flash(t('settings.general.pet_removed', { name: p.name }))
  } catch (e) {
    error.value = errorText(e)
  }
}

defineExpose({ restore })
</script>

<template>
  <fieldset class="s-group" data-group="pet">
    <legend>{{ t('settings.general.pet') }}</legend>
    <p class="s-hint">{{ t('settings.general.pet_hint') }}</p>
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>

    <div class="s-row">
      <span>{{ t('settings.general.pet_size') }}</span>
      <div class="s-choices">
        <label v-for="o in options('pet.size')" :key="o" class="s-choice">
          <input
            type="radio"
            name="pet-size"
            :checked="text('pet.size') === o"
            @change="save('pet.size', o)"
          />
          {{ sizeName(o) }}
        </label>
      </div>
    </div>

    <label class="s-choice">
      <input
        type="checkbox"
        role="switch"
        :checked="bool('pet.roam')"
        :disabled="!ready"
        @change="save('pet.roam', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('settings.general.pet_roam') }}
    </label>
    <p class="s-hint">{{ t('settings.general.pet_roam_hint') }}</p>

    <div role="radiogroup" :aria-label="t('settings.general.pet_choose')" class="pets">
      <div v-for="p in pets" :key="p.id" class="pet" :data-pet="p.id">
        <label class="s-choice">
          <input type="radio" name="pet-id" :checked="current() === p.id" @change="save('pet.id', p.id)" />
          <span class="name">{{ p.name }}</span>
          <span v-if="p.builtin" class="tag">{{ t('settings.general.pet_builtin') }}</span>
        </label>
        <span v-if="p.description" class="s-hint desc">{{ p.description }}</span>
        <button v-if="!p.builtin" type="button" class="link" @click="remove(p)">
          {{ t('settings.general.pet_remove') }}
        </button>
      </div>
    </div>
    <button type="button" :disabled="busy" @click="pick">{{ t('settings.general.pet_import') }}</button>

    <div v-if="found !== null" class="found">
      <p class="s-hint">{{ t('settings.general.pet_discover') }}</p>
      <p v-if="found.length === 0" class="s-note">{{ t('settings.general.pet_discover_none') }}</p>
      <ul v-else>
        <li v-for="c in found" :key="c.path" :data-candidate="c.id">
          <span class="name">{{ c.name }}</span>
          <span class="tag">{{ sourceName(c.source) }}</span>
          <button type="button" :disabled="busy" @click="importFrom(c.path)">
            {{
              c.imported ? t('settings.general.pet_discover_done') : t('settings.general.pet_discover_import')
            }}
          </button>
        </li>
      </ul>
    </div>
    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </fieldset>
</template>

<style scoped>
.pets {
  display: flex;
  flex-direction: column;
  gap: var(--xq-sp-1);
}

.pet {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-2);
  align-items: center;
}

.desc {
  flex-basis: 100%;
  margin: 0 0 0 var(--xq-sp-6);
}

.tag {
  padding: 0 var(--xq-sp-2);
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface-2);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-xs);
}

.link {
  border-color: transparent;
  background: transparent;
  color: var(--xq-text-2);
}

.found ul {
  display: flex;
  flex-direction: column;
  gap: var(--xq-sp-1);
  margin: 0;
  padding: 0;
  list-style: none;
}

.found li {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: center;
}
</style>
