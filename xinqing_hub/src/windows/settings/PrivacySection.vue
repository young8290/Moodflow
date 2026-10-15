<script setup lang="ts">
// 设置中心 · 隐私（07 FR-SET-09 上半部分，放在“隐私与关于”分类里）：
// 采集内容说明（与引导页同一张表）、逐项同意与撤回（FR-DAT-05）、最近 20 次出网记录（只有字段名和时间）、导出我的数据（FR-DAT-03）。
// 隐私说明全文（PrivacyNotice.vue，产品书 21 第 1 节）；删除全部数据（FR-DAT-04，ADR 0037）：输入“删除”二字确认，可选连 AI 密钥一起删，删完 Hub 重启、重新引导；
// 撤回 ① 时可选同时删除已有的情绪数据（FR-DAT-05 第 3 条）；从备份导入（FR-DAT-07）：合并或覆盖，设置可选，导入后请用户重启。
import { onMounted, ref } from 'vue'
import { save } from '@tauri-apps/plugin-dialog'
import { commands, unwrap, type ConsentItem, type ConsentState, type NetLogView } from '@/api'
import { errorText, isCopyKey, t } from '@/i18n'
import { CONSENT_NUMBER, sendsToThirdParty } from '../shared/consent'
import ImportData from '../shared/ImportData.vue'
import NoticeTable from '../shared/NoticeTable.vue'
import PrivacyNotice from './PrivacyNotice.vue'
import { apiName } from './apiName'

const consent = ref<ConsentState | null>(null)
const consentBusy = ref(false)
/** 刚撤回了会发给第三方的同意项：如实说明已发出的收不回来 */
const withdrawNotice = ref(false)
/** 撤回 ① 先确认一次：它是使用心晴的前提 */
const confirmSense = ref(false)
const consentError = ref<string | null>(null)
/** 撤回 ① 时顺便删掉情绪数据 */
const deleteMood = ref(false)
const moodDeleted = ref<string | null>(null)

const wipeSecrets = ref(false)
const wipeTyped = ref('')
const wiping = ref(false)
const wipeError = ref<string | null>(null)

const netlog = ref<NetLogView[] | null>(null)
const netlogError = ref<string | null>(null)

const exporting = ref(false)
const exportDone = ref<string | null>(null)
const exportError = ref<string | null>(null)

onMounted(() => {
  void loadConsent()
  void loadNetlog()
})

async function loadConsent(): Promise<void> {
  try {
    consent.value = await unwrap(commands.consentGet())
  } catch (e) {
    consentError.value = errorText(e)
  }
}

async function setConsent(item: ConsentItem, granted: boolean): Promise<void> {
  consentError.value = null
  consentBusy.value = true
  try {
    consent.value = await unwrap(commands.consentSet(item, granted))
    withdrawNotice.value = !granted && sendsToThirdParty(item)
    if (item === 'sense' && !granted && deleteMood.value) {
      const n = await unwrap(commands.dataDeleteMood())
      moodDeleted.value = t('privacy.mood_deleted', { n })
    }
  } catch (e) {
    consentError.value = errorText(e)
  } finally {
    consentBusy.value = false
    confirmSense.value = false
    deleteMood.value = false
  }
}

async function wipeAll(): Promise<void> {
  if (wipeTyped.value.trim() !== t('privacy.delete_type')) return
  wipeError.value = null
  wiping.value = true
  try {
    // 这台电脑上的界面偏好（小组件位置、首次引导交接等）也清掉，和新装一样
    try {
      localStorage.clear()
    } catch {
      // 清不掉不影响删除数据库
    }
    // 成功时 Hub 会重启，这里拿不到返回
    await unwrap(commands.dataDeleteAll(wipeSecrets.value))
  } catch (e) {
    wipeError.value = errorText(e)
    wiping.value = false
  }
}

function onToggle(item: ConsentItem, e: Event): void {
  const box = e.target as HTMLInputElement
  if (item === 'sense' && !box.checked) {
    // 先不改，等用户在下面确认
    box.checked = true
    confirmSense.value = true
    return
  }
  void setConsent(item, box.checked)
}

async function loadNetlog(): Promise<void> {
  netlogError.value = null
  try {
    netlog.value = await unwrap(commands.aiNetLogRecent())
  } catch (e) {
    netlogError.value = errorText(e)
  }
}

function result(r: NetLogView): string {
  if (r.status === 'ok') {
    return r.latency_ms === null
      ? t('privacy.netlog_ok_no_latency')
      : t('privacy.netlog_ok', { ms: r.latency_ms })
  }
  const key = `privacy.status.${r.status}`
  return t('privacy.netlog_fail', { reason: isCopyKey(key) ? t(key) : r.status })
}

const time = (ts: number) =>
  new Date(ts).toLocaleString('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  })

function defaultFileName(): string {
  const d = new Date()
  const ymd = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`
  return `${t('privacy.export_file')}-${ymd}.zip`
}

async function exportData(): Promise<void> {
  exportDone.value = null
  exportError.value = null
  let path: string | null
  try {
    path = await save({
      defaultPath: defaultFileName(),
      filters: [{ name: t('privacy.export_filter'), extensions: ['zip'] }],
    })
  } catch (e) {
    exportError.value = errorText(e)
    return
  }
  // 用户关掉了对话框
  if (!path) return
  exporting.value = true
  try {
    await unwrap(commands.dataExport(path))
    exportDone.value = t('error.export_done', { path })
  } catch (e) {
    exportError.value = errorText(e)
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <h2>{{ t('privacy.title') }}</h2>

  <h3>{{ t('privacy.what') }}</h3>
  <NoticeTable />
  <p class="small">{{ t('onboarding.third_party') }}</p>
  <PrivacyNotice />

  <h3>{{ t('privacy.consent') }}</h3>
  <p class="muted small">{{ t('privacy.consent_hint') }}</p>
  <div v-if="consent" class="consents">
    <label v-for="e in consent.items" :key="e.item" class="consent" :data-item="e.item">
      <input
        type="checkbox"
        :checked="e.granted"
        :disabled="consentBusy"
        @change="onToggle(e.item, $event)"
      />
      <span>{{ CONSENT_NUMBER[e.item] }} {{ t(`consent.${e.item}`) }}</span>
    </label>
  </div>
  <div v-if="confirmSense" class="confirm" role="alertdialog" :aria-label="t('privacy.sense_confirm')">
    <p>{{ t('privacy.sense_confirm') }}</p>
    <label class="check">
      <input v-model="deleteMood" type="checkbox" />
      <span>{{ t('privacy.sense_delete_mood') }}</span>
    </label>
    <button class="compact danger" :disabled="consentBusy" @click="setConsent('sense', false)">
      {{ t('privacy.sense_confirm_yes') }}
    </button>
    <button class="compact" @click="confirmSense = false">{{ t('privacy.sense_confirm_no') }}</button>
  </div>
  <p v-if="withdrawNotice" class="note" role="status">{{ t('error.withdraw_notice') }}</p>
  <p v-if="moodDeleted" class="note" role="status">{{ moodDeleted }}</p>
  <p v-if="consentError" class="error" role="alert">{{ consentError }}</p>

  <h3>{{ t('privacy.netlog') }}</h3>
  <p class="muted small">{{ t('privacy.netlog_hint') }}</p>
  <p v-if="netlog && netlog.length === 0" class="muted">{{ t('privacy.netlog_empty') }}</p>
  <div v-else-if="netlog" class="table-wrap">
    <table class="netlog">
      <thead>
        <tr>
          <th scope="col">{{ t('privacy.netlog_col_time') }}</th>
          <th scope="col">{{ t('privacy.netlog_col_api') }}</th>
          <th scope="col">{{ t('privacy.netlog_col_fields') }}</th>
          <th scope="col">{{ t('privacy.netlog_col_result') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(r, i) in netlog" :key="`${r.ts}-${i}`">
          <td class="nowrap">{{ time(r.ts) }}</td>
          <td>
            {{ apiName(r.api) }}
            <span v-if="r.model" class="model">{{ r.model }}</span>
          </td>
          <td class="fields">{{ r.fields.length ? r.fields.join('、') : t('privacy.netlog_no_fields') }}</td>
          <td :class="{ fail: r.status !== 'ok' }">{{ result(r) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
  <p v-if="netlogError" class="error" role="alert">{{ netlogError }}</p>
  <button class="compact" @click="loadNetlog()">{{ t('privacy.netlog_refresh') }}</button>

  <h3>{{ t('privacy.export') }}</h3>
  <p class="muted small">{{ t('privacy.export_hint') }}</p>
  <div class="row">
    <button :disabled="exporting" @click="exportData">
      {{ exporting ? t('privacy.exporting') : t('privacy.export_btn') }}
    </button>
    <span v-if="exportDone" class="done" role="status">{{ exportDone }}</span>
  </div>
  <p v-if="exportError" class="error" role="alert">{{ exportError }}</p>

  <h3>{{ t('privacy.import') }}</h3>
  <p class="muted small">{{ t('privacy.import_hint') }}</p>
  <ImportData />

  <h3>{{ t('privacy.delete') }}</h3>
  <p class="muted small">{{ t('privacy.delete_hint') }}</p>
  <div class="wipe" data-group="wipe">
    <label class="check">
      <input v-model="wipeSecrets" type="checkbox" :disabled="wiping" />
      <span>{{ t('privacy.delete_secrets') }}</span>
    </label>
    <label class="row">
      <span>{{ t('privacy.delete_type_label') }}</span>
      <input v-model="wipeTyped" type="text" class="typed" autocomplete="off" :disabled="wiping" />
    </label>
    <div class="row">
      <button
        class="danger"
        :disabled="wiping || wipeTyped.trim() !== t('privacy.delete_type')"
        @click="wipeAll"
      >
        {{ t('privacy.delete_btn') }}
      </button>
      <span v-if="wiping" role="status">{{ t('privacy.deleting') }}</span>
    </div>
    <p v-if="wipeError" class="error" role="alert">{{ wipeError }}</p>
  </div>
</template>

<style scoped>
h2 {
  margin-top: var(--xq-sp-5);
}

/* 小标题比 h2（16 px / 500）低一级：同字重、小一号、次要色 */
h3 {
  margin: var(--xq-sp-5) 0 var(--xq-sp-2);
  color: var(--xq-text-2);
  font-size: var(--xq-fs-md);
  font-weight: 500;
}

.small {
  margin: 0 0 var(--xq-sp-2);
  font-size: var(--xq-fs-sm);
}

.consents {
  display: grid;
  gap: var(--xq-sp-1);
}

/* 整行都能点，点击目标不小于 32 px 高（DS-A11Y-03） */
.consent,
.check {
  display: flex;
  gap: var(--xq-sp-2);
  align-items: flex-start;
  min-height: 32px;
  cursor: pointer;
}

.consent input {
  margin-top: 5px;
}

.confirm {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-2);
  align-items: center;
  margin-top: var(--xq-sp-2);
  padding: var(--xq-sp-2) var(--xq-sp-3);
  border: 1px solid var(--xq-danger);
  border-radius: var(--xq-radius-sm);
}

.confirm p {
  flex-basis: 100%;
  margin: 0;
}

.note {
  margin: var(--xq-sp-2) 0 0;
  padding: var(--xq-sp-2) var(--xq-sp-3);
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface-2);
  font-size: var(--xq-fs-sm);
}

.table-wrap {
  margin-bottom: var(--xq-sp-2);
  overflow-x: auto;
}

.netlog {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--xq-fs-sm);
  line-height: var(--xq-lh-sm);
}

.netlog th,
.netlog td {
  padding: var(--xq-sp-1) var(--xq-sp-2);
  border-bottom: 1px solid var(--xq-border);
  text-align: left;
  vertical-align: top;
}

.nowrap {
  white-space: nowrap;
}

.model {
  display: block;
  color: var(--xq-text-3);
  font-size: var(--xq-fs-xs);
  overflow-wrap: anywhere;
}

.fields {
  overflow-wrap: anywhere;
}

.fail {
  color: var(--xq-danger);
}

.row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--xq-sp-3);
  align-items: center;
}

.done {
  color: var(--xq-text-2);
  font-size: var(--xq-fs-sm);
  overflow-wrap: anywhere;
}

.compact {
  padding: 0 var(--xq-sp-3);
}

.danger {
  border-color: var(--xq-danger);
  color: var(--xq-danger);
}

.error {
  color: var(--xq-danger);
}

.wipe {
  display: grid;
  gap: var(--xq-sp-2);
  max-width: 560px;
}

.typed {
  width: 120px;
  min-height: 32px;
  padding: 0 var(--xq-sp-2);
  border: 1px solid var(--xq-border);
  border-radius: var(--xq-radius-sm);
  background: var(--xq-surface);
  color: var(--xq-text-1);
  font: inherit;
}
</style>
