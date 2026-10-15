// 专注计时（06 FR-RST-10）的显示用纯函数：倒计时 mm:ss、结束卡片那一行、自定义时长校验。
import type { FocusSummary } from '@/api'
import { t } from '@/i18n'

export const PRESETS = [25, 45, 60] as const
export const MIN_MINUTES = 10
export const MAX_MINUTES = 120
/** 结束卡片建议的休息时长（秒） */
export const REST_SECONDS = 5 * 60

/** 125 → "2:05"；一小时以上 "1:02:05" */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const ss = String(s % 60).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
}

/** 自定义时长：10–120 的整数，否则 null。数字输入框的 v-model 给的是数，空着时是空串 */
export function customMinutes(raw: string | number): number | null {
  if (typeof raw === 'string' && raw.trim() === '') return null
  const n = Number(raw)
  return Number.isInteger(n) && n >= MIN_MINUTES && n <= MAX_MINUTES ? n : null
}

/** “专注 25 分钟 · 按键 1,832 次 · 上屏约 960 字” */
export function doneLine(s: FocusSummary): string {
  const n = (x: number) => x.toLocaleString('en-US')
  return t('focus.done_line', { min: s.actual_min, keys: n(s.keys), chars: n(s.chars) })
}
