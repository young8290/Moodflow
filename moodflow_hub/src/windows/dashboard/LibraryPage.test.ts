import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import LibraryPage from './LibraryPage.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({
  library: vi.fn(),
  tools: vi.fn(),
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  statusListener: vi.fn(),
  closeListener: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm, open: mocks.open, save: mocks.save }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onCloseRequested: mocks.closeListener, close: vi.fn() }),
}))
vi.mock('@/api', () => ({
  commands: {
    libraryRequest: mocks.library,
    imeTools: mocks.tools,
    getStatus: async () => ({ paused: false }),
  },
  events: { statusChanged: { listen: mocks.statusListener } },
  unwrap: async (p: Promise<unknown>) => p,
}))
const original = {
  metadata: {
    id: '12345678-1234-4234-8234-123456789abc',
    version: 1,
    capsule: 'note',
    title: '已有笔记',
    updated_at: '2026-11-09',
    tags: ['本机'],
    custom_key: '保留未知字段',
  },
  body: '本机正文',
  revision: 'disk-1',
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.confirm.mockResolvedValue(false)
  mocks.statusListener.mockResolvedValue(vi.fn())
  mocks.closeListener.mockResolvedValue(vi.fn())
  mocks.library.mockImplementation(async (method: string, raw: string) =>
    JSON.stringify(
      method === 'list'
        ? {
            items: [
              {
                id: original.metadata.id,
                title: original.metadata.title,
                kind: 'note',
                tags: ['本机'],
                updated_at: '2026',
                revision: 'disk-1',
              },
            ],
            unreadable: [],
          }
        : method === 'new'
          ? {
              ...original,
              metadata: { ...original.metadata, id: '22345678-1234-4234-8234-123456789abc' },
              revision: '',
            }
          : method === 'save'
            ? { ...JSON.parse(raw).document, revision: 'disk-2' }
            : structuredClone(original),
    ),
  )
  mocks.tools.mockResolvedValue('{}')
})
const click = async (wrapper: ReturnType<typeof mount>, text: string) => {
  await wrapper
    .findAll('button')
    .find((b) => b.text() === text)!
    .trigger('click')
  await flushPromises()
}

describe('资料库', () => {
  it('保存带磁盘 revision，保留未知字段；只有显式操作才加入 Buffer', async () => {
    const wrapper = mount(LibraryPage)
    await flushPromises()
    await wrapper.get('.entry').trigger('click')
    await flushPromises()
    await wrapper.get('textarea').setValue('新的正文')
    expect(mocks.tools).not.toHaveBeenCalled()
    await click(wrapper, t('library.save'))
    const save = mocks.library.mock.calls.find((c) => c[0] === 'save')!
    expect(JSON.parse(save[1] as string)).toMatchObject({
      expectedRevision: 'disk-1',
      document: { body: '新的正文', metadata: { custom_key: '保留未知字段' } },
    })
    await click(wrapper, t('library.stage'))
    expect(mocks.tools).toHaveBeenCalledWith('buffer.stage', JSON.stringify({ text: '新的正文' }))
    wrapper.unmount()
  })
  it('冲突保留正在编辑的内容，另存为新条目不覆盖原资料', async () => {
    const wrapper = mount(LibraryPage)
    await flushPromises()
    await wrapper.get('.entry').trigger('click')
    await flushPromises()
    await wrapper.get('textarea').setValue('不能丢掉的内容')
    mocks.library.mockImplementationOnce(async () => {
      throw { message_key: 'library.conflict' }
    })
    await click(wrapper, t('library.save'))
    expect(wrapper.get('textarea').element.value).toBe('不能丢掉的内容')
    expect(wrapper.text()).toContain(t('library.conflict'))
    expect(await (wrapper.vm as unknown as { canLeave: () => Promise<boolean> }).canLeave()).toBe(false)
    await click(wrapper, t('library.duplicate'))
    await click(wrapper, t('library.save'))
    const calls = mocks.library.mock.calls.filter((c) => c[0] === 'save')
    expect(JSON.parse(calls.at(-1)![1] as string)).toMatchObject({
      expectedRevision: null,
      document: { body: '不能丢掉的内容', metadata: { id: '22345678-1234-4234-8234-123456789abc' } },
    })
    wrapper.unmount()
  })
  it('隐私事件立即隐藏正文，保留编辑供恢复后处理', async () => {
    let receive: (e: { payload: { paused: boolean } }) => void = () => {}
    mocks.statusListener.mockImplementation(async (fn: typeof receive) => {
      receive = fn
      return vi.fn()
    })
    const wrapper = mount(LibraryPage)
    await flushPromises()
    await wrapper.get('.entry').trigger('click')
    await flushPromises()
    receive({ payload: { paused: true } })
    await flushPromises()
    expect(wrapper.find('textarea').exists()).toBe(false)
    expect(wrapper.text()).toContain(t('library.privacy'))
    receive({ payload: { paused: false } })
    await flushPromises()
    expect(wrapper.get('textarea').element.value).toBe('本机正文')
    wrapper.unmount()
  })
})
