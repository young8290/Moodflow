import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import WorkspaceTerminal from './WorkspaceTerminal.vue'

const mocks = vi.hoisted(() => ({ call: vi.fn(), inputs: [] as ((text: string) => void)[], osc: vi.fn() }))
vi.mock('./workspaceTypes', () => ({ workspaceCall: mocks.call }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), confirm: vi.fn() }))
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit() {}
  },
}))
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    cols = 96
    rows = 24
    parser = { registerOscHandler: mocks.osc }
    onData(callback: (text: string) => void) {
      mocks.inputs.push(callback)
    }
    onBinary() {}
    loadAddon() {}
    open() {}
    write() {}
    reset() {}
    focus() {}
    dispose() {}
    resize(cols: number, rows: number) {
      this.cols = cols
      this.rows = rows
    }
  },
}))
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  mocks.inputs.length = 0
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  )
  mocks.call.mockResolvedValue({ data: '', after: 0, gap: false, running: true, exit_code: null })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

it('drops queued input after pause and reattach without ending the native process', async () => {
  let release!: () => void
  mocks.call.mockImplementation((method: string) =>
    method === 'terminal_input'
      ? new Promise<void>((resolve) => {
          release = resolve
        })
      : Promise.resolve({ data: '', after: 0, gap: false, running: true, exit_code: null }),
  )
  const wrapper = mount(WorkspaceTerminal, { props: { id: 'fixture-a', paused: false } })
  await flushPromises()
  expect(mocks.osc).toHaveBeenCalledWith(52, expect.any(Function))
  mocks.inputs[0]!('x'.repeat(9000))
  mocks.inputs[0]!('queued stale keystrokes')
  await flushPromises()
  expect(mocks.call.mock.calls.filter(([method]) => method === 'terminal_input')).toHaveLength(1)
  await wrapper.setProps({ paused: true })
  await wrapper.setProps({ paused: false, id: 'fixture-b' })
  await flushPromises()
  release()
  await flushPromises()
  const inputs = mocks.call.mock.calls.filter(([method]) => method === 'terminal_input')
  expect(inputs).toHaveLength(1)
  expect(inputs[0]![1]).toMatchObject({ id: 'fixture-a' })
  wrapper.unmount()
  expect(mocks.call.mock.calls.some(([method]) => method === 'terminal_end')).toBe(false)
})
