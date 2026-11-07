import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import type { PluginItem, PluginList, PluginPreview } from '@/api'
import { t } from '@/i18n'
import { capabilityText, contributesText, kindText, sortPlugins, toggledEnabled } from './plugins'

const contributes = (over: Partial<PluginItem['contributes'] & object> = {}) => ({
  candidates: false,
  candidate_filter: false,
  association: false,
  commit_transform: false,
  cmdbar_functions: [] as string[],
  buffer_action: null as string | null,
  has_settings: false,
  ...over,
})

const item = (over: Partial<PluginItem>): PluginItem => ({
  id: 'dev.example.emoji',
  name: '表情',
  version: '1.0.0',
  description: '打字时给出表情候选',
  license: 'MIT',
  kind: { type: 'wasm' },
  platforms: ['windows'],
  capabilities: ['storage'],
  contributes: contributes({ candidates: true }),
  enabled: true,
  state: 'loaded',
  source: 'local',
  ...over,
})

describe('插件页的说明文字', () => {
  it('能力写成通俗的话，联网带主机名，认不出的原样显示', () => {
    expect(capabilityText('clipboard_read')).toBe(t('plugins.cap.clipboard_read'))
    expect(capabilityText('network:api.example.com')).toBe(
      t('plugins.cap.network', { host: 'api.example.com' }),
    )
    expect(capabilityText('camera')).toBe('camera')
  })

  it('类型：沙箱插件，内容包带上类别', () => {
    expect(kindText({ type: 'wasm' })).toBe(t('plugins.kind.wasm'))
    expect(kindText({ type: 'content', content: 'pet' })).toBe(
      `${t('plugins.kind.content')} · ${t('plugins.content.pet')}`,
    )
    expect(kindText(null)).toBe('')
  })

  it('它会做什么：按扩展点逐条列出', () => {
    expect(
      contributesText(contributes({ candidates: true, cmdbar_functions: ['emoji', 'kaomoji'] })),
    ).toEqual([t('plugins.does.candidates'), t('plugins.does.cmdbar', { names: 'emoji、kaomoji' })])
    expect(contributesText(null)).toEqual([])
  })

  it('启用列表：保持顺序、不重复', () => {
    expect(toggledEnabled(['a', 'b'], 'c', true)).toEqual(['a', 'b', 'c'])
    expect(toggledEnabled(['a', 'b'], 'a', true)).toEqual(['b', 'a'])
    expect(toggledEnabled(['a', 'b'], 'a', false)).toEqual(['b'])
  })

  it('出了问题的排在前面', () => {
    const list = sortPlugins([
      item({ id: 'a', name: '甲', state: 'loaded' }),
      item({ id: 'b', name: '乙', state: 'auto_disabled' }),
    ])
    expect(list.map((p) => p.id)).toEqual(['b', 'a'])
  })
})

const mocks = vi.hoisted(() => ({
  list: null as PluginList | null,
  unavailable: false,
  preview: null as PluginPreview | null,
  open: vi.fn(),
  install: vi.fn(),
  uninstall: vi.fn(),
  reenable: vi.fn(),
  imeConfigSet: vi.fn(),
}))
const ok = (data: unknown = null) => Promise.resolve({ status: 'ok', data })

vi.mock('@/api', async (orig) => ({
  ...(await orig<typeof import('@/api')>()),
  commands: {
    pluginsList: async () =>
      mocks.unavailable
        ? { status: 'error', error: { code: 'ime.unavailable', message_key: 'error.ime_unavailable' } }
        : ok(mocks.list),
    pluginsInspect: async () => ok(mocks.preview),
    pluginsInstall: mocks.install,
    pluginsUninstall: mocks.uninstall,
    pluginsReenable: mocks.reenable,
    imeConfigSet: mocks.imeConfigSet,
  },
  events: { imeConfigChanged: { listen: async () => () => {} } },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))

const { default: PluginsSection } = await import('./PluginsSection.vue')

enableAutoUnmount(afterEach)

async function show(): Promise<VueWrapper> {
  const w = mount(PluginsSection, { attachTo: document.body })
  await flushPromises()
  return w
}
const plugin = (w: VueWrapper, id: string) => w.find(`[data-plugin="${id}"]`)
const button = (w: VueWrapper, text: string) => w.findAll('button').find((b) => b.text() === text)!

const preview = (over: Partial<PluginPreview> = {}): PluginPreview => ({
  id: 'dev.example.new',
  name: '新插件',
  version: '1.0.0',
  description: '',
  license: 'MIT',
  kind: { type: 'wasm' },
  platforms: ['windows'],
  capabilities: ['storage', 'notify'],
  contributes: contributes({ commit_transform: true }),
  added: ['storage', 'notify'],
  upgrade_from: null,
  source: 'local',
  ...over,
})

beforeEach(() => {
  mocks.unavailable = false
  mocks.list = {
    available: true,
    safe_mode: false,
    plugins: [
      item({}),
      item({ id: 'dev.example.broken', name: '坏掉的', state: 'auto_disabled', capabilities: [] }),
    ],
  }
  mocks.preview = preview()
  mocks.open.mockReset().mockResolvedValue('C:/Downloads/new.mfpack')
  mocks.install.mockReset().mockImplementation(() => ok())
  mocks.uninstall.mockReset().mockImplementation(() => ok())
  mocks.reenable.mockReset().mockImplementation(() => ok(true))
  mocks.imeConfigSet
    .mockReset()
    .mockImplementation((items: { key: string }[]) =>
      ok({ needs_restart: false, applied: items.length, skipped: [] }),
    )
})

describe('插件管理页', () => {
  it('列出插件：状态、未审核标记、权限与它会做什么；出问题的排前面', async () => {
    const w = await show()
    const rows = w.findAll('[data-plugin]')
    expect(rows[0]!.attributes('data-plugin')).toBe('dev.example.broken')
    const emoji = plugin(w, 'dev.example.emoji')
    expect(emoji.text()).toContain(t('plugins.state.loaded'))
    expect(emoji.text()).toContain(t('plugins.source_local'))
    expect(emoji.text()).toContain(t('plugins.cap.storage'))
    expect(emoji.text()).toContain(t('plugins.does.candidates'))
    expect(plugin(w, 'dev.example.broken').text()).toContain(t('plugins.caps_none'))
  })

  it('输入法没运行时只说明原因', async () => {
    mocks.unavailable = true
    const w = await show()
    expect(w.text()).toContain(t('error.ime_unavailable'))
    expect(w.find('[data-plugin]').exists()).toBe(false)
  })

  it('停用改的是 plugins.enabled，保留其他已启用的', async () => {
    mocks.list!.plugins.push(item({ id: 'dev.other', name: '另一个' }))
    const w = await show()
    await plugin(w, 'dev.example.emoji').find('input[role="switch"]').setValue(false)
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([
      { key: 'plugins.enabled', value: ['dev.example.broken', 'dev.other'] },
    ])
  })

  it('安全模式开关写 plugins.safe_mode', async () => {
    const w = await show()
    await w.find('[data-group="safe-mode"] input').setValue(true)
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'plugins.safe_mode', value: true }])
  })

  it('被自动停用的可以重新启用', async () => {
    const w = await show()
    await button(w, t('plugins.reenable')).trigger('click')
    await flushPromises()
    expect(mocks.reenable).toHaveBeenCalledWith('dev.example.broken')
  })

  it('安装：先列出权限，同意后才装，装完自动启用', async () => {
    const w = await show()
    await button(w, t('plugins.install')).trigger('click')
    await flushPromises()
    const consent = w.find('[data-confirm="install"]')
    expect(consent.text()).toContain(t('plugins.consent_install', { name: '新插件', version: '1.0.0' }))
    expect(consent.findAll('[data-list="added"] li').map((li) => li.text())).toEqual([
      t('plugins.cap.storage'),
      t('plugins.cap.notify'),
    ])
    expect(consent.text()).toContain(t('plugins.privacy_line'))
    expect(consent.text()).toContain(t('plugins.unreviewed_warning'))
    // 焦点在“取消”上
    expect(document.activeElement?.textContent?.trim()).toBe(t('plugins.cancel'))
    expect(mocks.install).not.toHaveBeenCalled()

    await button(w, t('plugins.consent_yes')).trigger('click')
    await flushPromises()
    expect(mocks.install).toHaveBeenCalledWith('C:/Downloads/new.mfpack', ['storage', 'notify'])
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([
      { key: 'plugins.enabled', value: ['dev.example.emoji', 'dev.example.broken', 'dev.example.new'] },
    ])
  })

  it('升级只列新增的权限，不改启用状态', async () => {
    mocks.preview = preview({
      id: 'dev.example.emoji',
      name: '表情',
      version: '1.1.0',
      added: ['clipboard_read'],
      upgrade_from: '1.0.0',
    })
    const w = await show()
    await button(w, t('plugins.install')).trigger('click')
    await flushPromises()
    const consent = w.find('[data-confirm="install"]')
    expect(consent.text()).toContain(
      t('plugins.consent_upgrade', { name: '表情', from: '1.0.0', version: '1.1.0' }),
    )
    expect(consent.text()).toContain(t('plugins.consent_new_caps'))
    expect(consent.findAll('[data-list="added"] li').map((li) => li.text())).toEqual([
      t('plugins.cap.clipboard_read'),
    ])
    await button(w, t('plugins.consent_yes_upgrade')).trigger('click')
    await flushPromises()
    expect(mocks.install).toHaveBeenCalledWith('C:/Downloads/new.mfpack', ['clipboard_read'])
    expect(mocks.imeConfigSet).not.toHaveBeenCalled()
  })

  it('取消选择文件或取消确认都不安装', async () => {
    mocks.open.mockResolvedValue(null)
    const w = await show()
    await button(w, t('plugins.install')).trigger('click')
    await flushPromises()
    expect(w.find('[data-confirm="install"]').exists()).toBe(false)

    mocks.open.mockResolvedValue('C:/x.mfpack')
    await button(w, t('plugins.install')).trigger('click')
    await flushPromises()
    await button(w, t('plugins.cancel')).trigger('click')
    await flushPromises()
    expect(w.find('[data-confirm="install"]').exists()).toBe(false)
    expect(mocks.install).not.toHaveBeenCalled()
  })

  it('核心发现确认后文件变了：提示重新选择并收起确认框', async () => {
    mocks.install.mockImplementation(() =>
      Promise.resolve({
        status: 'error',
        error: { code: 'plugins.consent_mismatch', message_key: 'error.plugin_consent_mismatch' },
      }),
    )
    const w = await show()
    await button(w, t('plugins.install')).trigger('click')
    await flushPromises()
    await button(w, t('plugins.consent_yes')).trigger('click')
    await flushPromises()
    expect(w.find('[role="alert"]').text()).toBe(t('error.plugin_consent_mismatch'))
    expect(w.find('[data-confirm="install"]').exists()).toBe(false)
  })

  it('卸载要确认，可选连数据一起删，并从启用列表里去掉', async () => {
    const w = await show()
    const row = plugin(w, 'dev.example.emoji')
    await row
      .findAll('button')
      .find((b) => b.text() === t('plugins.uninstall'))!
      .trigger('click')
    expect(mocks.uninstall).not.toHaveBeenCalled()
    await row.find('.s-confirm input[type="checkbox"]').setValue(true)
    await row.find('.s-danger').trigger('click')
    await flushPromises()
    expect(mocks.uninstall).toHaveBeenCalledWith('dev.example.emoji', true)
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([
      { key: 'plugins.enabled', value: ['dev.example.broken'] },
    ])
  })
})
