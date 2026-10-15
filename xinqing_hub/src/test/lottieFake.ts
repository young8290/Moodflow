// 测试用的假 lottie 播放器：jsdom 没有 canvas，真的 lottie-web 一加载就报错。
// 记下每只小精灵的调用，测试里取 `instances` 核对播了哪一段。
import { vi } from 'vitest'

export interface FakeAnimation {
  data: { nm: string }
  loop: boolean
  playSegments: ReturnType<typeof vi.fn>
  goToAndStop: ReturnType<typeof vi.fn>
  play: ReturnType<typeof vi.fn>
  pause: ReturnType<typeof vi.fn>
  destroy: ReturnType<typeof vi.fn>
  /** 模拟一段播完 */
  complete: () => void
}

export const instances: FakeAnimation[] = []

function loadAnimation(opts: { animationData: { nm: string }; loop: boolean }): FakeAnimation {
  let onComplete = () => {}
  const a: FakeAnimation & { addEventListener: (e: string, cb: () => void) => void } = {
    data: opts.animationData,
    loop: opts.loop,
    playSegments: vi.fn(),
    goToAndStop: vi.fn(),
    play: vi.fn(),
    pause: vi.fn(),
    destroy: vi.fn(),
    complete: () => onComplete(),
    addEventListener: (e, cb) => {
      if (e === 'complete') onComplete = cb
    },
  }
  instances.push(a)
  return a
}

export default { loadAnimation }
