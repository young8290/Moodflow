import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import type { Weather } from '@/api'
import { instances } from '@/test/lottieFake'
import WeatherSprite from './WeatherSprite.vue'
import { SPRITES, segment, type SpriteData } from './sprite'

const ALL: Weather[] = ['sunny', 'wind', 'cloudy', 'rain', 'storm', 'night']
type Sprite = { play: (a: 'shake' | 'approach') => void }

const last = () => instances[instances.length - 1]!
let reduce = false

beforeEach(() => {
  instances.length = 0
  reduce = false
  vi.stubGlobal('matchMedia', (q: string) => ({
    matches: q.includes('reduced-motion') && reduce,
    addEventListener: () => {},
    removeEventListener: () => {},
  }))
})
afterEach(() => vi.unstubAllGlobals())

describe('WeatherSprite（lottie）', () => {
  it('读屏名称形如“心晴：多云”，lottie 画的 SVG 对读屏隐藏', () => {
    const w = mount(WeatherSprite, { props: { weather: 'cloudy' } })
    expect(w.attributes('role')).toBe('img')
    expect(w.attributes('aria-label')).toBe('心晴：多云')
    expect(w.find('.host').attributes('aria-hidden')).toBe('true')
  })

  it('每种天气加载自己的动画，睁眼时循环播 loop 段', () => {
    for (const weather of ALL) {
      mount(WeatherSprite, { props: { weather } })
      expect(last().data.nm).toBe(`xq-sprite-${weather}`)
      expect(last().loop).toBe(true)
      expect(last().playSegments).toHaveBeenLastCalledWith(segment(SPRITES[weather], 'loop'), true)
    }
  })

  it('play() 播一次性的段，播完回到循环；闭着眼时不晃', async () => {
    const w = mount(WeatherSprite, { props: { weather: 'sunny' } })
    const a = last()
    ;(w.vm as unknown as Sprite).play('shake')
    expect(a.loop).toBe(false)
    expect(a.playSegments).toHaveBeenLastCalledWith(segment(SPRITES.sunny, 'shake'), true)
    a.complete()
    expect(a.loop).toBe(true)
    expect(a.playSegments).toHaveBeenLastCalledWith(segment(SPRITES.sunny, 'loop'), true)

    await w.setProps({ eyesClosed: true })
    a.complete()
    a.playSegments.mockClear()
    ;(w.vm as unknown as Sprite).play('approach')
    expect(a.playSegments).not.toHaveBeenCalled()
  })

  it('闭眼：先播 close 再循环 sleep；睁眼倒着播 close 再回到 loop', async () => {
    const w = mount(WeatherSprite, { props: { weather: 'rain' } })
    const a = last()
    const [from, to] = segment(SPRITES.rain, 'close')
    await w.setProps({ eyesClosed: true })
    expect(a.playSegments).toHaveBeenLastCalledWith([from, to], true)
    a.complete()
    expect(a.playSegments).toHaveBeenLastCalledWith(segment(SPRITES.rain, 'sleep'), true)
    await w.setProps({ eyesClosed: false })
    expect(a.playSegments).toHaveBeenLastCalledWith([to, from], true)
    a.complete()
    expect(a.playSegments).toHaveBeenLastCalledWith(segment(SPRITES.rain, 'loop'), true)
  })

  it('一开始就闭着眼：直接循环 sleep', () => {
    mount(WeatherSprite, { props: { weather: 'night', eyesClosed: true } })
    expect(last().playSegments).toHaveBeenLastCalledWith(segment(SPRITES.night, 'sleep'), true)
  })

  it('系统关闭动画时不播，只停在第一帧（DS-MOTION-03）', async () => {
    reduce = true
    const w = mount(WeatherSprite, { props: { weather: 'storm' } })
    const a = last()
    expect(a.playSegments).not.toHaveBeenCalled()
    expect(a.goToAndStop).toHaveBeenLastCalledWith(segment(SPRITES.storm, 'loop')[0], true)
    ;(w.vm as unknown as Sprite).play('shake')
    expect(a.playSegments).not.toHaveBeenCalled()
    await w.setProps({ eyesClosed: true })
    expect(a.goToAndStop).toHaveBeenLastCalledWith(segment(SPRITES.storm, 'sleep')[0], true)
  })

  it('窗口不可见时暂停，回来接着播（DS-MOTION-05）', () => {
    mount(WeatherSprite, { props: { weather: 'wind' } })
    const a = last()
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(a.pause).toHaveBeenCalled()
    hidden.mockReturnValue(false)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(a.play).toHaveBeenCalled()
    hidden.mockRestore()
  })

  it('换天气时重新加载，卸载时销毁', async () => {
    const w = mount(WeatherSprite, { props: { weather: 'sunny' } })
    const first = last()
    await w.setProps({ weather: 'cloudy' })
    await flushPromises()
    expect(first.destroy).toHaveBeenCalled()
    expect(last().data.nm).toBe('xq-sprite-cloudy')
    w.unmount()
    expect(last().destroy).toHaveBeenCalled()
  })
})

describe('小精灵动画数据（tools/gen_sprite_lottie.py）', () => {
  const seconds = (d: SpriteData, name: Parameters<typeof segment>[1]) => {
    const [a, b] = segment(d, name)
    return (b - a) / d.fr
  }
  const CLASSES = new Set(['xq-fill-w', 'xq-fill-on', 'xq-line-on', 'xq-line-text'])

  it('每种天气都有五段，时长符合 DS-MOTION-02', () => {
    for (const w of ALL) {
      const d = SPRITES[w]
      expect(seconds(d, 'loop'), w).toBeGreaterThanOrEqual(2)
      expect(seconds(d, 'loop'), w).toBeLessThanOrEqual(4)
      expect(seconds(d, 'shake'), w).toBeCloseTo(0.5)
      expect(seconds(d, 'approach'), w).toBeCloseTo(0.6)
      expect(seconds(d, 'close'), w).toBeGreaterThan(0)
      expect(seconds(d, 'sleep'), w).toBeGreaterThan(0)
    }
  })

  it('形状图层都带上色用的 class，且只有一种样式（颜色走设计令牌）', () => {
    for (const w of ALL) {
      for (const l of SPRITES[w].layers.filter((x) => x.ty === 4)) {
        expect(CLASSES.has(l.cl ?? ''), `${w}/${l.nm}`).toBe(true)
        const items = (l.shapes as { it: { ty: string }[] }[])[0]!.it
        expect(
          items.filter((i) => i.ty === 'fl' || i.ty === 'st'),
          `${w}/${l.nm}`,
        ).toHaveLength(1)
      }
    }
  })

  it('每种天气的配饰图层各不相同（DS-COLOR-02：状态不只靠颜色区分）', () => {
    const accessory = (w: Weather) =>
      SPRITES[w].layers
        .filter((l) => l.ty === 4 && !/^(body|eyes|mouth)/.test(l.nm))
        .map((l) => JSON.stringify(l.shapes))
        .join('|')
    expect(new Set(ALL.map(accessory)).size).toBe(ALL.length)
    for (const w of ALL) expect(accessory(w), w).not.toBe('')
  })

  it('闪电一个循环只暗一次，远低于每秒 3 次（DS-MOTION-04）', () => {
    const bolt = SPRITES.storm.layers.find((l) => l.nm === 'bolt')!
    const op = (bolt.ks.o as { k: { s: number[] }[] }).k.map((k) => k.s[0]!)
    const dips = op.filter((v, i) => v < 100 && (op[i - 1] ?? 100) === 100).length
    expect(dips / seconds(SPRITES.storm, 'loop')).toBeLessThanOrEqual(3)
    expect(dips).toBe(1)
  })
})
