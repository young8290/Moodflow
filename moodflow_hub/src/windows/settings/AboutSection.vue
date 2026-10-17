<script setup lang="ts">
// 设置中心 · 隐私与关于（07 FR-SET-09，P0）。上半部分“隐私”见 PrivacySection.vue，参加研究与开发者选项见 ResearchDevSection.vue；
// 下半部分“关于”：版本号、非官方分支声明、免责声明、开源许可列表。
import { onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import AppLogo from '@/components/AppLogo.vue'
import { t } from '@/i18n'
import { licenseEntries } from './licenses'
import PrivacySection from './PrivacySection.vue'
import ResearchDevSection from './ResearchDevSection.vue'
import RunInfoExport from './RunInfoExport.vue'

const version = ref<string | null>(null)
const licenses = licenseEntries()

onMounted(async () => {
  try {
    version.value = await getVersion()
  } catch {
    // 普通浏览器里预览（pnpm dev）拿不到版本号，不显示这一行
  }
})
</script>

<template>
  <section>
    <h1>{{ t('settings.privacy_about') }}</h1>

    <PrivacySection />
    <ResearchDevSection />

    <h2>{{ t('about.title') }}</h2>
    <div class="brand">
      <AppLogo :size="56" />
      <div>
        <p class="name">{{ t('window.widget') }}</p>
        <p class="muted">{{ t('about.tagline') }}</p>
        <p v-if="version" class="muted version">{{ t('about.version', { version }) }}</p>
      </div>
    </div>
    <!-- C-LAW-08 ④：注明基于上游开源输入法开发、与原项目无关；C-LAW-06：不提供医疗诊断或治疗 -->
    <p>{{ t('about.fork_notice') }}</p>
    <p>{{ t('about.disclaimer') }}</p>

    <RunInfoExport />

    <h2>{{ t('about.licenses') }}</h2>
    <p class="muted">{{ t('about.licenses_intro') }}</p>
    <ul class="licenses">
      <li v-for="l in licenses" :key="l.name">
        <details v-if="l.text">
          <summary>
            <span class="lib">{{ l.name }}</span> <span class="spdx">{{ l.license }}</span>
          </summary>
          <pre class="full">{{ l.text }}</pre>
        </details>
        <template v-else>
          <span class="lib">{{ l.name }}</span> <span class="spdx">{{ l.license }}</span>
        </template>
      </li>
    </ul>
    <p class="muted">{{ t('about.deps_note') }}</p>
  </section>
</template>

<style scoped>
h2 {
  margin-top: var(--mf-sp-5);
}

.brand {
  display: flex;
  gap: var(--mf-sp-4);
  align-items: center;
  margin-bottom: var(--mf-sp-4);
}

.brand p {
  margin: 0;
}

.name {
  font-size: var(--mf-fs-lg);
  line-height: var(--mf-lh-lg);
  font-weight: 500;
}

.version {
  font-size: var(--mf-fs-sm);
  line-height: var(--mf-lh-sm);
}

.licenses {
  margin: 0 0 var(--mf-sp-3);
  padding: 0;
  list-style: none;
}

.licenses li {
  padding: var(--mf-sp-2) 0;
  border-bottom: 1px solid var(--mf-border);
}

summary {
  min-height: 32px; /* DS-A11Y-03 */
  cursor: pointer;
}

.spdx {
  color: var(--mf-text-2);
  font-size: var(--mf-fs-sm);
}

.full {
  max-height: 320px;
  margin: var(--mf-sp-2) 0 0;
  padding: var(--mf-sp-3);
  overflow: auto;
  border-radius: var(--mf-radius-sm);
  background: var(--mf-surface-2);
  font-size: var(--mf-fs-xs);
  line-height: var(--mf-lh-xs);
  white-space: pre-wrap;
}
</style>
