import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import WorkspacePage from './WorkspacePage.vue'
import { t } from '@/i18n'
const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  tools: vi.fn(),
  listen: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: {
    workspaceRequest: mocks.call,
    imeTools: mocks.tools,
    getStatus: async () => ({ paused: false }),
  },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, save: mocks.save, confirm: mocks.confirm }))
vi.mock('./WorkspaceTerminal.vue', () => ({ default: { template: '<div />' } }))
const inboxId = '12345678-1234-4234-8234-123456789abc'
const chatId = '12345678-1234-4234-8234-123456789abd'
const external = '<img src=x onerror=alert(1)> 合成外部文字'
const session = (id: string, kind: 'chat' | 'inbox') => ({
  version: 1,
  id,
  kind,
  title: kind === 'chat' ? 'chat fixture' : 'inbox fixture',
  instructions: '',
  created_at: 1,
  updated_at: 1,
  archived: false,
  unread: false,
  model: null,
  messages:
    kind === 'inbox'
      ? [
          {
            id: 'message',
            role: 'external',
            body: external,
            created_at: 1,
            files: [],
            reply_to: null,
            mock: false,
          },
        ]
      : [],
})
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.open.mockResolvedValue(null)
  mocks.save.mockResolvedValue(null)
  mocks.confirm.mockResolvedValue(true)
  mocks.call.mockImplementation(async (method: string, payload: string) => {
    const p = JSON.parse(payload) as { id?: string }
    return JSON.stringify(
      method === 'list'
        ? {
            sessions: [
              { ...session(inboxId, 'inbox'), count: 1, revision: 'r1' },
              { ...session(chatId, 'chat'), count: 0, revision: 'r1' },
            ],
          }
        : method === 'gateway_status'
          ? { enabled: false, address: null, terminal_supported: true }
          : method === 'get'
            ? { session: session(p.id!, p.id === chatId ? 'chat' : 'inbox'), revision: 'r1' }
            : method === 'note'
              ? { session: session(inboxId, 'inbox'), revision: 'r2' }
              : {},
    )
  })
})
afterEach(() => vi.useRealTimers())
const render = () => mount(WorkspacePage, { global: { stubs: { WorkspaceTerminal: true } } })
const click = async (w: ReturnType<typeof render>, text: string) => {
  await w
    .findAll('button')
    .find((b) => b.text() === text)!
    .trigger('click')
  await flushPromises()
}
it('外部文字只作为文字展示，审核和点击后才加入 Buffer', async () => {
  const w = render()
  await flushPromises()
  await click(w, t('workspace.inbox'))
  await click(w, 'inbox fixture1 条消息')
  expect(w.text()).toContain(external)
  expect(w.find('img').exists()).toBe(false)
  expect(mocks.tools).not.toHaveBeenCalled()
  await click(w, t('workspace.review'))
  expect(mocks.tools).not.toHaveBeenCalled()
  await click(w, t('workspace.stage'))
  expect(mocks.tools).toHaveBeenCalledWith('buffer.stage', JSON.stringify({ text: external }))
  w.unmount()
})
it('收件输入框仅保存本机备注，不能触发 AI 或外部投递', async () => {
  const w = render()
  await flushPromises()
  await click(w, t('workspace.inbox'))
  await click(w, 'inbox fixture1 条消息')
  await w.find('textarea').setValue('本地合成备注')
  await click(w, t('workspace.save_note'))
  const note = mocks.call.mock.calls.find(([method]) => method === 'note')!
  expect(JSON.parse(note[1] as string)).toMatchObject({ id: inboxId, text: '本地合成备注', revision: 'r1' })
  expect(mocks.call.mock.calls.some(([method]) => method === 'chat')).toBe(false)
  expect(mocks.tools).not.toHaveBeenCalled()
  w.unmount()
})
it('对话需要明确出网同意，暂停后迟到回复无法恢复页面内容', async () => {
  let finish: ((body: string) => void) | undefined
  const original = mocks.call.getMockImplementation()!
  mocks.call.mockImplementation((method: string, payload: string) =>
    method === 'chat'
      ? new Promise<string>((resolve) => {
          finish = resolve
        })
      : original(method, payload),
  )
  const w = render()
  await flushPromises()
  await click(w, 'chat fixture0 条消息')
  await w.find('textarea').setValue('合成问题')
  const send = w.findAll('button').find((b) => b.text() === t('workspace.send'))!
  expect(send.attributes('disabled')).toBeDefined()
  await w.findAll('input[type=checkbox]').at(-1)!.setValue(true)
  await send.trigger('click')
  await flushPromises()
  const request = mocks.call.mock.calls.find(([method]) => method === 'chat')!
  expect(JSON.parse(request[1] as string)).toMatchObject({
    id: chatId,
    text: '合成问题',
    consent: true,
    include_history: true,
  })
  const listener = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  listener({ payload: { paused: true } })
  await flushPromises()
  const late = session(chatId, 'chat')
  late.title = '迟到的私密回复'
  finish!(JSON.stringify({ session: late, revision: 'r2' }))
  await flushPromises()
  expect(w.text()).not.toContain('迟到的私密回复')
  expect(w.find('textarea').exists()).toBe(false)
  w.unmount()
})
it('收文件对话框跨越暂停和恢复后不能写入旧会话', async () => {
  let finish: ((path: string) => void) | undefined
  mocks.open.mockImplementation(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve
      }),
  )
  const w = render()
  await flushPromises()
  await click(w, t('workspace.inbox'))
  await click(w, 'inbox fixture1 条消息')
  await click(w, t('workspace.receive_file'))
  const listener = mocks.listen.mock.calls[0]![0] as (event: { payload: { paused: boolean } }) => void
  listener({ payload: { paused: true } })
  await flushPromises()
  listener({ payload: { paused: false } })
  await flushPromises()
  finish!('explicit-before-pause.txt')
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'receive_file')).toBe(false)
  w.unmount()
})
