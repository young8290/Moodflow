import { describe, expect, it } from 'vitest'
import {
  STATES,
  frameAt,
  isThrow,
  mayRoam,
  petScale,
  reactionFor,
  restState,
  throwStep,
  walkTarget,
  type Bounds,
  type Mood,
} from './pet'

const calm: Mood = { weather: 'sunny', eyesClosed: false, focusing: false }
const bounds: Bounds = { minX: 0, maxX: 1000, minY: 0, floorY: 600 }

describe('桌宠的状态与反应（ADR 0044）', () => {
  it('九行与 Codex 一致', () => {
    expect(Object.values(STATES).map((s) => [s.row, s.frames])).toEqual([
      [0, 6],
      [1, 8],
      [2, 8],
      [3, 4],
      [4, 5],
      [5, 8],
      [6, 6],
      [7, 6],
      [8, 6],
    ])
  })

  it('帧按一轮时长平均分，循环播放', () => {
    expect(frameAt('waving', 0)).toBe(0)
    expect(frameAt('waving', 175)).toBe(1)
    expect(frameAt('waving', 700)).toBe(0)
    expect(frameAt('idle', -5)).toBe(0)
  })

  it('平时：专注认真看，闭眼打盹，下雨和夜里安静等，不会因为心情不好播“失败”', () => {
    expect(restState(calm)).toBe('idle')
    expect(restState({ ...calm, focusing: true })).toBe('review')
    expect(restState({ ...calm, eyesClosed: true, focusing: true })).toBe('waiting')
    expect(restState({ ...calm, weather: 'rain' })).toBe('waiting')
    for (const w of ['sunny', 'wind', 'cloudy', 'rain', 'storm', 'night'] as const)
      expect(restState({ ...calm, weather: w })).not.toBe('failed')
  })

  it('反应：闭眼和求助相关时不表演', () => {
    expect(reactionFor('typo', calm)).toEqual({ state: 'jumping', loops: 1 })
    expect(reactionFor('message', calm)).toEqual({ state: 'waving', loops: 2 })
    expect(reactionFor('error', calm)?.state).toBe('failed')
    expect(reactionFor('calm', calm)).toBeNull()
    expect(reactionFor('typo', { ...calm, eyesClosed: true })).toBeNull()
  })
})

describe('扔、落地与散步', () => {
  it('轻轻放下不算扔', () => {
    expect(isThrow(0.1, 0.2)).toBe(false)
    expect(isThrow(1.2, -0.4)).toBe(true)
    expect(isThrow(1.0, 0, 2)).toBe(false)
  })

  it('扔出去受重力落到地面，弹几下后停稳；撞墙反弹', () => {
    let body = { x: 500, y: 100, vx: 1.5, vy: -0.5 }
    let landed = 0
    let rest = false
    for (let i = 0; i < 2000 && !rest; i++) {
      const r = throwStep(body, 16, bounds)
      body = r.body
      if (r.landed) landed++
      rest = r.rest
      expect(body.x).toBeGreaterThanOrEqual(bounds.minX)
      expect(body.x).toBeLessThanOrEqual(bounds.maxX)
      expect(body.y).toBeLessThanOrEqual(bounds.floorY)
    }
    expect(rest).toBe(true)
    expect(body.y).toBe(bounds.floorY)
    expect(landed).toBeGreaterThanOrEqual(1)
  })

  it('散步不出工作区，碰边往回走', () => {
    expect(walkTarget(500, bounds, () => 0)).toBe(580)
    expect(walkTarget(990, bounds, () => 0.99)).toBeLessThan(990)
    expect(walkTarget(10, bounds, () => 0)).toBeGreaterThan(10)
  })

  it('专注、闭眼、忙着、不在地面、减少动态效果时不散步', () => {
    const base = { enabled: true, reducedMotion: false, mood: calm, busy: false, grounded: true }
    expect(mayRoam(base)).toBe(true)
    expect(mayRoam({ ...base, enabled: false })).toBe(false)
    expect(mayRoam({ ...base, reducedMotion: true })).toBe(false)
    expect(mayRoam({ ...base, mood: { ...calm, focusing: true } })).toBe(false)
    expect(mayRoam({ ...base, busy: true })).toBe(false)
    expect(mayRoam({ ...base, grounded: false })).toBe(false)
  })

  it('大小三档，不认识的按中号', () => {
    expect([petScale('small'), petScale('large'), petScale('huge'), petScale(null)]).toEqual([
      0.5, 1, 0.75, 0.75,
    ])
  })
})
