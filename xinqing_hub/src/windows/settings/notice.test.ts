import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { NOTICE, parseNotice, spans } from './notice'
import PrivacyNotice from './PrivacyNotice.vue'

describe('隐私说明全文（产品书 21 第 1 节）', () => {
  it('解析标题、段落、列表、表格与加粗 / 代码', () => {
    const b = parseNotice(
      [
        '# 标题',
        '',
        '## 1. 小节',
        '第一行 **重点**',
        '第二行 `code`',
        '',
        '- 甲',
        '- 乙',
        '',
        '| 列 | 值 |',
        '|---|---|',
        '| a | **b** |',
      ].join('\n'),
    )
    expect(b.map((x) => x.kind)).toEqual(['h1', 'h2', 'p', 'ul', 'table'])
    expect(b[2]).toEqual({
      kind: 'p',
      lines: [
        [{ text: '第一行 ' }, { text: '重点', bold: true }],
        [{ text: '第二行 ' }, { text: 'code', code: true }],
      ],
    })
    const table = b[4] as Extract<(typeof b)[number], { kind: 'table' }>
    expect(table.head.map((c) => c[0]!.text)).toEqual(['列', '值'])
    expect(table.rows).toEqual([[[{ text: 'a' }], [{ text: 'b', bold: true }]]])
  })

  it('文本里的 HTML 原样当文字，不会被插进页面', () => {
    expect(spans('<img src=x onerror=alert(1)>')).toEqual([{ text: '<img src=x onerror=alert(1)>' }])
  })

  it('随包的说明有 11 个小节，处理信息表与保留期限表都在', () => {
    expect(NOTICE[0]).toEqual({ kind: 'h1', text: 'Moodflow 隐私说明' })
    expect(NOTICE.filter((b) => b.kind === 'h2')).toHaveLength(11)
    const tables = NOTICE.filter((b) => b.kind === 'table')
    expect(tables).toHaveLength(2)
  })

  it('折叠着不渲染，展开后显示全文', async () => {
    const w = mount(PrivacyNotice)
    expect(w.find('.doc').exists()).toBe(false)
    const d = w.find('details')
    ;(d.element as HTMLDetailsElement).open = true
    await d.trigger('toggle')
    await flushPromises()
    expect(w.find('.doc .title').text()).toBe('Moodflow 隐私说明')
    expect(w.findAll('.doc h5')).toHaveLength(11)
    expect(w.text()).toContain('12356')
  })
})
