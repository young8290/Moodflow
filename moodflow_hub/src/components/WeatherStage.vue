<script setup lang="ts">
// 天气切换时新旧小精灵 400 ms 交叉淡化（07 FR-WGT-03、DS-MOTION-01）。
// 只有天气变了才淡化；闭眼、睁眼在同一个小精灵上直接切换。系统关闭动画时 base.css 把时长归零。
// play() 转给当前这只小精灵（“晃一下”“靠近”，DS-MOTION-02）。默认插槽叠在小精灵上面（装扮图层，FR-WGT-08），
// 换天气时不跟着淡化。
import { ref } from 'vue'
import type { Weather } from '@/api'
import WeatherSprite, { type SpriteAction } from './WeatherSprite.vue'

withDefaults(defineProps<{ weather: Weather; eyesClosed?: boolean; focusing?: boolean; size?: number }>(), {
  eyesClosed: false,
  focusing: false,
  size: 96,
})

const sprite = ref<InstanceType<typeof WeatherSprite> | null>(null)

defineExpose({ play: (a: SpriteAction) => sprite.value?.play(a) })
</script>

<template>
  <div class="stage" :style="{ width: `${size}px`, height: `${size}px` }">
    <Transition name="mf-crossfade">
      <WeatherSprite
        :key="weather"
        ref="sprite"
        :weather="weather"
        :eyes-closed="eyesClosed"
        :focusing="focusing"
        :size="size"
      />
    </Transition>
    <slot />
  </div>
</template>

<style scoped>
.stage {
  position: relative;
  flex: none;
}

.stage > * {
  position: absolute;
  inset: 0;
}

.mf-crossfade-enter-active,
.mf-crossfade-leave-active {
  transition: opacity var(--mf-dur-weather) var(--mf-ease);
}

.mf-crossfade-enter-from,
.mf-crossfade-leave-to {
  opacity: 0;
}
</style>
