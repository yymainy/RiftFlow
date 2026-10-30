<template>
  <div class="personal-workspace">
    <NAlert v-if="error" type="error" closable @close="error = ''">{{ error }}</NAlert>
    <NTabs type="segment" animated>
      <NTabPane name="memory" :tab="t('personalTools.memory')">
        <p class="hint">{{ t('personalTools.memoryHint') }}</p>
        <NForm label-placement="top">
          <NFormItem :label="t('personalTools.source')"
            ><NSelect v-model:value="matchKey" filterable :options="matches"
          /></NFormItem>
          <div class="form-grid">
            <NFormItem :label="t('personalTools.opponent')"
              ><NSelect v-model:value="opponent" filterable :options="champions"
            /></NFormItem>
            <NFormItem :label="t('personalTools.role')"
              ><NSelect v-model:value="role" :options="roles"
            /></NFormItem>
            <NFormItem :label="t('personalTools.status')"
              ><NSelect v-model:value="status" :options="statuses"
            /></NFormItem>
          </div>
          <NFormItem :label="t('personalTools.fact')"
            ><NInput v-model:value="fact" type="textarea" maxlength="500" show-count
          /></NFormItem>
          <NFormItem :label="t('personalTools.action')"
            ><NInput v-model:value="action" type="textarea" maxlength="500" show-count
          /></NFormItem>
        </NForm>
        <NSpace
          ><NButton
            type="primary"
            :loading="busy"
            :disabled="!matchKey || !opponent || !fact.trim() || !action.trim()"
            @click="saveNote"
            >{{ t('personalTools.save') }}</NButton
          ><NButton @click="reset">{{ t('personalTools.new') }}</NButton
          ><NButton @click="load">{{ t('archiveWorkspace.refresh') }}</NButton></NSpace
        >
        <NInput
          v-model:value="search"
          clearable
          class="search"
          :placeholder="t('personalTools.filter')"
        />
        <NEmpty v-if="!filteredNotes.length" :description="t('personalTools.emptyNotes')" />
        <NCard
          v-for="note in filteredNotes.slice(0, noteLimit)"
          :key="note.id"
          size="small"
          class="note"
        >
          <template #header>{{ note.championName }} → {{ championName(note.opponent) }}</template>
          <NSpace
            ><NTag size="small">{{ note.gameMode }}</NTag
            ><NTag size="small">{{ note.version || '—' }}</NTag
            ><NTag size="small">{{ t(`personalTools.${note.status}`) }}</NTag
            ><NTag size="small">{{ t(`personalTools.${note.role}`) }}</NTag></NSpace
          >
          <p>{{ note.fact }}</p>
          <p>{{ t('personalTools.action') }}：{{ note.action }}</p>
          <NSpace
            ><NButton size="small" @click="emit('open', note.matchKey)"
              >{{ t('personalTools.openMatch') }} {{ note.gameId }}</NButton
            ><NButton size="small" @click="edit(note)">{{ t('personalTools.edit') }}</NButton
            ><NPopconfirm @positive-click="removeNote(note.id)"
              ><template #trigger
                ><NButton size="small">{{ t('personalTools.delete') }}</NButton></template
              >{{ t('personalTools.confirmDelete') }}</NPopconfirm
            ></NSpace
          >
        </NCard>
        <NButton v-if="filteredNotes.length > noteLimit" class="search" @click="noteLimit += 20">{{
          t('personalTools.more')
        }}</NButton>
      </NTabPane>
      <NTabPane name="practice" :tab="t('personalTools.practice')">
        <p class="hint">{{ t('personalTools.practiceHint') }}</p>
        <NForm label-placement="top">
          <NFormItem :label="t('personalTools.source')"
            ><NSelect v-model:value="matchKey" filterable :options="matches"
          /></NFormItem>
          <div class="form-grid">
            <NFormItem :label="t('personalTools.metric')"
              ><NSelect v-model:value="metric" :options="metrics" /></NFormItem
            ><NFormItem :label="t('personalTools.target')"
              ><NInputNumber v-model:value="target" :min="0" :max="100" :precision="1"
            /></NFormItem>
          </div>
        </NForm>
        <NSpace
          ><NButton
            type="primary"
            :loading="busy"
            :disabled="!matchKey || target === null"
            @click="saveGoal"
            >{{ t('personalTools.startGoal') }}</NButton
          ><NButton @click="load">{{ t('archiveWorkspace.refresh') }}</NButton
          ><NPopconfirm v-if="data.goal" @positive-click="mutate('clearGoal')"
            ><template #trigger
              ><NButton>{{ t('personalTools.clearGoal') }}</NButton></template
            >{{ t('personalTools.confirmDelete') }}</NPopconfirm
          ></NSpace
        >
        <NCard
          v-if="data.goal"
          class="note"
          :title="`${data.goal.championName} · ${t(`personalTools.${data.goal.metric}`)} ${data.goal.metric === 'deaths' ? '≤' : '≥'} ${data.goal.target}`"
        >
          <p class="hint">
            {{ data.goal.gameMode }} / {{ data.goal.version || '—' }} / {{ data.goal.role }} ·
            {{ t('personalTools.sampleHint') }}
          </p>
          <div class="form-grid">
            <div v-for="group in ['before', 'after'] as const" :key="group">
              <h3>{{ t(`personalTools.${group}`) }} ({{ progress[group]?.length || 0 }}/5)</h3>
              <NEmpty v-if="!progress[group]?.length" :description="t('personalTools.noSample')" />
              <p v-for="row in progress[group]" :key="row.key">
                <NButton text @click="emit('open', row.key)">#{{ row.gameId }}</NButton> ·
                {{ row.value.toFixed(1) }} · {{ Math.round(row.duration / 60) }} min
                <NTag
                  v-if="group === 'after'"
                  size="small"
                  :type="row.met ? 'success' : 'warning'"
                  >{{ t(row.met ? 'personalTools.met' : 'personalTools.notMet') }}</NTag
                >
              </p>
            </div>
          </div>
          <NInput
            v-model:value="review"
            type="textarea"
            maxlength="1000"
            :placeholder="t('personalTools.reviewHint')"
          /><NButton class="search" :loading="busy" @click="mutate('reviewGoal', { review })">{{
            t('personalTools.saveReview')
          }}</NButton>
        </NCard>
      </NTabPane>
      <NTabPane name="batch" :tab="t('personalTools.batch')"><BatchWorkspace /></NTabPane>
      <NTabPane name="brief" :tab="t('personalTools.brief')"><PatchWorkspace /></NTabPane>
    </NTabs>
  </div>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useTranslation } from 'i18next-vue'
import {
  NAlert,
  NButton,
  NCard,
  NEmpty,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NPopconfirm,
  NSelect,
  NSpace,
  NTabPane,
  NTabs,
  NTag,
  useMessage
} from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useArchiveApi, type ArchivedMatch } from './archive-api'
import { archiveErrorMessage } from './archive-error'
import BatchWorkspace from './BatchWorkspace.vue'
import PatchWorkspace from './PatchWorkspace.vue'
export interface MemoryNote {
  id: string
  matchKey: string
  gameId: string
  championId: number
  championName: string
  opponent: number
  role: string
  gameMode: string
  version: string
  status: string
  fact: string
  action: string
  owner: string
  region: string
}
interface Goal {
  championName: string
  metric: string
  target: number
  gameMode: string
  version: string
  role: string
  review: string
}
interface Sample {
  key: string
  gameId: string
  duration: number
  value: number
  met: boolean
}
const emit = defineEmits<{ open: [key: string] }>()
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const api = useArchiveApi()
const message = useMessage()
const noteLimit = ref(20)
const data = ref<{ notes: MemoryNote[]; goal: Goal | null }>({ notes: [], goal: null })
const progress = ref<{ before: Sample[]; after: Sample[] }>({ before: [], after: [] })
const records = ref<ArchivedMatch[]>([])
const champions = ref<{ label: string; value: number }[]>([])
const matchKey = ref<string | null>(null),
  opponent = ref<number | null>(null),
  role = ref('unknown'),
  status = ref('unreviewed'),
  fact = ref(''),
  action = ref(''),
  editId = ref(''),
  search = ref(''),
  metric = ref('csPerMinute'),
  target = ref<number | null>(6),
  review = ref(''),
  error = ref(''),
  busy = ref(false)
const options = (keys: string[]) =>
  keys.map((value) => ({ value, label: t(`personalTools.${value}`) }))
const roles = computed(() => options(['unknown', 'top', 'jungle', 'middle', 'bottom', 'utility']))
const statuses = computed(() => options(['unreviewed', 'effective', 'ineffective', 'outdated']))
const metrics = computed(() =>
  options(['csPerMinute', 'csAt10', 'firstDeathMinute', 'deaths', 'controlWards'])
)
const matches = computed(() =>
  records.value.map((r) => ({
    value: r.key,
    label: `${r.championName} · ${r.gameMode} · #${r.gameId} · ${r.playerName}`
  }))
)
const championName = (id: number) =>
  champions.value.find((c) => c.value === id)?.label || String(id)
const filteredNotes = computed(() =>
  [...data.value.notes]
    .reverse()
    .filter((n) =>
      `${n.championName} ${championName(n.opponent)} ${n.fact} ${n.action} ${n.gameMode}`
        .toLowerCase()
        .includes(search.value.toLowerCase())
    )
)
async function load() {
  try {
    data.value = await ipc.call('riftflow-archive-main', 'personal', 'get')
    progress.value = await ipc.call('riftflow-archive-main', 'personal', 'progress')
    review.value = data.value.goal?.review || ''
    const first = await api.list({})
    records.value = first.records
    if (first.matched > 50) {
      for (let page = 2; page <= Math.min(Math.ceil(first.matched / 50), 20); page++)
        records.value.push(...(await api.list({ page })).records)
    }
    const all = await ipc.call<{ id: number; name: string }[]>('riftflow-archive-main', 'champions')
    champions.value = all.map((c: { id: number; name: string }) => ({ label: c.name, value: c.id }))
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  }
}
async function mutate(method: string, input: Record<string, unknown> = {}) {
  busy.value = true
  error.value = ''
  try {
    await ipc.call('riftflow-archive-main', 'personal', method, input)
    await load()
    message.success(t('personalTools.saved'))
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  } finally {
    busy.value = false
  }
}
async function saveNote() {
  await mutate('saveNote', {
    id: editId.value,
    matchKey: matchKey.value,
    opponent: opponent.value,
    role: role.value,
    status: status.value,
    fact: fact.value,
    action: action.value
  })
}
function reset() {
  editId.value = ''
  fact.value = ''
  action.value = ''
  status.value = 'unreviewed'
}
function edit(n: MemoryNote) {
  editId.value = n.id
  matchKey.value = n.matchKey
  opponent.value = n.opponent
  role.value = n.role
  status.value = n.status
  fact.value = n.fact
  action.value = n.action
}
async function removeNote(id: string) {
  await mutate('deleteNote', { id })
}
async function saveGoal() {
  await mutate('saveGoal', { matchKey: matchKey.value, metric: metric.value, target: target.value })
}
onMounted(load)
</script>
<style scoped>
.hint {
  color: var(--la-text-secondary);
  line-height: 1.8;
}
.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 16px;
}
.note,
.search {
  margin-top: 16px;
}
.note p {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.personal-workspace {
  padding-top: 12px;
}
.note h3 {
  font-size: 14px;
  margin: 12px 0;
}
</style>
