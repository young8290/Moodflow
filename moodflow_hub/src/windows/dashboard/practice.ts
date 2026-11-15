import type { PracticeCategory, PracticeEvent, PracticeMode } from '@/api'

export const MAX_PRACTICE_CHARS = 300
export const MAX_PRACTICE_EVENTS = 600
export const MAX_PRACTICE_MS = 30 * 60 * 1000
export const PRACTICE_MODES = ['passage', 'timed30', 'timed60', 'timed120'] as const
export const PRACTICE_CATEGORIES = ['chinese', 'english', 'mixed', 'custom'] as const

/** The original Chinese prompts remain available in the passage chooser. */
export const PASSAGES: readonly string[] = [
  '早上推开窗，风里带着一点桂花的味道。楼下的早点铺已经排起了队，蒸笼里冒出的白气把半条街都染得暖暖的。',
  '周末去了一趟城郊的湖边，水面很静，偶尔有几只白鹭贴着水飞过去。坐在长椅上发了一会儿呆，心里慢慢松了下来。',
  '下班路上下起了小雨，便利店门口挤满了躲雨的人。有人递给我一把旧伞，说反正他家就在前面，明天放回店里就好。',
  '学一样新东西，开头总是最难的。把它拆成每天十分钟的小事，坚持一个月再回头看，往往会发现自己已经走了很远。',
]

const BANK: Record<Exclude<PracticeCategory, 'custom'>, readonly string[]> = {
  chinese: PASSAGES,
  english: [
    'The small library opens just as the morning rain slows. A reader leaves a note between two pages, returns the book, and takes a quiet seat by the window.',
    'We started the walk before sunrise. The trail was narrow, the air was cool, and each bend showed us another part of the valley below.',
    'Keep the first draft simple. Write what you saw, change the parts you can explain better, and read the last sentence out loud before you put the page away.',
  ],
  mixed: [
    '周五 17:30 前把草稿存为 draft-02.txt。到会议室 B12 后，先用 Wi-Fi 连上内网，再打开 README 检查最后两项。',
    '这次要验收 3 个按钮：Save、Undo 和 Clear。测试记录用 UTF-8 编码，文件名写成 test-notes-03.md。',
    '列车于 08:45 从 A3 站台出发。请提前 15 分钟到站，手机里留一份 PDF 车票，抵达后从 Gate 2 离开。',
  ],
}

export interface TrainingRound {
  category: PracticeCategory
  mode: PracticeMode
  target: string
}

export function timeLimit(mode: PracticeMode): number {
  switch (mode) {
    case 'timed30':
      return 30_000
    case 'timed60':
      return 60_000
    case 'timed120':
      return 120_000
    case 'passage':
      return MAX_PRACTICE_MS
  }
}

export function passageChoices(category: Exclude<PracticeCategory, 'custom'>, mode: PracticeMode): string[] {
  const passages = BANK[category]
  if (mode === 'passage') return [...passages]
  // Give timed rounds a longer passage while keeping the scoring and review bounded.
  return passages.map((_, i) =>
    clipText(
      [passages[i], passages[(i + 1) % passages.length], passages[(i + 2) % passages.length]].join('\n'),
    ),
  )
}

export function clipText(value: string): string {
  return [...value.slice(0, MAX_PRACTICE_CHARS * 2)].slice(0, MAX_PRACTICE_CHARS).join('')
}

export function scoredTarget(round: TrainingRound, text: string): string {
  if (round.mode === 'passage') return round.target
  return [...round.target].slice(0, Math.max(1, [...text].length)).join('')
}

export class PracticeRecorder {
  private startedAt: number | null = null
  readonly events: PracticeEvent[] = []
  constructor(private readonly now: () => number = () => performance.now()) {}

  get started(): boolean {
    return this.startedAt !== null
  }

  get text(): string {
    return this.events.at(-1)?.text ?? ''
  }

  record(text: string, at = this.elapsed()): void {
    if (this.text === text || this.events.length >= MAX_PRACTICE_EVENTS) return
    this.startedAt ??= this.now()
    this.events.push({ t_ms: at, text })
  }

  elapsed(): number {
    return this.startedAt === null ? 0 : Math.max(0, Math.floor(this.now() - this.startedAt))
  }
}

export function reachedEnd(typed: string, target: string): boolean {
  return [...typed].length >= [...target].length
}
