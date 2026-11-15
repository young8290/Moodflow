<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import { reviewText, type ReviewPart } from './practiceReview'

const props = defineProps<{ target: string; typed: string }>()
const parts = computed(() => reviewText(props.target, props.typed))
const mistakes = computed(() =>
  parts.value.filter(
    (part): part is ReviewPart & { kind: Exclude<ReviewPart['kind'], 'match'> } => part.kind !== 'match',
  ),
)
function visible(value: string): string {
  return value.replaceAll(' ', '␣').replaceAll('\n', '↵').replaceAll('\t', '⇥')
}
</script>

<template>
  <details class="review">
    <summary>{{ t('dashboard.practice.review') }}</summary>
    <p v-if="!mistakes.length">{{ t('dashboard.practice.review_perfect') }}</p>
    <template v-else>
      <div class="comparison">
        <div>
          <h3>{{ t('dashboard.practice.review_expected') }}</h3>
          <p class="line">
            <template v-for="(part, i) in parts" :key="i"
              ><mark v-if="part.kind === 'replace' || part.kind === 'missing'">{{ part.expected }}</mark
              ><template v-else>{{ part.expected }}</template></template
            >
          </p>
        </div>
        <div>
          <h3>{{ t('dashboard.practice.review_actual') }}</h3>
          <p class="line">
            <template v-for="(part, i) in parts" :key="i"
              ><mark v-if="part.kind === 'replace' || part.kind === 'extra'">{{ part.actual }}</mark
              ><template v-else>{{ part.actual }}</template></template
            >
          </p>
        </div>
      </div>
      <ul class="changes">
        <li v-for="(part, i) in mistakes" :key="i">
          <strong>{{ t(`dashboard.practice.review_${part.kind}`) }}</strong>
          <code v-if="part.expected">{{ visible(part.expected) }}</code>
          <span v-if="part.expected && part.actual" aria-hidden="true">{{ '→' }}</span>
          <code v-if="part.actual">{{ visible(part.actual) }}</code>
        </li>
      </ul>
    </template>
  </details>
</template>

<style scoped>
.review {
  margin-block-start: var(--mf-sp-3);
}
summary {
  cursor: pointer;
}
.comparison {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(16rem, 100%), 1fr));
  gap: var(--mf-sp-3);
}
.comparison h3 {
  font-size: var(--mf-fs-sm);
}
.line {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.7;
}
mark {
  color: var(--mf-text-1);
  background: var(--mf-surface-2);
  text-decoration: underline var(--mf-danger) 2px;
}
.changes {
  padding-inline-start: var(--mf-sp-4);
}
.changes code {
  margin-inline: var(--mf-sp-1);
  overflow-wrap: anywhere;
}
</style>
