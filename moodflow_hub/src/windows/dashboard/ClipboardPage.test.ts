import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ClipboardPage from './ClipboardPage.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({ tools: vi.fn(), listen: vi.fn(), status: vi.fn() }))
vi.mock('@/api', () => ({
  commands: { imeTools: mocks.tools, getStatus: mocks.status },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (value: Promise<unknown>) => value,
}))
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.status.mockResolvedValue({ paused: false })
  mocks.tools.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'clipboard.list'
        ? {
            entries: [
              { id: 7, text: '例子正文', pinned: false, kind: 'text', copied_at: 1 },
              { id: 8, text: 'https://example.org', pinned: true, kind: 'link', copied_at: 2 },
            ],
          }
        : { changed: true },
    ),
  )
})
afterEach(() => vi.useRealTimers())
it('搜索和选择不执行复制或投递，显式暂存按条目 id 操作', async () => {
  const w = mount(ClipboardPage)
  await flushPromises()
  expect(mocks.tools).toHaveBeenCalledWith('clipboard.list', '{}')
  await w.find('input').setValue('例子')
  expect(w.findAll('.item')).toHaveLength(1)
  await w.find('.item').trigger('click')
  expect(w.find('pre').text()).toBe('例子正文')
  expect(mocks.tools.mock.calls.every(([method]) => method === 'clipboard.list')).toBe(true)
  await w
    .findAll('button')
    .find((b) => b.text() === t('clipboard.stage'))!
    .trigger('click')
  await flushPromises()
  expect(mocks.tools).toHaveBeenCalledWith(
    'clipboard.action',
    JSON.stringify({ action: 'stage', id: 7, pinned: true }),
  )
  expect(w.text()).toContain(t('library.staged'))
  w.unmount()
})
it('暂停立即清除正文，丢弃到达过晚的历史，取消删除不写入', async () => {
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
  const w = mount(ClipboardPage)
  await flushPromises()
  await w.find('.item').trigger('click')
  await w
    .findAll('button')
    .find((b) => b.text() === t('clipboard.remove'))!
    .trigger('click')
  expect(mocks.tools.mock.calls.every(([method]) => method === 'clipboard.list')).toBe(true)
  let finish: ((value: string) => void) | undefined
  mocks.tools.mockReturnValue(
    new Promise<string>((resolve) => {
      finish = resolve
    }),
  )
  await vi.advanceTimersByTimeAsync(3000)
  const receive = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  receive({ payload: { paused: true } })
  finish!(JSON.stringify({ entries: [{ id: 99, text: '迟到正文' }] }))
  await flushPromises()
  expect(w.find('pre').exists()).toBe(false)
  expect(w.text()).not.toContain('正文')
  const calls = mocks.tools.mock.calls.length
  await vi.advanceTimersByTimeAsync(6000)
  expect(mocks.tools).toHaveBeenCalledTimes(calls)
  w.unmount()
  confirm.mockRestore()
})
