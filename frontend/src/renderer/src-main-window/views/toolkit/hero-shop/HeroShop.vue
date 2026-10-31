<template>
  <NScrollbar class="h-full">
    <div class="workspace-content flex flex-col">
      <SettingsSection
        title="一键购买英雄"
        footer="仅使用蓝色精萃。先勾选英雄并核对总价，确认后逐个购买；失败时停止后续购买。"
      >
        <div class="flex flex-col gap-3 p-3">
          <NAlert v-if="!lcs.isConnected" type="info">请连接英雄联盟客户端后刷新。</NAlert>
          <NAlert v-if="error" type="error" :show-icon="true">{{ error }}</NAlert>
          <NAlert v-if="warnings.length" type="warning">{{ warnings.join('；') }}</NAlert>
          <div class="flex flex-wrap items-center gap-2">
            <span>蓝色精萃：{{ balance === null ? '未读取' : balance.toLocaleString() }}</span>
            <NButton
              size="small"
              :loading="loading"
              :disabled="busy || !lcs.isConnected"
              @click="refresh"
              >刷新</NButton
            >
            <NButton size="small" :disabled="loading || busy || !heroes.length" @click="selectAll"
              >全选可购英雄</NButton
            >
            <NButton size="small" :disabled="loading || busy" @click="excludeWeekly"
              >一键排除周免</NButton
            >
            <NButton size="small" :disabled="loading || busy" @click="selected = []"
              >清空选择</NButton
            >
          </div>
          <div class="rounded border border-current/10 p-3 text-sm">
            <div class="mb-2">当前周免（{{ weekly.length }} 位）</div>
            <div class="flex flex-wrap gap-1">
              <NTag v-for="hero in weekly" :key="hero.id" size="small" type="info"
                >{{ hero.name }}{{ hero.owned ? ' · 已拥有' : '' }}</NTag
              >
              <span v-if="!weekly.length">{{
                heroes.length ? '客户端当前未列出周免英雄' : '等待读取客户端'
              }}</span>
            </div>
          </div>
          <NInput v-model:value="search" placeholder="搜索英雄名称" clearable :disabled="busy" />
          <NAlert v-if="selectedWeekly.length" type="warning">
            已勾选 {{ selectedWeekly.length }} 位当前周免英雄：{{
              selectedWeekly.map((h) => h.name).join('、')
            }}。周免期间可免费使用，购买会消耗蓝色精萃。
          </NAlert>
          <NDataTable
            :columns="columns"
            :data="filtered"
            :row-key="(h: Hero) => h.id"
            :checked-row-keys="selected"
            @update:checked-row-keys="updateSelected"
            :loading="loading"
            size="small"
            :max-height="400"
          />
          <div class="flex flex-wrap items-center gap-2">
            <span>已选 {{ chosen.length }} 位 · 合计 {{ total.toLocaleString() }} 蓝色精萃</span>
            <NButton
              type="primary"
              size="small"
              :loading="preparing"
              :disabled="busy || loading || !chosen.length || balance === null || !lcs.isConnected"
              @click="prepare"
              >核对并购买</NButton
            >
            <NButton
              v-if="buying"
              size="small"
              type="warning"
              :disabled="stop"
              @click="stop = true"
              >{{ stop ? '本次完成后停止' : '停止后续购买' }}</NButton
            >
          </div>
          <NAlert v-if="results.length" type="success"
            >已确认购买 {{ results.length }} 位：{{ results.join('、') }}</NAlert
          >
          <div v-if="progress" class="text-sm">{{ progress }}</div>
        </div>
      </SettingsSection>
    </div>
  </NScrollbar>
  <NModal
    v-model:show="confirm"
    preset="card"
    title="确认购买英雄"
    style="width: 540px; max-width: 92vw"
    :mask-closable="false"
  >
    <template v-if="quote">
      <p>
        购买 {{ quote.heroes.length }} 位英雄，合计消耗
        <strong>{{ quote.total.toLocaleString() }}</strong> 蓝色精萃。
      </p>
      <p>
        当前余额 {{ quote.balance.toLocaleString() }}，购买后预计剩余
        {{ (quote.balance - quote.total).toLocaleString() }}。
      </p>
      <NScrollbar style="max-height: 220px"
        ><div v-for="h in quote.heroes" :key="h.id" class="py-1">
          {{ h.name }} · {{ h.price }} 蓝色精萃
          <NTag v-if="h.freeToPlay" size="small" type="warning">当前周免</NTag>
        </div></NScrollbar
      >
      <NAlert v-if="quote.heroes.some((h) => h.freeToPlay)" type="warning" class="my-3"
        >包含当前周免英雄。这些英雄本周可免费使用；继续购买仍会消耗蓝色精萃。</NAlert
      >
      <p class="text-sm">
        逐个提交并确认到账。关闭页面或点击停止，只会停止后续购买，已提交的购买无法撤回。
      </p>
      <div class="mt-3 flex justify-end gap-2">
        <NButton @click="confirm = false">取消</NButton
        ><NButton type="primary" @click="purchase">确认使用蓝色精萃购买</NButton>
      </div>
    </template>
  </NModal>
</template>
<script setup lang="ts">
import SettingsSection from '@renderer-shared/components/SettingsSection.vue'
import { archiveErrorMessage } from '@main-window/views/archive/archive-error'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import {
  NAlert,
  NButton,
  NDataTable,
  NInput,
  NModal,
  NScrollbar,
  NTag,
  type DataTableColumns
} from 'naive-ui'
import {
  computed,
  h,
  onMounted,
  onBeforeUnmount,
  onDeactivated,
  onActivated,
  ref,
  watch
} from 'vue'
interface Hero {
  id: number
  name: string
  owned: boolean
  freeToPlay: boolean
  price: number | null
}
interface Quote {
  id: number
  total: number
  balance: number
  heroes: Hero[]
}
const ipc = useInstance(RiftFlowIpcRenderer)
const lcs = useLeagueClientStore()
const heroes = ref<Hero[]>([]),
  selected = ref<number[]>([]),
  balance = ref<number | null>(null)
const loading = ref(false),
  preparing = ref(false),
  buying = ref(false),
  confirm = ref(false),
  stop = ref(false)
const quote = ref<Quote | null>(null),
  error = ref(''),
  search = ref(''),
  results = ref<string[]>([]),
  progress = ref('')
const warnings = ref<string[]>([])
const busy = computed(() => preparing.value || buying.value)
const weekly = computed(() => heroes.value.filter((h) => h.freeToPlay))
const chosen = computed(() =>
  heroes.value.filter((h) => selected.value.includes(h.id) && !h.owned && h.price !== null)
)
const selectedWeekly = computed(() => chosen.value.filter((h) => h.freeToPlay))
const total = computed(() => chosen.value.reduce((sum, h) => sum + (h.price ?? 0), 0))
const filtered = computed(() =>
  heroes.value.filter((h) => h.name.toLowerCase().includes(search.value.toLowerCase()))
)
const columns = computed<DataTableColumns<Hero>>(() => [
  {
    type: 'selection',
    disabled: (h) => busy.value || loading.value || h.owned || h.price === null
  },
  { title: '英雄', key: 'name' },
  {
    title: '当前周免',
    key: 'freeToPlay',
    render: (row) => (row.freeToPlay ? h(NTag, { size: 'small', type: 'info' }, () => '周免') : '—')
  },
  {
    title: '状态',
    key: 'owned',
    render: (row) => (row.owned ? '已拥有' : row.price === null ? '暂不可购' : '未拥有')
  },
  { title: '蓝色精萃', key: 'price', render: (row) => row.price?.toLocaleString() ?? '—' }
])
function updateSelected(keys: Array<string | number>) {
  if (!busy.value) selected.value = keys.map(Number)
}
function selectAll() {
  selected.value = heroes.value.filter((h) => !h.owned && h.price !== null).map((h) => h.id)
}
function excludeWeekly() {
  const ids = new Set(weekly.value.map((h) => h.id))
  selected.value = selected.value.filter((id) => !ids.has(id))
}
function message(e: unknown) {
  return archiveErrorMessage(e, '读取失败，请确认客户端已登录后重试')
}
let active = true
let generation = 0
async function refresh() {
  if (loading.value || busy.value) return
  loading.value = true
  error.value = ''
  warnings.value = []
  confirm.value = false
  quote.value = null
  selected.value = []
  try {
    const data = await ipc.call<{ balance: number | null; heroes: Hero[]; warnings?: string[] }>(
      'hero-shop-main',
      'list'
    )
    heroes.value = data.heroes
    balance.value = data.balance
    warnings.value = data.warnings ?? []
  } catch (e) {
    error.value = message(e)
    heroes.value = []
    balance.value = null
  } finally {
    loading.value = false
  }
}
async function prepare() {
  const current = generation
  preparing.value = true
  error.value = ''
  results.value = []
  progress.value = ''
  try {
    quote.value = await ipc.call<Quote>('hero-shop-main', 'prepare', {
      ids: chosen.value.map((h) => h.id)
    })
    if (active && current === generation) confirm.value = true
  } catch (e) {
    error.value = message(e)
  } finally {
    preparing.value = false
  }
}
async function purchase() {
  if (!quote.value || buying.value) return
  const order = quote.value
  confirm.value = false
  buying.value = true
  stop.value = false
  error.value = ''
  try {
    for (const hero of order.heroes) {
      if (stop.value || !active) break
      progress.value = `正在购买 ${hero.name}（${results.value.length + 1}/${order.heroes.length}）`
      const result = await ipc.call<{ name: string; price: number }>('hero-shop-main', 'buy', {
        id: order.id,
        heroId: hero.id
      })
      results.value.push(result.name)
      const local = heroes.value.find((h) => h.id === hero.id)
      if (local) local.owned = true
      selected.value = selected.value.filter((id) => id !== hero.id)
      if (balance.value !== null) balance.value -= result.price
    }
    progress.value = stop.value || !active ? '已停止后续购买，已完成的购买保留。' : '购买完成。'
  } catch (e) {
    error.value = message(e)
    progress.value = '已停止后续购买。请刷新余额与英雄资产后再操作。'
    selected.value = []
  } finally {
    buying.value = false
    quote.value = null
  }
}
function leave() {
  generation++
  active = false
  stop.value = true
  confirm.value = false
}
onMounted(() => {
  if (lcs.isConnected) void refresh()
})
onBeforeUnmount(leave)
onDeactivated(leave)
onActivated(() => {
  active = true
})
watch(
  () => lcs.isConnected,
  (connected) => {
    if (!connected) {
      leave()
      heroes.value = []
      selected.value = []
      balance.value = null
    } else {
      active = true
      void refresh()
    }
  }
)
</script>
