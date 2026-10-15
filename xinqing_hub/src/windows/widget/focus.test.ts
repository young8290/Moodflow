import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { t } from '@/i18n'
import { clock, customMinutes, doneLine } from './focus'
import FocusCard from '../cards/FocusCard.vue'

describe('专注计时的显示（FR-RST-10）', () => {
  it('倒计时写成 m:ss，一小时以上带小时', () => {
    expect(clock(1500)).toBe('25:00')
    expect(clock(65)).toBe('1:05')
    expect(clock(0)).toBe('0:00')
    expect(clock(3725)).toBe('1:02:05')
  })

  it('自定义时长只收 10～120 的整数', () => {
    expect(customMinutes('30')).toBe(30)
    expect(customMinutes(120)).toBe(120)
    expect(customMinutes('')).toBeNull()
    expect(customMinutes(9)).toBeNull()
    expect(customMinutes('12.5')).toBeNull()
  })

  it('结束卡片：一行小结，开始休息后倒数 5 分钟；不出现评价', async () => {
    const summary = { start_ms: 0, planned_min: 25, actual_min: 25, keys: 1832, chars: 960, completed: true }
    expect(doneLine(summary)).toBe('专注 25 分钟 · 按键 1,832 次 · 上屏约 960 字')
    const w = mount(FocusCard, { props: { summary: { ...summary, actual_min: 12, completed: false } } })
    expect(w.text()).toContain('专注 12 分钟')
    expect(w.text()).toContain(t('focus.rest_hint'))
    await w.find('button.primary').trigger('click')
    expect(w.find('[role=timer]').text()).toBe(t('focus.resting', { time: '5:00' }))
    await w.findAll('button').at(-1)?.trigger('click')
  })
})
