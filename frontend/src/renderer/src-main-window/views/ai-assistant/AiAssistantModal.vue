<template>
  <NModal
    :show="show"
    preset="card"
    :title="t('aiAssistant.title')"
    display-directive="show"
    :style="{ width: '960px', maxWidth: '94vw', height: '86vh' }"
    :content-style="{ padding: 0, minHeight: '0', overflow: 'hidden', flex: '1' }"
    :class="'ai-assistant-modal'"
    @update:show="emit('update:show', $event)"
  >
    <AiAssistant :active="show" />
  </NModal>
</template>
<script setup lang="ts">
import { NModal } from 'naive-ui'
import { useTranslation } from 'i18next-vue'
import AiAssistant from './AiAssistant.vue'
defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()
const { t } = useTranslation()
</script>
<style scoped>
.ai-assistant-modal {
  display: flex;
  flex-direction: column;
}
</style>
