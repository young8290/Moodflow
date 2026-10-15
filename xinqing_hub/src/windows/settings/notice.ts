// 隐私说明全文（产品书 21 第 1 节，hub_templates/privacy_notice.md，E 维护）在设置页的显示：
// 只认这份文本用到的几种 Markdown 写法——标题、段落、列表、表格、**加粗**、`代码`——拆成结构交给模板渲染，
// 不用 v-html，文本里即使混进了 HTML 也只会原样显示。
import noticeText from '../../../../hub_templates/privacy_notice.md?raw'

/** 一段文字里的片段：普通、加粗或代码 */
export interface Span {
  text: string
  bold?: boolean
  code?: boolean
}

export type Block =
  | { kind: 'h1' | 'h2'; text: string }
  | { kind: 'p'; lines: Span[][] }
  | { kind: 'ul'; items: Span[][] }
  | { kind: 'table'; head: Span[][]; rows: Span[][][] }

/** `**加粗**` 与 `` `代码` `` → 片段 */
export function spans(line: string): Span[] {
  const out: Span[] = []
  const re = /\*\*(.+?)\*\*|`([^`]+)`/g
  let last = 0
  for (const m of line.matchAll(re)) {
    if (m.index > last) out.push({ text: line.slice(last, m.index) })
    if (m[1] !== undefined) out.push({ text: m[1], bold: true })
    else out.push({ text: m[2]!, code: true })
    last = m.index + m[0].length
  }
  if (last < line.length) out.push({ text: line.slice(last) })
  return out
}

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\||\|$/g, '')
    .split('|')
    .map((c) => spans(c.trim()))

export function parseNotice(md: string): Block[] {
  const blocks: Block[] = []
  const lines = md.replace(/\r\n/g, '\n').split('\n')
  let i = 0
  while (i < lines.length) {
    const line = lines[i]!
    if (line.trim() === '' || line.trim() === '---') {
      i++
    } else if (line.startsWith('# ')) {
      blocks.push({ kind: 'h1', text: line.slice(2).trim() })
      i++
    } else if (line.startsWith('## ')) {
      blocks.push({ kind: 'h2', text: line.slice(3).trim() })
      i++
    } else if (line.startsWith('|')) {
      const rows: Span[][][] = []
      while (i < lines.length && lines[i]!.startsWith('|')) {
        // 表头下面的分隔行 |---|---|
        if (!/^\|[\s|:-]+\|?$/.test(lines[i]!.trim())) rows.push(cells(lines[i]!))
        i++
      }
      blocks.push({ kind: 'table', head: rows[0] ?? [], rows: rows.slice(1) })
    } else if (line.startsWith('- ')) {
      const items: Span[][] = []
      while (i < lines.length && lines[i]!.startsWith('- ')) items.push(spans(lines[i++]!.slice(2)))
      blocks.push({ kind: 'ul', items })
    } else {
      const para: Span[][] = []
      while (i < lines.length) {
        const l = lines[i]!
        if (l.trim() === '' || /^(#|\||- )/.test(l)) break
        para.push(spans(l))
        i++
      }
      blocks.push({ kind: 'p', lines: para })
    }
  }
  return blocks
}

export const NOTICE: Block[] = parseNotice(noticeText)
