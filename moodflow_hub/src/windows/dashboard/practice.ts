// 打字测速（XSIME 吸收计划 §3.3）的范文与事件记录。计分在 Rust（`practice_score`，与输入法核心同一份口径）。

/** 内置范文：日常叙述，长度在 40～70 字之间，打一段不到一分钟。 */
export const PASSAGES: readonly string[] = [
  '早上推开窗，风里带着一点桂花的味道。楼下的早点铺已经排起了队，蒸笼里冒出的白气把半条街都染得暖暖的。',
  '周末去了一趟城郊的湖边，水面很静，偶尔有几只白鹭贴着水飞过去。坐在长椅上发了一会儿呆，心里慢慢松了下来。',
  '下班路上下起了小雨，便利店门口挤满了躲雨的人。有人递给我一把旧伞，说反正他家就在前面，明天放回店里就好。',
  '学一样新东西，开头总是最难的。把它拆成每天十分钟的小事，坚持一个月再回头看，往往会发现自己已经走了很远。',
]

/** 换一段：返回与当前不同的下一段（只有一段时原样返回）。 */
export function nextPassage(current: string): string {
  const i = PASSAGES.indexOf(current)
  return PASSAGES[(i + 1) % PASSAGES.length] ?? PASSAGES[0]!
}

export interface PracticeEvent {
  t_ms: number
  text: string
}

/** 记录输入框内容变化。第一次有内容时开始计时，之后的时间都从那一刻算起。 */
export class PracticeRecorder {
  private startedAt: number | null = null
  readonly events: PracticeEvent[] = []

  constructor(private readonly now: () => number = () => performance.now()) {}

  get started(): boolean {
    return this.startedAt !== null
  }

  record(text: string): void {
    if (this.startedAt === null) {
      if (text.length === 0) return
      this.startedAt = this.now()
    }
    const last = this.events[this.events.length - 1]
    if (last && last.text === text) return
    this.events.push({ t_ms: this.now() - this.startedAt, text })
  }

  /** 结束时刻（从开始算起的毫秒）；还没开始时为 0。 */
  elapsed(): number {
    return this.startedAt === null ? 0 : this.now() - this.startedAt
  }
}

/** 打到和范文一样长（按字符数）就自动结束。 */
export function reachedEnd(typed: string, target: string): boolean {
  return [...typed].length >= [...target].length
}
