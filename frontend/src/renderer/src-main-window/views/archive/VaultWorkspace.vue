<template>
  <p class="hint">{{ t('personalTools.vaultHint') }}</p>
  <NAlert v-if="error" type="error" closable @close="error = ''">{{ error }}</NAlert>
  <NSpace class="toolbar"
    ><NInput
      v-model:value="label"
      :placeholder="t('personalTools.snapshotLabel')"
      maxlength="80"
    /><NButton type="primary" :loading="busy" @click="run('snapshot')">{{
      t('personalTools.snapshot')
    }}</NButton
    ><NButton @click="load">{{ t('archiveWorkspace.refresh') }}</NButton></NSpace
  >
  <NSelect
    v-model:value="snapshot"
    :options="
      snapshots.map((s) => ({
        value: s.id,
        label: `${new Date(s.at).toLocaleString()} · ${s.label}`
      }))
    "
    :placeholder="t('personalTools.chooseSnapshot')"
  />
  <NSpace class="toolbar"
    ><NButton :loading="busy" :disabled="!snapshot" @click="run('preview')">{{
      t('personalTools.diff')
    }}</NButton
    ><NPopconfirm @positive-click="run('restore')"
      ><template #trigger
        ><NButton type="warning" :loading="busy" :disabled="!keys.length">{{
          t('personalTools.restoreSelected')
        }}</NButton></template
      >{{ t('personalTools.confirmRestore') }}</NPopconfirm
    ><NPopconfirm @positive-click="run('delete')"
      ><template #trigger
        ><NButton :disabled="!snapshot" :loading="busy">{{
          t('personalTools.delete')
        }}</NButton></template
      >{{ t('personalTools.confirmDelete') }}</NPopconfirm
    ></NSpace
  >
  <NAlert v-if="previewed && !changes.length" type="success">{{
    t('personalTools.noChanges')
  }}</NAlert>
  <NDataTable
    v-if="changes.length"
    v-model:checked-row-keys="keys"
    :columns="columns"
    :data="changes"
    :row-key="(r) => r.key"
    :scroll-x="850"
    :pagination="{ pageSize: 20 }"
    size="small"
  />
</template>
<script setup lang="ts">
import { archiveErrorMessage } from './archive-error'
import { computed, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import {
  NAlert,
  NButton,
  NDataTable,
  NInput,
  NPopconfirm,
  NSelect,
  NSpace,
  useMessage,
  type DataTableColumns
} from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
interface Snapshot {
  id: string
  at: number
  label: string
}
interface Change {
  key: string
  file: string
  path: string
  before: unknown
  after: unknown
}
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const message = useMessage()
const snapshots = ref<Snapshot[]>([]),
  snapshot = ref<string | null>(null),
  label = ref(''),
  busy = ref(false),
  error = ref(''),
  changes = ref<Change[]>([]),
  keys = ref<(string | number)[]>([]),
  expected = ref<unknown>(null),
  previewed = ref(false)
const columns = computed<DataTableColumns<Change>>(() => [
  { type: 'selection' },
  { title: t('personalTools.file'), key: 'file', width: 180 },
  { title: t('personalTools.field'), key: 'path' },
  {
    title: t('personalTools.currentValue'),
    key: 'before',
    render: (r) => JSON.stringify(r.before)
  },
  { title: t('personalTools.snapshotValue'), key: 'after', render: (r) => JSON.stringify(r.after) }
])
async function load() {
  try {
    snapshots.value = await ipc.call('riftflow-archive-main', 'vault', 'list')
  } catch (e) {
    error.value = archiveErrorMessage(e, t('personalTools.operationFailed'))
  }
}
watch(snapshot, () => {
  changes.value = []
  keys.value = []
  expected.value = null
  previewed.value = false
})
async function run(action: string) {
  busy.value = true
  error.value = ''
  try {
    const result = await ipc.call<{ changes?: Change[]; expected?: unknown }>(
      'riftflow-archive-main',
      'vault',
      action,
      {
        id: snapshot.value,
        label: label.value || t('personalTools.snapshot'),
        keys: keys.value,
        expected: expected.value
      }
    )
    if (action === 'preview') {
      changes.value = result.changes || []
      expected.value = result.expected
      keys.value = []
      previewed.value = true
    } else {
      changes.value = []
      keys.value = []
      previewed.value = false
      message.success(t('personalTools.saved'))
      await load()
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
.hint {
  color: var(--la-text-secondary);
  line-height: 1.8;
}
.toolbar {
  margin: 16px 0;
}
</style>
