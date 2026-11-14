import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { t } from '@/i18n'
import CaptureCatalog from './CaptureCatalog.vue'
import type { CaptureBatchEntry, CaptureBatchReport, CaptureCatalogPage, CaptureRow } from './captureTypes'

const dialog = vi.hoisted(() => ({ open: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => dialog)

const usage = { count: 2, bytes: 800 }
const row = (id: string, name: string, revision = 'a'.repeat(64)): CaptureRow => ({
  asset: {
    id,
    name,
    extension: 'png',
    bytes: 400,
    created_at: 1,
    width: 20,
    height: 10,
    duration_ms: 0,
    permanent: false,
    sha256: 'f'.repeat(64),
  },
  revision,
  scope: 'temporary',
  in_use: false,
})
const first = row('abc12345-0000-4000-8000-000000000001', '第一份')
const second = row('def12345-0000-4000-8000-000000000002', '第二份', 'b'.repeat(64))
const item = (r: CaptureRow) => ({ id: r.asset.id, revision: r.revision })
const entry = (r: CaptureRow, status: CaptureBatchEntry['status']): CaptureBatchEntry => ({
  ...item(r),
  bytes: r.asset.bytes,
  status,
})

function page(rows: CaptureRow[], offset = 0): CaptureCatalogPage {
  return {
    rows,
    offset,
    limit: 1,
    matched: usage,
    storage: {
      total: usage,
      temporary: usage,
      kept: usage,
      library: usage,
      trash: usage,
      expired: usage,
      png: usage,
      gif: usage,
      mp4: usage,
      quota_bytes: 1024 ** 3 * 10,
    },
  }
}
function report(entries: CaptureBatchEntry[]): CaptureBatchReport {
  const ready = entries.filter((e) => e.status === 'ready')
  const done = entries.filter((e) => e.status === 'done')
  return {
    entries,
    ready: ready.length,
    done: done.length,
    ready_bytes: ready.reduce((sum, e) => sum + e.bytes, 0),
    done_bytes: done.reduce((sum, e) => sum + e.bytes, 0),
  }
}
const request = vi.fn()
function render() {
  return mount(CaptureCatalog, { props: { request, busy: false, refresh: 0 } })
}
async function click(wrapper: ReturnType<typeof render>, label: string) {
  const button = wrapper.findAll('button').find((b) => b.text() === label)
  expect(button?.exists()).toBe(true)
  await button!.trigger('click')
  await flushPromises()
}

beforeEach(() => {
  vi.useFakeTimers()
  request.mockReset()
  dialog.open.mockReset()
})
afterEach(() => vi.useRealTimers())

it('retains explicit selections across pages and sends only reviewed revisions for apply', async () => {
  request.mockImplementation(async (method, params) => {
    if (method === 'catalog_query') return params.offset === 0 ? page([first]) : page([second], 1)
    if (method === 'catalog_preview') return report([entry(first, 'in_use'), entry(second, 'ready')])
    return report([entry(second, 'stale')])
  })
  const w = render()
  await flushPromises()
  await click(w, t('capture.catalog_select_page'))
  await click(w, t('capture.catalog_next'))
  await click(w, t('capture.catalog_select_page'))
  await w.get(`[aria-label="${t('capture.catalog_action')}"]`).setValue('trash')
  await click(w, t('capture.catalog_review'))
  expect(request).toHaveBeenCalledWith('catalog_preview', {
    operation: 'trash',
    items: [item(first), item(second)],
  })
  expect(request.mock.calls.some(([method]) => method === 'catalog_apply')).toBe(false)
  expect(w.text()).toContain(t('capture.in_use'))
  await click(w, t('capture.catalog_confirm'))
  expect(request).toHaveBeenCalledWith('catalog_apply', { operation: 'trash', items: [item(second)] })
  expect(w.text()).toContain(t('capture.catalog_stale'))
  expect(w.text()).toContain(t('capture.in_use'))
  w.unmount()
})

it('ignores late query results, coalesces search input, and resets paging when the filter changes', async () => {
  let late: ((p: CaptureCatalogPage) => void) | undefined
  request.mockImplementation(async (method, params) => {
    expect(method).toBe('catalog_query')
    if (params.search === '第一') return new Promise<CaptureCatalogPage>((resolve) => (late = resolve))
    return params.search ? page([second]) : page([first])
  })
  const w = render()
  await flushPromises()
  const input = w.get('input[type="search"]')
  await input.setValue('第')
  await input.setValue('第一')
  await vi.advanceTimersByTimeAsync(200)
  expect(request).toHaveBeenCalledTimes(2)
  await input.setValue('第二')
  await vi.advanceTimersByTimeAsync(200)
  await flushPromises()
  late!(page([first]))
  await flushPromises()
  expect(w.text()).toContain('第二份')
  expect(w.text()).not.toContain('第一份')
  await w.get(`[aria-label="${t('capture.catalog_format')}"]`).setValue('mp4')
  await flushPromises()
  expect(request).toHaveBeenLastCalledWith(
    'catalog_query',
    expect.objectContaining({ offset: 0, search: '第二', format: 'mp4' }),
  )
  w.unmount()
})

it('does not publish an export when the destination dialog resolves after leaving the page', async () => {
  request.mockImplementation(async (method) =>
    method === 'catalog_query' ? page([first]) : report([entry(first, 'ready')]),
  )
  let finish: ((path: string) => void) | undefined
  dialog.open.mockImplementation(() => new Promise<string>((resolve) => (finish = resolve)))
  const w = render()
  await flushPromises()
  await click(w, t('capture.catalog_select_page'))
  await w.get(`[aria-label="${t('capture.catalog_action')}"]`).setValue('export')
  await click(w, t('capture.catalog_review'))
  await click(w, t('capture.catalog_pick_destination'))
  expect(dialog.open).toHaveBeenCalledWith({ directory: true, multiple: false })
  w.unmount()
  finish!('C:/synthetic')
  await flushPromises()
  expect(request.mock.calls.some(([method]) => method === 'catalog_apply')).toBe(false)
})
