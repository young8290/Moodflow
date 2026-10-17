// 出网记录与接口状态里的接口名（`jev`、`llm/chat`）→ 中文用途，文案在 [privacy.api]；不认识的显示原值。
import { isCopyKey, t } from '@/i18n'

export function apiName(api: string): string {
  const key = `privacy.api.${api.replace('/', '_')}`
  return isCopyKey(key) ? t(key) : api
}
