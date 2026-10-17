// 图表用色：从设计令牌读当前值（tokens.css），ECharts 拿不到 CSS 变量，只能在画之前解析成具体颜色。
// 规则（dataviz 规范）：线和点用 --mf-chart-1；网格、坐标轴一档灰（--mf-border），轴上文字用文字令牌，不用系列色。

export interface ChartColors {
  /** 单系列的线与点 */
  series: string
  /** 图表底色：点外圈的 2 px 描边用它，把点和线分开 */
  surface: string
  /** 网格线、坐标轴、参考线 */
  grid: string
  /** 轴标签、参考线标签 */
  axisText: string
  /** 提示框正文 */
  text: string
  /** 提示框底色 */
  tooltipBg: string
  /** 热力图里“0”的格子：比底色深一档，看得出格子在那儿 */
  empty: string
  /** 打字心电图上成串退格的标记（07 FR-DSH-02 规定用红色；另有三角形状与图例，不只靠颜色） */
  alert: string
}

/** 读不到令牌时（测试环境）的回落值，与浅色令牌一致 */
const FALLBACK: ChartColors = {
  series: '#c47a1f',
  surface: '#ffffff',
  grid: '#e7e1d8',
  axisText: '#746e64',
  text: '#2b2a28',
  tooltipBg: '#ffffff',
  empty: '#f6f3ee',
  alert: '#c23a2e',
}

/**
 * 令牌的值可能是系统颜色（高对比度主题下是 `CanvasText`、`Canvas` 等，tokens.css 的 forced-colors 一节），
 * ECharts 认不出这些名字。借一个看不见的元素让浏览器算出实际的 rgb()，算不出（测试环境）再用令牌原文。
 */
export function chartColors(root: Element = document.documentElement): ChartColors {
  const css = getComputedStyle(root)
  const probe = document.createElement('span')
  probe.style.display = 'none'
  ;(document.body ?? root).append(probe)
  const v = (name: string, fallback: string) => {
    const raw = css.getPropertyValue(name).trim()
    if (!raw) return fallback
    probe.style.color = ''
    probe.style.color = `var(${name})`
    const computed = getComputedStyle(probe).color
    return computed.startsWith('rgb') ? computed : raw
  }
  try {
    return {
      series: v('--mf-chart-1', FALLBACK.series),
      surface: v('--mf-surface', FALLBACK.surface),
      grid: v('--mf-border', FALLBACK.grid),
      axisText: v('--mf-text-3', FALLBACK.axisText),
      text: v('--mf-text-1', FALLBACK.text),
      tooltipBg: v('--mf-surface', FALLBACK.tooltipBg),
      empty: v('--mf-surface-2', FALLBACK.empty),
      alert: v('--mf-danger', FALLBACK.alert),
    }
  } finally {
    probe.remove()
  }
}

export { FALLBACK as FALLBACK_CHART_COLORS }
