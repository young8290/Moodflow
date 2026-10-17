// 设置页里的单独同意开关（07 FR-SET-04 / 07 / 10，09 第 2.1 节）：读 consent_get，改用 consent_set。
// 撤回会把数据发给第三方的那几项时如实说明：已经发出去的收不回来（FR-DAT-05）。
import { onMounted, reactive, ref, shallowRef } from 'vue'
import { commands, unwrap, type ConsentItem, type ConsentState } from '@/api'
import { errorText } from '@/i18n'
import { sendsToThirdParty } from '../shared/consent'

export function useConsents() {
  const state = shallowRef<ConsentState | null>(null)
  const error = ref<string | null>(null)
  /** 这次打开设置后撤回过的项：在开关下面留一句说明 */
  const withdrawn = reactive(new Set<ConsentItem>())

  onMounted(async () => {
    try {
      state.value = await unwrap(commands.consentGet())
    } catch (e) {
      error.value = errorText(e)
    }
  })

  /** null：还没读到（开关先不可点） */
  function granted(item: ConsentItem): boolean | null {
    if (!state.value) return null
    return state.value.items.find((i) => i.item === item)?.granted ?? false
  }

  async function set(item: ConsentItem, on: boolean): Promise<boolean> {
    error.value = null
    try {
      state.value = await unwrap(commands.consentSet(item, on))
      if (!on && sendsToThirdParty(item)) withdrawn.add(item)
      else withdrawn.delete(item)
      return true
    } catch (e) {
      error.value = errorText(e)
      return false
    }
  }

  return { state, error, withdrawn, granted, set }
}
