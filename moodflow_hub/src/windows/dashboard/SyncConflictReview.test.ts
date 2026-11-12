import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import SyncConflictReview from './SyncConflictReview.vue'
import { t } from '@/i18n'
const mocks = vi.hoisted(() => ({ call: vi.fn(), confirm: vi.fn() }))
vi.mock('@/api', () => ({ commands: { syncRequest: mocks.call }, unwrap: async (p: Promise<unknown>) => p }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm }))
const name = 'saved-conflict.md'
const metadata = {
  id: '12345678-1234-4234-8234-123456789abc',
  title: '保留版本',
  tags: ['标签'],
  capsule: 'note',
}
const entry = {
  name,
  id: metadata.id,
  title: metadata.title,
  kind: 'note',
  tags: metadata.tags,
  updated_at: '2026-11-01',
  revision: 'source-hash',
}
const initial = {
  name,
  saved: { metadata, body: '<img src=x onerror=alert(1)> 合成副本', revision: 'source-hash' },
  current: { metadata: { ...metadata, title: '当前版本' }, body: '当前正文', revision: 'current-hash' },
  current_state: 'available',
  source_revision: 'source-hash',
  can_replace: true,
  diff: {
    rows: [{ kind: 'added', left: null, right: 1, text: '<img src=x onerror=alert(1)> 合成副本' }],
    added: 1,
    removed: 1,
    detailed: true,
    truncated: false,
    left_lines: 1,
    right_lines: 1,
  },
  fields: [{ key: 'custom', current: '当前字段', saved: '保留字段' }],
  fields_truncated: false,
}
const render = () => mount(SyncConflictReview, { props: { disabled: false, refreshKey: 0 } })
async function click(w: ReturnType<typeof render>, label: string) {
  await w
    .findAll('button')
    .find((b) => b.text() === label)!
    .trigger('click')
  await flushPromises()
}
async function choose(w: ReturnType<typeof render>) {
  await flushPromises()
  await w.get('.versions button').trigger('click')
  await flushPromises()
}
function titleInput(w: ReturnType<typeof render>) {
  return w
    .findAll('label')
    .find((l) => l.text().startsWith(t('sync.review_name')))!
    .get('input')
}
beforeEach(() => {
  vi.clearAllMocks()
  mocks.confirm.mockResolvedValue(true)
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'review_list'
        ? { items: [entry], unreadable: [], total: 1 }
        : method === 'review'
          ? structuredClone(initial)
          : { id: 'new-id', backup: 'backup.md', repeated: false },
    ),
  )
})
describe('同步冲突对比与合并', () => {
  it('只读加载副本与差异，所有外部文字作为纯文本展示', async () => {
    const w = render()
    await choose(w)
    expect(mocks.call.mock.calls.map((c) => c[0])).toEqual(['review_list', 'review'])
    expect(w.text()).toContain(initial.saved.body)
    expect(w.find('img').exists()).toBe(false)
    expect(w.text()).toContain('当前字段')
    expect(w.text()).toContain('保留字段')
    expect((w.get('.review select').element as HTMLSelectElement).value).toBe('copy')
    w.unmount()
  })
  it('另存携带源摘要和稳定操作标识，未明确确认不保存', async () => {
    const w = render()
    await choose(w)
    mocks.confirm.mockResolvedValue(false)
    await click(w, t('sync.review_apply'))
    expect(mocks.call.mock.calls.some((c) => c[0] === 'review_resolve')).toBe(false)
    mocks.confirm.mockResolvedValue(true)
    await click(w, t('sync.review_apply'))
    const request = JSON.parse(mocks.call.mock.calls.find((c) => c[0] === 'review_resolve')![1])
    expect(request).toMatchObject({
      confirm: true,
      backups: false,
      resolution: {
        name,
        source_revision: 'source-hash',
        current_revision: 'current-hash',
        mode: 'copy',
        title: metadata.title,
        body: initial.saved.body,
        tags: ['标签'],
      },
    })
    expect(request.resolution.operation_id).toMatch(/^[a-f0-9-]{36}$/)
    expect(
      w
        .findAll('button')
        .find((b) => b.text() === t('sync.review_apply'))!
        .attributes('disabled'),
    ).toBeDefined()
    w.unmount()
  })
  it('手动合并明确携带当前版本，正文选择不触发自动保存', async () => {
    const w = render()
    await choose(w)
    await click(w, t('sync.review_use_current'))
    expect(mocks.call.mock.calls.some((c) => c[0] === 'review_resolve')).toBe(false)
    await w.get('.review select').setValue('replace')
    await titleInput(w).setValue('合并标题')
    await click(w, t('sync.review_apply'))
    const request = JSON.parse(mocks.call.mock.calls.find((c) => c[0] === 'review_resolve')![1])
    expect(request.resolution).toMatchObject({
      mode: 'replace',
      current_revision: 'current-hash',
      body: '当前正文',
      title: '合并标题',
    })
    expect(w.text()).toContain(t('sync.review_backed_up'))
    w.unmount()
  })
  it('失败重试复用操作标识，编辑改变后生成新标识', async () => {
    const original = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((method: string) =>
      method === 'review_resolve' ? Promise.reject({ message_key: 'sync.local_conflict' }) : original(method),
    )
    const w = render()
    await choose(w)
    await click(w, t('sync.review_apply'))
    await click(w, t('sync.review_apply'))
    let requests = mocks.call.mock.calls.filter((c) => c[0] === 'review_resolve').map((c) => JSON.parse(c[1]))
    expect(requests[0].resolution.operation_id).toBe(requests[1].resolution.operation_id)
    await titleInput(w).setValue('另一个结果')
    await click(w, t('sync.review_apply'))
    requests = mocks.call.mock.calls.filter((c) => c[0] === 'review_resolve').map((c) => JSON.parse(c[1]))
    expect(requests[2].resolution.operation_id).not.toBe(requests[0].resolution.operation_id)
    expect(w.get('[role="alert"]').text()).toBe(t('sync.local_conflict'))
    w.unmount()
  })
  it('未保存编辑阻止切换来源，备份入口使用独立来源参数', async () => {
    const w = render()
    await choose(w)
    await titleInput(w).setValue('不能丢失')
    mocks.confirm.mockResolvedValue(false)
    await click(w, t('sync.review_backups'))
    expect(mocks.call.mock.calls.filter((c) => c[0] === 'review_list')).toHaveLength(1)
    expect(await (w.vm as unknown as { canLeave: () => Promise<boolean> }).canLeave()).toBe(false)
    mocks.confirm.mockResolvedValue(true)
    await click(w, t('sync.review_backups'))
    expect(JSON.parse(mocks.call.mock.calls.filter((c) => c[0] === 'review_list').at(-1)![1])).toMatchObject({
      backups: true,
    })
    w.unmount()
  })
  it('当前资料已删除时不提供原标识合并入口', async () => {
    const original = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((method: string) =>
      method === 'review'
        ? Promise.resolve(
            JSON.stringify({ ...initial, current: null, current_state: 'deleted', can_replace: false }),
          )
        : original(method),
    )
    const w = render()
    await choose(w)
    expect(
      w
        .get('.review select')
        .findAll('option')
        .map((o) => o.attributes('value')),
    ).toEqual(['copy'])
    expect(w.text()).toContain(t('sync.review_state_deleted'))
    w.unmount()
  })
  it('隐私卸载后的迟到确认不会保存旧版本', async () => {
    let done: (answer: boolean) => void = () => {}
    mocks.confirm.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          done = resolve
        }),
    )
    const w = render()
    await choose(w)
    await w
      .findAll('button')
      .find((b) => b.text() === t('sync.review_apply'))!
      .trigger('click')
    await flushPromises()
    w.unmount()
    done(true)
    await flushPromises()
    expect(mocks.call.mock.calls.some((c) => c[0] === 'review_resolve')).toBe(false)
  })
  it('大文档比较显示简化提示，文献副本不提供正文与替换编辑', async () => {
    const original = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((method: string) =>
      method === 'review'
        ? Promise.resolve(
            JSON.stringify({
              ...initial,
              saved: { ...initial.saved, metadata: { ...metadata, capsule: 'pdf' } },
              can_replace: false,
              diff: { ...initial.diff, detailed: false, truncated: true },
            }),
          )
        : original(method),
    )
    const w = render()
    await choose(w)
    expect(w.text()).toContain(t('sync.review_limited'))
    expect(w.text()).toContain(t('sync.review_bibliography'))
    expect(w.find('textarea').exists()).toBe(false)
    w.unmount()
  })
  it('删除副本需要明确确认和预览摘要，不执行资料替换', async () => {
    const w = render()
    await choose(w)
    mocks.confirm.mockResolvedValue(false)
    await click(w, t('sync.review_delete'))
    expect(mocks.call.mock.calls.some((c) => c[0] === 'review_delete')).toBe(false)
    mocks.confirm.mockResolvedValue(true)
    await click(w, t('sync.review_delete'))
    const request = JSON.parse(mocks.call.mock.calls.find((c) => c[0] === 'review_delete')![1])
    expect(request).toEqual({ name, revision: 'source-hash', backups: false, confirm: true })
    expect(mocks.call.mock.calls.some((c) => c[0] === 'review_resolve')).toBe(false)
    w.unmount()
  })
})
