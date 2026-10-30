<template>
  <div class="skin-assets">
    <header class="page-heading">
      <div>
        <h1>{{ t('skinAssets.title') }}</h1>
        <p class="hint">{{ t('skinAssets.hint') }}</p>
      </div>
      <NSpace align="center">
        <NButton type="primary" :loading="busy" :disabled="!canRefresh || busy" @click="refresh">{{
          t('skinAssets.refresh')
        }}</NButton>
        <NButton :disabled="!assets?.skins?.length || busy" @click="exportCsv">{{
          t('skinAssets.export')
        }}</NButton>
      </NSpace>
    </header>
    <NAlert v-if="error" type="error">{{ error }}</NAlert>
    <NAlert v-if="!canRefresh" type="info">{{ t('skinAssets.connectHint') }}</NAlert>
    <template v-if="assets?.skins?.length">
      <p class="hint snapshot">
        {{ assets.playerName }} · {{ assets.region }} ·
        {{ new Date(assets.updatedAt).toLocaleString() }}
      </p>
      <NSpace class="counts">
        <NTag type="success">{{ t('skinAssets.ownedCount', { count: assets.owned }) }}</NTag>
        <NTag type="warning">{{ t('skinAssets.missingCount', { count: assets.missing }) }}</NTag>
        <NTag>{{ t('skinAssets.totalCount', { count: assets.total }) }}</NTag>
      </NSpace>
      <div class="filters">
        <NInput v-model:value="query" clearable :placeholder="t('skinAssets.search')" />
        <NSelect v-model:value="status" :options="statusOptions" />
      </div>
      <div class="collection-layout">
        <section class="champion-panel" :aria-label="t('skinAssets.championList')">
          <div class="panel-heading">
            {{ t('skinAssets.championList') }} · {{ visibleGroups.length }}
          </div>
          <div class="champion-list">
            <NButton
              v-for="group in visibleGroups"
              :key="group.id"
              class="champion-button"
              :type="selectedGroup?.id === group.id ? 'primary' : 'default'"
              :secondary="selectedGroup?.id !== group.id"
              @click="selectedChampion = group.id"
            >
              <span class="champion-line"
                ><span>{{ group.name }}</span
                ><span>{{ group.owned }}/{{ group.total }}</span></span
              >
              <span class="completion-track"
                ><span
                  :style="{ width: `${group.total ? (group.owned / group.total) * 100 : 0}%` }"
              /></span>
            </NButton>
          </div>
        </section>
        <section class="skin-panel" :aria-label="t('skinAssets.skinList')">
          <template v-if="selectedGroup">
            <div class="skin-heading">
              <div>
                <h2>{{ selectedGroup.name }}</h2>
                <p class="hint">
                  {{
                    t('skinAssets.championProgress', {
                      owned: selectedGroup.owned,
                      total: selectedGroup.total
                    })
                  }}
                </p>
              </div>
              <NTag :type="selectedGroup.owned === selectedGroup.total ? 'success' : 'default'"
                >{{ Math.round((selectedGroup.owned / selectedGroup.total) * 100) }}%</NTag
              >
            </div>
            <NDataTable
              :columns="columns"
              :data="visibleSkins"
              :row-key="(row: Skin) => row.id"
              :pagination="{ pageSize: 20 }"
              size="small"
              :scroll-x="420"
            >
              <template #empty><NEmpty :description="t('skinAssets.noMatch')" /></template>
            </NDataTable>
          </template>
          <NEmpty v-else :description="t('skinAssets.noMatch')" />
        </section>
      </div>
    </template>
    <NEmpty v-else :description="t('skinAssets.empty')" />
  </div>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useTranslation } from 'i18next-vue'
import {
  NAlert,
  NButton,
  NDataTable,
  NEmpty,
  NInput,
  NSelect,
  NSpace,
  NTag,
  type DataTableColumns
} from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { archiveErrorMessage } from './archive-error'

interface Skin {
  id: number
  championId: number
  championName: string
  name: string
  owned: boolean
}
interface Assets {
  owner: string
  region: string
  playerName: string
  updatedAt: number
  total: number
  owned: number
  missing: number
  skins: Skin[]
}
interface ChampionGroup {
  id: number
  name: string
  owned: number
  total: number
  skins: Skin[]
}
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const client = useLeagueClientStore()
const canRefresh = computed(() => client.isConnected && !!client.summoner.me?.puuid)
const assets = ref<Assets | null>(null)
const busy = ref(false)
const error = ref('')
const query = ref('')
const status = ref('all')
const selectedChampion = ref<number | null>(null)
const statusOptions = computed(() => [
  { label: t('skinAssets.all'), value: 'all' },
  { label: t('skinAssets.owned'), value: 'owned' },
  { label: t('skinAssets.missing'), value: 'missing' }
])
const groups = computed(() => {
  const map = new Map<number, ChampionGroup>()
  for (const skin of assets.value?.skins || []) {
    let group = map.get(skin.championId)
    if (!group) {
      group = { id: skin.championId, name: skin.championName, owned: 0, total: 0, skins: [] }
      map.set(skin.championId, group)
    }
    group.skins.push(skin)
    group.total++
    if (skin.owned) group.owned++
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'))
})
const normalizedQuery = computed(() => query.value.trim().toLocaleLowerCase())
const visibleGroups = computed(() =>
  groups.value.filter((group) => {
    const skins = group.skins.filter(
      (skin) => status.value === 'all' || skin.owned === (status.value === 'owned')
    )
    return (
      skins.length > 0 &&
      (!normalizedQuery.value ||
        group.name.toLocaleLowerCase().includes(normalizedQuery.value) ||
        skins.some((skin) =>
          `${skin.name} ${skin.id}`.toLocaleLowerCase().includes(normalizedQuery.value)
        ))
    )
  })
)
const selectedGroup = computed(
  () =>
    visibleGroups.value.find((group) => group.id === selectedChampion.value) ||
    visibleGroups.value[0] ||
    null
)
const visibleSkins = computed(() => {
  const group = selectedGroup.value
  if (!group) return []
  return group.skins
    .filter(
      (skin) =>
        (status.value === 'all' || skin.owned === (status.value === 'owned')) &&
        (!normalizedQuery.value ||
          group.name.toLocaleLowerCase().includes(normalizedQuery.value) ||
          `${skin.name} ${skin.id}`.toLocaleLowerCase().includes(normalizedQuery.value))
    )
    .sort((a, b) => Number(b.owned) - Number(a.owned) || a.id - b.id)
})
const columns = computed<DataTableColumns<Skin>>(() => [
  { title: t('skinAssets.skin'), key: 'name' },
  {
    title: t('skinAssets.status'),
    key: 'owned',
    render: (row) => t(row.owned ? 'skinAssets.owned' : 'skinAssets.missing')
  }
])
async function load(action: 'cached' | 'refresh') {
  busy.value = true
  error.value = ''
  try {
    const result = await ipc.call<Assets>('riftflow-archive-main', 'assets', action)
    if (result?.skins) {
      assets.value = result
      selectedChampion.value = null
    }
  } catch (cause) {
    error.value = archiveErrorMessage(cause, t('personalTools.operationFailed'))
  } finally {
    busy.value = false
  }
}
function refresh() {
  void load('refresh')
}
async function exportCsv() {
  try {
    await ipc.call('riftflow-archive-main', 'assets', 'export')
  } catch (cause) {
    error.value = archiveErrorMessage(cause, t('personalTools.operationFailed'))
  }
}
onMounted(() => void load('cached'))
</script>
<style scoped>
.skin-assets {
  display: grid;
  gap: 16px;
  padding: 24px;
}
.page-heading,
.skin-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
.page-heading h1,
.skin-heading h2 {
  margin: 0 0 6px;
  font-size: 22px;
}
.skin-heading h2 {
  font-size: 19px;
}
.page-heading p,
.skin-heading p,
.snapshot {
  margin: 0;
}
.hint {
  color: var(--la-text-secondary);
}
.filters {
  display: grid;
  grid-template-columns: minmax(240px, 1fr) 180px;
  gap: 12px;
}
.collection-layout {
  display: grid;
  grid-template-columns: minmax(230px, 280px) minmax(0, 1fr);
  gap: 16px;
  min-height: 500px;
}
.champion-panel,
.skin-panel {
  border: 1px solid var(--la-border);
  background: var(--la-surface);
  border-radius: 8px;
  padding: 12px;
  min-width: 0;
}
.panel-heading {
  font-weight: 600;
  padding: 3px 4px 12px;
}
.champion-list {
  display: grid;
  align-content: start;
  gap: 6px;
  max-height: min(62vh, 700px);
  overflow: auto;
  padding-right: 4px;
}
.champion-button {
  height: auto;
  min-height: 46px;
  width: 100%;
  padding: 6px 10px;
}
.champion-button :deep(.n-button__content) {
  display: grid;
  width: 100%;
  gap: 5px;
}
.champion-line {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
}
.completion-track {
  display: block;
  height: 3px;
  background: var(--la-border);
  border-radius: 3px;
  overflow: hidden;
  width: 100%;
}
.completion-track > span {
  display: block;
  height: 100%;
  background: rgb(var(--la-card-border-rgb) / 0.65);
}
.skin-heading {
  margin-bottom: 14px;
}
@media (max-width: 850px) {
  .collection-layout {
    grid-template-columns: 1fr;
  }
  .champion-list {
    max-height: 220px;
  }
  .filters {
    grid-template-columns: 1fr;
  }
}
</style>
