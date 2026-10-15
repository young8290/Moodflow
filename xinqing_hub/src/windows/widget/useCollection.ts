// 晴天收集与换装（05 FR-REV-04、07 FR-WGT-08）：打开时、窗口重新获得焦点时问一次后端（新攒的点数在这时结算解锁），
// 换装与关闭功能时跟着 collection:changed 走。
import { computed, onBeforeUnmount, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { commands, events, unwrap, type CollectionView } from '@/api'

export function useCollection() {
  const view = ref<CollectionView | null>(null)
  const panelOpen = ref(false)
  const unlisten: UnlistenFn[] = []

  /** 正在穿的装扮；关闭晴天收集时不显示 */
  const equipped = computed(() => (view.value?.enabled ? view.value.equipped : null))
  /** 右键菜单里出现“换装”：开着且至少解锁了一件 */
  const canDress = computed(() => !!view.value?.enabled && view.value.items.some((i) => i.unlocked))

  async function refresh(): Promise<void> {
    try {
      view.value = await unwrap(commands.collectionGet())
    } catch (e) {
      console.warn('读取晴天收集失败', e)
    }
  }

  async function init(): Promise<void> {
    const again = () => void refresh()
    window.addEventListener('focus', again)
    unlisten.push(() => window.removeEventListener('focus', again))
    unlisten.push(await events.collectionChanged.listen((e) => (view.value = e.payload.view)))
    await refresh()
  }

  async function equip(item: string | null): Promise<void> {
    view.value = await unwrap(commands.collectionEquip(item))
  }

  onBeforeUnmount(() => {
    for (const u of unlisten) u()
  })

  return { view, panelOpen, equipped, canDress, init, refresh, equip }
}
