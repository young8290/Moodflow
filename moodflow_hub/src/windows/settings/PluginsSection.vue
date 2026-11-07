<script setup lang="ts">
// 设置中心 · 插件（07 FR-SET-12，plugin-system.md §11 P5，ADR 0054）：已装列表（名称、版本、类型、来源、状态、权限、
// 它会做什么）、启用 / 停用、重新启用被自动停用的、卸载（可连私有数据一起删）、顶部的安全模式；从本地文件安装，
// 装之前用通俗的话逐项列出要同意的权限，升级只列新增的。装、卸、列表都在输入法核心里做（plugins.*）；
// 启用列表与安全模式是输入法配置 `plugins.enabled` / `plugins.safe_mode`。还没有签名市场，本地装的一律标“未审核”。
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import {
  CommandError,
  commands,
  events,
  unwrap,
  type PluginItem,
  type PluginList,
  type PluginPreview,
} from '@/api'
import { errorText, t } from '@/i18n'
import SectionHead from './SectionHead.vue'
import {
  capabilityText,
  contributesText,
  enabledIds,
  kindText,
  sortPlugins,
  stateIsProblem,
  stateText,
  toggledEnabled,
} from './plugins'

const list = ref<PluginList | null>(null)
/** 输入法没在运行 */
const offline = ref(false)
const error = ref<string | null>(null)
const notice = ref<string | null>(null)
const busy = ref(false)
/** 安装确认：选中的文件与核心给的预览 */
const pending = ref<{ path: string; preview: PluginPreview } | null>(null)
/** 正在确认卸载的插件 id 与“同时删除数据” */
const removing = ref<{ id: string; deleteData: boolean } | null>(null)
const cancelBtn = ref<HTMLButtonElement | null>(null)
let noticeTimer: ReturnType<typeof setTimeout> | undefined
let unlisten: (() => void) | undefined
let disposed = false

function flash(text: string): void {
  notice.value = text
  clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => (notice.value = null), 2500)
}

async function load(): Promise<void> {
  try {
    list.value = await unwrap(commands.pluginsList())
    offline.value = false
  } catch (e) {
    list.value = null
    if (e instanceof CommandError && e.ui.code === 'ime.unavailable') offline.value = true
    else error.value = errorText(e)
  }
}

onMounted(async () => {
  window.addEventListener('focus', load)
  await load()
  // 别处改了启用列表或安全模式（通用表单、核心重启）也跟着刷新
  const off = await events.imeConfigChanged.listen(() => void load())
  if (disposed) off()
  else unlisten = off
})
onBeforeUnmount(() => {
  disposed = true
  window.removeEventListener('focus', load)
  unlisten?.()
  clearTimeout(noticeTimer)
})

/** 写一项输入法配置；核心跳过时显示原因。返回是否写成。 */
async function setIme(key: string, value: unknown): Promise<boolean> {
  try {
    const r = await unwrap(commands.imeConfigSet([{ key, value }]))
    const skipped = r.skipped.find((s) => s.key === key)
    if (skipped) error.value = skipped.reason
    return !skipped
  } catch (e) {
    error.value = errorText(e)
    return false
  }
}

async function setSafeMode(on: boolean): Promise<void> {
  error.value = null
  if (await setIme('plugins.safe_mode', on)) flash(t(on ? 'plugins.safe_mode_on' : 'plugins.safe_mode_off'))
  await load()
}

async function setEnabled(p: PluginItem, on: boolean): Promise<void> {
  if (!list.value) return
  error.value = null
  if (await setIme('plugins.enabled', toggledEnabled(enabledIds(list.value.plugins), p.id, on)))
    flash(t(on ? 'plugins.enabled_done' : 'plugins.disabled_done', { name: p.name }))
  await load()
}

async function reenable(p: PluginItem): Promise<void> {
  error.value = null
  try {
    await unwrap(commands.pluginsReenable(p.id))
    flash(t('plugins.reenabled', { name: p.name }))
  } catch (e) {
    error.value = errorText(e)
  }
  await load()
}

async function pick(): Promise<void> {
  error.value = null
  pending.value = null
  try {
    const path = await open({
      multiple: false,
      directory: false,
      title: t('plugins.dialog_title'),
      filters: [{ name: t('plugins.dialog_filter'), extensions: ['mfpack', 'zip'] }],
    })
    if (typeof path !== 'string') return
    busy.value = true
    pending.value = { path, preview: await unwrap(commands.pluginsInspect(path)) }
    await nextTick()
    // 焦点落在“取消”：误按回车不会直接装
    cancelBtn.value?.focus()
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}

async function install(): Promise<void> {
  const p = pending.value
  if (!p || !list.value) return
  error.value = null
  busy.value = true
  try {
    await unwrap(commands.pluginsInstall(p.path, p.preview.added))
    pending.value = null
    // 首次安装直接启用（用户刚同意过权限）；升级保持原来的启用状态
    const fresh = p.preview.upgrade_from === null
    const ids = enabledIds(list.value.plugins)
    if (fresh && !ids.includes(p.preview.id))
      await setIme('plugins.enabled', toggledEnabled(ids, p.preview.id, true))
    flash(
      t(fresh ? 'plugins.installed' : 'plugins.upgraded', {
        name: p.preview.name,
        version: p.preview.version,
      }),
    )
  } catch (e) {
    error.value = errorText(e)
    // 文件在确认之后变了：收起确认框，让用户重新选
    if (e instanceof CommandError && e.ui.code === 'plugins.consent_mismatch') pending.value = null
  } finally {
    busy.value = false
  }
  await load()
}

async function uninstall(p: PluginItem): Promise<void> {
  const r = removing.value
  if (!r || r.id !== p.id || !list.value) return
  error.value = null
  busy.value = true
  try {
    await unwrap(commands.pluginsUninstall(p.id, r.deleteData))
    removing.value = null
    const ids = enabledIds(list.value.plugins)
    if (ids.includes(p.id)) await setIme('plugins.enabled', toggledEnabled(ids, p.id, false))
    flash(t('plugins.removed', { name: p.name }))
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
  await load()
}
</script>

<template>
  <section>
    <SectionHead :title="t('settings.plugins_nav')" :restorable="false" />
    <p class="s-hint">{{ t('plugins.intro') }}</p>
    <p v-if="error" class="s-error" role="alert">{{ error }}</p>
    <p v-if="offline" class="s-note">{{ t('error.ime_unavailable') }}</p>
    <p v-else-if="list && !list.available" class="s-note">{{ t('plugins.no_user_dir') }}</p>

    <template v-if="list && list.available">
      <fieldset class="s-group" data-group="safe-mode">
        <legend>{{ t('plugins.safe_mode') }}</legend>
        <label class="s-choice">
          <input
            type="checkbox"
            role="switch"
            :checked="list.safe_mode"
            @change="setSafeMode(($event.target as HTMLInputElement).checked)"
          />
          {{ t('plugins.safe_mode_label') }}
        </label>
        <p class="s-hint">{{ t('plugins.safe_mode_hint') }}</p>
      </fieldset>

      <fieldset class="s-group" data-group="installed">
        <legend>{{ t('plugins.installed_title') }}</legend>
        <p v-if="list.plugins.length === 0" class="s-note">{{ t('plugins.empty') }}</p>
        <ul v-else class="plugins">
          <li
            v-for="p in sortPlugins(list.plugins)"
            :key="p.id"
            class="plugin"
            :data-plugin="p.id"
            :data-state="p.state"
          >
            <div class="title">
              <span class="name">{{ p.name }}</span>
              <span class="ver">{{ p.version }}</span>
              <span v-if="kindText(p.kind)" class="tag">{{ kindText(p.kind) }}</span>
              <span class="tag warn">{{ t('plugins.source_local') }}</span>
              <span class="state" :class="{ problem: stateIsProblem(p.state) }">{{
                stateText(p.state)
              }}</span>
            </div>
            <p class="id">{{ p.id }}</p>
            <p v-if="p.description" class="desc">{{ p.description }}</p>
            <ul v-if="contributesText(p.contributes).length" class="does">
              <li v-for="line in contributesText(p.contributes)" :key="line">{{ line }}</li>
            </ul>
            <p class="caps-title">{{ t('plugins.caps_title') }}</p>
            <ul class="caps">
              <li v-if="p.capabilities.length === 0">{{ t('plugins.caps_none') }}</li>
              <li v-for="c in p.capabilities" :key="c">{{ capabilityText(c) }}</li>
            </ul>
            <div class="actions">
              <label class="s-choice">
                <input
                  type="checkbox"
                  role="switch"
                  :checked="p.enabled"
                  :disabled="busy"
                  @change="setEnabled(p, ($event.target as HTMLInputElement).checked)"
                />
                {{ t('plugins.enable') }}
              </label>
              <button
                v-if="p.state === 'auto_disabled'"
                type="button"
                class="s-compact"
                :disabled="busy"
                @click="reenable(p)"
              >
                {{ t('plugins.reenable') }}
              </button>
              <button
                v-if="removing?.id !== p.id"
                type="button"
                class="s-compact"
                :disabled="busy"
                @click="removing = { id: p.id, deleteData: false }"
              >
                {{ t('plugins.uninstall') }}
              </button>
            </div>
            <div
              v-if="removing?.id === p.id"
              class="s-confirm"
              role="group"
              :aria-label="t('plugins.uninstall_confirm', { name: p.name })"
            >
              <p>{{ t('plugins.uninstall_confirm', { name: p.name }) }}</p>
              <label class="s-choice">
                <input v-model="removing.deleteData" type="checkbox" />
                {{ t('plugins.uninstall_delete_data') }}
              </label>
              <div class="s-row">
                <button type="button" class="s-compact s-danger" :disabled="busy" @click="uninstall(p)">
                  {{ t('plugins.uninstall_yes') }}
                </button>
                <button type="button" class="s-compact" @click="removing = null">
                  {{ t('plugins.cancel') }}
                </button>
              </div>
            </div>
          </li>
        </ul>
        <button v-if="!pending" type="button" :disabled="busy" @click="pick">
          {{ t('plugins.install') }}
        </button>

        <div
          v-if="pending"
          class="s-confirm consent"
          role="group"
          data-confirm="install"
          :aria-label="t('plugins.consent_title')"
        >
          <p class="consent-head">
            {{
              pending.preview.upgrade_from === null
                ? t('plugins.consent_install', {
                    name: pending.preview.name,
                    version: pending.preview.version,
                  })
                : t('plugins.consent_upgrade', {
                    name: pending.preview.name,
                    from: pending.preview.upgrade_from,
                    version: pending.preview.version,
                  })
            }}
          </p>
          <p v-if="pending.preview.description" class="desc">{{ pending.preview.description }}</p>
          <ul v-if="contributesText(pending.preview.contributes).length" class="does">
            <li v-for="line in contributesText(pending.preview.contributes)" :key="line">{{ line }}</li>
          </ul>
          <p>
            {{
              pending.preview.added.length === 0
                ? t(
                    pending.preview.upgrade_from === null
                      ? 'plugins.consent_no_caps'
                      : 'plugins.consent_no_new_caps',
                  )
                : t(
                    pending.preview.upgrade_from === null
                      ? 'plugins.consent_caps'
                      : 'plugins.consent_new_caps',
                  )
            }}
          </p>
          <ul v-if="pending.preview.added.length" class="caps" data-list="added">
            <li v-for="c in pending.preview.added" :key="c">{{ capabilityText(c) }}</li>
          </ul>
          <p class="s-hint">{{ t('plugins.privacy_line') }}</p>
          <p class="s-hint warn-text">{{ t('plugins.unreviewed_warning') }}</p>
          <div class="s-row">
            <button type="button" class="s-compact" :disabled="busy" @click="install">
              {{
                pending.preview.upgrade_from === null
                  ? t('plugins.consent_yes')
                  : t('plugins.consent_yes_upgrade')
              }}
            </button>
            <button ref="cancelBtn" type="button" class="s-compact" @click="pending = null">
              {{ t('plugins.cancel') }}
            </button>
          </div>
        </div>
      </fieldset>
    </template>
    <p v-if="notice" class="s-saved" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.plugins {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-3);
  margin: 0;
  padding: 0;
  list-style: none;
}

.plugin {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-1);
  padding: var(--mf-sp-3);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}

.title {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-2);
  align-items: baseline;
}

.name {
  font-weight: 600;
}

.ver,
.id {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.id {
  margin: 0;
}

.desc {
  margin: 0;
}

.tag {
  padding: 0 var(--mf-sp-2);
  border-radius: var(--mf-radius-sm);
  background: var(--mf-surface-2);
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.tag.warn,
.warn-text {
  color: var(--mf-danger);
}

.state {
  margin-left: auto;
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.state.problem {
  color: var(--mf-danger);
  font-weight: 600;
}

.does,
.caps {
  margin: 0;
  padding-left: var(--mf-sp-5);
}

.caps-title {
  margin: var(--mf-sp-1) 0 0;
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-3);
  align-items: center;
  margin-top: var(--mf-sp-1);
}

.consent-head {
  font-weight: 600;
}
</style>
