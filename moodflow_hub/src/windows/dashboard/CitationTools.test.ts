import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import CitationTools from './CitationTools.vue'
import { t } from '@/i18n'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  tools: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))
vi.mock('@/api', () => ({
  commands: { citationRequest: mocks.call, imeTools: mocks.tools },
  unwrap: async (p: Promise<unknown>) => p,
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, save: mocks.save, confirm: mocks.confirm }))
const reference = {
  title: '导入文献',
  kind: 'journalArticle',
  authors: 'Smith, Jane',
  year: '2024',
  container: 'Journal',
  doi: '10.1234/example',
  csl: { custom: 'preserved' },
}
const metadata = {
  title: '当前资料',
  reference_type: 'book',
  reference_authors: '作者',
  reference_year: '2024',
  reference_publisher: '出版社',
  reference_doi: '10.1234/example',
}
beforeEach(() => {
  vi.clearAllMocks()
  mocks.confirm.mockResolvedValue(false)
  mocks.open.mockResolvedValue('fixture.bib')
  mocks.call.mockImplementation(async (method: string) =>
    JSON.stringify(
      method === 'format'
        ? { missing: [], text: 'Formatted reference' }
        : method === 'doi'
          ? { reference }
          : { references: [reference] },
    ),
  )
  mocks.tools.mockResolvedValue('{}')
})
const click = async (w: ReturnType<typeof mount>, text: string) => {
  await w
    .findAll('button')
    .find((b) => b.text() === text)!
    .trigger('click')
  await flushPromises()
}

describe('文献工具', () => {
  it('导入仅预览；明确填入才更新编辑器，保留完整 CSL', async () => {
    const w = mount(CitationTools, { props: { metadata } })
    await click(w, t('citation.import'))
    expect(w.emitted('apply')).toBeUndefined()
    expect(mocks.tools).not.toHaveBeenCalled()
    await click(w, t('citation.apply'))
    expect(w.emitted('apply')?.[0]?.[0]).toMatchObject({
      title: reference.title,
      reference_csl: { custom: 'preserved' },
    })
    expect(mocks.call.mock.calls.some(([method]) => method === 'save')).toBe(false)
    w.unmount()
  })

  it('DOI 必须单次确认；格式化也不自动投递或暂存', async () => {
    const w = mount(CitationTools, { props: { metadata } })
    await click(w, t('citation.doi'))
    expect(mocks.call).not.toHaveBeenCalled()
    mocks.confirm.mockResolvedValueOnce(true)
    await click(w, t('citation.doi'))
    expect(mocks.call).toHaveBeenCalledWith('doi', JSON.stringify({ doi: '10.1234/example', consent: true }))
    expect(w.emitted('apply')).toBeUndefined()
    await click(w, t('citation.format'))
    expect(w.find('blockquote').text()).toBe('Formatted reference')
    expect(mocks.tools).not.toHaveBeenCalled()
    await click(w, t('library.stage'))
    expect(mocks.tools).toHaveBeenCalledWith('buffer.stage', JSON.stringify({ text: 'Formatted reference' }))
    w.unmount()
  })
})
