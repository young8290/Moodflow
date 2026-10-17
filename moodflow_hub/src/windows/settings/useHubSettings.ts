// 设置中心各分类共用的 Hub 设置读写（07 FR-SET-01）：修改即保存、底部短暂提示“已保存”、每个分类“恢复默认”。
// 取值范围、选项与出厂值一律取自 settings_schema（ADR 0019 第 9 条），前端不另抄一份。
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { commands, type SettingField, type SettingValue } from '@/api'
import { errorText, t } from '@/i18n'
import { useSettingsStore } from '@/stores/settings'

/** 提示显示多久（毫秒） */
const NOTICE_MS = 2000

/** 底部的短暂提示（“已保存”“已恢复默认”），2 秒后消失 */
export function useNotice() {
  const notice = ref<string | null>(null)
  let timer: ReturnType<typeof setTimeout> | undefined
  onBeforeUnmount(() => clearTimeout(timer))
  function flash(text: string): void {
    notice.value = text
    clearTimeout(timer)
    timer = setTimeout(() => (notice.value = null), NOTICE_MS)
  }
  return { notice, flash }
}

export function sameValue(a: SettingValue | undefined, b: SettingValue | undefined): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

/**
 * 读入 `keys` 并订阅变化；`restore()` 把这些键改回出厂值（只改和出厂值不同的，逐个写，写不进的停下报错）。
 * `notice` 是“已保存”“已恢复默认”这类短暂提示，`error` 是最近一次失败的原因。
 */
export function useHubSettings(keys: readonly string[]) {
  const settings = useSettingsStore()
  const schema = shallowRef<Record<string, SettingField>>({})
  const ready = ref(false)
  const error = ref<string | null>(null)
  const { notice, flash } = useNotice()

  onMounted(async () => {
    try {
      const [fields] = await Promise.all([commands.settingsSchema(), settings.init([...keys])])
      schema.value = Object.fromEntries(fields.map((f) => [f.key, f]))
      ready.value = true
    } catch (e) {
      error.value = errorText(e)
    }
  })

  function value(key: string): SettingValue | undefined {
    return settings.values[key] ?? schema.value[key]?.default
  }
  const bool = (key: string): boolean => value(key) === true
  const num = (key: string): number | null => {
    const v = value(key)
    return typeof v === 'number' ? v : null
  }
  const text = (key: string): string => {
    const v = value(key)
    return typeof v === 'string' ? v : ''
  }

  /** 数值键的范围；schema 还没读到时为 null */
  function range(key: string): { min: number; max: number } | null {
    const k = schema.value[key]?.kind
    if (!k || (k.type !== 'number' && k.type !== 'int') || k.min === null || k.max === null) return null
    return { min: k.min, max: k.max }
  }

  /** 枚举键的选项 */
  function options(key: string): string[] {
    const k = schema.value[key]?.kind
    return k?.type === 'choice' ? k.options : []
  }

  async function save(key: string, v: SettingValue): Promise<boolean> {
    error.value = null
    try {
      await settings.set(key, v)
      flash(t('settings.saved'))
      return true
    } catch (e) {
      error.value = errorText(e)
      return false
    }
  }

  async function restore(): Promise<boolean> {
    error.value = null
    try {
      for (const key of keys) {
        const d = schema.value[key]?.default
        if (d !== undefined && !sameValue(settings.values[key], d)) await settings.set(key, d)
      }
      flash(t('settings.restored'))
      return true
    } catch (e) {
      error.value = errorText(e)
      return false
    }
  }

  return {
    settings,
    schema,
    ready,
    error,
    notice,
    flash,
    value,
    bool,
    num,
    text,
    range,
    options,
    save,
    restore,
  }
}
