import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import VaultPage from './VaultPage.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  listen: vi.fn(),
  close: vi.fn(),
  confirm: vi.fn(),
  closeListener: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: { vaultRequest: mocks.call, getStatus: async () => ({ paused: false }) },
  events: { statusChanged: { listen: mocks.listen } },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm, open: mocks.open, save: mocks.save }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onCloseRequested: mocks.closeListener, close: mocks.close }),
}))
const fixture = {
  id: '12345678-1234-4234-8234-123456789abc',
  title: '测试凭据',
  account: '测试账号',
  password: '',
  url: 'https://example.invalid',
  notes: '测试备注',
  totp: null,
}
let unlocked = false
let deadline = 0
let revision = 'disk-1'
beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'],
  })
  unlocked = false
  deadline = 0
  revision = 'disk-1'
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.closeListener.mockResolvedValue(vi.fn())
  mocks.confirm.mockResolvedValue(false)
  mocks.open.mockResolvedValue(null)
  mocks.save.mockResolvedValue(null)
  mocks.call.mockImplementation(async (method: string) => {
    if (method === 'unlock') {
      unlocked = true
      deadline = performance.now() + 60000
      return '{}'
    }
    if (method === 'lock') {
      unlocked = false
      return '{}'
    }
    if (method === 'status')
      return JSON.stringify({
        supported: true,
        exists: true,
        unlocked: unlocked && performance.now() < deadline,
        blocked: false,
        remaining_ms: Math.max(0, deadline - performance.now()),
      })
    if (method === 'list')
      return JSON.stringify({ items: [{ id: fixture.id, title: fixture.title }], revision })
    if (method === 'get')
      return JSON.stringify({
        entry: fixture,
        revision,
        has_password: true,
        has_totp: true,
        totp_settings: { algorithm: 'SHA256', digits: 8, period: 60 },
      })
    if (method === 'reveal') return JSON.stringify({ value: 'fixture-only-password', reveal_ms: 15000 })
    if (method === 'save') {
      revision = 'disk-2'
      return JSON.stringify({ id: fixture.id, revision })
    }
    return '{}'
  })
})
afterEach(() => vi.useRealTimers())
const click = async (w: ReturnType<typeof mount>, text: string) => {
  await w
    .findAll('button')
    .find((b) => b.text() === text)!
    .trigger('click')
  await flushPromises()
}
async function openEntry() {
  const w = mount(VaultPage)
  await flushPromises()
  await w.find('input[type=password]').setValue('fixture master phrase')
  await w.find('form').trigger('submit')
  await flushPromises()
  await click(w, fixture.title)
  return w
}

describe('加密密码库界面', () => {
  it('加密备份必须设置独立短语并选择文件，取消对话框不导出', async () => {
    const w = await openEntry()
    await click(w, t('vault.backup'))
    const inputs = w.findAll('.master-change input')
    await inputs[0]!.setValue('separate recovery phrase fixture')
    await inputs[1]!.setValue('separate recovery phrase fixture')
    await w.find('.master-change').trigger('submit')
    await flushPromises()
    expect(mocks.call.mock.calls.some(([method]) => method === 'export_backup')).toBe(false)
    mocks.save.mockResolvedValue('C:/fixture/vault.mfvbackup')
    await w.find('.master-change').trigger('submit')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith(
      'export_backup',
      JSON.stringify({
        path: 'C:/fixture/vault.mfvbackup',
        recovery: 'separate recovery phrase fixture',
        revision: 'disk-1',
      }),
    )
    expect(w.find('.master-change').exists()).toBe(false)
    w.unmount()
  })

  it('恢复先预览，取消不创建库；确认后携带同一文件的哈希并清空口令', async () => {
    const original = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation(async (method: string) => {
      if (method === 'status')
        return JSON.stringify({
          supported: true,
          exists: false,
          unlocked: false,
          blocked: false,
          remaining_ms: 0,
        })
      if (method === 'preview_restore')
        return JSON.stringify({ count: 2, fingerprint: 'verified-preview-hash' })
      return original(method)
    })
    mocks.open.mockResolvedValue('C:/fixture/backup.mfvbackup')
    const w = mount(VaultPage)
    await flushPromises()
    const fill = async () => {
      await w.findAll('input[type=password]')[0]!.setValue('new local master phrase')
      await w.findAll('input[type=password]')[1]!.setValue('new local master phrase')
      await w.findAll('input[type=password]')[2]!.setValue('separate recovery phrase fixture')
    }
    await fill()
    await click(w, t('vault.restore'))
    expect(mocks.call).toHaveBeenCalledWith(
      'preview_restore',
      JSON.stringify({ path: 'C:/fixture/backup.mfvbackup', recovery: 'separate recovery phrase fixture' }),
    )
    expect(mocks.call.mock.calls.some(([method]) => method === 'restore')).toBe(false)
    expect(w.find('input[type=password]').element).toHaveProperty('value', '')
    mocks.confirm.mockResolvedValue(true)
    await fill()
    await click(w, t('vault.restore'))
    expect(mocks.call).toHaveBeenCalledWith(
      'restore',
      JSON.stringify({
        path: 'C:/fixture/backup.mfvbackup',
        recovery: 'separate recovery phrase fixture',
        fingerprint: 'verified-preview-hash',
        master: 'new local master phrase',
      }),
    )
    expect(w.find('input[type=password]').element).toHaveProperty('value', '')
    w.unmount()
  })
  it('详情不主动索取密码；明确显示 15 秒后移除，复制只发条目标识', async () => {
    const w = await openEntry()
    expect(w.find('output').exists()).toBe(false)
    expect(mocks.call.mock.calls.some(([method]) => method === 'reveal' || method === 'copy')).toBe(false)
    await click(w, t('vault.reveal_password'))
    expect(w.find('output').text()).toBe('fixture-only-password')
    await vi.advanceTimersByTimeAsync(15000)
    await flushPromises()
    expect(w.find('output').exists()).toBe(false)
    await click(w, t('vault.copy_password'))
    expect(mocks.call).toHaveBeenLastCalledWith(
      'copy',
      JSON.stringify({ id: fixture.id, revision: 'disk-1', field: 'password' }),
    )
    w.unmount()
  })

  it('保存空白密码保留原密文凭据，携带 revision 和现有验证码参数', async () => {
    const w = await openEntry()
    await w.find('.editor input').setValue('修改后的名称')
    await w.find('form.editor').trigger('submit')
    await flushPromises()
    const call = mocks.call.mock.calls.find(([method]) => method === 'save')!
    const payload = JSON.parse(call[1] as string)
    expect(payload.revision).toBe('disk-1')
    expect(payload.keep_password).toBe(true)
    expect(payload.keep_totp).toBe(true)
    expect(payload.entry.password).toBe('')
    expect(payload.totp_settings).toEqual({ algorithm: 'SHA256', digits: 8, period: 60 })
    w.unmount()
  })

  it('隐私暂停或 60 秒到期清空编辑器和显示值，恢复后重新解锁', async () => {
    const w = await openEntry()
    await click(w, t('vault.reveal_password'))
    const listener = mocks.listen.mock.calls[0]![0] as (e: { payload: { paused: boolean } }) => void
    listener({ payload: { paused: true } })
    await flushPromises()
    expect(w.find('output').exists()).toBe(false)
    expect(w.find('form.editor').exists()).toBe(false)
    expect(w.text()).not.toContain(fixture.account)
    listener({ payload: { paused: false } })
    await flushPromises()
    expect(w.find('input[type=password]').element).toHaveProperty('value', '')
    await w.find('input[type=password]').setValue('fixture master phrase')
    await w.find('form').trigger('submit')
    await flushPromises()
    await click(w, fixture.title)
    await vi.advanceTimersByTimeAsync(60001)
    await flushPromises()
    expect(w.find('form.editor').exists()).toBe(false)
    expect(w.text()).not.toContain(fixture.account)
    w.unmount()
  })
})
