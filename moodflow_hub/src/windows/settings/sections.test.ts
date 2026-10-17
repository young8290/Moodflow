import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { ConsentItem, SettingField, SettingValue } from '@/api'
import { t } from '@/i18n'
import { validResearchId } from './care'
import { hubHotkeyOk } from './hotkey'

const mocks = vi.hoisted(() => ({
  values: {} as Record<string, SettingValue>,
  settingsSet: vi.fn(),
  consent: {} as Partial<Record<ConsentItem, boolean>>,
  consentSet: vi.fn(),
  ime: {} as Record<string, unknown>,
  imeDefaults: {} as Record<string, unknown>,
  imeAvailable: true,
  imeConfigSet: vi.fn(),
  hotkeyStatus: vi.fn(),
  baselineReset: vi.fn(),
  researchClear: vi.fn(),
  openWindow: vi.fn(),
  emit: vi.fn(),
  autostart: false as boolean | null,
  autostartSet: vi.fn(),
  pets: [] as { id: string; name: string; description: string; builtin: boolean; version: number }[],
  petImport: vi.fn(),
  petRemove: vi.fn(),
  found: [] as { path: string; id: string; name: string; source: string; imported: boolean }[],
}))
const ok = (data: unknown = null) => Promise.resolve({ status: 'ok', data })
const ITEMS: ConsentItem[] = ['sense', 'jev_features', 'schedule', 'llm_summary', 'enhanced', 'rewrite']
const consentState = () => ({
  policy_ver: 1,
  items: ITEMS.map((item) => ({ item, granted: mocks.consent[item] ?? false })),
})

/** 与 core/src/domain/settings.rs 的 KEYS 同形的一部分 */
const SCHEMA: SettingField[] = [
  { key: 'ui.theme', kind: { type: 'choice', options: ['system', 'light', 'dark'] }, default: 'system' },
  { key: 'widget.visible', kind: { type: 'bool' }, default: true },
  { key: 'widget.opacity', kind: { type: 'number', min: 0.7, max: 1 }, default: 1 },
  { key: 'widget.topmost', kind: { type: 'bool' }, default: true },
  { key: 'widget.autohide', kind: { type: 'bool' }, default: false },
  { key: 'hotkey.chat', kind: { type: 'text', item: 'hotkey' }, default: 'ctrl+alt+q' },
  { key: 'hotkey.widget', kind: { type: 'text', item: 'hotkey' }, default: 'ctrl+alt+w' },
  { key: 'hotkey.dashboard', kind: { type: 'text', item: 'hotkey' }, default: 'ctrl+alt+d' },
  { key: 'collection.enabled', kind: { type: 'bool' }, default: true },
  { key: 'pet.id', kind: { type: 'text', item: 'pet_id' }, default: 'qingqing' },
  { key: 'pet.size', kind: { type: 'choice', options: ['small', 'medium', 'large'] }, default: 'medium' },
  { key: 'pet.roam', kind: { type: 'bool' }, default: true },
  { key: 'rest.eye.enabled', kind: { type: 'bool' }, default: true },
  { key: 'rest.eye.interval', kind: { type: 'number', min: 15, max: 60 }, default: 20 },
  { key: 'rest.water.enabled', kind: { type: 'bool' }, default: true },
  { key: 'rest.water.interval', kind: { type: 'number', min: 30, max: 120 }, default: 60 },
  { key: 'rest.move.enabled', kind: { type: 'bool' }, default: true },
  { key: 'rest.move.interval', kind: { type: 'number', min: 30, max: 90 }, default: 50 },
  { key: 'rest.night.enabled', kind: { type: 'bool' }, default: true },
  {
    key: 'rest.night.start',
    kind: { type: 'choice', options: ['22:30', '23:00', '23:30', '00:00'] },
    default: '23:30',
  },
  { key: 'rest.count_when_paused', kind: { type: 'bool' }, default: true },
  { key: 'rest.focus_period', kind: { type: 'list', item: 'time_range', max_items: 1 }, default: [] },
  {
    key: 'sch.default_offsets',
    kind: { type: 'choice', options: ['0', '5', '10', '30', '60', '1440'] },
    default: '10',
  },
  { key: 'todo.daily_digest', kind: { type: 'bool' }, default: false },
  { key: 'research.id', kind: { type: 'text', item: 'research_id' }, default: '' },
  { key: 'research.enabled', kind: { type: 'bool' }, default: false },
  { key: 'dev.demo', kind: { type: 'bool' }, default: false },
]
const defaults = () => Object.fromEntries(SCHEMA.map((f) => [f.key, f.default]))

vi.mock('@/api', async (orig) => ({
  ...(await orig<typeof import('@/api')>()),
  commands: {
    settingsSchema: async () => SCHEMA,
    settingsGet: async (key: string) => ok(mocks.values[key]),
    settingsSet: mocks.settingsSet,
    consentGet: async () => ok(consentState()),
    consentSet: mocks.consentSet,
    imeConfigGet: async () =>
      mocks.imeAvailable
        ? ok({ values: { moodflow: mocks.ime } })
        : { status: 'error', error: { code: 'ime.unavailable', message_key: 'error.ime_unavailable' } },
    imeConfigDefaults: async () => ok({ values: { moodflow: mocks.imeDefaults } }),
    imeConfigSet: mocks.imeConfigSet,
    hotkeyStatus: mocks.hotkeyStatus,
    getStatus: async () => ok({ baseline_progress: 40 }),
    baselineReset: mocks.baselineReset,
    aiUsageToday: async () => ok([{ kind: 'rewrite', used: 3, cap: 100 }]),
    researchClear: mocks.researchClear,
    demoStatus: async () => ok({ enabled: false, speed: 1 }),
    openWindow: mocks.openWindow,
    autostartGet: async () =>
      mocks.autostart === null
        ? { status: 'error', error: { code: 'autostart.failed', message_key: 'error.generic' } }
        : ok(mocks.autostart),
    autostartSet: mocks.autostartSet,
    runningApps: async () => ['Code.exe', 'WeChat.exe'],
    petList: async () => ok(mocks.pets),
    petDiscover: async () => ok(mocks.found),
    petImport: mocks.petImport,
    petRemove: mocks.petRemove,
  },
  events: {
    settingsChanged: { listen: async () => () => {} },
    imeConfigChanged: { listen: async () => () => {} },
    statusChanged: { listen: async () => () => {} },
  },
}))
vi.mock('@tauri-apps/api/event', () => ({ emit: mocks.emit, listen: async () => () => {} }))

const { default: GeneralSection } = await import('./GeneralSection.vue')
const { default: SenseSection } = await import('./SenseSection.vue')
const { default: RestSection } = await import('./RestSection.vue')
const { default: AgendaSection } = await import('./AgendaSection.vue')
const { default: RewriteSection } = await import('./RewriteSection.vue')
const { default: ResearchDevSection } = await import('./ResearchDevSection.vue')

enableAutoUnmount(afterEach)

async function open(c: Parameters<typeof mount>[0]): Promise<VueWrapper> {
  const w = mount(c, { attachTo: document.body })
  await flushPromises()
  return w
}
const group = (w: VueWrapper, name: string) => w.find(`[data-group="${name}"]`)

/** 在录制框里按下一组键 */
async function record(w: VueWrapper, id: string, init: KeyboardEventInit): Promise<void> {
  const btn = w.find(`#${id}`)
  await btn.trigger('click')
  await btn.trigger('keydown', init)
  await flushPromises()
}

beforeEach(() => {
  setActivePinia(createPinia())
  mocks.values = defaults()
  mocks.settingsSet.mockReset().mockImplementation(() => ok())
  mocks.autostart = false
  mocks.autostartSet.mockReset().mockImplementation((on: boolean) => ok(on))
  mocks.consent = { sense: true, jev_features: true, schedule: false, rewrite: true }
  mocks.consentSet.mockReset().mockImplementation((item: ConsentItem, g: boolean) => {
    mocks.consent[item] = g
    return ok(consentState())
  })
  mocks.ime = {
    enabled: true,
    app_blocklist: ['KeePass.exe'],
    app_allowlist: [],
    remember_pause: false,
    pause_hotkey: 'ctrl+alt+p',
    rewrite_hotkey: 'ctrl+alt+r',
  }
  mocks.imeDefaults = { ...mocks.ime, app_blocklist: [], remember_pause: false }
  mocks.imeAvailable = true
  mocks.imeConfigSet.mockReset().mockImplementation((items: { key: string; value: unknown }[]) => {
    for (const i of items) mocks.ime[i.key.replace('moodflow.', '')] = i.value
    return ok({ needs_restart: false, applied: items.length, skipped: [] })
  })
  mocks.hotkeyStatus.mockReset().mockResolvedValue([])
  mocks.baselineReset.mockReset().mockImplementation(() => ok())
  mocks.researchClear.mockReset().mockImplementation(() => ok(4))
  mocks.openWindow.mockReset().mockImplementation(() => ok())
  mocks.emit.mockReset().mockResolvedValue(undefined)
  mocks.pets = [{ id: 'qingqing', name: '晴晴', description: 'Moodflow 的小精灵', builtin: true, version: 1 }]
  mocks.found = []
  mocks.petImport.mockReset()
  mocks.petRemove.mockReset().mockImplementation(() => ok())
})

describe('Hub 快捷键与研究编号的格式（与后端一致）', () => {
  it('hubHotkeyOk：至少一个 Ctrl / Alt / Win，主键是字母、数字或 F1–F24', () => {
    for (const good of ['ctrl+alt+q', 'alt+1', 'win+f24', 'ctrl+shift+f1'])
      expect(hubHotkeyOk(good), good).toBe(true)
    for (const bad of ['shift+a', 'f5', 'ctrl+.', 'ctrl+space', 'alt+f25', 'ctrl+f0', 'ctrl+alt+enter'])
      expect(hubHotkeyOk(bad), bad).toBe(false)
  })

  it('validResearchId：空串或 16 字以内的字母、数字、- 和 _', () => {
    for (const good of ['', 'P07', 'study_2026-a', 'a'.repeat(16)])
      expect(validResearchId(good), good).toBe(true)
    for (const bad of ['张三', 'P 07', 'a'.repeat(17), 'x/y']) expect(validResearchId(bad), bad).toBe(false)
  })
})

describe('设置中心 · 常规（FR-SET-03）', () => {
  it('小组件开关点了就存，底部提示“已保存”', async () => {
    const w = await open(GeneralSection)
    const boxes = group(w, 'widget').findAll('input[type=checkbox]')
    expect(boxes.map((b) => (b.element as HTMLInputElement).checked)).toEqual([true, true, false])
    await boxes[2]!.setValue(true)
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('widget.autohide', true)
    expect(w.find('.s-saved').text()).toBe(t('settings.saved'))
  })

  it('不透明度的范围取自 schema，按百分比显示，存成小数', async () => {
    const w = await open(GeneralSection)
    const range = group(w, 'widget').find('input[type=range]')
    expect(range.attributes('min')).toBe('70')
    expect(range.attributes('max')).toBe('100')
    await range.setValue('85')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('widget.opacity', 0.85)
  })

  it('登录时后台启动：读系统启动项，开关后显示实际状态；恢复默认会关掉；读不到时不显示（FR-OPS-03）', async () => {
    let w = await open(GeneralSection)
    const sw = group(w, 'login-start').find('input')
    expect((sw.element as HTMLInputElement).checked).toBe(false)
    await sw.setValue(true)
    await flushPromises()
    expect(mocks.autostartSet).toHaveBeenCalledWith(true)
    expect(group(w, 'login-start').text()).toContain(t('settings.on'))
    w.unmount()

    mocks.autostart = true
    mocks.autostartSet.mockClear()
    w = await open(GeneralSection)
    await w.find('.s-head button').trigger('click')
    await w.find('.s-confirm .s-danger').trigger('click')
    await flushPromises()
    expect(mocks.autostartSet).toHaveBeenCalledWith(false)
    w.unmount()

    mocks.autostart = null
    w = await open(GeneralSection)
    expect(group(w, 'login-start').exists()).toBe(false)
  })

  it('晴天收集开关默认开，关掉就保存（FR-REV-04）', async () => {
    const w = await open(GeneralSection)
    const sw = group(w, 'collection').find('input')
    expect((sw.element as HTMLInputElement).checked).toBe(true)
    await sw.setValue(false)
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('collection.enabled', false)
  })

  it('Moodflow 功能总开关写输入法配置；输入法没在运行时不可点并说明', async () => {
    let w = await open(GeneralSection)
    const sw = group(w, 'enabled').find('input')
    await sw.setValue(false)
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'moodflow.enabled', value: false }])
    w.unmount()

    mocks.imeAvailable = false
    w = await open(GeneralSection)
    expect(group(w, 'enabled').find('input').attributes('disabled')).toBeDefined()
    expect(group(w, 'enabled').text()).toContain(t('settings.sense.ime_needed'))
  })

  it('快捷键：输入法能用、Hub 不收的组合先拦下；合格的存规范写法；清除存空串', async () => {
    const w = await open(GeneralSection)
    await record(w, 'hk-chat', { code: 'Period', ctrlKey: true })
    expect(w.find('[data-key="hotkey.chat"] [role=alert]').text()).toBe(t('settings.general.hotkey_invalid'))
    expect(mocks.settingsSet).not.toHaveBeenCalled()

    await record(w, 'hk-chat', { code: 'KeyJ', ctrlKey: true, altKey: true })
    expect(mocks.settingsSet).toHaveBeenCalledWith('hotkey.chat', 'ctrl+alt+j')

    await w.find('[data-key="hotkey.widget"] .clear').trigger('click')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenLastCalledWith('hotkey.widget', '')
  })

  it('快捷键和输入法的无痕模式快捷键重复时提示是哪一项，不保存', async () => {
    const w = await open(GeneralSection)
    await record(w, 'hk-dashboard', { code: 'KeyP', ctrlKey: true, altKey: true })
    expect(w.find('[data-key="hotkey.dashboard"] [role=alert]').text()).toBe(
      t('ime.hotkey.taken', { name: t('ime.field.moodflow_pause_hotkey') }),
    )
    expect(mocks.settingsSet).not.toHaveBeenCalled()
  })

  it('注册不上（被别的软件占用）的快捷键显示 error.hotkey_conflict', async () => {
    mocks.hotkeyStatus.mockResolvedValue([{ action: 'widget', keys: 'ctrl+alt+w', conflict: true }])
    const w = await open(GeneralSection)
    expect(w.find('[data-key="hotkey.widget"] [role=alert]').text()).toBe(t('error.hotkey_conflict'))
    expect(w.find('[data-key="hotkey.chat"] [role=alert]').exists()).toBe(false)
  })

  it('恢复默认：先确认；确认后只写和出厂值不同的键', async () => {
    mocks.values['ui.theme'] = 'dark'
    mocks.values['hotkey.chat'] = ''
    const w = await open(GeneralSection)
    await w.find('.s-head button').trigger('click')
    expect(w.find('.s-confirm').text()).toContain(t('settings.restore_confirm'))
    expect(mocks.settingsSet).not.toHaveBeenCalled()
    await w.find('.s-confirm .s-danger').trigger('click')
    await flushPromises()
    expect(mocks.settingsSet.mock.calls).toEqual([
      ['ui.theme', 'system'],
      ['hotkey.chat', 'ctrl+alt+q'],
    ])
    expect(w.find('.s-saved').text()).toBe(t('settings.restored'))
    // 总开关不随恢复默认改
    expect(mocks.imeConfigSet).not.toHaveBeenCalled()
  })
})

describe('设置中心 · 常规 · 桌宠（ADR 0044）', () => {
  it('列出晴晴和导入的宠物，选了就存 pet.id；导入的能删，晴晴不能删', async () => {
    mocks.pets.push({ id: 'boba', name: 'Boba', description: '', builtin: false, version: 1 })
    const w = await open(GeneralSection)
    const pets = group(w, 'pet').findAll('.pet')
    expect(pets.map((p) => p.attributes('data-pet'))).toEqual(['qingqing', 'boba'])
    expect((pets[0]!.find('input').element as HTMLInputElement).checked).toBe(true)
    expect(pets[0]!.find('button').exists()).toBe(false)
    await pets[1]!.find('input').setValue(true)
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('pet.id', 'boba')
    await pets[1]!.find('button').trigger('click')
    await flushPromises()
    expect(mocks.petRemove).toHaveBeenCalledWith('boba')
  })

  it('Codex / petdex 里装过的宠物一键导入，导入后换上它', async () => {
    mocks.found = [
      { path: '/home/u/.codex/pets/mochi', id: 'mochi', name: 'Mochi', source: 'codex', imported: false },
    ]
    mocks.petImport.mockImplementation(() =>
      ok({ id: 'mochi', name: 'Mochi', description: '', builtin: false, version: 2 }),
    )
    const w = await open(GeneralSection)
    const row = group(w, 'pet').find('[data-candidate="mochi"]')
    expect(row.text()).toContain('Codex')
    await row.find('button').trigger('click')
    await flushPromises()
    expect(mocks.petImport).toHaveBeenCalledWith('/home/u/.codex/pets/mochi')
    expect(mocks.settingsSet).toHaveBeenCalledWith('pet.id', 'mochi')
    expect(group(w, 'pet').find('.s-saved').text()).toBe(
      t('settings.general.pet_imported', { name: 'Mochi' }),
    )
  })

  it('导入失败给可读提示；大小与散步照常存', async () => {
    mocks.found = [{ path: '/x', id: 'bad', name: 'Bad', source: 'petdex', imported: false }]
    mocks.petImport.mockImplementation(() =>
      Promise.resolve({
        status: 'error',
        error: { code: 'pet.invalid', message_key: 'pet.error.bad_sheet' },
      }),
    )
    const w = await open(GeneralSection)
    await group(w, 'pet').find('[data-candidate="bad"] button').trigger('click')
    await flushPromises()
    expect(group(w, 'pet').find('[role=alert]').text()).toBe(t('pet.error.bad_sheet'))
    const sizes = group(w, 'pet').findAll('input[name=pet-size]')
    await sizes[2]!.setValue(true)
    await group(w, 'pet').find('input[role=switch]').setValue(false)
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('pet.size', 'large')
    expect(mocks.settingsSet).toHaveBeenCalledWith('pet.roam', false)
  })
})

describe('设置中心 · 感知（FR-SET-04）', () => {
  it('程序名输入框获得焦点时给出正在运行的程序当候选', async () => {
    const w = await open(SenseSection)
    const input = w.find('input[list="mf-running-apps"]')
    expect(w.findAll('#mf-running-apps option')).toHaveLength(0)
    await input.trigger('focus')
    await flushPromises()
    expect(w.findAll('#mf-running-apps option').map((o) => o.attributes('value'))).toEqual([
      'Code.exe',
      'WeChat.exe',
    ])
  })

  it('云端判断是同意 ②：撤回时如实说明已发出的收不回来', async () => {
    const w = await open(SenseSection)
    await group(w, 'cloud').find('input').setValue(false)
    await flushPromises()
    expect(mocks.consentSet).toHaveBeenCalledWith('jev_features', false)
    expect(group(w, 'cloud').find('[role=status]').text()).toBe(t('error.withdraw_notice'))
  })

  it('增强模式：勾选只先说明会发送上屏文字，点“我明白”才记同意 ⑤', async () => {
    const w = await open(SenseSection)
    await group(w, 'enhanced').find('input').setValue(true)
    expect(group(w, 'enhanced').text()).toContain(t('settings.sense.enhanced_warning'))
    expect(mocks.consentSet).not.toHaveBeenCalled()
    await group(w, 'enhanced').find('.s-confirm button').trigger('click')
    await flushPromises()
    expect(mocks.consentSet).toHaveBeenCalledWith('enhanced', true)
    expect(group(w, 'enhanced').find('.s-confirm').exists()).toBe(false)
  })

  it('不感知的程序：带路径、重复的先拦下；合格的追加后整份写回输入法配置', async () => {
    const w = await open(SenseSection)
    const g = group(w, 'moodflow.app_blocklist')
    expect(g.find('.s-items').text()).toContain('KeePass.exe')
    const input = g.find('input[type=text]')
    await input.setValue('C:\\bank\\a.exe')
    await g.find('form').trigger('submit')
    expect(g.find('[role=alert]').text()).toBe(t('settings.sense.app_invalid'))
    await input.setValue('keepass.EXE')
    await g.find('form').trigger('submit')
    expect(g.find('[role=alert]').text()).toBe(t('settings.sense.app_duplicate'))
    expect(mocks.imeConfigSet).not.toHaveBeenCalled()

    await input.setValue(' Bank.exe ')
    await g.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([
      { key: 'moodflow.app_blocklist', value: ['KeePass.exe', 'Bank.exe'] },
    ])
    expect(group(w, 'moodflow.app_blocklist').find('.s-items').text()).toContain('Bank.exe')
  })

  it('基线：显示熟悉进度；“重新熟悉”先确认再调 baseline_reset', async () => {
    const w = await open(SenseSection)
    const g = group(w, 'baseline')
    expect(g.text()).toContain(t('settings.sense.baseline_progress', { pct: 40 }))
    await g.find('button').trigger('click')
    expect(mocks.baselineReset).not.toHaveBeenCalled()
    await g.find('.s-confirm .s-danger').trigger('click')
    await flushPromises()
    expect(mocks.baselineReset).toHaveBeenCalledOnce()
    expect(g.text()).toContain(t('settings.sense.baseline_progress', { pct: 0 }))
  })

  it('恢复默认按输入法的出厂值写回名单，只写不同的项', async () => {
    const w = await open(SenseSection)
    await w.find('.s-head button').trigger('click')
    await w.find('.s-confirm .s-danger').trigger('click')
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'moodflow.app_blocklist', value: [] }])
  })

  it('输入法没在运行：名单等几项不可改，也不给恢复默认', async () => {
    mocks.imeAvailable = false
    const w = await open(SenseSection)
    expect(w.text()).toContain(t('settings.sense.ime_needed'))
    expect(group(w, 'pause').attributes('disabled')).toBeDefined()
    expect(w.find('.s-head button').exists()).toBe(false)
  })
})

describe('设置中心 · 提醒（FR-SET-06）', () => {
  it('间隔的范围取自 schema：超出时提示范围、不保存；合格的取整后保存', async () => {
    const w = await open(RestSection)
    const g = group(w, 'eye')
    const input = g.find('input[type=number]')
    expect(input.attributes('min')).toBe('15')
    expect(input.attributes('max')).toBe('60')
    await input.setValue('90')
    expect(g.find('[role=alert]').text()).toBe(t('settings.rest.interval_invalid', { min: 15, max: 60 }))
    expect(mocks.settingsSet).not.toHaveBeenCalled()
    await input.setValue('24.6')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('rest.eye.interval', 25)
  })

  it('关掉一类提醒后它的间隔不可改；深夜提醒的起始时刻用 schema 的选项', async () => {
    const w = await open(RestSection)
    await group(w, 'water').find('input[type=checkbox]').setValue(false)
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('rest.water.enabled', false)
    expect(group(w, 'water').find('input[type=number]').attributes('disabled')).toBeDefined()
    const opts = group(w, 'night')
      .findAll('option')
      .map((o) => o.text())
    expect(opts).toEqual(['22:30', '23:00', '23:30', '00:00'])
  })
})

describe('设置中心 · 提醒 · 专注时段（FR-RST-09）', () => {
  it('没设时选起止时间设定；起止相同提示不保存', async () => {
    const w = await open(RestSection)
    const g = group(w, 'focus')
    const [from, to] = g.findAll('input[type=time]')
    await from!.setValue('14:00')
    await to!.setValue('14:00')
    await g.find('form').trigger('submit')
    expect(g.find('[role=alert]').text()).toBe(t('settings.rest.focus_invalid'))
    expect(mocks.settingsSet).not.toHaveBeenCalled()
    await to!.setValue('17:30')
    await g.find('form').trigger('submit')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('rest.focus_period', ['14:00-17:30'])
  })

  it('已设时显示时段（跨午夜注明次日），可以取消', async () => {
    mocks.values['rest.focus_period'] = ['22:00-01:00']
    const w = await open(RestSection)
    const g = group(w, 'focus')
    expect(g.find('.current').text()).toBe(
      t('settings.rest.focus_current', { range: '22:00–01:00' }) + t('settings.care.quiet_next_day'),
    )
    await g.find('button').trigger('click')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('rest.focus_period', [])
  })
})

describe('设置中心 · 日程与待办（FR-SET-07）', () => {
  it('识别开关是同意 ③；提醒提前量用中文显示；每日汇总默认关', async () => {
    const w = await open(AgendaSection)
    const sw = group(w, 'recognize').find('input')
    expect((sw.element as HTMLInputElement).checked).toBe(false)
    await sw.setValue(true)
    await flushPromises()
    expect(mocks.consentSet).toHaveBeenCalledWith('schedule', true)

    const opts = group(w, 'offset')
      .findAll('option')
      .map((o) => o.text())
    expect(opts).toEqual([
      '准时提醒',
      '提前 5 分钟',
      '提前 10 分钟',
      '提前 30 分钟',
      '提前 1 小时',
      '提前 1 天',
    ])
    await group(w, 'offset').find('select').setValue('60')
    expect(mocks.settingsSet).toHaveBeenCalledWith('sch.default_offsets', '60')
    expect((group(w, 'digest').find('input').element as HTMLInputElement).checked).toBe(false)
  })

  it('“打开日程与待办”打开看板并跳到那一页', async () => {
    const w = await open(AgendaSection)
    await group(w, 'export').find('button').trigger('click')
    await flushPromises()
    expect(mocks.openWindow).toHaveBeenCalledWith('dashboard')
    expect(mocks.emit).toHaveBeenCalledWith('mf:dashboard-nav', { page: 'schedule' })
  })
})

describe('设置中心 · 温柔改写（FR-SET-10）', () => {
  it('显示今日改写次数；快捷键写输入法配置', async () => {
    const w = await open(RewriteSection)
    expect(group(w, 'consent').text()).toContain(t('settings.rewrite.usage', { used: 3, cap: 100 }))
    await record(w, 'hk-rewrite', { code: 'KeyG', ctrlKey: true, altKey: true })
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'moodflow.rewrite_hotkey', value: 'ctrl+alt+g' }])
  })

  it('起始风格与剪贴板来源写输入法配置；没配过时按“更温和”、开着剪贴板显示（ADR 0041）', async () => {
    const w = await open(RewriteSection)
    const radios = group(w, 'style').findAll<HTMLInputElement>('input[type=radio]')
    expect(radios.map((r) => r.element.checked)).toEqual([true, false, false, false])
    await radios[2]!.trigger('change')
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'moodflow.rewrite_style', value: 'concise' }])
    const clip = group(w, 'sources').find<HTMLInputElement>('input')
    expect(clip.element.checked).toBe(true)
    await clip.setValue(false)
    await flushPromises()
    expect(mocks.imeConfigSet).toHaveBeenCalledWith([{ key: 'moodflow.rewrite_clipboard', value: false }])
  })
})

describe('设置中心 · 参加研究与开发者选项（FR-DMO-03、FR-DMO-04）', () => {
  it('没填研究编号时开关不可点；编号格式不对先拦下；合格的保存', async () => {
    const w = await open(ResearchDevSection)
    const g = group(w, 'research')
    expect(g.find('input[type=checkbox]').attributes('disabled')).toBeDefined()
    const id = g.find('#research-id')
    await id.setValue('张三')
    expect(g.find('[role=alert]').text()).toBe(t('privacy.research.id_invalid'))
    await id.setValue('P07')
    await flushPromises()
    expect(mocks.settingsSet).toHaveBeenCalledWith('research.id', 'P07')
    expect(g.find('input[type=checkbox]').attributes('disabled')).toBeUndefined()
  })

  it('删除研究期间的心情记录：先确认，再调 research_clear 并说删了几条', async () => {
    const w = await open(ResearchDevSection)
    const g = group(w, 'research')
    await g.findAll('button').at(-1)!.trigger('click')
    expect(mocks.researchClear).not.toHaveBeenCalled()
    await g.find('.s-confirm .s-danger').trigger('click')
    await flushPromises()
    expect(mocks.researchClear).toHaveBeenCalledOnce()
    expect(w.find('.s-saved').text()).toBe(t('privacy.research.cleared', { n: 4 }))
  })

  it('演示模式开关存 dev.demo', async () => {
    const w = await open(ResearchDevSection)
    await group(w, 'dev').find('input').setValue(true)
    expect(mocks.settingsSet).toHaveBeenCalledWith('dev.demo', true)
  })
})
