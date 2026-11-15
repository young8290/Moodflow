import { describe, expect, it } from 'vitest'
import {
  clipText,
  MAX_PRACTICE_CHARS,
  passageChoices,
  PASSAGES,
  PracticeRecorder,
  reachedEnd,
  scoredTarget,
} from './practice'
import { reviewText } from './practiceReview'

describe('打字训练', () => {
  it('保留原有的中文范文，并为限时模式提供更长的有界练习', () => {
    expect(passageChoices('chinese', 'passage')).toEqual(PASSAGES)
    for (const kind of ['chinese', 'english', 'mixed'] as const) {
      const brief = passageChoices(kind, 'passage')
      const timed = passageChoices(kind, 'timed60')
      expect(new Set(timed).size).toBe(brief.length)
      expect(timed.every((p, i) => p.startsWith(brief[i]!) && [...p].length <= MAX_PRACTICE_CHARS)).toBe(true)
    }
  })

  it('从第一个上屏字符开始计时，重复内容不记', () => {
    let now = 100
    const r = new PracticeRecorder(() => now)
    r.record('')
    expect(r.started).toBe(false)
    now = 1000
    r.record('今')
    now = 1500
    r.record('今')
    now = 2000
    r.record('今天')
    expect(r.events).toEqual([
      { t_ms: 0, text: '今' },
      { t_ms: 1000, text: '今天' },
    ])
    now = 3000
    expect(r.elapsed()).toBe(2000)
  })

  it('全文和限时使用不同的计分范围，按 Unicode 字符计数', () => {
    const target = '今日☀𠮷村'
    expect(reachedEnd('今日☀𠮷', target)).toBe(false)
    expect(reachedEnd(target, target)).toBe(true)
    expect(scoredTarget({ mode: 'timed60', category: 'custom', target }, '今日☀𠮷')).toBe('今日☀𠮷')
    expect(scoredTarget({ mode: 'passage', category: 'custom', target }, '今')).toBe(target)
    expect(clipText('𠮷'.repeat(301))).toBe('𠮷'.repeat(300))
  })

  it('复盘对齐整个输入，识别中途漏字、错字和末尾多字', () => {
    expect(reviewText('甲乙𠮷丁戊己', '甲𠮷红戊己!')).toEqual([
      { kind: 'match', expected: '甲', actual: '甲' },
      { kind: 'missing', expected: '乙', actual: '' },
      { kind: 'match', expected: '𠮷', actual: '𠮷' },
      { kind: 'replace', expected: '丁', actual: '红' },
      { kind: 'match', expected: '戊己', actual: '戊己' },
      { kind: 'extra', expected: '', actual: '!' },
    ])
  })
})
