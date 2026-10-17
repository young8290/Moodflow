<script setup lang="ts">
// 隐私说明全文（07 FR-SET-09、产品书 21 第 1 节）：折叠在“隐私”里，展开才渲染。文本在 hub_templates/privacy_notice.md（E 维护），
// 结构由 notice.ts 解析，这里只按结构排版，不插入 HTML。
import { ref } from 'vue'
import { t } from '@/i18n'
import { NOTICE, type Span } from './notice'

const opened = ref(false)
const key = (s: Span[], i: number) => `${i}-${s.map((x) => x.text).join('')}`
</script>

<template>
  <details class="notice-full" @toggle="opened = ($event.target as HTMLDetailsElement).open">
    <summary>{{ t('privacy.notice_full') }}</summary>
    <article v-if="opened" class="doc">
      <template v-for="(b, i) in NOTICE" :key="i">
        <h4 v-if="b.kind === 'h1'" class="title">{{ b.text }}</h4>
        <h5 v-else-if="b.kind === 'h2'">{{ b.text }}</h5>
        <p v-else-if="b.kind === 'p'">
          <template v-for="(line, j) in b.lines" :key="key(line, j)">
            <br v-if="j > 0" />
            <template v-for="(s, k) in line" :key="k">
              <strong v-if="s.bold">{{ s.text }}</strong>
              <code v-else-if="s.code">{{ s.text }}</code>
              <template v-else>{{ s.text }}</template>
            </template>
          </template>
        </p>
        <ul v-else-if="b.kind === 'ul'">
          <li v-for="(item, j) in b.items" :key="key(item, j)">
            <template v-for="(s, k) in item" :key="k">
              <strong v-if="s.bold">{{ s.text }}</strong>
              <code v-else-if="s.code">{{ s.text }}</code>
              <template v-else>{{ s.text }}</template>
            </template>
          </li>
        </ul>
        <div v-else-if="b.kind === 'table'" class="table-wrap">
          <table>
            <thead>
              <tr>
                <th v-for="(c, j) in b.head" :key="j" scope="col">
                  <template v-for="(s, k) in c" :key="k">{{ s.text }}</template>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(r, j) in b.rows" :key="j">
                <td v-for="(c, k) in r" :key="k">
                  <template v-for="(s, m) in c" :key="m">
                    <strong v-if="s.bold">{{ s.text }}</strong>
                    <code v-else-if="s.code">{{ s.text }}</code>
                    <template v-else>{{ s.text }}</template>
                  </template>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
    </article>
  </details>
</template>

<style scoped>
.notice-full summary {
  min-height: 32px;
  cursor: pointer;
}

.doc {
  max-width: 680px;
  font-size: var(--mf-fs-sm);
  line-height: var(--mf-lh-sm);
}

.doc .title {
  margin: var(--mf-sp-3) 0;
  font-size: var(--mf-fs-lg);
  font-weight: 600;
}

.doc h5 {
  margin: var(--mf-sp-4) 0 var(--mf-sp-1);
  font-size: var(--mf-fs-md);
  font-weight: 600;
}

.doc p,
.doc ul {
  margin: 0 0 var(--mf-sp-2);
}

.doc code {
  padding: 0 var(--mf-sp-1);
  border-radius: var(--mf-radius-sm);
  background: var(--mf-surface-2);
}

.table-wrap {
  overflow-x: auto;
}

.doc table {
  width: 100%;
  margin-bottom: var(--mf-sp-2);
  border-collapse: collapse;
}

.doc th,
.doc td {
  padding: var(--mf-sp-1) var(--mf-sp-2);
  border-bottom: 1px solid var(--mf-border);
  text-align: left;
  vertical-align: top;
}

.doc th {
  color: var(--mf-text-2);
  font-weight: 400;
}
</style>
