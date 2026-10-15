<script setup lang="ts">
// 小精灵装扮图层（07 FR-WGT-08、05 FR-REV-04）：八件装扮各一个自制的矢量图形，叠在小精灵上面。
// 六种天气的小精灵身体是同一个圆（圆心 48,50、半径 30，见 tools/gen_sprite_lottie.py），所以锚点通用：
// 头顶 y≈20、眼睛 y≈47、下巴 y≈80。颜色只用设计令牌；描边用正文色，高对比度下跟随系统。纯装饰，读屏不念。
defineProps<{ item: string; size?: number }>()
</script>

<template>
  <svg
    class="outfit"
    :width="size ?? 96"
    :height="size ?? 96"
    viewBox="0 0 96 96"
    aria-hidden="true"
    focusable="false"
    :data-item="item"
  >
    <!-- 草帽：宽帽檐 + 帽顶 + 一圈帽带 -->
    <g v-if="item === 'straw_hat'">
      <ellipse class="fill-sun line" cx="48" cy="22" rx="30" ry="6" />
      <path class="fill-sun line" d="M34 22c0-10 6-15 14-15s14 5 14 15z" />
      <path class="fill-storm" d="M34.6 18h26.8v3.4H34.6z" />
    </g>
    <!-- 围巾：绕下巴一圈，右边垂下一截 -->
    <g v-else-if="item === 'scarf'">
      <path class="fill-storm line" d="M24 70c8 8 40 8 48 0l2 6c-10 9-42 9-52 0z" />
      <path class="fill-storm line" d="M60 76l3 14h7l-3-15z" />
    </g>
    <!-- 耳机：头箍 + 两侧耳罩 -->
    <g v-else-if="item === 'headphones'">
      <path class="no-fill line-thick" d="M20 50c0-20 12-31 28-31s28 11 28 31" />
      <rect class="fill-rain line" x="13" y="44" width="10" height="17" rx="4" />
      <rect class="fill-rain line" x="73" y="44" width="10" height="17" rx="4" />
    </g>
    <!-- 小花：右上角五瓣花 -->
    <g v-else-if="item === 'flower'">
      <circle
        v-for="k in 5"
        :key="k"
        class="fill-storm line"
        :cx="68 + 6 * Math.cos((k * 72 * Math.PI) / 180)"
        :cy="22 + 6 * Math.sin((k * 72 * Math.PI) / 180)"
        r="4.5"
      />
      <circle class="fill-sun line" cx="68" cy="22" r="3.5" />
    </g>
    <!-- 眼镜：两个圆框 + 鼻梁 -->
    <g v-else-if="item === 'glasses'">
      <circle class="no-fill line-thick" cx="38" cy="47" r="7" />
      <circle class="no-fill line-thick" cx="58" cy="47" r="7" />
      <path class="no-fill line-thick" d="M45 47h6" />
    </g>
    <!-- 毛线帽：半圆帽身 + 翻边 + 顶上的毛球 -->
    <g v-else-if="item === 'beanie'">
      <path class="fill-cloudy line" d="M26 30c0-14 10-22 22-22s22 8 22 22z" />
      <rect class="fill-cloudy line" x="24" y="27" width="48" height="8" rx="4" />
      <circle class="fill-wind line" cx="48" cy="7" r="4.5" />
    </g>
    <!-- 蝴蝶结：右上方 -->
    <g v-else-if="item === 'bow'">
      <path class="fill-storm line" d="M66 20l-10-6v13zM66 20l10-6v13z" />
      <circle class="fill-storm line" cx="66" cy="20" r="3" />
    </g>
    <!-- 星星发卡：左上方五角星 -->
    <g v-else-if="item === 'star_clip'">
      <path
        class="fill-sun line"
        d="M28 12l2.6 5.4 5.9.8-4.3 4.1 1 5.8-5.2-2.8-5.2 2.8 1-5.8-4.3-4.1 5.9-.8z"
      />
    </g>
  </svg>
</template>

<style scoped>
.outfit {
  pointer-events: none;
}

.line {
  stroke: var(--xq-text-1);
  stroke-width: 1.5;
  stroke-linejoin: round;
}

.line-thick {
  stroke: var(--xq-text-1);
  stroke-width: 2.5;
  stroke-linecap: round;
}

.no-fill {
  fill: none;
}

.fill-sun {
  fill: var(--xq-w-sunny);
}

.fill-storm {
  fill: var(--xq-w-storm);
}

.fill-rain {
  fill: var(--xq-w-rain);
}

.fill-cloudy {
  fill: var(--xq-w-cloudy);
}

.fill-wind {
  fill: var(--xq-w-wind);
}

@media (forced-colors: active) {
  .line,
  .line-thick {
    stroke: CanvasText;
  }
}
</style>
