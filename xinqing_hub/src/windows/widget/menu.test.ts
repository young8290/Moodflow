import { describe, expect, it } from 'vitest'
import type { StatusSnapshot } from '@/api'
import { menuEntries } from './menu'

const snap: StatusSnapshot = {
  state: 'fluent',
  weather: 'sunny',
  prob: null,
  offline: false,
  paused: false,
  connected: true,
  baseline_progress: 100,
}

describe('menuEntries', () => {
  it('按规格顺序：我现在…、开始专注…、暂停感知、打开看板、待确认日程与待办、设置、隐藏小组件', () => {
    expect(menuEntries(snap).map((e) => e.text)).toEqual([
      '我现在…',
      '开始专注…',
      '暂停感知',
      '打开看板',
      '待确认日程与待办',
      '设置',
      '隐藏小组件',
    ])
  })

  it('有待确认的日程与待办时带上数量', () => {
    expect(menuEntries(snap, 3)[4]).toEqual({ id: 'pending', text: '待确认日程与待办（3）' })
  })

  it('已暂停时换成“恢复感知”', () => {
    expect(menuEntries({ ...snap, paused: true })[2]).toEqual({ id: 'resume', text: '恢复感知' })
  })

  it('还没拿到状态时按未暂停处理', () => {
    expect(menuEntries(null)[2]?.id).toBe('pause')
  })

  it('解锁了装扮才出现“换装”，排在专注后面（FR-WGT-08）', () => {
    expect(menuEntries(snap).some((e) => e.id === 'outfit')).toBe(false)
    expect(menuEntries(snap, 0, false, true)[2]).toEqual({ id: 'outfit', text: '换装' })
  })

  it('专注中换成“结束专注”（FR-RST-10）', () => {
    expect(menuEntries(snap, 0, true)[1]).toEqual({ id: 'focus_stop', text: '结束专注' })
  })
})
