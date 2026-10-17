// 桌宠（ADR 0044）：小组件的身体是一张 Codex 桌宠格式的精灵图。这里是不碰窗口与计时器的部分：
// 九种状态在图上的行、帧数和一轮时长（与 Codex 相同），心晴的状态与事件该放哪一段动画，
// 以及扔出去以后的下落、反弹和在屏幕底边散步的路线。都是纯函数，有单测。
import type { Weather } from '@/api'

export const CELL_W = 192
export const CELL_H = 208
export const COLUMNS = 8

export type PetState =
  | 'idle'
  | 'running-right'
  | 'running-left'
  | 'waving'
  | 'jumping'
  | 'failed'
  | 'waiting'
  | 'running'
  | 'review'

export interface StateSpec {
  row: number
  frames: number
  /** 播一轮的毫秒数 */
  ms: number
}

/** Codex 桌宠的九行（v2 多出的两行留给别的应用，这里不用）。 */
export const STATES: Record<PetState, StateSpec> = {
  idle: { row: 0, frames: 6, ms: 1100 },
  'running-right': { row: 1, frames: 8, ms: 1060 },
  'running-left': { row: 2, frames: 8, ms: 1060 },
  waving: { row: 3, frames: 4, ms: 700 },
  jumping: { row: 4, frames: 5, ms: 840 },
  failed: { row: 5, frames: 8, ms: 1220 },
  waiting: { row: 6, frames: 6, ms: 1010 },
  running: { row: 7, frames: 6, ms: 820 },
  review: { row: 8, frames: 6, ms: 1030 },
}

/** 播到第 `elapsed` 毫秒时是第几帧（循环）。 */
export function frameAt(state: PetState, elapsed: number): number {
  const s = STATES[state]
  const per = s.ms / s.frames
  return Math.floor(Math.max(0, elapsed) / per) % s.frames
}

/** 精灵图上第几行第几帧的位置（以格为单位，乘缩放后的格宽高就是 background-position）。 */
export function cellOf(state: PetState, frame: number): { col: number; row: number } {
  return { col: frame, row: STATES[state].row }
}

export interface Mood {
  weather: Weather
  /** 闭眼：暂停、无痕或密码框 */
  eyesClosed: boolean
  focusing: boolean
}

/**
 * 没有事件时的样子：专注中“认真看”，闭眼时打盹，低落和疲惫时安静地等，其他时候自在地待着。
 * 不会因为心情不好就播“失败”那一段（C-ETH：不评判用户）。
 */
export function restState(m: Mood): PetState {
  if (m.eyesClosed) return 'waiting'
  if (m.focusing) return 'review'
  if (m.weather === 'rain' || m.weather === 'night') return 'waiting'
  return 'idle'
}

/** 小组件上发生的事 → 播一段反应。 */
export type PetEvent =
  | 'typo' // 打错字：吓一跳
  | 'message' // 新的暖心话、提醒、周信：招手
  | 'click' // 被点了一下：招手
  | 'landed' // 扔出去落地：跳一下
  | 'error' // 操作失败（我们自己的错）：哭一下
  | 'focus_done' // 专注结束：跑两圈
  | 'calm' // 求助相关的消息：不表演，安静待着

export interface Reaction {
  state: PetState
  /** 播几轮 */
  loops: number
}

/** 闭眼时不表演（FR-WGT-03：暂停时不晃）；求助入口出现时不表演（安全优先，ADR 0028）。 */
export function reactionFor(e: PetEvent, m: Mood): Reaction | null {
  if (m.eyesClosed || e === 'calm') return null
  switch (e) {
    case 'typo':
      return { state: 'jumping', loops: 1 }
    case 'message':
      return { state: 'waving', loops: 2 }
    case 'click':
      return { state: 'waving', loops: 1 }
    case 'landed':
      return { state: 'jumping', loops: 1 }
    case 'error':
      return { state: 'failed', loops: 1 }
    case 'focus_done':
      return { state: 'running', loops: 2 }
  }
}

export interface Bounds {
  /** 窗口左上角能到的范围（物理像素） */
  minX: number
  maxX: number
  minY: number
  /** 地面：窗口贴着工作区底边时的 y */
  floorY: number
}

export interface Body {
  x: number
  y: number
  /** 像素 / 毫秒 */
  vx: number
  vy: number
}

/** 重力（像素 / 毫秒²，按显示缩放再乘） */
export const GRAVITY = 0.0028
/** 落地、撞墙后剩下的速度 */
export const BOUNCE = 0.42
/** 地面上的滑动摩擦（每毫秒） */
export const FRICTION = 0.994
/** 松手时速度低于这个（像素 / 毫秒）就算放下，不扔 */
export const THROW_MIN_SPEED = 0.6

/** 松手：甩得够快才扔出去（之后受重力落到地面）；轻轻放下就停在原处。 */
export function isThrow(vx: number, vy: number, scale = 1): boolean {
  return Math.hypot(vx, vy) >= THROW_MIN_SPEED * scale
}

/**
 * 扔出去以后走一步（`dt` 毫秒）：受重力，撞左右墙和天花板反弹，落到地面反弹几下后停住。
 * 返回新的位置速度，以及这一步是否落地（第一次碰到地面）和是否已经停稳。
 */
export function throwStep(
  b: Body,
  dt: number,
  bounds: Bounds,
  scale = 1,
): { body: Body; landed: boolean; rest: boolean } {
  let { x, y, vx, vy } = b
  vy += GRAVITY * scale * dt
  x += vx * dt
  y += vy * dt
  let landed = false
  if (x < bounds.minX) {
    x = bounds.minX
    vx = -vx * BOUNCE
  } else if (x > bounds.maxX) {
    x = bounds.maxX
    vx = -vx * BOUNCE
  }
  if (y < bounds.minY) {
    y = bounds.minY
    vy = -vy * BOUNCE
  }
  if (y >= bounds.floorY) {
    y = bounds.floorY
    landed = vy > 0.15 * scale
    vy = landed ? -vy * BOUNCE : 0
    vx *= FRICTION ** dt
  }
  const rest = y === bounds.floorY && vy === 0 && Math.abs(vx) < 0.02 * scale
  return { body: { x, y, vx: rest ? 0 : vx, vy }, landed, rest }
}

/** 在地面上吗（容一个像素的误差） */
export function onFloor(y: number, bounds: Bounds): boolean {
  return Math.abs(y - bounds.floorY) <= 1
}

/** 散步：每次走 80–320 像素（按缩放），不出工作区；`rand` 取 0–1，便于测试。 */
export function walkTarget(x: number, bounds: Bounds, rand: () => number, scale = 1): number {
  const dist = (80 + rand() * 240) * scale
  const right = rand() < 0.5
  let to = right ? x + dist : x - dist
  // 碰到边就往回走
  if (to > bounds.maxX) to = x - dist
  if (to < bounds.minX) to = x + dist
  return Math.min(Math.max(to, bounds.minX), bounds.maxX)
}

/** 散步速度（像素 / 毫秒，按缩放） */
export const WALK_SPEED = 0.07
/** 两次散步之间等多久（毫秒）：25–70 秒 */
export function walkDelay(rand: () => number): number {
  return 25_000 + rand() * 45_000
}

/** 要不要散步：开着设置、在地面上、没在专注 / 闭眼 / 被悬停 / 开着面板，且没要求减少动态效果。 */
export function mayRoam(o: {
  enabled: boolean
  reducedMotion: boolean
  mood: Mood
  busy: boolean
  grounded: boolean
}): boolean {
  return o.enabled && !o.reducedMotion && !o.mood.focusing && !o.mood.eyesClosed && !o.busy && o.grounded
}

/** 三档大小：精灵图缩放到多少 */
export const SIZES = { small: 0.5, medium: 0.75, large: 1 } as const
export type PetSize = keyof typeof SIZES

export function petScale(v: unknown): number {
  return typeof v === 'string' && v in SIZES ? SIZES[v as PetSize] : SIZES.medium
}
