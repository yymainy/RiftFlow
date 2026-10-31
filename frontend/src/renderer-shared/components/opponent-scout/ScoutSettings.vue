<template>
  <SettingsRow
    v-if="supported"
    :label="t('opponentScout.settings')"
    :label-description="t('opponentScout.settingsDescription')"
    :label-width="400"
  >
    <div class="flex flex-wrap items-center gap-3">
      <NSwitch :value="enabled" size="small" :loading="saving" @update:value="setEnabled" />
      <span>{{ t('opponentScout.autoShow') }}</span>
      <NButton size="small" @click="open">{{ t('opponentScout.open') }}</NButton>
      <span v-if="error" role="alert">{{ error }}</span>
    </div>
  </SettingsRow>
</template>
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NButton, NSwitch } from 'naive-ui'
import SettingsRow from '@renderer-shared/components/SettingsRow.vue'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const enabled = ref(false)
const saving = ref(false)
const supported = ref(false)
const error = ref('')
onMounted(async () => {
  try {
    enabled.value = await ipc.call('opponent-scout-main', 'control', 'getEnabled')
    supported.value = true
  } catch {
    /* The preserved Electron host does not provide this Tauri window. */
  }
})
ipc.onEventVue('opponent-scout-main', 'enabled-changed', (value: boolean) => {
  enabled.value = value
})
async function setEnabled(value: boolean) {
  saving.value = true
  try {
    await ipc.call('opponent-scout-main', 'control', 'setEnabled', value)
    enabled.value = value
    error.value = ''
  } catch {
    error.value = t('opponentScout.windowError')
  } finally {
    saving.value = false
  }
}
async function open() {
  try {
    await ipc.call('opponent-scout-main', 'control', 'show')
    error.value = ''
  } catch {
    error.value = t('opponentScout.windowError')
  }
}
</script>
