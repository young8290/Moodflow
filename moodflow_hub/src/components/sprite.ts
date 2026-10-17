// 天气小精灵的 lottie 动画数据（16 D-03、07 DS-MOTION-02）。文件由 tools/gen_sprite_lottie.py 生成（不要手改），
// 每种天气一个，用标记分出五段：loop（4 秒循环）、shake（晃一下 0.5 秒）、approach（靠近 0.6 秒）、
// close（闭眼 0.3 秒，倒着播是睁眼）、sleep（闭眼时的循环）、focus（专注计时中的“专心”循环，ADR 0041）。
import type { Weather } from '@/api'
import cloudy from '@/assets/sprite/cloudy.json'
import night from '@/assets/sprite/night.json'
import rain from '@/assets/sprite/rain.json'
import storm from '@/assets/sprite/storm.json'
import sunny from '@/assets/sprite/sunny.json'
import wind from '@/assets/sprite/wind.json'

export type SpriteSegment = 'loop' | 'shake' | 'approach' | 'close' | 'sleep' | 'focus'

export interface SpriteMarker {
  /** 起始帧 */
  tm: number
  /** 段名 */
  cm: string
  /** 帧数 */
  dr: number
}

export interface SpriteData {
  fr: number
  nm: string
  markers: SpriteMarker[]
  layers: { nm: string; ty: number; cl?: string; shapes?: unknown[]; ks: Record<string, unknown> }[]
}

export const SPRITES: Record<Weather, SpriteData> = { sunny, wind, cloudy, rain, storm, night }

/** 某一段的 [起始帧, 结束帧] */
export function segment(data: SpriteData, name: SpriteSegment): [number, number] {
  const m = data.markers.find((x) => x.cm === name)
  if (!m) throw new Error(`小精灵动画 ${data.nm} 缺少 ${name} 段`)
  return [m.tm, m.tm + m.dr]
}
