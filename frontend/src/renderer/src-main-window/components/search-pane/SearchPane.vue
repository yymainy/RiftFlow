<template>
  <div
    class="player-search-workspace flex h-full overflow-hidden rounded-lg border border-solid border-white/10 bg-neutral-100 dark:bg-neutral-900"
  >
    <SearchPaneSidebar ref="sidebarRef" @navigate-to-summoner="handleComponentNavigateToSummoner" />
    <SearchPaneSearchArea
      ref="searchAreaRef"
      @navigate-to-summoner="handleComponentNavigateToSummoner"
    />
  </div>
</template>

<script setup lang="ts">
import { onUnmounted, useTemplateRef } from 'vue'

import SearchPaneSearchArea from './SearchPaneSearchArea.vue'
import SearchPaneSidebar from './SearchPaneSidebar.vue'

const sidebarRef = useTemplateRef('sidebarRef')
const searchAreaRef = useTemplateRef('searchAreaRef')

const emits = defineEmits<{
  navigateToSummoner: [puuid: string, sgpServerId: string | null, setCurrent?: boolean]
}>()

const handleComponentNavigateToSummoner = (
  puuid: string,
  sgpServerId: string | null,
  setCurrent?: boolean
) => {
  emits('navigateToSummoner', puuid, sgpServerId, setCurrent)
}

const cancel = () => {
  searchAreaRef.value?.cancel()
}

const reset = () => {
  sidebarRef.value?.reset()
  searchAreaRef.value?.reset()
}

reset()

defineExpose({
  reset,
  cancel
})

onUnmounted(() => {
  cancel()
})
</script>

<style scoped>
.player-search-workspace {
  background: var(--la-surface);
  border: 1px solid var(--la-border);
  border-radius: 8px;
}
.player-search-workspace :deep(.text-base.font-bold) {
  font-size: 14px;
  font-weight: 600;
}
</style>
