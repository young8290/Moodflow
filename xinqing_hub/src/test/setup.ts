// Vitest 全局准备：所有测试都用假的 lottie 播放器（见 lottieFake.ts）。
import { vi } from 'vitest'

vi.mock('lottie-web/build/player/lottie_light', () => import('./lottieFake'))
