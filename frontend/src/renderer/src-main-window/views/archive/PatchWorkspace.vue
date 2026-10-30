<template>
  <p class="hint">{{ t('personalTools.briefHint') }}</p>
  <NAlert v-if="error" type="error">{{ error }}</NAlert>
  <NAlert v-if="warnings.length" type="info">{{ warnings.join(' ') }}</NAlert>
  <p class="hint" v-if="published.length">
    {{
      t('personalTools.latestPublished', {
        version: published[0],
        source: t(region === 'cn' ? 'personalTools.regionCn' : 'personalTools.regionInternational')
      })
    }}
  </p>
  <NForm label-placement="top">
    <NFormItem :label="t('personalTools.patchRegion')"
      ><NSelect v-model:value="region" :options="regionOptions" :disabled="busy"
    /></NFormItem>
    <div class="versions">
      <NFormItem :label="t('personalTools.recentRange')"
        ><NSelect v-model:value="recent" :options="rangeOptions"
      /></NFormItem>
      <NFormItem :label="t('personalTools.endingVersion')"
        ><NSelect
          v-model:value="endVersion"
          :options="versionOptions"
          :loading="optionsBusy"
          :placeholder="t('personalTools.autoVersion')"
      /></NFormItem>
    </div>
    <p class="hint">
      {{
        selectedVersions.length
          ? t('personalTools.coveredVersions', { versions: selectedVersions.join('、') })
          : t('personalTools.versionUnavailable')
      }}
    </p>
    <NFormItem :label="t('personalTools.followChampions')"
      ><NSelect v-model:value="selected" multiple filterable :options="champions"
    /></NFormItem>
    <NFormItem :label="t('personalTools.followItems')"
      ><NSelect
        v-model:value="selectedItems"
        multiple
        filterable
        :options="items"
        :loading="optionsBusy"
        :placeholder="t('personalTools.searchChineseItems')"
    /></NFormItem>
  </NForm>
  <NSpace
    ><NButton
      type="primary"
      :loading="busy"
      :disabled="
        !selectedVersions.length || (!selected.length && !selectedItems.length) || optionsBusy
      "
      @click="load"
      >{{ t('personalTools.fetchBrief') }}</NButton
    ><NButton @click="favorites">{{ t('personalTools.fromArchive') }}</NButton>
    <NButton :loading="optionsBusy" @click="loadOptions(true)">{{
      t('personalTools.refreshCatalog')
    }}</NButton></NSpace
  >
  <NCard
    v-for="report in reports"
    :key="report.version"
    :title="report.version"
    class="report"
    size="small"
  >
    <NSpace
      ><NButton text type="primary" @click="open(report.source)">{{
        t('personalTools.officialSource')
      }}</NButton
      ><NTag size="small">{{ report.region }}</NTag
      ><span class="hint">{{
        report.at ? new Date(report.at).toLocaleString() : '—'
      }}</span></NSpace
    >
    <NAlert v-if="report.warning" type="warning">{{ report.warning }}</NAlert>
    <NEmpty
      v-if="report.available && !report.sections.length"
      :description="t('personalTools.noRelevant')"
    />
    <article v-for="section in report.sections" :key="`${section.context}:${section.title}`">
      <h3>{{ section.title }} · {{ section.context }}</h3>
      <NTag
        v-if="
          section.championIds.some(
            (id) => config.settings.runesV2[id] || config.settings.summonerSpells[id]
          )
        "
        size="small"
        type="warning"
        >{{ t('personalTools.checkPreset') }}</NTag
      >
      <p>{{ section.text }}</p>
    </article>
  </NCard>
</template>
<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NAlert, NButton, NCard, NEmpty, NForm, NFormItem, NSelect, NSpace, NTag } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useAutoChampConfigStore } from '@renderer-shared/shards/auto-champ-config/store'
import { useArchiveApi } from './archive-api'
import { archiveErrorMessage } from './archive-error'
interface Report {
  version: string
  source: string
  region: string
  at: number | null
  warning: string
  available: boolean
  sections: { title: string; context: string; text: string; championIds: number[] }[]
}
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const api = useArchiveApi()
const config = useAutoChampConfigStore()
const champions = ref<{ value: number; label: string }[]>([]),
  region = ref<'cn' | 'international'>('cn'),
  selected = ref<number[]>([]),
  recent = ref(3),
  endVersion = ref<string | null>(null),
  published = ref<string[]>([]),
  selectedItems = ref<string[]>([]),
  items = ref<{ value: string; label: string; english: string }[]>([]),
  warnings = ref<string[]>([]),
  optionsBusy = ref(false),
  reports = ref<Report[]>([]),
  busy = ref(false),
  error = ref('')
let initialized = false
let catalogRequest = 0
const regionOptions = computed(() => [
  { value: 'cn', label: t('personalTools.regionCn') },
  { value: 'international', label: t('personalTools.regionInternational') }
])
watch(
  region,
  () => {
    if (!initialized) return
    published.value = []
    endVersion.value = null
    reports.value = []
    error.value = ''
    warnings.value = []
    void loadOptions()
  },
  { flush: 'sync' }
)
const rangeOptions = computed(() =>
  [1, 3, 6].map((count) => ({ value: count, label: t('personalTools.recentVersions', { count }) }))
)
const versionOptions = computed(() =>
  published.value.map((version, index) => ({
    value: version,
    label: index === 0 ? t('personalTools.latestVersionOption', { version }) : version
  }))
)
const selectedVersions = computed(() => {
  const index = published.value.indexOf(endVersion.value || '')
  return index < 0 ? [] : published.value.slice(index, index + recent.value)
})
async function loadOptions(refresh = false) {
  const request = ++catalogRequest
  const selectedRegion = region.value
  optionsBusy.value = true
  try {
    const data = await ipc.call<{
      versions: string[]
      items: { value: string; label: string; english: string }[]
      warnings: string[]
    }>('riftflow-archive-main', 'patches', { action: 'options', refresh, region: selectedRegion })
    if (request !== catalogRequest || selectedRegion !== region.value) return
    published.value = data.versions
    items.value = data.items
    warnings.value = data.warnings
    if (!endVersion.value || !published.value.includes(endVersion.value))
      endVersion.value = published.value[0] || null
    selectedItems.value = selectedItems.value.filter((id) =>
      items.value.some((item) => item.value === id)
    )
  } catch (e) {
    if (request !== catalogRequest) return
    error.value = archiveErrorMessage(e, t('personalTools.catalogFailed'))
  } finally {
    if (request === catalogRequest) optionsBusy.value = false
  }
}
function open(url: string) {
  window.open(url, '_blank')
}
async function favorites() {
  try {
    const usage = new Map<number, number>()
    const first = await api.list({})
    for (let page = 1; page <= Math.min(20, Math.max(1, Math.ceil(first.matched / 50))); page++) {
      for (const r of page === 1 ? first.records : (await api.list({ page })).records)
        usage.set(r.championId, (usage.get(r.championId) || 0) + 1)
    }
    selected.value = [...usage]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5)
      .map(([id]) => id)
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  }
}
async function load() {
  busy.value = true
  error.value = ''
  try {
    const result = await ipc.call<{ reports: Report[] }>('riftflow-archive-main', 'patches', {
      versions: selectedVersions.value,
      region: region.value,
      endVersion: endVersion.value,
      recent: recent.value,
      champions: selected.value,
      items: selectedItems.value
    })
    reports.value = result.reports
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  } finally {
    busy.value = false
  }
}
onMounted(async () => {
  try {
    champions.value = (
      await ipc.call<{ id: number; name: string }[]>('riftflow-archive-main', 'champions')
    ).map((c) => ({ value: c.id, label: c.name }))
    const prefs = await ipc.call<{
      endVersion?: string
      region?: 'cn' | 'international'
      recent?: number
      champions: number[]
      items?: string[]
    } | null>('riftflow-archive-main', 'patches', { action: 'preferences' })
    if (prefs) {
      if (prefs.region === 'cn' || prefs.region === 'international') region.value = prefs.region
      endVersion.value = prefs.endVersion || null
      if ([1, 3, 6].includes(prefs.recent || 0)) recent.value = prefs.recent!
      selected.value = prefs.champions
      selectedItems.value = prefs.items || []
    }
    initialized = true
    await loadOptions()
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  }
})
</script>
<style scoped>
.hint {
  color: var(--la-text-secondary);
  line-height: 1.8;
}
.versions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
}
.report {
  margin-top: 16px;
}
.report h3 {
  font-size: 14px;
  margin: 18px 0 8px;
}
.report p {
  line-height: 1.9;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
