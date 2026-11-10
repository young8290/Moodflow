import { expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import MathPreview from './MathPreview.vue'
it('renders local math and rejects active content, remote assets and unbounded macros', async () => {
  const w = mount(MathPreview, { props: { text: 'x^2 + \\alpha' } })
  await flushPromises()
  expect(w.find('.katex').exists()).toBe(true)
  for (const text of [
    '\\includegraphics{https://example.invalid/x}',
    '\\href{javascript:alert(1)}{x}',
    '<img src=x onerror=alert(1)>',
    '\\def\\x{\\x}\\x',
    '{'.repeat(65) + 'x' + '}'.repeat(65),
  ]) {
    await w.setProps({ text })
    await flushPromises()
    expect(w.find('img').exists()).toBe(false)
    expect(w.find('a').exists()).toBe(false)
    expect(w.find('[role=status]').exists()).toBe(true)
  }
  await w.setProps({ text: 'y_2' })
  await flushPromises()
  expect(w.find('.katex').exists()).toBe(true)
  w.unmount()
})
