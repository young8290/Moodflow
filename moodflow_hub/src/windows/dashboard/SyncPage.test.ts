import { beforeEach, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import SyncPage from './SyncPage.vue'
import SyncVaultCopies from './SyncVaultCopies.vue'
import { t } from '@/i18n'
const mocks = vi.hoisted(() => ({ call: vi.fn(), open: vi.fn(), save: vi.fn(), confirm: vi.fn() }))
const privacy = ref(false)
vi.mock('../shared/useToolPrivacy', () => ({ useToolPrivacy: () => privacy }))
vi.mock('@/api', () => ({ commands: { syncRequest: mocks.call }, unwrap: async (p: Promise<unknown>) => p }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, save: mocks.save, confirm: mocks.confirm }))
const profile = { target: { kind: 'folder', path: 'D:\\synthetic-sync-fixture' }, encrypted: true }
const phrase = 'synthetic recovery phrase with sufficient length'
beforeEach(() => {
  vi.clearAllMocks()
  privacy.value = false
  mocks.confirm.mockResolvedValue(true)
  mocks.open.mockResolvedValue(null)
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'status'
        ? { profile, revision: 'r1', busy: false }
        : method.endsWith('list') || method === 'conflicts'
          ? { items: [] }
          : {},
    ),
  )
})
const button = (w: VueWrapper, key: Parameters<typeof t>[0]) =>
  w.findAll('button').find((b) => b.text() === t(key))!
const field = (w: VueWrapper, key: Parameters<typeof t>[0]) =>
  w
    .findAll('label')
    .find((l) => l.text().startsWith(t(key)))!
    .get('input')
const page = () => mount(SyncPage, { global: { stubs: { SyncVaultCopies: true, SyncConflictReview: true } } })
it('requires explicit consent and excludes vault copies by default, then clears secrets', async () => {
  const w = page()
  await flushPromises()
  await field(w, 'sync.recovery_label').setValue(phrase)
  expect(button(w, 'sync.run').attributes('disabled')).toBeDefined()
  await field(w, 'sync.consent_label').setValue(true)
  await button(w, 'sync.run').trigger('click')
  await flushPromises()
  const call = mocks.call.mock.calls.find(([method]) => method === 'run')!
  expect(JSON.parse(call[1] as string)).toMatchObject({
    consent: true,
    recovery: phrase,
    include_vault: false,
  })
  expect((field(w, 'sync.recovery_label').element as HTMLInputElement).value).toBe('')
  expect((field(w, 'sync.consent_label').element as HTMLInputElement).checked).toBe(false)
  w.unmount()
})
it('cancels on pause and discards a late sync report without automatic resume', async () => {
  let finish!: (text: string) => void
  mocks.call.mockImplementation((method: string) =>
    method === 'run'
      ? new Promise<string>((r) => {
          finish = r
        })
      : Promise.resolve(
          JSON.stringify(
            method === 'status'
              ? { profile, revision: 'r1', busy: false }
              : method === 'conflicts'
                ? { items: [] }
                : {},
          ),
        ),
  )
  const w = page()
  await flushPromises()
  await field(w, 'sync.recovery_label').setValue(phrase)
  await field(w, 'sync.consent_label').setValue(true)
  await button(w, 'sync.run').trigger('click')
  await flushPromises()
  privacy.value = true
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'cancel')).toBe(true)
  finish(
    JSON.stringify({
      report: { uploaded: 991, downloaded: 0, deleted: 0, conflicts: 0, deferred: 0 },
      skipped: 0,
    }),
  )
  await flushPromises()
  privacy.value = false
  await flushPromises()
  expect(w.text()).not.toContain('991')
  expect((field(w, 'sync.recovery_label').element as HTMLInputElement).value).toBe('')
  expect(mocks.call.mock.calls.filter(([method]) => method === 'run')).toHaveLength(1)
  w.unmount()
})
it('does not change destination using a file dialog opened before pause', async () => {
  let finish!: (path: string) => void
  mocks.open.mockReturnValue(
    new Promise<string>((r) => {
      finish = r
    }),
  )
  const w = page()
  await flushPromises()
  await button(w, 'sync.choose').trigger('click')
  privacy.value = true
  await flushPromises()
  privacy.value = false
  await flushPromises()
  finish('D:\\late-wrong-folder')
  await flushPromises()
  expect((field(w, 'sync.directory').element as HTMLInputElement).value).toBe(profile.target.path)
  w.unmount()
})
it('cannot import a vault backup from a dialog after pausing the copies panel', async () => {
  let finish!: (path: string) => void
  mocks.open.mockReturnValue(
    new Promise<string>((r) => {
      finish = r
    }),
  )
  const w = mount(SyncVaultCopies, { props: { paused: false, busy: false, encrypted: true } })
  await flushPromises()
  await field(w, 'sync.backup_recovery').setValue(phrase)
  await button(w, 'sync.backup_import').trigger('click')
  await w.setProps({ paused: true })
  await w.setProps({ paused: false })
  finish('D:\\late.mfvbackup')
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'backup_import')).toBe(false)
  expect((field(w, 'sync.backup_recovery').element as HTMLInputElement).value).toBe('')
  w.unmount()
})

it('forwards the conflict editor leave guard to dashboard navigation', async () => {
  const editor = defineComponent({
    setup(_, { expose }) {
      expose({ canLeave: async () => false })
      return () => null
    },
  })
  const w = mount(SyncPage, { global: { stubs: { SyncVaultCopies: true, SyncConflictReview: editor } } })
  await flushPromises()
  expect(await (w.vm as unknown as { canLeave: () => Promise<boolean> }).canLeave()).toBe(false)
  w.unmount()
})
