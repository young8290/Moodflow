import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { t } from '@/i18n'
import AiToolsPage from './AiToolsPage.vue'

const mocks = vi.hoisted(() => ({ call: vi.fn(), config: vi.fn(), ime: vi.fn(), open: vi.fn() }))
const paused = ref(false)
vi.mock('../shared/useToolPrivacy', () => ({ useToolPrivacy: () => paused }))
vi.mock('@/api', () => ({
  commands: { toolsRequest: mocks.call, aiConfigGet: mocks.config, imeTools: mocks.ime },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))
vi.mock('./MathPreview.vue', () => ({ default: { template: '<div />' } }))
enableAutoUnmount(afterEach)
const config = {
  source: 'saved',
  llm: { base_url: 'https://ai.example.invalid/v1', models: ['selected-model'], key_tail: null },
  jev: null,
}
const reply = {
  text: '<img src=x onerror=alert(1)> 合成文字',
  model: 'selected-model',
  mock: false,
  sources: [],
}
const button = (w: VueWrapper, key: Parameters<typeof t>[0]) =>
  w.findAll('button').find((b) => b.text() === t(key))!
async function page(): Promise<VueWrapper> {
  const w = mount(AiToolsPage)
  await flushPromises()
  return w
}
beforeEach(() => {
  vi.clearAllMocks()
  paused.value = false
  mocks.config.mockResolvedValue(config)
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(method === 'run' ? reply : method === 'unicode' ? { text: 'α + β', unsupported: [] } : {}),
  )
  mocks.ime.mockResolvedValue('{}')
  mocks.open.mockResolvedValue(null)
})
it('requires per-request consent, pins endpoint/model and stages only after an explicit click', async () => {
  const w = await page()
  await w.get('textarea').setValue('本次合成输入')
  expect(button(w, 'ai_tools.run').attributes('disabled')).toBeDefined()
  expect(mocks.call).not.toHaveBeenCalled()
  await w.get('input[type=checkbox]').setValue(true)
  await button(w, 'ai_tools.run').trigger('click')
  await flushPromises()
  const run = mocks.call.mock.calls.find(([method]) => method === 'run')!
  expect(JSON.parse(run[1] as string)).toMatchObject({
    text: '本次合成输入',
    model: 'selected-model',
    endpoint: config.llm.base_url,
    consent: true,
    images: [],
  })
  expect(w.find('img').exists()).toBe(false)
  expect(mocks.ime).not.toHaveBeenCalled()
  await button(w, 'ai_tools.stage').trigger('click')
  await flushPromises()
  expect(mocks.ime).toHaveBeenCalledWith('buffer.stage', JSON.stringify({ text: reply.text }))
  expect((w.get('input[type=checkbox]').element as HTMLInputElement).checked).toBe(false)
})
it('cancels changed input, drops late output and preserves text on failure', async () => {
  let finish!: (value: string) => void
  mocks.call.mockImplementation((method: string) =>
    method === 'run'
      ? new Promise<string>((resolve) => {
          finish = resolve
        })
      : Promise.resolve('{}'),
  )
  const w = await page()
  await w.get('textarea').setValue('first')
  await w.get('input[type=checkbox]').setValue(true)
  await button(w, 'ai_tools.run').trigger('click')
  await w.get('textarea').setValue('new input')
  finish(JSON.stringify(reply))
  await flushPromises()
  expect(mocks.call.mock.calls.some(([method]) => method === 'cancel')).toBe(true)
  expect(w.find('.result').exists()).toBe(false)
  expect((w.get('textarea').element as HTMLTextAreaElement).value).toBe('new input')
  mocks.call.mockRejectedValueOnce(new Error('synthetic failure'))
  await w.get('input[type=checkbox]').setValue(true)
  await button(w, 'ai_tools.run').trigger('click')
  await flushPromises()
  expect((w.get('textarea').element as HTMLTextAreaElement).value).toBe('new input')
})
it('requires separate Crossref consent and stages bibliography together with the paragraph', async () => {
  const w = await page()
  await button(w, 'ai_tools.action_reference').trigger('click')
  await w.get('textarea').setValue('synthetic research')
  await w.findAll('input[type=checkbox]')[0]!.setValue(true)
  expect(button(w, 'ai_tools.run').attributes('disabled')).toBeDefined()
  await w.findAll('input[type=checkbox]')[1]!.setValue(true)
  const text = '文献支持这一观点[1]。\n\n[1] Author. Title. DOI: 10.1234/verified'
  mocks.call.mockResolvedValueOnce(JSON.stringify({ ...reply, text }))
  await button(w, 'ai_tools.run').trigger('click')
  await flushPromises()
  expect(JSON.parse(mocks.call.mock.calls[0]![1] as string)).toMatchObject({
    action: 'reference',
    search_consent: true,
  })
  await button(w, 'ai_tools.stage').trigger('click')
  await flushPromises()
  expect(mocks.ime).toHaveBeenCalledExactlyOnceWith('buffer.stage', JSON.stringify({ text }))
})
it('keeps Unicode conversion offline and rejects too many or late image selections', async () => {
  mocks.config.mockResolvedValue({ source: 'none', llm: null, jev: null })
  const w = await page()
  await button(w, 'ai_tools.action_latex').trigger('click')
  await w.get('textarea').setValue('\\alpha + \\beta')
  await button(w, 'ai_tools.unicode').trigger('click')
  await flushPromises()
  expect(mocks.call).toHaveBeenCalledWith('unicode', JSON.stringify({ text: '\\alpha + \\beta' }))
  expect(mocks.call.mock.calls.some(([method]) => method === 'run')).toBe(false)
  mocks.open.mockResolvedValueOnce(['a.png', 'b.png', 'c.png', 'd.png'])
  await button(w, 'ai_tools.choose_images').trigger('click')
  await flushPromises()
  expect(w.text()).toContain(t('ai_tools.image_limit'))
  let finish!: (paths: string[]) => void
  mocks.open.mockImplementationOnce(
    () =>
      new Promise<string[]>((resolve) => {
        finish = resolve
      }),
  )
  await button(w, 'ai_tools.choose_images').trigger('click')
  await w.get('textarea').setValue('changed')
  finish(['late.png'])
  await flushPromises()
  expect(w.find('.images').exists()).toBe(false)
})
it('drops replies while paused and never automatically resends after resuming', async () => {
  let finish!: (value: string) => void
  mocks.call.mockImplementation((method: string) =>
    method === 'run'
      ? new Promise<string>((resolve) => {
          finish = resolve
        })
      : Promise.resolve('{}'),
  )
  const w = await page()
  await w.get('textarea').setValue('input')
  await w.get('input[type=checkbox]').setValue(true)
  await button(w, 'ai_tools.run').trigger('click')
  paused.value = true
  await flushPromises()
  finish(JSON.stringify(reply))
  await flushPromises()
  expect(w.find('.result').exists()).toBe(false)
  paused.value = false
  await flushPromises()
  expect(mocks.call.mock.calls.filter(([method]) => method === 'run')).toHaveLength(1)
})
