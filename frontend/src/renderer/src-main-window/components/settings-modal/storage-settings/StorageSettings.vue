<template>
  <NTabs size="small" class="settings-storage-tabs h-full" type="line" animated>
    <NTabPane
      :tab="t('settings.storage.tabs.tagged-players')"
      name="tagged-players"
      display-directive="show"
    >
      <TaggedPlayers />
    </NTabPane>
    <NTabPane :tab="t('settings.storage.tabs.settings')" name="settings" display-directive="show">
      <SavedSettings />
    </NTabPane>
  </NTabs>
</template>

<script setup lang="ts">
import { useTranslation } from 'i18next-vue'
import { NTabPane, NTabs } from 'naive-ui'

import SavedSettings from './SavedSettings.vue'
import TaggedPlayers from './TaggedPlayers.vue'

const { t } = useTranslation()
</script>
