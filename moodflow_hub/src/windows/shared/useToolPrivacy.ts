import { onBeforeUnmount, onMounted, ref } from 'vue'
import { commands, events, unwrap } from '@/api'
import type { UnlistenFn } from '@tauri-apps/api/event'

/** Tool pages start closed until the authoritative privacy snapshot arrives. */
export function useToolPrivacy() {
  const paused = ref(true)
  let stop: UnlistenFn | undefined
  let disposed = false
  onMounted(async () => {
    let receivedEvent = false
    try {
      const unlisten = await events.statusChanged.listen((event) => {
        receivedEvent = true
        paused.value = event.payload.paused
      })
      if (disposed) return unlisten()
      stop = unlisten
      const snapshot = await unwrap(commands.getStatus())
      if (!disposed && !receivedEvent) paused.value = snapshot.paused
    } catch {
      paused.value = true
    }
  })
  onBeforeUnmount(() => {
    disposed = true
    stop?.()
  })
  return paused
}
