import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import TranslationPage from './TranslationPage.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({ tools: vi.fn(), listen: vi.fn(), status: vi.fn() }))
vi.mock('@/api', () => ({
  commands: { imeTools: mocks.tools, getStatus: mocks.status },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (p: Promise<unknown>) => p,
}))
const response = (word: string) =>
  JSON.stringify({
    entries: [{ simplified: word, traditional: word, pinyin: 'ni3 hao3', definitions: ['hello'] }],
    segments: [],
    complete: true,
    matched: true,
    entry_count: 125394,
  })
beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
  mocks.status.mockResolvedValue({ paused: false })
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.tools.mockImplementation(async (method: string) =>
    method === 'buffer.read' ? JSON.stringify({ text: '你好' }) : response('你好'),
  )
})
afterEach(() => vi.useRealTimers())

describe('离线查词', () => {
  it('400 ms 去抖，旧查询不能覆盖新结果；仅按钮可暂存结果', async () => {
    let old: ((v: string) => void) | undefined
    mocks.tools.mockImplementation((method: string, raw: string) => {
      if (method !== 'translate.lookup') return Promise.resolve('{}')
      return JSON.parse(raw).query === '旧词'
        ? new Promise<string>((resolve) => {
            old = resolve
          })
        : Promise.resolve(response('新词'))
    })
    const w = mount(TranslationPage)
    await flushPromises()
    await w.find('textarea').setValue('旧词')
    await vi.advanceTimersByTimeAsync(399)
    expect(mocks.tools).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    await w.find('textarea').setValue('新词')
    await vi.advanceTimersByTimeAsync(400)
    await flushPromises()
    old!(response('旧词'))
    await flushPromises()
    expect(w.find('h2').text()).toBe('新词')
    expect(mocks.tools.mock.calls.every(([method]) => method === 'translate.lookup')).toBe(true)
    await w
      .findAll('button')
      .find((b) => b.text() === t('translate.stage_zh'))!
      .trigger('click')
    await flushPromises()
    expect(mocks.tools).toHaveBeenLastCalledWith('buffer.stage', JSON.stringify({ text: '新词' }))
    w.unmount()
  })

  it('主动读取 Buffer；暂停立即隐藏正文、取消后续查询', async () => {
    const w = mount(TranslationPage)
    await flushPromises()
    expect(mocks.tools).not.toHaveBeenCalled()
    await w.find('button').trigger('click')
    await flushPromises()
    expect(mocks.tools).toHaveBeenCalledWith('buffer.read', '{}')
    await vi.advanceTimersByTimeAsync(400)
    await flushPromises()
    expect(w.text()).toContain('你好')
    const listener = mocks.listen.mock.calls[0]![0] as (e: { payload: { paused: boolean } }) => void
    listener({ payload: { paused: true } })
    await flushPromises()
    expect(w.find('textarea').exists()).toBe(false)
    expect(w.text()).not.toContain('你好')
    const calls = mocks.tools.mock.calls.length
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.tools).toHaveBeenCalledTimes(calls)
    w.unmount()
  })
})
