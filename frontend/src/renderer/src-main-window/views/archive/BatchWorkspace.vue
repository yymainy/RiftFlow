<template>
  <p class="hint">{{ t('personalTools.batchHint') }}</p>
  <NAlert v-if="error" type="error">{{ error }}</NAlert>
  <NForm label-placement="top">
    <NFormItem :label="t('personalTools.configType')"
      ><NSelect v-model:value="kind" :options="kindOptions"
    /></NFormItem>
    <NFormItem :label="t('personalTools.template')"
      ><NSelect v-model:value="source" :options="templates" filterable
    /></NFormItem>
    <NAlert v-if="!templates.length" type="info">{{ t('personalTools.noTemplate') }}</NAlert>
    <NFormItem :label="t('personalTools.targets')"
      ><NSelect v-model:value="targets" multiple filterable :options="champions"
    /></NFormItem>
    <NFormItem :label="t('personalTools.exceptions')"
      ><NSelect
        v-model:value="exceptions"
        multiple
        filterable
        :options="champions.filter((c) => targets.includes(c.value))"
    /></NFormItem>
  </NForm>
  <NSpace
    ><NButton
      :loading="busy"
      :disabled="!source || !effectiveTargets.length"
      @click="run('preview')"
      >{{ t('personalTools.preview') }}</NButton
    ><NPopconfirm @positive-click="run('apply')"
      ><template #trigger
        ><NButton type="primary" :loading="busy" :disabled="!rows.length">{{
          t('personalTools.applyBatch')
        }}</NButton></template
      >{{ t('personalTools.confirmBatch', { count: rows.length }) }}</NPopconfirm
    ><NPopconfirm @positive-click="run('undo')"
      ><template #trigger
        ><NButton :loading="busy">{{ t('personalTools.undo') }}</NButton></template
      >{{ t('personalTools.confirmUndo') }}</NPopconfirm
    ></NSpace
  >
  <NDataTable
    v-if="rows.length"
    class="preview"
    :columns="columns"
    :data="rows"
    :row-key="(r) => r.id"
    size="small"
    :pagination="{ pageSize: 20 }"
  />
  <ItemBatchWorkspace />
</template>
<script setup lang="ts">
import ItemBatchWorkspace from './ItemBatchWorkspace.vue'
import { archiveErrorMessage } from './archive-error'
import { computed, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import {
  NAlert,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NPopconfirm,
  NSelect,
  NSpace,
  useMessage
} from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useAutoChampConfigStore } from '@renderer-shared/shards/auto-champ-config/store'
interface Row {
  id: string
  before: unknown
  after: unknown
}
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const config = useAutoChampConfigStore()
const message = useMessage()
const champions = ref<{ value: number; label: string }[]>([]),
  kind = ref<'runes' | 'spells'>('runes'),
  source = ref<string | null>(null),
  targets = ref<number[]>([]),
  exceptions = ref<number[]>([]),
  rows = ref<Row[]>([]),
  busy = ref(false),
  error = ref('')
const name = (id: string) => champions.value.find((c) => c.value === Number(id))?.label || id
const kindOptions = computed(() => [
  { label: t('personalTools.runes'), value: 'runes' },
  { label: t('personalTools.spells'), value: 'spells' }
])
const templates = computed(() =>
  Object.entries(
    kind.value === 'spells' ? config.settings.summonerSpells : config.settings.runesV2
  ).flatMap(([id, modes]) =>
    Object.entries(modes)
      .filter(([mode, r]) => r && (kind.value === 'runes' || mode === 'aram'))
      .map(([mode]) => ({ value: `${id}:${mode}`, label: `${name(id)} · ${mode}` }))
  )
)
const effectiveTargets = computed(() =>
  targets.value.filter((id) => !exceptions.value.includes(id))
)
const columns = computed(() => [
  { title: t('personalTools.champion'), key: 'id', render: (r: Row) => name(r.id) },
  {
    title: t('personalTools.beforeConfig'),
    key: 'before',
    render: (r: Row) => JSON.stringify(r.before)
  },
  {
    title: t('personalTools.afterConfig'),
    key: 'after',
    render: (r: Row) => JSON.stringify(r.after)
  }
])
watch(kind, () => {
  source.value = null
  rows.value = []
})
watch([source, targets, exceptions], () => (rows.value = []), { deep: true })
async function run(action: string) {
  busy.value = true
  error.value = ''
  try {
    const result = await ipc.call<{ rows?: Row[]; count?: number }>(
      'riftflow-archive-main',
      'batch',
      action,
      {
        kind: kind.value,
        source: source.value,
        targets: effectiveTargets.value,
        expected: rows.value
      }
    )
    if (action === 'preview') rows.value = result.rows || []
    else {
      rows.value = []
      message.success(t('personalTools.batchDone', { count: result.count }))
    }
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  } finally {
    busy.value = false
  }
}
onMounted(async () => {
  try {
    const all = await ipc.call<{ id: number; name: string }[]>('riftflow-archive-main', 'champions')
    champions.value = all.map((c) => ({ value: c.id, label: c.name }))
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
.preview {
  margin-top: 16px;
}
</style>
