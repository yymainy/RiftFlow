<template>
  <NConfigProvider
    :theme-overrides="themeOverrides"
    :theme="naiveUiTheme"
    :locale="naiveUiLocale.locale"
    :date-locale="naiveUiLocale.dateLocale"
    abstract
    inline-theme-disabled
  >
    <NMessageProvider placement="bottom">
      <NNotificationProvider>
        <NDialogProvider>
          <GameResourceProvider :value="gameResourceProvider">
            <AiAdviceWindow v-if="isAiAdvice" />
            <OpponentScout v-else-if="isOpponentScout" />
            <App v-else />
          </GameResourceProvider>
        </NDialogProvider>
      </NNotificationProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>

<script setup lang="ts">
import { useColorThemeAttr } from '@renderer-shared/composables/useColorThemeAttr'
import {
  createRiftFlowGameResourceProvider,
  GameResourceProvider
} from '@renderer-shared/providers/game-resource'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import {
  getNaiveUiLocale,
  getNaiveUiTheme,
  getNaiveUiThemeOverrides
} from '@renderer-shared/theme/naive-ui'
import { NConfigProvider, NDialogProvider, NMessageProvider, NNotificationProvider } from 'naive-ui'
import { computed } from 'vue'

import App from './App.vue'
import OpponentScout from '@renderer-shared/components/opponent-scout/OpponentScout.vue'
import AiAdviceWindow from '@renderer-shared/components/ai-advice/AiAdviceWindow.vue'

const isOpponentScout = location.pathname.endsWith('/opponent-scout-window.html')
const isAiAdvice = location.pathname.endsWith('/ai-advice-window.html')

const as = useAppCommonStore()
const gameResourceProvider = createRiftFlowGameResourceProvider()

const themeOverrides = computed(() => {
  return getNaiveUiThemeOverrides(as.themeId)
})

const naiveUiLocale = computed(() => {
  return getNaiveUiLocale(as.settings.locale)
})

const naiveUiTheme = computed(() => {
  return getNaiveUiTheme(as.colorTheme)
})

useColorThemeAttr(
  () => as.colorTheme,
  () => as.themeId
)
</script>

<style></style>
