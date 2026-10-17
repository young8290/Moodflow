// 桌宠怎么动（ADR 0044）：按住拖着走（往哪边拖就往哪边跑）；甩出去会受重力落到屏幕底边、弹几下再停，
// 轻轻放下就停在原处；在地上时隔一阵子沿底边散步。单击（没拖动）交给 `onClick`。
// 窗口位置都是物理像素；拖动时一直接着鼠标，停下后统一吸附、记住位置（useWidgetWindow.drive）。
import { onBeforeUnmount, type Ref } from 'vue'
import {
  WALK_SPEED,
  isThrow,
  mayRoam,
  onFloor,
  throwStep,
  walkDelay,
  walkTarget,
  type Body,
  type Mood,
  type PetEvent,
  type PetState,
} from './pet'
import type { PetArea } from './useWidgetWindow'

/** 按下后移动超过 4 px 才算拖动（FR-WGT-06：否则是单击） */
const DRAG_THRESHOLD = 4
/** 算松手速度只看最后这么久的移动 */
const SAMPLE_MS = 90

export interface PetMotionDeps {
  area: () => Promise<PetArea | null>
  moveTo: (x: number, y: number) => Promise<void>
  drive: (on: boolean) => Promise<void>
  motion: Ref<PetState | null>
  react: (e: PetEvent) => void
  mood: Ref<Mood | null>
  roam: Ref<boolean>
  reducedMotion: Ref<boolean>
  /** 被悬停、开着面板或菜单时不散步 */
  busy: Ref<boolean>
  onClick: () => void
}

export function usePetMotion(d: PetMotionDeps) {
  /** 按下的位置；窗口位置异步取回后填进 `area`，取回之前的移动不算拖动 */
  let press: { sx: number; sy: number; id: number; area: PetArea | null } | null = null
  let dragging = false
  let samples: { t: number; x: number; y: number }[] = []
  let raf = 0
  let walkTimer: ReturnType<typeof setTimeout> | undefined
  let walking = false
  let last = { x: 0, y: 0 }

  function stopAnim(): void {
    cancelAnimationFrame(raf)
    raf = 0
  }

  async function end(): Promise<void> {
    stopAnim()
    walking = false
    d.motion.value = null
    await d.drive(false)
    scheduleWalk()
  }

  function onPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return
    stopAnim()
    walking = false
    const p = { sx: e.screenX, sy: e.screenY, id: e.pointerId, area: null as PetArea | null }
    press = p
    void d.area().then((a) => {
      if (press === p) p.area = a
    })
  }

  function onPointerMove(e: PointerEvent): void {
    if (!press?.area) return
    const area = press.area
    const dx = e.screenX - press.sx
    const dy = e.screenY - press.sy
    if (!dragging) {
      if (Math.hypot(dx, dy) <= DRAG_THRESHOLD) return
      dragging = true
      samples = []
      last = { x: area.x, y: area.y }
      clearTimeout(walkTimer)
      void d.drive(true)
      try {
        ;(e.currentTarget as Element | null)?.setPointerCapture?.(press.id)
      } catch {
        // 拿不到指针捕获也能拖，只是移出窗口时可能断
      }
    }
    const k = area.scale
    const x = Math.min(Math.max(area.x + dx * k, area.minX), area.maxX)
    const y = Math.min(Math.max(area.y + dy * k, area.minY), area.floorY)
    const dirX = x - last.x
    if (Math.abs(dirX) > 1) d.motion.value = dirX > 0 ? 'running-right' : 'running-left'
    last = { x, y }
    samples.push({ t: e.timeStamp, x, y })
    while (samples.length > 2 && e.timeStamp - (samples[0]?.t ?? e.timeStamp) > SAMPLE_MS) samples.shift()
    void d.moveTo(x, y)
  }

  function onPointerUp(): void {
    const wasPress = press !== null
    const area = press?.area ?? null
    press = null
    if (!dragging) {
      if (wasPress) {
        d.react('click')
        d.onClick()
      }
      return
    }
    dragging = false
    const a = samples[0]
    const b = samples[samples.length - 1]
    const dt = a && b ? b.t - a.t : 0
    const vx = a && b && dt > 0 ? (b.x - a.x) / dt : 0
    const vy = a && b && dt > 0 ? (b.y - a.y) / dt : 0
    const scale = area?.scale ?? 1
    if (!area || d.reducedMotion.value || !isThrow(vx, vy, scale)) {
      void end()
      return
    }
    fly({ x: last.x, y: last.y, vx, vy }, area)
  }

  /** 扔出去：每帧走一步，落地跳一下，停稳后吸附 */
  function fly(start: Body, area: PetArea): void {
    let body = start
    let prev = performance.now()
    const bounds = { minX: area.minX, maxX: area.maxX, minY: area.minY, floorY: area.floorY }
    const step = (now: number) => {
      const dt = Math.min(now - prev, 40)
      prev = now
      const r = throwStep(body, dt, bounds, area.scale)
      body = r.body
      if (Math.abs(body.vx) > 0.05) d.motion.value = body.vx > 0 ? 'running-right' : 'running-left'
      else d.motion.value = null
      if (r.landed) d.react('landed')
      void d.moveTo(body.x, body.y)
      if (r.rest) void end()
      else raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
  }

  /** 沿底边散步一段 */
  async function walk(): Promise<void> {
    const mood = d.mood.value
    const area = await d.area()
    if (
      !area ||
      !mood ||
      dragging ||
      raf !== 0 ||
      !mayRoam({
        enabled: d.roam.value,
        reducedMotion: d.reducedMotion.value,
        mood,
        busy: d.busy.value,
        grounded: onFloor(area.y, area),
      })
    ) {
      scheduleWalk()
      return
    }
    const to = walkTarget(area.x, area, Math.random, area.scale)
    const dir = to > area.x ? 1 : -1
    let x = area.x
    let prev = performance.now()
    walking = true
    await d.drive(true)
    d.motion.value = dir > 0 ? 'running-right' : 'running-left'
    const step = (now: number) => {
      // 被悬停、开了面板、开始专注：停下
      if (!walking || d.busy.value || d.mood.value?.focusing) {
        void end()
        return
      }
      const dt = Math.min(now - prev, 40)
      prev = now
      x += dir * WALK_SPEED * area.scale * dt
      const done = dir > 0 ? x >= to : x <= to
      void d.moveTo(done ? to : x, area.floorY)
      if (done) void end()
      else raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
  }

  function scheduleWalk(): void {
    clearTimeout(walkTimer)
    if (!d.roam.value) return
    walkTimer = setTimeout(() => void walk().catch(() => scheduleWalk()), walkDelay(Math.random))
  }

  onBeforeUnmount(() => {
    stopAnim()
    clearTimeout(walkTimer)
  })

  function onPointerCancel(): void {
    press = null
    if (dragging) {
      dragging = false
      void end()
    }
  }

  return {
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    scheduleWalk,
    stop: () => (walking = false),
  }
}
