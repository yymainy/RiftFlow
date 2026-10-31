<template>
  <div class="absolute right-5 bottom-4 z-20 flex items-center gap-2">
    <NPopover v-if="relevant.length" trigger="click" placement="top-end" :width="320">
      <template #trigger
        ><NButton size="small">{{
          t('aiAssistant.noteReminder', { count: relevant.length })
        }}</NButton></template
      >
      <div class="max-h-80 overflow-auto">
        <div v-for="note in relevant" :key="note.id" class="mb-3">
          <div class="font-semibold">{{ note.title }}</div>
          <div class="text-xs opacity-60">
            {{
              note.championId
                ? league.gameData.championName(note.championId)
                : t('aiAssistant.generalNote')
            }}
          </div>
          <div class="mt-1 text-sm break-words whitespace-pre-wrap">{{ note.body }}</div>
        </div>
      </div>
    </NPopover>
    <NButton size="small" @click="openAiAssistant">{{ t('aiAssistant.title') }}</NButton>
  </div>
</template>
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NButton, NPopover } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import type { AiNote } from './context'
import { useMainWindowAppContext } from '@main-window/context'
const { t } = useTranslation()
const { openAiAssistant } = useMainWindowAppContext()
const ipc = useInstance(RiftFlowIpcRenderer)
const league = useLeagueClientStore()
const ongoing = useOngoingGameStore()
const notes = ref<AiNote[]>([])
const relevant = computed(() => {
  const champions = new Set(Object.values(ongoing.championSelections))
  return notes.value.filter((note) => !note.championId || champions.has(note.championId))
})
watch(
  () => league.gameflow.session?.gameData.gameId,
  async () => {
    try {
      notes.value = await ipc.call('ai-assistant-main', 'notes')
    } catch {
      notes.value = []
    }
  },
  { immediate: true }
)
</script>
