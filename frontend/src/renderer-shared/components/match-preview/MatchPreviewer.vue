<template>
  <NModal
    v-model:show="show"
    :class="{
      'shadow-none!': shadowless
    }"
  >
    <div class="box-border w-226 max-w-[calc(100vw-32px)] p-4">
      <MatchCard
        v-if="summary"
        :summary="summary"
        :details="details"
        :puuid="puuid"
        :loading-details="loadingDetails"
        :hide-privacy="hidePrivacy"
        :can-dry-run-ongoing-game="canDryRunOngoingGame"
        is-expanded
        @navigate-to-summoner-by-puuid="emits('navigateToSummonerByPuuid', $event)"
        @dry-run-ongoing-game="emits('dryRunOngoingGame', $event)"
        @load-details="handleLoadDetails"
      />
      <div
        class="flex h-50 w-full items-center justify-center rounded-md border border-solid border-black/10 bg-(--la-card-surface-95) dark:border-white/10"
        v-else
      >
        <template v-if="loadingSummary">
          <div class="flex items-center gap-2">
            <NSpin :size="16" />
            <span>{{ $t('playerTabs.matchHistory.previewer.loading') }}</span>
          </div>
        </template>

        <template v-else>{{ $t('playerTabs.matchHistory.previewer.noData') }}</template>
      </div>
    </div>
  </NModal>
</template>

<script lang="ts" setup>
import type { LcuOrSgpGameDetails, LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'
import type { DraftOptions } from '@shared/shards/ongoing-game'
import { NModal, NSpin } from 'naive-ui'

import MatchCard from '../match-card/MatchCard.vue'

const {
  summary = null,
  details = null,
  puuid,
  hidePrivacy = false,
  canDryRunOngoingGame = false,
  loadingSummary = false,
  loadingDetails = false,
  shadowless = false
} = defineProps<{
  summary?: LcuOrSgpGameSummary | null
  details?: LcuOrSgpGameDetails | null
  puuid?: string
  hidePrivacy?: boolean
  canDryRunOngoingGame?: boolean
  loadingSummary?: boolean
  loadingDetails?: boolean
  shadowless?: boolean
}>()

const emits = defineEmits<{
  loadDetails: [summary: LcuOrSgpGameSummary]
  navigateToSummonerByPuuid: [puuid: string]
  dryRunOngoingGame: [draft: DraftOptions]
}>()

const show = defineModel<boolean>('show', { default: false })

const handleLoadDetails = () => {
  if (summary) {
    emits('loadDetails', summary)
  }
}
</script>
