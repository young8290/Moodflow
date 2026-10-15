// “常规”“感知”“温柔改写”里存在输入法配置的几项（xinqing.*，ADR 0016）：经 ime_config_get / ime_config_set 读写，
// 恢复默认取 ime_config_defaults（wind-rpc `config.getDefaults`）。输入法没在运行时这几项不可改，页面照常显示其余设置。
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { CommandError, commands, events, unwrap } from '@/api'
import { errorText } from '@/i18n'
import { getPath } from './imeForm'

export function useImeKeys(keys: readonly string[]) {
  const values = shallowRef<unknown>(null)
  /** null：还在读；false：输入法没在运行或读不到 */
  const available = ref<boolean | null>(null)
  const error = ref<string | null>(null)
  let unlisten: (() => void) | undefined
  let disposed = false

  async function load(): Promise<void> {
    try {
      values.value = (await unwrap(commands.imeConfigGet())).values
      available.value = true
    } catch (e) {
      available.value = false
      if (!(e instanceof CommandError && e.ui.code === 'ime.unavailable')) error.value = errorText(e)
    }
  }

  onMounted(async () => {
    window.addEventListener('focus', load)
    await load()
    const off = await events.imeConfigChanged.listen(() => void load())
    if (disposed) off()
    else unlisten = off
  })
  onBeforeUnmount(() => {
    disposed = true
    window.removeEventListener('focus', load)
    unlisten?.()
  })

  const get = (key: string): unknown => getPath(values.value, key)

  /** 写一项；核心跳过时把原因放进 `error`，返回是否写成 */
  async function set(key: string, value: unknown): Promise<boolean> {
    error.value = null
    try {
      const r = await unwrap(commands.imeConfigSet([{ key, value }]))
      const skipped = r.skipped.find((s) => s.key === key)
      if (skipped) error.value = skipped.reason
      await load()
      return !skipped
    } catch (e) {
      error.value = errorText(e)
      return false
    }
  }

  /** 把 `keys` 改回输入法的出厂值（只写和当前不同的）；返回是否全部写成 */
  async function restore(): Promise<boolean> {
    if (!available.value) return true
    error.value = null
    try {
      const defaults = (await unwrap(commands.imeConfigDefaults())).values
      const items = keys
        .map((key) => ({ key, value: getPath(defaults, key) }))
        .filter((i) => i.value !== undefined && JSON.stringify(i.value) !== JSON.stringify(get(i.key)))
      if (items.length === 0) return true
      const r = await unwrap(commands.imeConfigSet(items))
      if (r.skipped[0]) error.value = r.skipped[0].reason
      await load()
      return r.skipped.length === 0
    } catch (e) {
      error.value = errorText(e)
      return false
    }
  }

  return { values, available, error, get, set, restore, load }
}
