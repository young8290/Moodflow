import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import InputTools from './InputTools.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({ tools: vi.fn(), open: vi.fn(), save: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, save: mocks.save }))
vi.mock('@/api', () => ({ commands: { imeTools: mocks.tools }, unwrap: async (p: Promise<unknown>) => p }))
const profile = {
  version: 1,
  name: '测试方案',
  left_keys: ['a'],
  right_keys: ['j'],
  interval_ms: 300,
  mappings: [{ keys: ['a', 'j'], output: 'ni', kind: 'syllable' }],
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.tools.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'storage.health'
        ? { available: false }
        : method === 'chord.get' || method === 'chord.save'
          ? { text: JSON.stringify(profile), revision: 'draft-1', activeRevision: 'active-1', enabled: false }
          : method === 'scheme.rimePreview'
            ? {
                sha256: 'preview-hash',
                schemes: [
                  {
                    id: 'rime-demo',
                    name: '演示方案',
                    engine: '码表',
                    preview: { willAdd: ['demo'], conflicts: [] },
                  },
                ],
                diagnostics: ['拼写规则需要另行核对'],
              }
            : method === 'scheme.rimeImport'
              ? { results: [{ id: 'rime-demo', result: { imported: ['demo'], conflicts: [] } }] }
              : { text: JSON.stringify(profile) },
    ),
  )
})
const click = async (wrapper: ReturnType<typeof mount>, text: string) => {
  const button = wrapper.findAll('button').find((b) => b.text() === text)
  expect(button).toBeDefined()
  await button!.trigger('click')
  await flushPromises()
}

describe('Windows 输入工具', () => {
  it('显示持续的存储故障，编辑键位后仅按显式保存写入，并带上两份 revision', async () => {
    const wrapper = mount(InputTools)
    await flushPromises()
    expect(wrapper.text()).toContain(t('tools.storage_warning'))
    await wrapper.get('input[maxlength="128"]').setValue('我的方案')
    const key = wrapper.findAll('.keyboard button').find((b) => b.text() === 'A')!
    await key.trigger('click')
    expect(mocks.tools.mock.calls.some((c) => c[0] === 'chord.save')).toBe(false)
    await click(wrapper, t('tools.save_draft'))
    const call = mocks.tools.mock.calls.find((c) => c[0] === 'chord.save')!
    const payload = JSON.parse(call[1] as string)
    expect(payload).toMatchObject({ revision: 'draft-1', activeRevision: 'active-1', apply: false })
    expect(JSON.parse(payload.text)).toMatchObject({ name: '我的方案', right_keys: ['j', 'a'] })
    wrapper.unmount()
  })
  it('Rime 先预览再确认导入，冻结源包哈希且默认不覆盖冲突', async () => {
    mocks.open.mockResolvedValue('D:/rime archive.zip')
    const wrapper = mount(InputTools)
    await flushPromises()
    await click(wrapper, t('tools.choose_zip'))
    expect(wrapper.text()).toContain('演示方案')
    expect(mocks.tools.mock.calls.some((c) => c[0] === 'scheme.rimeImport')).toBe(false)
    await click(wrapper, t('tools.import_selected'))
    const call = mocks.tools.mock.calls.find((c) => c[0] === 'scheme.rimeImport')!
    expect(JSON.parse(call[1] as string)).toMatchObject({
      path: 'D:/rime archive.zip',
      sha256: 'preview-hash',
      ids: ['rime-demo'],
      strategy: 'merge',
    })
    expect(mocks.tools.mock.calls.some((c) => c[0] === 'buffer.stage')).toBe(false)
    wrapper.unmount()
  })
})
