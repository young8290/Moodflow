import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({
  save: vi.fn(),
  exportRun: vi.fn(),
}))
const FILES = [
  { name: 'README.txt', what: 'readme' },
  { name: 'ai.json', what: 'ai' },
  { name: 'hub-log.txt', what: 'log' },
]

vi.mock('@tauri-apps/plugin-dialog', () => ({ save: mocks.save }))
vi.mock('@/api', async (orig) => ({
  ...(await orig<typeof import('@/api')>()),
  commands: {
    diagnosticsPreview: async () => FILES,
    diagnosticsExport: mocks.exportRun,
  },
}))

const { default: RunInfoExport } = await import('./RunInfoExport.vue')

describe('导出运行信息（FR-OPS-04）', () => {
  beforeEach(() => {
    mocks.save.mockReset().mockResolvedValue('C:\\Users\\me\\run.zip')
    mocks.exportRun.mockReset().mockResolvedValue({ status: 'ok', data: 3 })
  })

  it('先列出会打包的文件和说明，确认后选位置再导出', async () => {
    const w = mount(RunInfoExport)
    expect(w.text()).not.toContain('诊断')
    await w.find('button').trigger('click')
    await flushPromises()
    const items = w.findAll('.files li').map((li) => li.text())
    expect(items).toEqual([
      `README.txt ${t('about.file_readme')}`,
      `ai.json ${t('about.file_ai')}`,
      `hub-log.txt ${t('about.file_log')}`,
    ])
    expect(mocks.exportRun).not.toHaveBeenCalled()
    await w.find('button.primary').trigger('click')
    await flushPromises()
    expect(mocks.save.mock.calls[0]![0].filters[0].extensions).toEqual(['zip'])
    expect(mocks.exportRun).toHaveBeenCalledWith('C:\\Users\\me\\run.zip')
    expect(w.find('[role=status]').text()).toBe(t('about.run_info_done', { path: 'C:\\Users\\me\\run.zip' }))
    expect(w.find('.files').exists()).toBe(false)
  })

  it('关掉“另存为”不导出；导出失败说明原因', async () => {
    mocks.save.mockResolvedValue(null)
    const w = mount(RunInfoExport)
    await w.find('button').trigger('click')
    await flushPromises()
    await w.find('button.primary').trigger('click')
    await flushPromises()
    expect(mocks.exportRun).not.toHaveBeenCalled()

    mocks.save.mockResolvedValue('D:\\x.zip')
    mocks.exportRun.mockResolvedValue({
      status: 'error',
      error: { code: 'diagnostics.write_failed', message_key: 'error.generic' },
    })
    await w.find('button.primary').trigger('click')
    await flushPromises()
    expect(w.find('[role=alert]').text()).toBe(t('error.generic'))
  })
})
