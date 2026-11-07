// 插件管理页的纯函数（07 FR-SET-12）：能力与状态的通俗说明、插件“会做什么”、启用列表的增删、插件设置的校验。
// 能力字符串与核心 moodflow-webdata `plugins.rs` 的 `cap_str` 一致。
import type { PluginContributes, PluginItem, PluginKind, PluginSettingField, PluginState } from '@/api'
import { t, type CopyKey } from '@/i18n'

const CAP_LABEL: Record<string, CopyKey> = {
  storage: 'plugins.cap.storage',
  secrets: 'plugins.cap.secrets',
  clipboard_read: 'plugins.cap.clipboard_read',
  context_recent: 'plugins.cap.context_recent',
  notify: 'plugins.cap.notify',
}

/** 一项能力的通俗说明；认不出的能力原样显示（宁可难看，也不能藏起来）。 */
export function capabilityText(cap: string): string {
  if (cap.startsWith('network:')) return t('plugins.cap.network', { host: cap.slice('network:'.length) })
  const key = CAP_LABEL[cap]
  return key ? t(key) : cap
}

const STATE_LABEL: Record<PluginState, CopyKey> = {
  disabled: 'plugins.state.disabled',
  safe_mode: 'plugins.state.safe_mode',
  loaded: 'plugins.state.loaded',
  auto_disabled: 'plugins.state.auto_disabled',
  load_failed: 'plugins.state.load_failed',
}

export function stateText(s: PluginState): string {
  return t(STATE_LABEL[s])
}

/** 出了问题、需要用户留意的状态 */
export function stateIsProblem(s: PluginState): boolean {
  return s === 'auto_disabled' || s === 'load_failed'
}

const CONTENT_LABEL: Record<string, CopyKey> = {
  scheme: 'plugins.content.scheme',
  theme: 'plugins.content.theme',
  phrases: 'plugins.content.phrases',
  soft_keyboard: 'plugins.content.soft_keyboard',
  punctuation: 'plugins.content.punctuation',
  chord_keymap: 'plugins.content.chord_keymap',
  hub_templates: 'plugins.content.hub_templates',
  pet: 'plugins.content.pet',
  quick_input_formats: 'plugins.content.quick_input_formats',
}

/** “沙箱插件” / “内容包 · 主题”；清单读不出来时为空 */
export function kindText(kind: PluginKind | null): string {
  if (!kind) return ''
  if (kind.type === 'wasm') return t('plugins.kind.wasm')
  const sub = kind.content ? CONTENT_LABEL[kind.content] : undefined
  return sub ? `${t('plugins.kind.content')} · ${t(sub)}` : t('plugins.kind.content')
}

/** 插件接入的扩展点，写成几句“它会……” */
export function contributesText(c: PluginContributes | null): string[] {
  if (!c) return []
  const out: string[] = []
  if (c.candidates) out.push(t('plugins.does.candidates'))
  if (c.candidate_filter) out.push(t('plugins.does.candidate_filter'))
  if (c.association) out.push(t('plugins.does.association'))
  if (c.commit_transform) out.push(t('plugins.does.commit_transform'))
  if (c.cmdbar_functions.length > 0)
    out.push(t('plugins.does.cmdbar', { names: c.cmdbar_functions.join('、') }))
  if (c.buffer_action) out.push(t('plugins.does.buffer_action', { label: c.buffer_action }))
  return out
}

/** 启用 / 停用后的 `plugins.enabled`：保持原有顺序，不重复。 */
export function toggledEnabled(list: readonly string[], id: string, on: boolean): string[] {
  const rest = list.filter((x) => x !== id)
  return on ? [...rest, id] : rest
}

/** 当前的启用列表（从插件列表里取，和核心读到的配置一致）。 */
export function enabledIds(plugins: readonly PluginItem[]): string[] {
  return plugins.filter((p) => p.enabled).map((p) => p.id)
}

/** 列表顺序：出了问题的在前，其余按名称。 */
export function sortPlugins(plugins: readonly PluginItem[]): PluginItem[] {
  return [...plugins].sort(
    (a, b) =>
      Number(stateIsProblem(b.state)) - Number(stateIsProblem(a.state)) ||
      a.name.localeCompare(b.name, 'zh-CN'),
  )
}

/**
 * 插件设置一项的问题（界面先拦，核心保存时按同一份声明再校验）：数字要在范围内，文字不超过长度。
 * 没问题返回 null；密钥字段、开关、单选不在界面上出错。
 */
export function fieldProblem(f: PluginSettingField, value: unknown): string | null {
  if (f.type === 'number') {
    const min = f.min ?? -Infinity
    const max = f.max ?? Infinity
    if (typeof value !== 'number' || !Number.isFinite(value) || value < min || value > max)
      return t('plugins.settings_number_range', { min: f.min ?? '', max: f.max ?? '' })
  }
  if (f.type === 'text' && f.max_len != null && typeof value === 'string' && [...value].length > f.max_len)
    return t('plugins.settings_text_too_long', { n: f.max_len })
  return null
}
