<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { convertFileSrc } from '@tauri-apps/api/core'
import { commands, unwrap } from '@/api'
import { errorText, t } from '@/i18n'
import { useToolPrivacy } from '../shared/useToolPrivacy'
import type { CaptureImage } from '../dashboard/captureTypes'
const id = new URLSearchParams(location.search).get('asset') ?? ''
const paused = useToolPrivacy()
const image = ref<CaptureImage | null>(null)
const opacity = ref(100)
const error = ref('')
let generation = 0
let disposed = false
async function call(method: string, params: object = {}): Promise<unknown> {
  return JSON.parse(await unwrap(commands.captureRequest(method, JSON.stringify({ id, ...params }))))
}
watch(paused, async (value) => {
  const mine = ++generation
  image.value = null
  if (value) return
  try {
    const result = (await call('asset')) as CaptureImage
    if (mine === generation && !disposed && !paused.value) image.value = result
  } catch (e) {
    if (mine === generation && !disposed && !paused.value) error.value = errorText(e)
  }
})
async function copy(): Promise<void> {
  try {
    await call('copy_image')
  } catch (e) {
    error.value = errorText(e)
  }
}
async function passthrough(): Promise<void> {
  try {
    await call('pin_passthrough', { enabled: true })
  } catch (e) {
    error.value = errorText(e)
  }
}
onBeforeUnmount(() => {
  disposed = true
  generation++
  image.value = null
  void call('release').catch(() => {})
})
</script>
<template>
  <div class="pin" :style="{ opacity: opacity / 100 }">
    <p v-if="paused">
      {{ t('capture.privacy') }} <button @click="getCurrentWindow().close()">{{ t('capture.unpin') }}</button>
    </p>
    <img
      v-else-if="image"
      :src="convertFileSrc(image.path)"
      :alt="t('capture.title')"
      draggable="false"
      @mousedown="getCurrentWindow().startDragging()"
    />
    <nav v-if="!paused" class="controls">
      <button @click="copy">{{ t('capture.copy') }}</button
      ><button @click="passthrough">{{ t('capture.passthrough') }}</button
      ><label
        >{{ t('capture.opacity') }}<input v-model.number="opacity" type="range" min="20" max="100" /></label
      ><button @click="getCurrentWindow().close()">{{ t('capture.unpin') }}</button>
    </nav>
    <p v-if="error" role="alert">{{ error }}</p>
  </div>
</template>
<style scoped>
:global(html),
:global(body),
:global(#app) {
  background: transparent;
  overflow: hidden;
}
.pin {
  width: 100vw;
  height: 100vh;
  position: relative;
  background: transparent;
}
img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  cursor: move;
  user-select: none;
}
.controls {
  position: absolute;
  inset: 0 0 auto;
  padding: var(--mf-sp-2);
  display: flex;
  flex-wrap: wrap;
  gap: var(--mf-sp-1);
  align-items: center;
  background: var(--mf-surface);
  opacity: 0;
  transition: opacity var(--mf-dur-micro);
}
.pin:hover .controls,
.pin:focus-within .controls {
  opacity: 1;
}
label {
  font-size: var(--mf-fs-xs);
}
</style>
