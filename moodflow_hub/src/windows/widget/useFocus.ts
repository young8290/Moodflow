// 小组件上的专注计时（06 FR-RST-10）：打开时问一次后端（窗口重开后接上倒计时），之后跟着 focus:tick / focus:done 走。
import { onBeforeUnmount, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { commands, events, unwrap } from '@/api'

export function useFocus() {
  const active = ref(false)
  const remaining = ref(0)
  const panelOpen = ref(false)
  const unlisten: UnlistenFn[] = []

  async function init(): Promise<void> {
    unlisten.push(
      await events.focusTick.listen((e) => {
        active.value = true
        remaining.value = e.payload.remaining_s
      }),
      await events.focusDone.listen(() => {
        active.value = false
        remaining.value = 0
      }),
    )
    const s = await commands.focusStatus()
    active.value = s.active
    remaining.value = s.remaining_s
  }

  async function start(minutes: number): Promise<void> {
    const s = await unwrap(commands.focusStart(minutes))
    active.value = s.active
    remaining.value = s.remaining_s
    panelOpen.value = false
  }

  async function stop(): Promise<void> {
    await commands.focusStop()
    active.value = false
    remaining.value = 0
  }

  onBeforeUnmount(() => {
    for (const u of unlisten) u()
  })

  return { active, remaining, panelOpen, init, start, stop }
}
