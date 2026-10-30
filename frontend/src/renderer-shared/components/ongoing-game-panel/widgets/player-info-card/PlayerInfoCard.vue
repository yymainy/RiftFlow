<template>
  <div
    :class="[
      'player-info-card relative box-border flex flex-col overflow-hidden rounded-lg border p-2 transition-colors'
    ]"
    :style="{
      width: FIXED_CARD_WIDTH_PX_LITERAL,
      borderColor: premadeTeamId ? premadeColors[premadeTeamId]?.borderColor : undefined
    }"
  >
    <!-- premade deco -->
    <div
      class="absolute top-0 right-0 z-0 h-4 w-4 translate-x-1/2 -translate-y-1/2 rotate-45"
      :style="{
        backgroundColor: premadeTeamId ? premadeColors[premadeTeamId]?.foregroundColor : undefined
      }"
    />

    <PlayerInfoCardHeader :puuid="puuid" />
    <PlayerInfoCardStats :puuid="puuid" />
    <AccountSignals :puuid="puuid" />
    <PlayerInfoCardJunglePathing :puuid="puuid" />

    <PlayerCardTagsArea :puuid="puuid" />
    <PlayerInfoCardChampionUsage :puuid="puuid" />
    <PlayerInfoCardMatchHistory :puuid="puuid" />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import AccountSignals from './account-signals/AccountSignals.vue'

import { useGameResourceProvider } from '../../../../providers/game-resource'
import {
  FIXED_CARD_WIDTH_PX_LITERAL,
  PREMADE_TEAM_COLORS,
  PREMADE_TEAM_COLORS_LIGHT
} from '../../constants'
import { useOngoingGamePanel } from '../../context'
import PlayerCardTagsArea from './player-card-tags/TagsArea.vue'
import PlayerInfoCardChampionUsage from './PlayerInfoCardChampionUsage.vue'
import PlayerInfoCardHeader from './PlayerInfoCardHeader.vue'
import PlayerInfoCardJunglePathing from './PlayerInfoCardJunglePathing.vue'
import PlayerInfoCardMatchHistory from './PlayerInfoCardMatchHistory.vue'
import PlayerInfoCardStats from './PlayerInfoCardStats.vue'

const { puuid } = defineProps<{
  puuid: string
}>()

const { mergedPremadeTeams } = useOngoingGamePanel()
const resources = useGameResourceProvider()

const premadeTeamId = computed(() => mergedPremadeTeams.value.premadeTeamIdMap[puuid])

const premadeColors = computed(() => {
  return resources.runtime.colorMode === 'dark' ? PREMADE_TEAM_COLORS : PREMADE_TEAM_COLORS_LIGHT
})
</script>

<style scoped>
.player-info-card {
  border-color: rgb(var(--la-card-border-rgb) / 0.12);
  background: var(--la-card-surface-95);
  color: var(--la-color-text-primary);
  font-variant-numeric: tabular-nums;
}
.player-info-card:hover {
  background: var(--la-card-surface-90);
}
</style>
