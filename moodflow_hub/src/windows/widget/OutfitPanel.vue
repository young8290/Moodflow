<script setup lang="ts">
// 换装（07 FR-WGT-08）：八件装扮，已解锁的可以点着穿上，同一时间只穿一件，“不穿”脱下。
// 上面写晴天值和下一件还差几点；不说“连续”“断签”，不催（DS-COPY-10）。Esc 关闭。
import { computed, onMounted, ref } from 'vue'
import type { CollectionView } from '@/api'
import SpriteOutfit from '@/components/SpriteOutfit.vue'
import { t, type CopyKey } from '@/i18n'

const props = defineProps<{ view: CollectionView }>()
const emit = defineEmits<{ equip: [item: string | null]; close: [] }>()
const root = ref<HTMLElement | null>(null)

const name = (id: string) => t(`collection.item_${id}` as CopyKey)
const progress = computed(() => {
  const next = props.view.next_item
    ? t('collection.next', { need: props.view.next_need, item: name(props.view.next_item) })
    : t('collection.all_unlocked')
  return `${t('collection.points', { points: props.view.points })} · ${next}`
})

onMounted(() => root.value?.querySelector<HTMLButtonElement>('button:not([disabled])')?.focus())
</script>

<template>
  <div
    ref="root"
    class="outfit-panel"
    role="dialog"
    :aria-label="t('collection.title')"
    @pointerdown.stop
    @keydown.esc.stop="emit('close')"
  >
    <p class="title">{{ t('collection.title') }}</p>
    <p class="points">{{ progress }}</p>
    <ul class="items">
      <li v-for="i in view.items" :key="i.id">
        <button
          class="item"
          :disabled="!i.unlocked"
          :aria-pressed="i.equipped"
          :aria-label="i.unlocked ? name(i.id) : t('collection.locked', { item: name(i.id) })"
          :title="i.unlocked ? name(i.id) : t('collection.locked', { item: name(i.id) })"
          @click.stop="emit('equip', i.equipped ? null : i.id)"
        >
          <SpriteOutfit :item="i.id" :size="28" />
        </button>
      </li>
    </ul>
    <p class="hint">{{ t('collection.how') }}</p>
    <div class="actions">
      <button class="compact" :disabled="!view.equipped" @click.stop="emit('equip', null)">
        {{ t('collection.none') }}
      </button>
      <button class="compact" @click.stop="emit('close')">{{ t('collection.done') }}</button>
    </div>
  </div>
</template>

<style scoped>
.outfit-panel {
  display: flex;
  flex-direction: column;
  gap: var(--mf-sp-2);
  padding: var(--mf-sp-3);
  background: var(--mf-surface);
  border: 1px solid var(--mf-border);
  border-radius: var(--mf-radius-card);
}

.title,
.points,
.hint {
  margin: 0;
}

.title {
  font-weight: 600;
}

.points,
.hint {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-xs);
}

.items {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--mf-sp-1);
  margin: 0;
  padding: 0;
  list-style: none;
}

.item {
  display: grid;
  place-items: center;
  width: 100%;
  min-height: 36px;
  padding: 2px;
}

.item[disabled] {
  opacity: 0.35;
}

.item[aria-pressed='true'] {
  outline: 2px solid var(--mf-focus);
}

.actions {
  display: flex;
  gap: var(--mf-sp-2);
  justify-content: flex-end;
}
</style>
