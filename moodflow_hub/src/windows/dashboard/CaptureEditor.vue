<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { t } from '@/i18n'
import { capturePoint, captureCrop, type CaptureImage, type CaptureCrop, type Point } from './captureTypes'
const props = defineProps<{ image: CaptureImage; busy: boolean }>()
const emit = defineEmits<{ save: [png: string]; selection: [crop: CaptureCrop | null] }>()
type Tool = 'region' | 'rectangle' | 'arrow' | 'pen' | 'blur' | 'label'
interface Mark {
  tool: Tool
  start: Point
  end: Point
  points: Point[]
  text: string
  color: string
  width: number
}
const tool = ref<Tool>('region')
const marks = ref<Mark[]>([])
const draft = ref<Mark | null>(null)
const color = ref('#e34036')
const stroke = ref(4)
const text = ref('')
const ratio = ref('free')
const selection = ref<CaptureCrop | null>(null)
const error = ref('')
const exporting = ref(false)
const padding = ref(0)
const rounding = ref(0)
const background = ref('#eee9e2')
const shadow = ref(false)
const canvas = ref<HTMLCanvasElement | null>(null)
const pointer = ref<Point | null>(null)
let disposed = false
let generation = 0
let bitmap: HTMLImageElement | null = null
const source = computed(() => convertFileSrc(props.image.path))
const width = computed(() => props.image.asset.width)
const height = computed(() => props.image.asset.height)
const viewMarks = computed(() =>
  draft.value && draft.value.tool !== 'region' ? [...marks.value, draft.value] : marks.value,
)
const path = (mark: Mark) => mark.points.map((p, index) => `${index ? 'L' : 'M'}${p.x},${p.y}`).join(' ')
const pointCount = () =>
  marks.value.reduce((sum, mark) => sum + mark.points.length, 0) + (draft.value?.points.length ?? 0)
const region = (mark: Mark) => ({
  x: Math.min(mark.start.x, mark.end.x),
  y: Math.min(mark.start.y, mark.end.y),
  width: Math.abs(mark.end.x - mark.start.x),
  height: Math.abs(mark.end.y - mark.start.y),
})
function point(event: PointerEvent): Point {
  return capturePoint(
    event.clientX,
    event.clientY,
    (event.currentTarget as SVGElement).getBoundingClientRect(),
    width.value,
    height.value,
  )
}
function begin(event: PointerEvent): void {
  if (
    props.busy ||
    exporting.value ||
    marks.value.length >= 200 ||
    pointCount() >= 32768 ||
    event.button !== 0
  )
    return
  const p = point(event)
  draft.value = {
    tool: tool.value,
    start: p,
    end: p,
    points: [p],
    text: text.value.slice(0, 256),
    color: color.value,
    width: stroke.value,
  }
  ;(event.currentTarget as SVGElement).setPointerCapture(event.pointerId)
}
function move(event: PointerEvent): void {
  const p = point(event)
  pointer.value = p
  if (!draft.value || exporting.value) return
  draft.value.end = p
  if (draft.value.tool === 'pen' && draft.value.points.length < 4096 && pointCount() < 32768)
    draft.value.points.push(p)
  if (draft.value.tool === 'region') {
    selection.value = captureCrop(
      draft.value.start,
      p,
      width.value,
      height.value,
      ratio.value === 'square' ? 1 : ratio.value === 'wide' ? 16 / 9 : null,
    )
    emit('selection', selection.value.width && selection.value.height ? selection.value : null)
  }
}
function finish(): void {
  if (!draft.value) return
  if (draft.value.tool !== 'region') marks.value.push(draft.value)
  draft.value = null
}
function reset(): void {
  marks.value = []
  selection.value = null
  draft.value = null
  emit('selection', null)
}
watch(
  () => props.image.asset.id,
  () => {
    generation++
    reset()
  },
)
async function save(crop: boolean): Promise<void> {
  if (exporting.value || props.busy) return
  const current = generation
  exporting.value = true
  error.value = ''
  try {
    bitmap = new Image()
    bitmap.crossOrigin = 'anonymous'
    bitmap.src = source.value
    await bitmap.decode()
    if (disposed || current !== generation) return
    const region =
      crop && selection.value ? selection.value : { x: 0, y: 0, width: width.value, height: height.value }
    const output = document.createElement('canvas')
    canvas.value = output
    output.width = region.width
    output.height = region.height
    const context = output.getContext('2d')
    if (!context) throw new Error()
    context.translate(-region.x, -region.y)
    context.drawImage(bitmap, 0, 0)
    for (const mark of marks.value) {
      context.strokeStyle = mark.color
      context.fillStyle = mark.color
      context.lineWidth = mark.width
      context.lineCap = 'round'
      context.lineJoin = 'round'
      const x = Math.min(mark.start.x, mark.end.x)
      const y = Math.min(mark.start.y, mark.end.y)
      if (mark.tool === 'rectangle')
        context.strokeRect(x, y, Math.abs(mark.end.x - mark.start.x), Math.abs(mark.end.y - mark.start.y))
      if (mark.tool === 'blur')
        context.fillRect(x, y, Math.abs(mark.end.x - mark.start.x), Math.abs(mark.end.y - mark.start.y))
      if (mark.tool === 'label') {
        context.font = `${Math.max(14, mark.width * 5)}px sans-serif`
        context.fillText(mark.text, mark.start.x, mark.start.y)
      }
      if (mark.tool === 'pen' || mark.tool === 'arrow') {
        context.beginPath()
        context.moveTo(mark.start.x, mark.start.y)
        for (const p of mark.tool === 'pen' ? mark.points : [mark.end]) context.lineTo(p.x, p.y)
        context.stroke()
        if (mark.tool === 'arrow') {
          const angle = Math.atan2(mark.end.y - mark.start.y, mark.end.x - mark.start.x)
          const length = Math.max(12, mark.width * 4)
          context.beginPath()
          context.moveTo(mark.end.x, mark.end.y)
          context.lineTo(
            mark.end.x - length * Math.cos(angle - Math.PI / 6),
            mark.end.y - length * Math.sin(angle - Math.PI / 6),
          )
          context.lineTo(
            mark.end.x - length * Math.cos(angle + Math.PI / 6),
            mark.end.y - length * Math.sin(angle + Math.PI / 6),
          )
          context.closePath()
          context.fill()
        }
      }
    }
    const pad = Math.max(0, Math.min(160, Math.round(Number(padding.value))))
    const radius = Math.max(0, Math.min(80, Number(rounding.value)))
    if (!Number.isFinite(pad) || !Number.isFinite(radius)) throw new Error()
    const framed = document.createElement('canvas')
    framed.width = output.width + pad * 2
    framed.height = output.height + pad * 2
    if (framed.width > 32768 || framed.height > 32768 || framed.width * framed.height > 64000000)
      throw new Error()
    const frame = framed.getContext('2d')
    if (!frame) throw new Error()
    frame.fillStyle = background.value
    frame.fillRect(0, 0, framed.width, framed.height)
    const clippedRadius = Math.min(radius, output.width / 2, output.height / 2)
    frame.beginPath()
    frame.roundRect(pad, pad, output.width, output.height, clippedRadius)
    if (shadow.value && pad > 0) {
      frame.shadowColor = '#00000055'
      frame.shadowBlur = Math.min(pad, 24)
      frame.shadowOffsetY = Math.min(pad / 3, 8)
      frame.fillStyle = '#ffffff'
      frame.fill()
      frame.shadowColor = 'transparent'
      frame.shadowOffsetY = 0
    }
    frame.clip()
    frame.drawImage(output, pad, pad)
    const png = (pad || radius || shadow.value ? framed : output).toDataURL('image/png')
    framed.width = 0
    if (png.length > 24 * 1024 * 1024) throw new Error()
    emit('save', png)
  } catch {
    error.value = t('capture.failed')
  } finally {
    exporting.value = false
    if (canvas.value) canvas.value.width = 0
    canvas.value = null
    if (bitmap) bitmap.src = ''
    bitmap = null
  }
}
onBeforeUnmount(() => {
  disposed = true
  if (bitmap) bitmap.src = ''
  if (canvas.value) canvas.value.width = 0
  marks.value = []
  draft.value = null
})
</script>
<template>
  <div class="editor">
    <div class="controls">
      <label
        >{{ t('capture.padding')
        }}<input v-model.number="padding" type="number" min="0" max="160" :disabled="busy || exporting"
      /></label>
      <label
        >{{ t('capture.rounding')
        }}<input v-model.number="rounding" type="number" min="0" max="80" :disabled="busy || exporting"
      /></label>
      <label
        >{{ t('capture.background') }}<input v-model="background" type="color" :disabled="busy || exporting"
      /></label>
      <label
        ><input v-model="shadow" type="checkbox" :disabled="busy || exporting" />{{
          t('capture.shadow')
        }}</label
      >
    </div>
    <div class="controls">
      <label
        >{{ t('capture.tool')
        }}<select v-model="tool" :disabled="busy">
          <option value="region">{{ t('capture.region') }}</option>
          <option value="rectangle">{{ t('capture.rectangle') }}</option>
          <option value="arrow">{{ t('capture.arrow') }}</option>
          <option value="pen">{{ t('capture.pen') }}</option>
          <option value="blur">{{ t('capture.blur') }}</option>
          <option value="label">{{ t('capture.label') }}</option>
        </select></label
      >
      <label
        >{{ t('capture.aspect')
        }}<select v-model="ratio">
          <option value="free">{{ t('capture.free') }}</option>
          <option value="square">{{ t('capture.square') }}</option>
          <option value="wide">{{ t('capture.wide') }}</option>
        </select></label
      >
      <label>{{ t('capture.color') }}<input v-model="color" type="color" /></label>
      <label>{{ t('capture.stroke') }}<input v-model.number="stroke" type="number" min="1" max="40" /></label>
      <label v-if="tool === 'label'"
        >{{ t('capture.label_text') }}<input v-model="text" maxlength="256"
      /></label>
      <button :disabled="busy || !marks.length" @click="marks.pop()">{{ t('capture.undo') }}</button>
      <button :disabled="busy" @click="reset">{{ t('capture.reset') }}</button>
    </div>
    <p v-if="selection">{{ t('capture.selection', selection) }}</p>
    <div class="picture">
      <img :src="source" :alt="t('capture.title')" draggable="false" />
      <svg
        :viewBox="`0 0 ${width} ${height}`"
        @pointerdown.prevent="begin"
        @pointermove="move"
        @pointerup="finish"
        @pointercancel="draft = null"
        @pointerleave="pointer = null"
      >
        <defs>
          <marker
            id="arrow-head"
            markerWidth="8"
            markerHeight="8"
            refX="6"
            refY="3"
            orient="auto"
            markerUnits="strokeWidth"
          >
            <path d="M0,0 L0,6 L6,3 z" fill="context-stroke" />
          </marker>
        </defs>
        <g
          v-for="(mark, index) in viewMarks"
          :key="index"
          :stroke="mark.color"
          :fill="mark.tool === 'blur' || mark.tool === 'label' ? mark.color : 'none'"
          :stroke-width="mark.width"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <rect v-if="mark.tool === 'rectangle' || mark.tool === 'blur'" v-bind="region(mark)" />
          <path v-if="mark.tool === 'pen'" :d="path(mark)" />
          <line
            v-if="mark.tool === 'arrow'"
            :x1="mark.start.x"
            :y1="mark.start.y"
            :x2="mark.end.x"
            :y2="mark.end.y"
            marker-end="url(#arrow-head)"
          />
          <text
            v-if="mark.tool === 'label'"
            :x="mark.start.x"
            :y="mark.start.y"
            :font-size="Math.max(14, mark.width * 5)"
            stroke="none"
          >
            {{ mark.text }}
          </text>
        </g>
        <rect v-if="selection" v-bind="selection" class="selection" vector-effect="non-scaling-stroke" />
        <g v-if="pointer" class="crosshair">
          <line :x1="pointer.x" :x2="pointer.x" y1="0" :y2="height" />
          <line x1="0" :x2="width" :y1="pointer.y" :y2="pointer.y" />
        </g>
      </svg>
      <div
        v-if="pointer"
        class="loupe"
        aria-hidden="true"
        :style="{
          backgroundImage: `url('${source}')`,
          backgroundSize: `${width * 8}px ${height * 8}px`,
          backgroundPosition: `${60 - pointer.x * 8}px ${60 - pointer.y * 8}px`,
        }"
      />
    </div>
    <p v-if="error" role="alert">{{ error }}</p>
    <div class="controls">
      <button :disabled="busy || !selection?.width || !selection?.height" @click="save(true)">
        {{ t('capture.apply_crop') }}</button
      ><button :disabled="busy || !marks.length" @click="save(false)">{{ t('capture.save_edit') }}</button>
    </div>
  </div>
</template>
<style scoped>
.editor {
  display: grid;
  gap: var(--mf-sp-3);
}
.controls {
  display: flex;
  gap: var(--mf-sp-2);
  flex-wrap: wrap;
  align-items: end;
}
label {
  display: grid;
  gap: var(--mf-sp-1);
  font-size: var(--mf-fs-sm);
}
input[type='number'] {
  width: 70px;
}
.picture {
  position: relative;
  line-height: 0;
  background: var(--mf-surface-2);
  border: 1px solid var(--mf-border);
}
img {
  display: block;
  width: 100%;
  height: auto;
  user-select: none;
}
svg {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  touch-action: none;
  cursor: crosshair;
}
.selection {
  fill: var(--mf-focus);
  fill-opacity: 0.12;
  stroke: var(--mf-focus);
  stroke-width: 2px;
}
.crosshair {
  stroke: var(--mf-focus);
  stroke-width: 1;
  stroke-opacity: 0.5;
  pointer-events: none;
}
.loupe {
  position: absolute;
  bottom: var(--mf-sp-2);
  right: var(--mf-sp-2);
  width: 120px;
  height: 120px;
  border: 2px solid var(--mf-focus);
  background-repeat: no-repeat;
  background-color: var(--mf-surface);
  image-rendering: pixelated;
  pointer-events: none;
}
</style>
