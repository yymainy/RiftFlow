<template>
  <div class="flex items-start gap-3">
    <NSwitch :value="enabled" :loading="saving" :disabled="!supported" @update:value="setEnabled" />
    <div>
      <div>{{ t('aiAssistant.autoAdvice') }}</div>
      <div class="mt-1 text-xs opacity-60">{{ t('aiAssistant.autoHint') }}</div>
      <div v-if="error" class="mt-1 text-xs" role="alert">{{ error }}</div>
    </div>
  </div>
</template>
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NSwitch } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const enabled = ref(false),
  saving = ref(false),
  supported = ref(false),
  error = ref('')
onMounted(async () => {
  try {
    enabled.value = await ipc.call('ai-advice-main', 'control', 'getAuto')
    supported.value = true
  } catch {}
})
ipc.onEventVue('ai-advice-main', 'auto-advice-changed', (value: boolean) => (enabled.value = value))
async function setEnabled(value: boolean) {
  saving.value = true
  try {
    await ipc.call('ai-advice-main', 'control', 'setAuto', value)
    enabled.value = value
    error.value = ''
  } catch {
    error.value = t('aiAssistant.saveError')
  } finally {
    saving.value = false
  }
}
</script>
