import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, expect, it, vi } from 'vitest'
import { t, type CopyKey } from '@/i18n'
import MappingPage from './MappingPage.vue'

const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  listen: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: { imeTools: mocks.tools, getStatus: async () => ({ paused: false }) },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: mocks.save, confirm: mocks.confirm }))

const stored = { word: '完成', added: ['✔'], hidden: [] }
const preview = {
  builtin: ['✔️', '☑'],
  effective: ['✔', '☑'],
  enabled: true,
  scope: 'exact',
  show_as: 'after',
}

beforeEach(() => {
  vi.resetAllMocks()
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.tools.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'mapping.list'
        ? { rows: [stored], total: 1, offset: 0, limit: 24 }
        : method === 'mapping.get'
          ? { row: stored, preview }
          : method === 'mapping.preview'
            ? { ...preview, effective: ['🌙', '✔'] }
            : {},
    ),
  )
})
function render() {
  return mount(MappingPage)
}
async function click(w: ReturnType<typeof render>, key: CopyKey) {
  const button = w.findAll('button').find((b) => b.text() === t(key))
  expect(button).toBeDefined()
  await button!.trigger('click')
  await flushPromises()
}

it('validates FE0F duplicates and saves the ordered draft against the originally loaded row', async () => {
  const w = render()
  await flushPromises()
  await w.get('.rows button').trigger('click')
  await flushPromises()

  const symbol = w.findAll('input').at(-1)!
  await symbol.setValue('✔️')
  await w.findAll('form').at(-1)!.trigger('submit')
  await flushPromises()
  expect(w.text()).toContain(t('mapping.duplicate'))
  await symbol.setValue('🌙')
  await w.findAll('form').at(-1)!.trigger('submit')
  await flushPromises()
  const byLabel = (label: string) => w.findAll('button').find((b) => b.attributes('aria-label') === label)!
  await byLabel(t('mapping.move_up_for', { item: '🌙' })).trigger('click')
  await byLabel(t('mapping.hide_for', { item: '☑' })).trigger('click')
  expect(mocks.tools.mock.calls.some(([method]) => method === 'mapping.apply')).toBe(false)
  await click(w, 'mapping.preview')
  expect(w.get('.effective').text()).toContain('🌙')
  await click(w, 'mapping.save')
  expect(mocks.tools).toHaveBeenCalledWith(
    'mapping.apply',
    JSON.stringify({
      edits: [{ before: stored, after: { word: '完成', added: ['🌙', '✔'], hidden: ['☑'] } }],
    }),
  )
  w.unmount()
})

it('requires an import preview and invalidates it after changing the conflict policy', async () => {
  const prior = mocks.tools.getMockImplementation()!
  const edit = { before: stored, after: { word: '完成', added: ['🌙'], hidden: [] } }
  mocks.tools.mockImplementation(async (method, params) =>
    method === 'mapping.importPreview'
      ? JSON.stringify({
          edits: [edit],
          new_count: 0,
          conflict_count: 1,
          skipped_count: 0,
          unchanged_count: 0,
        })
      : prior(method, params),
  )
  const w = render()
  await flushPromises()
  await click(w, 'mapping.import')
  await w.get('textarea').setValue('{"word":"完成","added":["🌙"]}')
  await click(w, 'mapping.import_preview')
  expect(w.find('tbody tr').text()).toContain('🌙')
  expect(mocks.tools.mock.calls.some(([method]) => method === 'mapping.apply')).toBe(false)
  await w.get('select').setValue('replace')
  expect(w.find('tbody').exists()).toBe(false)
  await click(w, 'mapping.import_preview')
  await click(w, 'mapping.import_apply')
  expect(mocks.tools).toHaveBeenCalledWith('mapping.apply', JSON.stringify({ edits: [edit] }))
  w.unmount()
})

it('discards late private preview results after pause', async () => {
  let resolve: ((value: string) => void) | undefined
  const previous = mocks.tools.getMockImplementation()!
  mocks.tools.mockImplementation((method, params) =>
    method === 'mapping.get'
      ? new Promise<string>((done) => {
          resolve = done
        })
      : previous(method, params),
  )
  const w = render()
  await flushPromises()
  await w.get('.rows button').trigger('click')
  const listener = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  listener({ payload: { paused: true } })
  await flushPromises()
  resolve!(JSON.stringify({ row: { word: '不应可见' }, preview }))
  await flushPromises()
  expect(w.text()).toBe(t('library.privacy'))
  expect(w.text()).not.toContain('不应可见')
  w.unmount()
})
