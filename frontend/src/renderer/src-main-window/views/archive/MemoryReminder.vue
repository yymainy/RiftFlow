<template>
  <NAlert
    v-if="notes.length"
    type="info"
    class="memory-reminder"
    :title="t('personalTools.reminder')"
  >
    <div v-for="note in notes" :key="note.id" class="memory-line">
      <span>{{ note.action }}</span
      ><NButton text size="small" @click="open(note.matchKey)">{{
        t('personalTools.openMatch')
      }}</NButton>
    </div>
    <span class="hint">{{ t('personalTools.oldVersion') }}</span>
  </NAlert>
</template>
<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useTranslation } from 'i18next-vue'
import { NAlert, NButton } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import type { MemoryNote } from './PersonalWorkspace.vue'
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const league = useLeagueClientStore()
const router = useRouter()
const notes = ref<MemoryNote[]>([])
let generation = 0
watch(
  () =>
    [
      league.gameflow.phase,
      league.champSelect.currentChampion,
      league.summoner.me?.puuid,
      league.champSelect.session
    ] as const,
  async () => {
    const current = ++generation
    notes.value = []
    if (league.gameflow.phase !== 'ChampSelect' || !league.champSelect.currentChampion) return
    try {
      const data = await ipc.call<{ notes: MemoryNote[] }>(
        'riftflow-archive-main',
        'personal',
        'get'
      )
      if (current !== generation) return
      const session = league.champSelect.session
      const opponents = session?.theirTeam.map((p) => p.championId).filter(Boolean) || []
      const own = session?.myTeam.find((p) => p.cellId === session.localPlayerCellId)
      const role = own?.assignedPosition || ''
      const mode = league.gameflow.session?.gameData.queue.gameMode
      notes.value = data.notes
        .filter(
          (n) =>
            n.owner === league.summoner.me?.puuid &&
            n.region === `${league.auth?.region}:${league.auth?.rsoPlatformId}` &&
            n.championId === league.champSelect.currentChampion &&
            n.gameMode === mode &&
            !['outdated', 'ineffective'].includes(n.status) &&
            (n.role === 'unknown' || n.role === role) &&
            opponents.includes(n.opponent)
        )
        .slice(-3)
        .reverse()
    } catch {
      notes.value = []
    }
  },
  { immediate: true }
)
function open(key: string) {
  router.push({ name: 'archive', query: { match: key } })
}
</script>
<style scoped>
.memory-reminder {
  margin: 12px 20px 0;
}
.memory-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  white-space: pre-wrap;
}
.hint {
  font-size: 12px;
  color: var(--la-text-secondary);
}
</style>
