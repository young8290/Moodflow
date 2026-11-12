import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import WorkspaceExplorer from './WorkspaceExplorer.vue'
import { t } from '@/i18n'
const mocks = vi.hoisted(() => ({ call: vi.fn(), confirm: vi.fn(), save: vi.fn() }))
vi.mock('./workspaceTypes', () => ({ workspaceCall: mocks.call }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm, save: mocks.save }))
const hit = {
  id: '12345678-1234-4234-8234-123456789abc',
  kind: 'inbox',
  title: '合成会话',
  revision: 'a'.repeat(64),
  archived: false,
  unread: true,
  count: 5,
  matched_messages: 1,
  matches: [{ id: 'message', role: 'external', snippet: '<img src=x onerror=alert(1)> 外部消息' }],
}
const page = { items: [hit], total: 41, offset: 0, limit: 20, unreadable: [] }
function render(beforeChange = vi.fn().mockResolvedValue(true)) {
  return mount(WorkspaceExplorer, { props: { disabled: false, beforeChange } })
}
async function click(w: ReturnType<typeof render>, label: string) {
  await w
    .findAll('button')
    .find((b) => b.text() === label)!
    .trigger('click')
  await flushPromises()
}
async function expand(w: ReturnType<typeof render>) {
  await click(w, t('workspace.explorer_title'))
}
beforeEach(() => {
  vi.clearAllMocks()
  mocks.confirm.mockResolvedValue(true)
  mocks.save.mockResolvedValue('transcript.md')
  mocks.call.mockImplementation(async (method: string, payload: { offset?: number }) =>
    method === 'search'
      ? { ...structuredClone(page), offset: payload.offset ?? 0 }
      : method === 'manage'
        ? [{ id: hit.id, status: 'conflict', revision: null }]
        : {},
  )
})
describe('工作区检索与管理', () => {
  it('默认仅检索本机未归档会话，筛选与查询明确提交', async () => {
    const w = render()
    await expand(w)
    expect(mocks.call).toHaveBeenCalledWith(
      'search',
      expect.objectContaining({ query: '', archive: 'active', kind: null, limit: 20 }),
    )
    await w.get('input[type="search"]').setValue('项目 资料')
    const filters = w.findAll('select')
    await filters[0]!.setValue('inbox')
    await filters[1]!.setValue('all')
    await filters[2]!.setValue('messages')
    await filters[3]!.setValue('title')
    await w.get('form').trigger('submit')
    await flushPromises()
    expect(mocks.call).toHaveBeenLastCalledWith(
      'search',
      expect.objectContaining({
        query: '项目 资料',
        kind: 'inbox',
        archive: 'all',
        scope: 'messages',
        sort: 'title',
        offset: 0,
      }),
    )
    w.unmount()
  })
  it('命中片段仅作为文字展示，打开会话传递明确标识', async () => {
    const w = render()
    await expand(w)
    expect(w.text()).toContain(hit.matches[0]!.snippet)
    expect(w.find('img').exists()).toBe(false)
    await click(w, t('workspace.explorer_open'))
    expect(w.emitted('open')![0]![0]).toMatchObject({ id: hit.id, kind: 'inbox' })
    expect(mocks.call.mock.calls.some(([m]) => m === 'chat' || m === 'terminal_input')).toBe(false)
    w.unmount()
  })
  it('批量管理携带检索快照，部分冲突可见且刷新后清空选择', async () => {
    const w = render()
    await expand(w)
    await click(w, t('workspace.explorer_select_page'))
    await click(w, t('workspace.explorer_archive_true'))
    expect(mocks.call).toHaveBeenCalledWith('manage', {
      selection: [{ id: hit.id, revision: hit.revision }],
      change: { type: 'archive', value: true },
    })
    expect(w.text()).toContain(t('workspace.explorer_conflict'))
    expect(w.emitted('changed')).toHaveLength(1)
    expect(w.findAll('input:checked')).toHaveLength(0)
    w.unmount()
  })
  it('未保存编辑保护与取消确认都阻止管理写入', async () => {
    const before = vi.fn().mockResolvedValue(false)
    const w = render(before)
    await expand(w)
    await click(w, t('workspace.explorer_select_page'))
    await click(w, t('workspace.explorer_unread_false'))
    expect(mocks.confirm).not.toHaveBeenCalled()
    before.mockResolvedValue(true)
    mocks.confirm.mockResolvedValue(false)
    await click(w, t('workspace.explorer_unread_false'))
    expect(mocks.call.mock.calls.some(([m]) => m === 'manage')).toBe(false)
    w.unmount()
  })
  it('重命名冲突保留输入并使用原 revision', async () => {
    const w = render()
    await expand(w)
    await click(w, t('workspace.explorer_rename'))
    await w.get('.search-hit input:not([type="checkbox"])').setValue('新名称')
    const original = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((method: string, payload: object) =>
      method === 'rename' ? Promise.reject({ message_key: 'workspace.conflict' }) : original(method, payload),
    )
    await w.get('.search-hit form').trigger('submit')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('rename', {
      selection: { id: hit.id, revision: hit.revision },
      title: '新名称',
    })
    expect((w.get('.search-hit input:not([type="checkbox"])').element as HTMLInputElement).value).toBe(
      '新名称',
    )
    expect(w.get('[role="alert"]').text()).toBe(t('workspace.conflict'))
    w.unmount()
  })
  it('导出默认不含指令，需明确选择格式与授权', async () => {
    const w = render()
    await expand(w)
    await click(w, t('workspace.explorer_export'))
    expect(mocks.call).toHaveBeenCalledWith('transcript', {
      selection: { id: hit.id, revision: hit.revision },
      format: 'markdown',
      include_instructions: false,
      path: 'transcript.md',
    })
    await w.get('.export-options select').setValue('json')
    await w.get('.export-options input[type="checkbox"]').setValue(true)
    await click(w, t('workspace.explorer_export'))
    expect(mocks.call).toHaveBeenLastCalledWith(
      'transcript',
      expect.objectContaining({ format: 'json', include_instructions: true }),
    )
    w.unmount()
  })
  it('文件对话框跨越隐私卸载后不导出旧会话', async () => {
    let resolve: (path: string) => void = () => {}
    mocks.save.mockImplementation(
      () =>
        new Promise<string>((done) => {
          resolve = done
        }),
    )
    const w = render()
    await expand(w)
    await w
      .findAll('button')
      .find((b) => b.text() === t('workspace.explorer_export'))!
      .trigger('click')
    await flushPromises()
    w.unmount()
    resolve('late.md')
    await flushPromises()
    expect(mocks.call.mock.calls.some(([m]) => m === 'transcript')).toBe(false)
  })
  it('翻页明确传递偏移，旧页选择不授权新页操作', async () => {
    const w = render()
    await expand(w)
    await click(w, t('workspace.explorer_select_page'))
    await click(w, t('workspace.explorer_next'))
    expect(mocks.call).toHaveBeenLastCalledWith('search', expect.objectContaining({ offset: 20 }))
    expect(w.findAll('input:checked')).toHaveLength(0)
    w.unmount()
  })
})
