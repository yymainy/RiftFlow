<template>
  <div class="archive-workspace">
    <header class="archive-heading">
      <div>
        <h1>{{ t('archiveWorkspace.title') }}</h1>
        <p>{{ t('archiveWorkspace.subtitle') }}</p>
      </div>
      <NTag :bordered="false" :type="diagnostics?.connected ? 'success' : 'default'">{{
        t(diagnostics?.connected ? 'archiveWorkspace.online' : 'archiveWorkspace.offline')
      }}</NTag>
    </header>
    <NAlert v-if="error" type="error" closable @close="error = ''">{{ error }}</NAlert>
    <NTabs v-model:value="tab" type="line" animated>
      <NTabPane name="archive" :tab="t('archiveWorkspace.archive')">
        <div class="archive-toolbar">
          <NButton
            type="primary"
            :loading="working || diagnostics?.busy"
            :disabled="
              !diagnostics?.connected ||
              !diagnostics?.loggedIn ||
              diagnostics?.paused ||
              working ||
              diagnostics?.busy
            "
            @click="synchronize"
            >{{ t('archiveWorkspace.sync') }}</NButton
          >
          <NButton :disabled="working" @click="refresh">{{
            t('archiveWorkspace.refresh')
          }}</NButton>
          <NButton :disabled="working || !list.total" @click="exportFile('csv')">{{
            t('archiveWorkspace.csv')
          }}</NButton>
          <NButton :disabled="working" @click="exportFile('json')">{{
            t('archiveWorkspace.backup')
          }}</NButton>
          <NButton :disabled="working" @click="restore">{{
            t('archiveWorkspace.restore')
          }}</NButton>
        </div>
        <p class="archive-hint">
          {{ t(diagnostics?.paused ? 'archiveWorkspace.paused' : 'archiveWorkspace.autoHint') }}
        </p>
        <div class="archive-filters">
          <NInput v-model:value="query" clearable :placeholder="t('archiveWorkspace.search')" />
          <NSelect
            v-model:value="mode"
            clearable
            :options="list.modes.map((value) => ({ label: value, value }))"
            :placeholder="t('archiveWorkspace.mode')"
          />
          <NDatePicker
            v-model:value="dateRange"
            type="daterange"
            clearable
            :start-placeholder="t('archiveWorkspace.start')"
            :end-placeholder="t('archiveWorkspace.end')"
          />
        </div>
        <div class="archive-summary">
          {{ t('archiveWorkspace.count', { total: list.total, matched: list.matched })
          }}<span v-if="diagnostics?.lastSync.at">
            · {{ t('archiveWorkspace.lastSync') }} {{ date(diagnostics.lastSync.at) }}</span
          >
        </div>
        <NDataTable
          :columns="columns"
          :data="list.records"
          :row-key="(row) => row.key"
          :loading="loading"
          :scroll-x="1050"
          size="small"
          striped
          :row-props="rowProps"
        >
          <template #empty
            ><NEmpty
              :description="
                t(list.total ? 'archiveWorkspace.noMatches' : 'archiveWorkspace.empty')
              "
          /></template>
        </NDataTable>
        <NPagination
          v-if="list.matched > 50"
          v-model:page="page"
          :page-size="50"
          :item-count="list.matched"
          class="archive-pagination"
        />
      </NTabPane>
      <NTabPane name="diagnostics" :tab="t('archiveWorkspace.diagnostics')">
        <div class="diagnostic-grid">
          <article v-for="step in steps" :key="step.key" class="diagnostic-card">
            <NTag :type="step.ready ? 'success' : 'warning'" :bordered="false">{{
              t(step.ready ? 'archiveWorkspace.ready' : 'archiveWorkspace.waiting')
            }}</NTag>
            <h2>{{ step.title }}</h2>
            <p>{{ step.description }}</p>
          </article>
        </div>
        <NDescriptions bordered :column="1" size="small" class="archive-descriptions">
          <NDescriptionsItem :label="t('archiveWorkspace.region')">{{
            diagnostics?.region || '—'
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.account')">{{
            diagnostics?.playerName || '—'
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.phase')">{{
            diagnostics?.phase || '—'
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.permission')">{{
            t(diagnostics?.elevated ? 'archiveWorkspace.admin' : 'archiveWorkspace.normal')
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.source')">{{
            t('archiveWorkspace.sourceHint')
          }}</NDescriptionsItem>
        </NDescriptions>
        <NAlert
          v-if="diagnostics?.lastSync.success === false"
          type="warning"
          class="archive-status"
          >{{ diagnostics?.lastSync.reason || t('archiveWorkspace.syncFailed') }}</NAlert
        >
        <NAlert type="info" class="archive-status">{{
          t('archiveWorkspace.diagnosticHint')
        }}</NAlert>
        <div class="archive-toolbar">
          <NButton @click="refresh">{{ t('archiveWorkspace.refresh') }}</NButton
          ><NButton @click="openData">{{ t('archiveWorkspace.openData') }}</NButton
          ><NButton @click="copyDiagnostics">{{ t('archiveWorkspace.copyDiagnostics') }}</NButton>
        </div>
      </NTabPane>
      <NTabPane name="personal" :tab="t('personalTools.toolbox')"
        ><PersonalWorkspace @open="openKey"
      /></NTabPane>
      <NTabPane name="vault" :tab="t('personalTools.vault')"><VaultWorkspace /></NTabPane>
    </NTabs>
    <NAlert v-if="diagnostics?.lastSync.failures?.length" type="warning" class="archive-status"
      ><div v-for="failure in diagnostics.lastSync.failures" :key="failure.gameId">
        #{{ failure.gameId }} · {{ failure.reason }}
      </div></NAlert
    >
    <NDrawer
      :show="!!selected"
      width="min(780px, 94vw)"
      @update:show="
        (show) => {
          if (!show) selected = null
        }
      "
    >
      <NDrawerContent
        v-if="selected"
        :title="`${selected.championName} · ${selected.gameId}`"
        closable
      >
        <NDescriptions :column="2" bordered size="small">
          <NDescriptionsItem :label="t('archiveWorkspace.account')">{{
            selected.playerName
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.region')">{{
            selected.region
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.playedAt')">{{
            date(selected.createdAt)
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.savedAt')">{{
            date(selected.savedAt)
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.version')">{{
            selected.version || '—'
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.completeness')">{{
            t(selected.complete ? 'archiveWorkspace.full' : 'archiveWorkspace.partial')
          }}</NDescriptionsItem>
          <NDescriptionsItem label="K / D / A"
            >{{ selected.kills }} / {{ selected.deaths }} /
            {{ selected.assists }}</NDescriptionsItem
          >
          <NDescriptionsItem :label="t('archiveWorkspace.source')">{{
            selected.source
          }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.cs')">{{ selected.cs }}</NDescriptionsItem>
          <NDescriptionsItem :label="t('archiveWorkspace.damage')">{{
            selected.damage ?? '—'
          }}</NDescriptionsItem>
        </NDescriptions>
        <p class="archive-hint">{{ t('archiveWorkspace.detailHint') }}</p>
        <NDataTable
          :columns="participantColumns"
          :data="participants"
          size="small"
          :scroll-x="580"
        />
        <NCollapse class="archive-status"
          ><NCollapseItem :title="t('archiveWorkspace.raw')"
            ><NCode
              :code="JSON.stringify(selected.game, null, 2)"
              language="json"
              word-wrap /></NCollapseItem
        ></NCollapse>
      </NDrawerContent>
    </NDrawer>
  </div>
</template>

<script setup lang="tsx">
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useTranslation } from 'i18next-vue'
import { archiveErrorMessage } from './archive-error'
import {
  NAlert,
  NButton,
  NCode,
  NCollapse,
  NCollapseItem,
  NDataTable,
  NDatePicker,
  NDescriptions,
  NDescriptionsItem,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NInput,
  NPagination,
  NSelect,
  NTabPane,
  NTabs,
  NTag,
  useMessage,
  type DataTableColumns
} from 'naive-ui'
import { computed, onActivated, onDeactivated, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  useArchiveApi,
  type ArchivedMatch,
  type ArchiveList,
  type Diagnostics
} from './archive-api'

import { useRoute } from 'vue-router'
import PersonalWorkspace from './PersonalWorkspace.vue'
import VaultWorkspace from './VaultWorkspace.vue'

const { t } = useTranslation()
const api = useArchiveApi()
const ipc = useInstance(RiftFlowIpcRenderer)
const leagueClient = useLeagueClientStore()
const message = useMessage()
const route = useRoute()
watch(
  () => route.query.match,
  (key) => {
    if (typeof key === 'string') openKey(key)
  }
)
onMounted(() => {
  if (typeof route.query.match === 'string') openKey(route.query.match)
})
const tab = ref('archive')
const query = ref('')
const mode = ref<string | null>(null)
const dateRange = ref<[number, number] | null>(null)
const page = ref(1)
const loading = ref(false)
const working = ref(false)
const error = ref('')
const selected = ref<ArchivedMatch | null>(null)
const diagnostics = ref<Diagnostics | null>(null)
const list = ref<ArchiveList>({ records: [], modes: [], total: 0, matched: 0 })
let generation = 0
const date = (value?: number) => (value ? new Date(value).toLocaleString() : '—')
const report = (reason: unknown) => {
  error.value = archiveErrorMessage(reason, t('personalTools.operationFailed'))
}

async function loadList() {
  const current = ++generation
  loading.value = true
  try {
    const result = await api.list({
      query: query.value,
      mode: mode.value || '',
      page: page.value,
      start: dateRange.value?.[0],
      end: dateRange.value ? dateRange.value[1] + 86400000 - 1 : undefined
    })
    if (current === generation) list.value = result
  } catch (reason) {
    if (current === generation) report(reason)
  } finally {
    if (current === generation) loading.value = false
  }
}
async function refresh() {
  try {
    diagnostics.value = await api.diagnostics()
  } catch (reason) {
    report(reason)
  }
  await loadList()
}
async function synchronize() {
  working.value = true
  error.value = ''
  try {
    const result = await api.sync()
    if (result.success === false) message.warning(t('archiveWorkspace.syncFailed'))
    else message.success(t('archiveWorkspace.synced', { count: result.saved }))
  } catch (reason) {
    report(reason)
  } finally {
    working.value = false
    await refresh()
  }
}
async function exportFile(format: 'csv' | 'json') {
  error.value = ''
  working.value = true
  try {
    const result = await api.export(format)
    if (!result.cancelled) message.success(t('archiveWorkspace.exported'))
  } catch (reason) {
    report(reason)
  } finally {
    working.value = false
  }
}
async function restore() {
  error.value = ''
  working.value = true
  try {
    const result = await api.restore()
    if (!result.cancelled) {
      message.success(t('archiveWorkspace.restored', { count: result.count }))
      await refresh()
    }
  } catch (reason) {
    report(reason)
  } finally {
    working.value = false
  }
}
async function openKey(key: string) {
  try {
    selected.value = await api.get(key)
  } catch (reason) {
    report(reason)
  }
}
async function openRecord(row: ArchivedMatch) {
  try {
    selected.value = await api.get(row.key)
  } catch (reason) {
    report(reason)
  }
}
const rowProps = (row: ArchivedMatch) => ({
  onClick: () => openRecord(row),
  style: 'cursor: pointer'
})
const columns = computed<DataTableColumns<ArchivedMatch>>(() => [
  {
    title: t('archiveWorkspace.playedAt'),
    key: 'createdAt',
    width: 172,
    render: (row) => date(row.createdAt)
  },
  { title: t('archiveWorkspace.champion'), key: 'championName', width: 110 },
  {
    title: t('archiveWorkspace.result'),
    key: 'win',
    width: 78,
    render: (row) => (
      <NTag
        size="small"
        bordered={false}
        type={row.win === true ? 'success' : row.win === false ? 'error' : 'default'}
      >
        {t(
          row.win === true
            ? 'archiveWorkspace.win'
            : row.win === false
              ? 'archiveWorkspace.loss'
              : 'archiveWorkspace.unknown'
        )}
      </NTag>
    )
  },
  {
    title: 'K / D / A',
    key: 'kda',
    width: 105,
    render: (row) => `${row.kills ?? '—'} / ${row.deaths ?? '—'} / ${row.assists ?? '—'}`
  },
  {
    title: t('archiveWorkspace.mode'),
    key: 'gameMode',
    width: 110,
    render: (row) => `${row.gameMode} · ${row.queueId ?? '—'}`
  },
  {
    title: t('archiveWorkspace.account'),
    key: 'playerName',
    width: 140,
    ellipsis: { tooltip: true }
  },
  { title: t('archiveWorkspace.region'), key: 'region', width: 120 },
  { title: t('archiveWorkspace.version'), key: 'version', width: 105 },
  {
    title: t('archiveWorkspace.completeness'),
    key: 'complete',
    width: 100,
    render: (row) => t(row.complete ? 'archiveWorkspace.full' : 'archiveWorkspace.partial')
  }
])
const participants = computed(() =>
  (selected.value?.game?.participants || []).map((participant) => ({
    id: participant.participantId,
    team: participant.teamId,
    name:
      selected.value?.game?.participantIdentities.find(
        (identity) => identity.participantId === participant.participantId
      )?.player.gameName || '—',
    champion:
      leagueClient.gameData.champions[participant.championId]?.name ||
      String(participant.championId),
    kda: `${participant.stats.kills} / ${participant.stats.deaths} / ${participant.stats.assists}`,
    damage: participant.stats.totalDamageDealtToChampions
  }))
)
const participantColumns = computed(() => [
  { title: t('archiveWorkspace.account'), key: 'name' },
  { title: t('archiveWorkspace.champion'), key: 'champion' },
  { title: t('archiveWorkspace.team'), key: 'team' },
  { title: 'K / D / A', key: 'kda' },
  { title: t('archiveWorkspace.damage'), key: 'damage' }
])
const steps = computed(() => [
  {
    key: 'process',
    ready: !!diagnostics.value?.detectedClients || !!diagnostics.value?.connected,
    title: t('archiveWorkspace.processTitle'),
    description: t('archiveWorkspace.processHint')
  },
  {
    key: 'connection',
    ready: !!diagnostics.value?.connected,
    title: t('archiveWorkspace.connectionTitle'),
    description: t('archiveWorkspace.connectionHint')
  },
  {
    key: 'login',
    ready: !!diagnostics.value?.loggedIn,
    title: t('archiveWorkspace.loginTitle'),
    description: t('archiveWorkspace.loginHint')
  },
  {
    key: 'archive',
    ready: !!list.value.total,
    title: t('archiveWorkspace.localTitle'),
    description: t('archiveWorkspace.localHint', { count: list.value.total })
  }
])
async function openData() {
  try {
    await ipc.call('app-common-main', 'openUserDataDir')
  } catch (reason) {
    report(reason)
  }
}
async function copyDiagnostics() {
  try {
    await navigator.clipboard.writeText(JSON.stringify(diagnostics.value, null, 2))
    message.success(t('archiveWorkspace.copied'))
  } catch (reason) {
    report(reason)
  }
}
watch([query, mode, dateRange], () => {
  page.value = 1
  void loadList()
})
watch(page, () => void loadList())
let timer: ReturnType<typeof setInterval> | undefined
function startPolling() {
  if (timer) return
  void refresh()
  timer = setInterval(async () => {
    try {
      const next = await api.diagnostics()
      const changed =
        next.archiveCount !== diagnostics.value?.archiveCount ||
        next.lastSync.at !== diagnostics.value?.lastSync.at
      diagnostics.value = next
      if (changed) await loadList()
    } catch (reason) {
      report(reason)
    }
  }, 5000)
}
function stopPolling() {
  if (timer) clearInterval(timer)
  timer = undefined
}
onMounted(startPolling)
onActivated(startPolling)
onDeactivated(stopPolling)
onUnmounted(stopPolling)
</script>

<style scoped>
.archive-workspace {
  height: 100%;
  overflow: auto;
  padding: 24px;
  box-sizing: border-box;
  color: var(--la-color-text-primary);
}
.archive-heading {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin-bottom: 16px;
}
.archive-heading h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 600;
}
.archive-heading p,
.archive-hint,
.archive-summary {
  font-size: 12px;
  color: var(--la-text-secondary);
  line-height: 1.7;
}
.archive-heading p {
  margin: 6px 0 0;
}
.archive-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 12px 0;
}
.archive-filters {
  display: grid;
  grid-template-columns: minmax(180px, 1fr) 160px minmax(250px, 1fr);
  gap: 12px;
  margin: 18px 0 12px;
}
.archive-summary {
  margin-bottom: 12px;
}
.archive-pagination {
  justify-content: flex-end;
  margin-top: 16px;
}
.diagnostic-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  margin: 12px 0 20px;
}
.diagnostic-card {
  border: 1px solid var(--la-border);
  background: var(--la-surface);
  border-radius: 8px;
  padding: 16px;
}
.diagnostic-card h2 {
  margin: 14px 0 8px;
  font-size: 14px;
  font-weight: 600;
}
.diagnostic-card p {
  margin: 0;
  font-size: 12px;
  line-height: 1.8;
  color: var(--la-text-secondary);
}
.archive-status {
  margin-top: 16px;
}
@media (max-width: 1050px) {
  .diagnostic-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .archive-filters {
    grid-template-columns: minmax(180px, 1fr) 140px;
  }
  .archive-filters > :last-child {
    grid-column: 1 / -1;
  }
}
</style>
