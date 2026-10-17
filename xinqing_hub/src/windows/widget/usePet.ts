// 桌宠的动画（ADR 0044）：读当前宠物的精灵图（设置 `pet.id`，找不到时后端给内置的晴晴），按“动作 → 反应 → 平时”
// 的优先级决定播哪一行：被拖着、扔着、散步时播跑动；事件来了播一段反应（播完回到平时）；平时按心情与专注决定。
// 系统要求减少动态效果时只画每段的第一帧，不播反应。
import { computed, onBeforeUnmount, ref, shallowRef, watch, type Ref } from 'vue'
import { commands, unwrap, type PetSheet } from '@/api'
import { useSettingsStore } from '@/stores/settings'
import {
  STATES,
  cellOf,
  frameAt,
  petScale,
  reactionFor,
  restState,
  type Mood,
  type PetEvent,
  type PetState,
} from './pet'

export const PET_KEYS = ['pet.id', 'pet.size', 'pet.roam'] as const

function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches
  } catch {
    return false
  }
}

export function usePet(mood: Ref<Mood | null>) {
  const settings = useSettingsStore()
  const sheet = shallowRef<PetSheet | null>(null)
  const reducedMotion = ref(prefersReducedMotion())
  const id = computed(() => {
    const v = settings.values['pet.id']
    return typeof v === 'string' ? v : 'qingqing'
  })
  const scale = computed(() => petScale(settings.values['pet.size']))
  const roam = computed(() => settings.values['pet.roam'] !== false)

  /** 拖动、扔、散步时的跑动方向；为 null 时看反应与平时 */
  const motion = ref<PetState | null>(null)
  const reaction = ref<{ state: PetState; until: number } | null>(null)
  const rest = computed<PetState>(() => (mood.value ? restState(mood.value) : 'idle'))
  const state = computed<PetState>(() => motion.value ?? reaction.value?.state ?? rest.value)

  const col = ref(0)
  let started = performance.now()
  let raf = 0

  watch(state, () => {
    started = performance.now()
    col.value = 0
  })

  function tick(now: number): void {
    if (reaction.value && now >= reaction.value.until) reaction.value = null
    col.value = reducedMotion.value ? 0 : frameAt(state.value, now - started)
    raf = requestAnimationFrame(tick)
  }

  async function load(): Promise<void> {
    try {
      sheet.value = await unwrap(commands.petSheet(id.value))
    } catch (e) {
      console.warn('读取桌宠精灵图失败', e)
    }
  }

  /** 播一段反应（闭眼、减少动态效果、求助消息时不播） */
  function react(e: PetEvent): void {
    if (reducedMotion.value || !mood.value) return
    const r = reactionFor(e, mood.value)
    if (!r) return
    reaction.value = { state: r.state, until: performance.now() + STATES[r.state].ms * r.loops }
  }

  let media: MediaQueryList | null = null
  const onMedia = (e: MediaQueryListEvent) => (reducedMotion.value = e.matches)

  function start(): void {
    void load()
    try {
      media = window.matchMedia('(prefers-reduced-motion: reduce)')
      media.addEventListener('change', onMedia)
    } catch {
      // 没有 matchMedia（测试环境）就按默认
    }
    raf = requestAnimationFrame(tick)
  }

  watch(id, () => void load())

  onBeforeUnmount(() => {
    cancelAnimationFrame(raf)
    media?.removeEventListener('change', onMedia)
  })

  const cell = computed(() => cellOf(state.value, col.value))

  return { sheet, scale, roam, reducedMotion, state, motion, cell, react, start, reload: load }
}
