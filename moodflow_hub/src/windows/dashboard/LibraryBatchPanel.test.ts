import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import LibraryBatchPanel from './LibraryBatchPanel.vue'
import { t } from '@/i18n'
const mocks = vi.hoisted(() => ({ request: vi.fn(), open: vi.fn(), save: vi.fn(), confirm: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, save: mocks.save, confirm: mocks.confirm }))
vi.mock('@/api', () => ({
  commands: { libraryRequest: mocks.request },
  unwrap: async (p: Promise<unknown>) => p,
}))
const preview = {
  sources: [{ path: 'fixture.mflibrary', digest: 'source-hash' }],
  bytes: 100,
  items: [
    {
      index: 0,
      source: 0,
      id: 'new',
      title: '新笔记',
      kind: 'note',
      tags: [],
      status: 'new',
      expected_revision: null,
    },
    {
      index: 1,
      source: 0,
      id: 'old',
      title: '已有笔记',
      kind: 'note',
      tags: [],
      status: 'conflict',
      expected_revision: 'disk-1',
    },
    {
      index: 2,
      source: 0,
      id: 'deleted',
      title: '删除的笔记',
      kind: 'note',
      tags: [],
      status: 'deleted',
      expected_revision: null,
    },
  ],
}
const entries = [{ id: 'old', title: '已有笔记', revision: 'disk-1' }]
function mountPanel(beforeChange = vi.fn().mockResolvedValue(true)) {
  return mount(LibraryBatchPanel, { props: { items: entries, disabled: false, beforeChange } })
}
async function click(w: ReturnType<typeof mountPanel>, label: string) {
  await w
    .findAll('button')
    .find((b) => b.text() === label)!
    .trigger('click')
  await flushPromises()
}
async function browse(w: ReturnType<typeof mountPanel>) {
  await click(w, t('library.batch_title'))
  await click(w, t('library.batch_browse'))
}
beforeEach(() => {
  vi.clearAllMocks()
  mocks.open.mockResolvedValue(['fixture.mflibrary'])
  mocks.save.mockResolvedValue('output.mflibrary')
  mocks.confirm.mockResolvedValue(true)
  mocks.request.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'batch_preview'
        ? structuredClone(preview)
        : method === 'batch_apply'
          ? [
              { index: 0, id: 'created', status: 'imported' },
              { index: 1, id: null, status: 'conflict' },
            ]
          : { exported: 1 },
    ),
  )
})
describe('批量资料迁移', () => {
  it('只读预览默认新增新条目、跳过冲突，不提供复活已删除标识的替换操作', async () => {
    const w = mountPanel()
    await browse(w)
    expect(mocks.request.mock.calls.map((c) => c[0])).toEqual(['batch_preview'])
    const selects = w.findAll('select')
    expect((selects[0]!.element as HTMLSelectElement).value).toBe('create')
    expect((selects[1]!.element as HTMLSelectElement).value).toBe('skip')
    expect(selects[2]!.findAll('option').map((o) => o.attributes('value'))).toEqual(['skip', 'copy'])
    w.unmount()
  })
  it('替换携带预览时的本机 revision，成功条目不重复导入', async () => {
    const w = mountPanel()
    await browse(w)
    await w.findAll('select')[1]!.setValue('replace')
    await click(w, t('library.batch_apply', { count: 2 }))
    const apply = mocks.request.mock.calls.find((c) => c[0] === 'batch_apply')!
    expect(JSON.parse(apply[1])).toMatchObject({
      sources: preview.sources,
      decisions: [
        { index: 0, mode: 'create', expected_revision: null },
        { index: 1, mode: 'replace', expected_revision: 'disk-1' },
        { index: 2, mode: 'skip', expected_revision: null },
      ],
    })
    expect(w.emitted('changed')).toHaveLength(1)
    expect(w.findAll('select')[0]!.attributes('disabled')).toBeDefined()
    await click(w, t('library.batch_copy_all'))
    await click(w, t('library.batch_apply', { count: 2 }))
    const last = mocks.request.mock.calls.filter((c) => c[0] === 'batch_apply').at(-1)!
    expect(JSON.parse(last[1]).decisions.map((d: { index: number }) => d.index)).not.toContain(0)
    w.unmount()
  })
  it('未保存编辑拒绝离开及取消最终确认均不写入', async () => {
    const beforeChange = vi.fn().mockResolvedValue(false)
    const w = mountPanel(beforeChange)
    await browse(w)
    await click(w, t('library.batch_apply', { count: 1 }))
    expect(mocks.confirm).not.toHaveBeenCalled()
    beforeChange.mockResolvedValue(true)
    mocks.confirm.mockResolvedValue(false)
    await click(w, t('library.batch_apply', { count: 1 }))
    expect(mocks.request.mock.calls.filter((c) => c[0] === 'batch_apply')).toHaveLength(0)
    w.unmount()
  })
  it('源变化提示保留预览并允许重新选择文件', async () => {
    const w = mountPanel()
    await browse(w)
    mocks.request.mockRejectedValueOnce({ message_key: 'library.batch_source_changed' })
    await click(w, t('library.batch_apply', { count: 1 }))
    expect(w.get('[role="alert"]').text()).toBe(t('library.batch_source_changed'))
    expect(w.findAll('select')).toHaveLength(3)
    expect(w.emitted('changed')).toBeUndefined()
    w.unmount()
  })
  it('导出只携带选中条目的磁盘版本，不受之后列表刷新影响', async () => {
    const w = mountPanel()
    await click(w, t('library.batch_title'))
    await w.get('input[type="checkbox"]').setValue(true)
    await w.setProps({ items: [{ ...entries[0]!, revision: 'disk-2' }] })
    await click(w, t('library.batch_export', { count: 1 }))
    const call = mocks.request.mock.calls.find((c) => c[0] === 'batch_export')!
    expect(JSON.parse(call[1])).toEqual({
      path: 'output.mflibrary',
      selection: [{ id: 'old', revision: 'disk-1' }],
    })
    w.unmount()
  })
  it('批量导出有明确 200 条限制且取消对话框不调用命令', async () => {
    const w = mountPanel()
    await w.setProps({
      items: Array.from({ length: 201 }, (_, i) => ({ id: String(i), title: String(i), revision: '1' })),
    })
    await click(w, t('library.batch_title'))
    await click(w, t('library.batch_select_visible'))
    expect(w.findAll('input:checked')).toHaveLength(200)
    mocks.save.mockResolvedValue(null)
    await click(w, t('library.batch_export', { count: 200 }))
    expect(mocks.request).not.toHaveBeenCalled()
    w.unmount()
  })
})
