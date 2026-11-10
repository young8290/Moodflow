import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { t } from '@/i18n'
import PluginMarketSection from './PluginMarketSection.vue'

const mocks = vi.hoisted(() => ({ call: vi.fn(), confirm: vi.fn() }))
vi.mock('@/api', () => ({
  commands: { marketRequest: mocks.call },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm }))
enableAutoUnmount(afterEach)
const source = { label: 'Test catalog', url: 'https://example.com/index.json', publicKey: '1'.repeat(64) }
const preview = {
  token: 'one-use-token',
  source,
  fingerprint: 'abc',
  sha256: 'abc123',
  preview: {
    id: 'org.example.demo',
    name: 'Demo',
    version: '1.0.0',
    description: 'Local fixture',
    upgrade_from: null,
    contributes: null,
    added: ['storage', 'clipboard_read'],
  },
}
const catalog = () => ({
  source,
  revision: 'revision-one',
  fingerprint: 'abc',
  generatedAt: '2026-11-09T12:00:00Z',
  expired: false,
  revoked: [],
  items: [
    {
      id: 'org.example.demo',
      name: 'Demo',
      description: '<img src=x onerror=alert(1)>',
      author: 'Author',
      repo: 'https://example.com/repo',
      release: { version: '1.0.0', url: 'https://example.com/demo.mfpack', size: 100, sha256: 'abc123' },
      installed: null,
      update: false,
    },
  ],
})
const button = (w: VueWrapper, key: Parameters<typeof t>[0]) =>
  w.findAll('button').find((b) => b.text() === t(key))!
async function page(community = false): Promise<VueWrapper> {
  const w = mount(PluginMarketSection, { props: { community } })
  await flushPromises()
  return w
}
beforeEach(() => {
  vi.clearAllMocks()
  mocks.confirm.mockResolvedValue(false)
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'preview'
        ? preview
        : method === 'install'
          ? { id: 'org.example.demo', fresh: true }
          : catalog(),
    ),
  )
})
it('does not fetch until explicit consent, and renders catalog text without HTML', async () => {
  const w = await page()
  expect(mocks.call.mock.calls.map(([method]) => method)).toEqual(['catalog'])
  expect(w.find('img').exists()).toBe(false)
  expect(w.text()).toContain('<img src=x onerror=alert(1)>')
  expect(button(w, 'market.refresh').attributes('disabled')).toBeDefined()
  await w.findAll('input[type=checkbox]')[1]!.setValue(true)
  await button(w, 'market.refresh').trigger('click')
  await flushPromises()
  expect(mocks.call).toHaveBeenCalledWith('refresh', JSON.stringify({ consent: true }))
  expect(button(w, 'market.refresh').attributes('disabled')).toBeDefined()
})
it('requires explicit source trust and passes the current revision; clearing needs confirmation', async () => {
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(method === 'catalog' ? { ...catalog(), source: null, items: [] } : catalog()),
  )
  const w = await page()
  expect(w.text()).toContain(t('market.no_official'))
  const fields = w.findAll('input:not([type=checkbox])')
  await fields[0]!.setValue('My catalog')
  await fields[1]!.setValue(source.url)
  await fields[2]!.setValue(source.publicKey)
  expect(button(w, 'market.save_source').attributes('disabled')).toBeDefined()
  await w.get('input[type=checkbox]').setValue(true)
  await button(w, 'market.save_source').trigger('click')
  await flushPromises()
  const cfg = mocks.call.mock.calls.find(([method]) => method === 'configure')!
  expect(JSON.parse(cfg[1] as string)).toEqual({
    revision: 'revision-one',
    confirmed: true,
    source: { ...source, label: 'My catalog' },
  })
  mocks.call.mockClear()
  await button(w, 'market.clear_source').trigger('click')
  await flushPromises()
  expect(mocks.call).not.toHaveBeenCalled()
  mocks.confirm.mockResolvedValue(true)
  await button(w, 'market.clear_source').trigger('click')
  await flushPromises()
  expect(mocks.call).toHaveBeenCalledWith(
    'configure',
    JSON.stringify({ revision: 'revision-one', confirmed: true, source: null }),
  )
})
it('downloads a preview, shows added permissions, and installs only with its token after agreement', async () => {
  const w = await page()
  await button(w, 'market.download').trigger('click')
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'install')).toBe(false)
  const consent = w.get('[data-market-preview]')
  expect(consent.text()).toContain(t('plugins.cap.storage'))
  expect(consent.text()).toContain(t('plugins.cap.clipboard_read'))
  expect(consent.text()).toContain('abc123')
  expect(button(w, 'market.install').attributes('disabled')).toBeDefined()
  await consent.get('input[type=checkbox]').setValue(true)
  await button(w, 'market.install').trigger('click')
  await flushPromises()
  const call = mocks.call.mock.calls.find(([method]) => method === 'install')!
  expect(JSON.parse(call[1] as string)).toEqual({
    token: 'one-use-token',
    accept: ['storage', 'clipboard_read'],
    confirmed: true,
  })
  expect(w.emitted('installed')).toEqual([[{ id: 'org.example.demo', fresh: true }]])
})
it('blocks expired downloads and drops a late preview after cancellation', async () => {
  mocks.call.mockImplementation(async () => JSON.stringify({ ...catalog(), expired: true }))
  const expired = await page()
  expect(button(expired, 'market.download').attributes('disabled')).toBeDefined()
  expect(expired.text()).toContain(t('market.expired'))
  mocks.call.mockImplementation(async () => JSON.stringify(catalog()))
  const w = await page()
  let resolve!: (value: string) => void
  mocks.call.mockImplementation((method: string) =>
    method === 'preview'
      ? new Promise<string>((done) => {
          resolve = done
        })
      : Promise.resolve('{}'),
  )
  await button(w, 'market.download').trigger('click')
  await flushPromises()
  await button(w, 'plugins.cancel').trigger('click')
  await flushPromises()
  resolve(JSON.stringify(preview))
  await flushPromises()
  expect(w.find('[data-market-preview]').exists()).toBe(false)
  expect(mocks.call).toHaveBeenCalledWith('cancel', '{}')
})
it('community opens only named project destinations and embeds no external page', async () => {
  const w = await page(true)
  expect(mocks.call).not.toHaveBeenCalled()
  await button(w, 'market.issues').trigger('click')
  await flushPromises()
  expect(mocks.call).toHaveBeenCalledWith('community', JSON.stringify({ target: 'issues' }))
  expect(w.find('iframe').exists()).toBe(false)
})
