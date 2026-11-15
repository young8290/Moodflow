import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { t } from '@/i18n'
import ChordPractice from './ChordPractice.vue'
import { startDrill, type ChordOutcome, type ChordProfile, type ChordStroke } from './chordPractice'

const { imeTools } = vi.hoisted(() => ({ imeTools: vi.fn() }))
vi.mock('@/api', () => ({ commands: { imeTools }, unwrap: async (promise: Promise<string>) => promise }))

const profile: ChordProfile = {
  version: 1,
  name: '未保存方案',
  left_keys: ['a', 's'],
  right_keys: ['j'],
  interval_ms: 300,
  mappings: [
    { keys: ['a', 'j'], output: 'ni', kind: 'syllable' },
    { keys: ['s'], output: 'ni', kind: 'syllable' },
  ],
}
const details = { physical: { a: 'a', s: 's', j: 'j' }, intervalMs: 300, maxStrokes: 128 }
const outcome = (keys: string[], output: string | null, idle = false): ChordOutcome => ({
  keys,
  output,
  kind: output === null ? null : 'syllable',
  atMs: 340,
  idle,
})

function press(box: DOMWrapper<Element>, type: 'keydown' | 'keyup', code: string, extras = {}): void {
  box.element.dispatchEvent(new KeyboardEvent(type, { bubbles: true, cancelable: true, code, ...extras }))
}

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(300)
  await flushPromises()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  imeTools.mockReset()
  imeTools.mockImplementation(async (_method: string, payload: string) => {
    const { strokes } = JSON.parse(payload) as { strokes: ChordStroke[] }
    const keys = [...new Set(strokes.map(({ key }) => key))].sort()
    const result = keys.join('') === 'aj' || keys.join('') === 's' ? 'ni' : null
    return JSON.stringify({ ...details, outcomes: keys.length ? [outcome(keys, result)] : [] })
  })
})
afterEach(() => vi.useRealTimers())

describe('并击试练台', () => {
  it('用未保存的草稿重放按下与松开，滤掉重复键和快捷键，只调用只读方法', async () => {
    const wrapper = mount(ChordPractice, { props: { profile } })
    await flushPromises()
    const box = wrapper.get('.capture')
    press(box, 'keydown', 'KeyA')
    await flushPromises()
    expect(box.findAll('.held').map((el) => el.text())).toEqual(['A'])
    press(box, 'keydown', 'KeyA', { repeat: true })
    press(box, 'keydown', 'KeyS', { ctrlKey: true })
    press(box, 'keyup', 'KeyA')
    await vi.advanceTimersByTimeAsync(100)
    press(box, 'keydown', 'KeyJ')
    press(box, 'keyup', 'KeyJ')
    expect(imeTools).toHaveBeenCalledTimes(1)
    await settle()
    expect(imeTools.mock.calls.map(([method]) => method)).toEqual(['chord.rehearse', 'chord.rehearse'])
    const payload = JSON.parse(imeTools.mock.calls[1][1] as string)
    expect(JSON.parse(payload.text)).toEqual(profile)
    expect(payload.strokes.map(({ key, down }: ChordStroke) => [key, down])).toEqual([
      ['a', true],
      ['a', false],
      ['j', true],
      ['j', false],
    ])
    expect(box.findAll('.held')).toHaveLength(0)
    expect(wrapper.get('.history').text()).toContain('A + J')
    expect(wrapper.get('.history').text()).toContain('ni')
    wrapper.unmount()
  })

  it('双手练习不接受同音单手组合，重试通过后首试统计仍为零', async () => {
    const wrapper = mount(ChordPractice, { props: { profile } })
    await flushPromises()
    await wrapper
      .findAll('button')
      .find((b) => b.text() === t('rehearsal.drill'))!
      .trigger('click')
    await wrapper.get('select').setValue('both')
    const box = wrapper.get('.capture')
    press(box, 'keydown', 'KeyS')
    press(box, 'keyup', 'KeyS')
    await settle()
    expect(wrapper.text()).toContain(t('rehearsal.retry'))
    expect(wrapper.find('.summary').exists()).toBe(false)
    press(box, 'keydown', 'KeyA')
    press(box, 'keydown', 'KeyJ')
    press(box, 'keyup', 'KeyJ')
    press(box, 'keyup', 'KeyA')
    await settle()
    expect(
      wrapper
        .get('.summary')
        .findAll('dd')
        .map((el) => el.text()),
    ).toEqual(['1/1', '0/1', '2'])
    wrapper.unmount()
  })

  it('失焦取消未完成试打；编辑后多个核对请求只接收最新草稿', async () => {
    const wrapper = mount(ChordPractice, { props: { profile } })
    await flushPromises()
    const box = wrapper.get('.capture')
    press(box, 'keydown', 'KeyA')
    press(box, 'keyup', 'KeyA')
    await box.trigger('blur')
    await settle()
    expect(imeTools).toHaveBeenCalledTimes(1)
    expect(wrapper.text()).toContain(t('rehearsal.interrupted'))

    let oldResponse: (value: string) => void = () => {}
    imeTools.mockImplementationOnce(() => new Promise<string>((resolve) => (oldResponse = resolve)))
    await wrapper.setProps({ profile: { ...profile, name: '草稿 B' } })
    imeTools.mockResolvedValueOnce(JSON.stringify({ error: 'empty_profile' }))
    await wrapper.setProps({ profile: { ...profile, name: '草稿 C', mappings: [] } })
    await flushPromises()
    oldResponse(JSON.stringify({ ...details, outcomes: [] }))
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toBe(t('rehearsal.empty_profile'))
    expect(wrapper.find('.capture').exists()).toBe(false)
    wrapper.unmount()
  })

  it('下一轮移动到前一轮未显示的组合', () => {
    const pool = Array.from({ length: 11 }, (_, i) => ({ ...profile.mappings[0]!, output: String(i) }))
    expect(startDrill(pool, 0).targets.map(({ output }) => output)).toEqual([
      '0',
      '1',
      '2',
      '3',
      '4',
      '5',
      '6',
      '7',
    ])
    expect(startDrill(pool, 1).targets.map(({ output }) => output)).toEqual([
      '8',
      '9',
      '10',
      '0',
      '1',
      '2',
      '3',
      '4',
    ])
  })
})
