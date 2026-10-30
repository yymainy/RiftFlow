<template>
  <div class="item-batch">
    <h3>{{ t('personalTools.itemBatch') }}</h3>
    <p class="hint">{{ t('personalTools.itemBatchHint') }}</p>
    <NAlert v-if="error" type="error">{{ error }}</NAlert>
    <NAlert v-if="hint" type="info">{{ hint }}</NAlert>
    <NForm label-placement="top">
      <NFormItem :label="t('personalTools.template')">
        <NSelect
          v-model:value="source"
          :options="templates"
          filterable
          :placeholder="t('personalTools.itemTemplate')"
        />
      </NFormItem>
      <NFormItem :label="t('personalTools.targets')">
        <NSelect v-model:value="targets" multiple filterable :options="champions" />
      </NFormItem>
      <NFormItem :label="t('personalTools.exceptions')">
        <NSelect
          v-model:value="exceptions"
          multiple
          filterable
          :options="champions.filter((c) => targets.includes(c.value))"
        />
      </NFormItem>
    </NForm>
    <NSpace>
      <NButton :loading="busy" @click="load">{{ t('archiveWorkspace.refresh') }}</NButton>
      <NButton
        :loading="busy"
        :disabled="!source || !effectiveTargets.length"
        @click="run('preview')"
        >{{ t('personalTools.preview') }}</NButton
      >
      <NPopconfirm @positive-click="run('apply')">
        <template #trigger
          ><NButton type="primary" :loading="busy" :disabled="!rows.length">{{
            t('personalTools.applyBatch')
          }}</NButton></template
        >
        {{ t('personalTools.confirmItemBatch', { count: rows.length }) }}
      </NPopconfirm>
      <NPopconfirm @positive-click="run('undo')">
        <template #trigger
          ><NButton :loading="busy">{{ t('personalTools.undo') }}</NButton></template
        >
        {{ t('personalTools.confirmItemUndo') }}
      </NPopconfirm>
    </NSpace>
    <NDataTable
      v-if="rows.length"
      class="preview"
      :columns="columns"
      :data="rows"
      :row-key="(r) => r.id"
      size="small"
      :pagination="{ pageSize: 20 }"
    />
  </div>
</template>
<script setup lang="ts">
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
import { archiveErrorMessage } from './archive-error'

type Option<T> = { value: T; label: string }
type Row = { id: number; title: string; blocks: number }
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const message = useMessage()
const templates = ref<Option<string>[]>([])
const champions = ref<Option<number>[]>([])
const source = ref<string | null>(null)
const targets = ref<number[]>([])
const exceptions = ref<number[]>([])
const rows = ref<Row[]>([])
const busy = ref(false)
const error = ref('')
const hint = ref('')
const effectiveTargets = computed(() =>
  targets.value.filter((id) => !exceptions.value.includes(id))
)
const championName = (id: number) =>
  champions.value.find((c) => c.value === id)?.label || String(id)
const columns = computed(() => [
  { title: t('personalTools.champion'), key: 'id', render: (row: Row) => championName(row.id) },
  { title: t('personalTools.template'), key: 'title' },
  { title: t('personalTools.itemBlocks'), key: 'blocks' }
])
watch(
  [source, targets, exceptions],
  () => {
    rows.value = []
  },
  { deep: true }
)

async function load() {
  busy.value = true
  error.value = ''
  hint.value = ''
  try {
    const state = await ipc.call<{ loggedIn: boolean; paused: boolean }>(
      'riftflow-archive-main',
      'diagnostics'
    )
    if (!state.loggedIn || state.paused) {
      templates.value = []
      source.value = null
      rows.value = []
      hint.value = t(state.paused ? 'personalTools.itemPaused' : 'personalTools.itemConnect')
      return
    }
    const [list, all] = await Promise.all([
      ipc.call<{ templates: Option<string>[] }>('riftflow-archive-main', 'itemBatch', 'list'),
      ipc.call<{ id: number; name: string }[]>('riftflow-archive-main', 'champions')
    ])
    templates.value = list.templates
    if (!templates.value.length) hint.value = t('personalTools.itemNoTemplates')
    champions.value = all.map((c) => ({ value: c.id, label: c.name }))
    if (source.value && !templates.value.some((t) => t.value === source.value)) source.value = null
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.itemReadFailed'))
  } finally {
    busy.value = false
  }
}

async function run(action: 'preview' | 'apply' | 'undo') {
  busy.value = true
  error.value = ''
  try {
    const result = await ipc.call<{ rows?: Row[]; count?: number }>(
      'riftflow-archive-main',
      'itemBatch',
      action,
      {
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
onMounted(load)
</script>
<style scoped>
.item-batch {
  margin-top: 28px;
  border-top: 1px solid var(--la-border);
  padding-top: 20px;
}
.hint {
  color: var(--la-text-secondary);
  line-height: 1.8;
}
.preview {
  margin-top: 16px;
}
</style>
