<template>
  <span class="riftflow-name" :class="{ 'riftflow-name--bold': bold }">{{
    text ?? t('appName', { ns: 'common' })
  }}</span>
</template>

<script setup lang="ts">
import { useTranslation } from 'i18next-vue'

defineProps<{
  bold?: boolean
  text?: string
}>()

const { t } = useTranslation()
</script>

<style scoped>
.riftflow-name {
  color: var(--la-color-text-primary);
}
.riftflow-name--bold {
  font-weight: 600;
}
</style>
