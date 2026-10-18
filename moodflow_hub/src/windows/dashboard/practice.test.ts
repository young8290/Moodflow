import { describe, expect, it } from 'vitest'
import { nextPassage, PASSAGES, PracticeRecorder, reachedEnd } from './practice'

describe('打字测速', () => {
  it('范文长度合适且互不相同', () => {
    expect(new Set(PASSAGES).size).toBe(PASSAGES.length)
    for (const p of PASSAGES) {
      expect([...p].length).toBeGreaterThanOrEqual(40)
      expect([...p].length).toBeLessThanOrEqual(70)
    }
  })

  it('换一段总是换到另一段', () => {
    for (const p of PASSAGES) expect(nextPassage(p)).not.toBe(p)
  })

  it('从第一个字开始计时，重复内容不记', () => {
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

  it('按字符数判断打完', () => {
    expect(reachedEnd('今天', '今天')).toBe(true)
    expect(reachedEnd('今', '今天')).toBe(false)
  })
})
