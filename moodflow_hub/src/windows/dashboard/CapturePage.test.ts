import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import CapturePage from './CapturePage.vue'
import { t } from '@/i18n'
import { capturePoint, captureCrop } from './captureTypes'
const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  tools: vi.fn(),
  listen: vi.fn(),
  confirm: vi.fn(),
  save: vi.fn(),
  open: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: { captureRequest: mocks.call, imeTools: mocks.tools, getStatus: async () => ({ paused: false }) },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm, save: mocks.save, open: mocks.open }))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc: (path: string) => path }))
const asset = {
  id: '12345678-1234-4234-8234-123456789abc',
  extension: 'png',
  created_at: 1,
  bytes: 100,
  width: 1000,
  height: 500,
  duration_ms: 0,
  permanent: false,
  sha256: 'a'.repeat(64),
}
const emptyStorage = {
  ...Object.fromEntries(
    ['total', 'temporary', 'kept', 'library', 'trash', 'expired', 'png', 'gif', 'mp4'].map((kind) => [
      kind,
      { count: 0, bytes: 0 },
    ]),
  ),
  quota_bytes: 1024 ** 3 * 10,
}
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.confirm.mockResolvedValue(false)
  mocks.save.mockResolvedValue(null)
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'status'
        ? { supported: true, helper: true }
        : method === 'targets'
          ? {
              monitors: [{ index: 1, name: 'fixture', width: 1000, height: 500 }],
              windows: [{ hwnd: 71, pid: 17, title: 'fixture window', width: 800, height: 400 }],
              ocr_available: true,
            }
          : method === 'catalog_query'
            ? { rows: [], storage: emptyStorage, matched: { count: 0, bytes: 0 }, offset: 0, limit: 24 }
            : method === 'start' || method === 'asset'
              ? { asset, path: 'fixture.png' }
              : method === 'ocr'
                ? { text: '合成取字结果' }
                : {},
    ),
  )
})
afterEach(() => vi.useRealTimers())
const click = async (w: ReturnType<typeof mount>, label: string) => {
  await w
    .findAll('button')
    .find((b) => b.text() === label)!
    .trigger('click')
  await flushPromises()
}
const render = () =>
  mount(CapturePage, { global: { stubs: { CaptureEditor: { template: '<div class="capture-editor" />' } } } })
it('像素映射不受负坐标或 DPI 缩放影响，固定比例裁剪不越界', () => {
  expect(capturePoint(-1420, 550, { left: -1920, top: 300, width: 1000, height: 500 }, 2000, 1000)).toEqual({
    x: 1000,
    y: 500,
  })
  expect(capturePoint(5000, -100, { left: 0, top: 0, width: 500, height: 250 }, 1000, 500)).toEqual({
    x: 1000,
    y: 0,
  })
  expect(captureCrop({ x: 1800, y: 900 }, { x: 200, y: 100 }, 2000, 1000, 1)).toEqual({
    x: 900,
    y: 0,
    width: 900,
    height: 900,
  })
})
it('只在明确点击后采集和取字，审核后才暂存 Buffer', async () => {
  const w = render()
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'start')).toBe(false)
  await click(w, t('capture.shot'))
  const start = mocks.call.mock.calls.find(([method]) => method === 'start')!
  expect(JSON.parse(start[1] as string).request).toMatchObject({
    operation: 'screenshot',
    target: { kind: 'monitor', index: 1 },
    crop: null,
    audio: false,
  })
  expect(mocks.tools).not.toHaveBeenCalled()
  await click(w, t('capture.ocr'))
  expect(w.find('textarea').element).toHaveProperty('value', '合成取字结果')
  expect(mocks.tools).not.toHaveBeenCalled()
  await click(w, t('capture.stage'))
  expect(mocks.tools).toHaveBeenCalledWith('buffer.stage', JSON.stringify({ text: '合成取字结果' }))
  w.unmount()
})
it('暂停终止任务并清空画面，迟到结果不能恢复内容', async () => {
  let finish: ((result: string) => void) | undefined
  const original = mocks.call.getMockImplementation()!
  mocks.call.mockImplementation((method: string, params: string) =>
    method === 'start'
      ? new Promise<string>((resolve) => {
          finish = resolve
        })
      : original(method, params),
  )
  const w = render()
  await flushPromises()
  await w
    .findAll('button')
    .find((b) => b.text() === t('capture.shot'))!
    .trigger('click')
  await flushPromises()
  const listener = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  listener({ payload: { paused: true } })
  finish!(JSON.stringify({ asset, path: 'late-private-fixture.png' }))
  await flushPromises()
  expect(w.find('.capture-editor').exists()).toBe(false)
  expect(w.find('textarea').exists()).toBe(false)
  expect(mocks.call).toHaveBeenCalledWith('abort', '{}')
  w.unmount()
})

it('保存对话框跨越暂停和恢复后不得导出旧资产', async () => {
  let finish: ((path: string) => void) | undefined
  mocks.save.mockImplementation(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve
      }),
  )
  const w = render()
  await flushPromises()
  await click(w, t('capture.shot'))
  await click(w, t('capture.export'))
  const listener = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  listener({ payload: { paused: true } })
  await flushPromises()
  listener({ payload: { paused: false } })
  await flushPromises()
  finish!('selected-before-pause.png')
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'export')).toBe(false)
  w.unmount()
})
