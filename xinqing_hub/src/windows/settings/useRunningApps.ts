// “从正在运行的程序中选择”（07 FR-SET-04/05）：程序名输入框获得焦点时向后端要一次正在运行的程序名，
// 放进输入框的 <datalist> 当候选（系统自带的下拉，键盘和读屏都能用）。只有可执行文件名，只在本机。
import { ref } from 'vue'
import { commands } from '@/api'

export function useRunningApps() {
  const apps = ref<string[]>([])
  let loaded = false

  async function load(): Promise<void> {
    if (loaded) return
    loaded = true
    try {
      apps.value = await commands.runningApps()
    } catch {
      // 取不到就没有候选，照样可以手填
      loaded = false
    }
  }

  return { apps, load }
}
