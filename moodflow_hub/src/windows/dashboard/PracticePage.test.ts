import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { t } from '@/i18n'
import PracticePage from './PracticePage.vue'

const api = vi.hoisted(() => ({
  practiceScore: vi.fn(),
  practiceHistoryList: vi.fn(),
  practiceHistorySave: vi.fn(),
  practiceHistoryDelete: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: api,
  unwrap: async (p: Promise<unknown>) => p,
}))

const score = {
  elapsed_ms: 30_000,
  target_chars: 1,
  typed_chars: 1,
  distance: 0,
  final_accuracy: 1,
  process_accuracy: 1,
  effective_cpm: 2,
}
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['performance', 'setInterval', 'clearInterval'] })
  vi.resetAllMocks()
  api.practiceScore.mockResolvedValue(score)
  api.practiceHistoryList.mockResolvedValue([])
})
afterEach(() => vi.useRealTimers())

const button = (w: ReturnType<typeof mount>, key: Parameters<typeof t>[0]) =>
  w.findAll('button').find((b) => b.text() === t(key))!

it('限时结束只记录已上屏字符，主动保存才提交数字成绩', async () => {
  const w = mount(PracticePage)
  await w.findAll('select')[1]!.setValue('timed30')
  const input = w.get<HTMLTextAreaElement>('textarea.input')
  await input.trigger('compositionstart')
  input.element.value = 'zao'
  await input.trigger('input')
  await vi.advanceTimersByTimeAsync(31_000)
  expect(api.practiceScore).not.toHaveBeenCalled()
  input.element.value = '早'
  await input.trigger('compositionend')
  await input.trigger('compositionstart')
  input.element.value = '早shang'
  await input.trigger('input')
  await vi.advanceTimersByTimeAsync(30_000)
  await flushPromises()
  expect(api.practiceScore).toHaveBeenCalledExactlyOnceWith('早', [{ t_ms: 0, text: '早' }], 30_000)
  expect(input.element.value).toBe('早')
  await input.trigger('compositionend')
  expect(api.practiceScore).toHaveBeenCalledTimes(1)
  expect(api.practiceHistorySave).not.toHaveBeenCalled()
  api.practiceHistorySave.mockImplementation(async (data) => ({ date: '2026-10-03T04:05:06Z', data }))
  await button(w, 'dashboard.practice.history_save').trigger('click')
  await flushPromises()
  expect(api.practiceHistorySave).toHaveBeenCalledWith({
    id: expect.any(String),
    mode: 'timed30',
    category: 'chinese',
    score,
  })
  w.unmount()
})

it('重开一轮时丢弃迟到的成绩，并保留当前范文和模式', async () => {
  let resolve: ((value: typeof score) => void) | undefined
  api.practiceScore.mockImplementation(
    () =>
      new Promise<typeof score>((done) => {
        resolve = done
      }),
  )
  const w = mount(PracticePage)
  await w.findAll('select')[0]!.setValue('english')
  const before = w.get('blockquote').text()
  const input = w.get<HTMLTextAreaElement>('textarea.input')
  await input.setValue('The')
  await button(w, 'dashboard.practice.finish').trigger('click')
  expect(api.practiceScore).toHaveBeenCalledTimes(1)
  await button(w, 'dashboard.practice.again').trigger('click')
  resolve!(score)
  await flushPromises()
  expect(w.text()).not.toContain(t('dashboard.practice.result'))
  expect(w.get('blockquote').text()).toBe(before)
  expect(input.element.value).toBe('')
  w.unmount()
})
