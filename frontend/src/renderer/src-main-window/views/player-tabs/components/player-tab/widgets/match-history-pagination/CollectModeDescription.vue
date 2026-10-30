<template>
  <TranslationComponent
    :translation="t('playerTabs.matchHistory.collectMode.collectedPageDescription')"
  >
    <template #scanned>
      <TranslationComponent
        :translation="
          t('playerTabs.matchHistory.collectMode.scannedMatches', {
            count: scannedCount
          })
        "
      >
        <template #count>
          <span class="collect-mode-count">{{ scannedCount }}</span>
        </template>
      </TranslationComponent>
    </template>
    <template #collected>
      <TranslationComponent
        :translation="
          t('playerTabs.matchHistory.collectMode.collectedMatches', {
            count: collectedGamesCount
          })
        "
      >
        <template #count>
          <span class="collect-mode-count">{{ collectedGamesCount }}</span>
        </template>
      </TranslationComponent>
    </template>
  </TranslationComponent>
</template>

<script setup lang="ts">
import { TranslationComponent, useTranslation } from 'i18next-vue'

defineProps<{
  scannedCount: number
  collectedGamesCount: number
}>()

const { t } = useTranslation()
</script>

<style scoped>
@reference '@renderer-shared/assets/css/tailwind.css';

.collect-mode-count {
  @apply text-riftflow-700 dark:text-riftflow-300 font-bold;
}
</style>
