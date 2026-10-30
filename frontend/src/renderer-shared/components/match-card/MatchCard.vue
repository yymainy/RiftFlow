<template>
  <div class="relative w-full min-w-175" :class="{ 'workbench-match-row': tableLayout }">
    <MatchCardOverview @toggle-expand="isExpanded = !isExpanded" />

    <NDrawer
      v-if="tableLayout"
      v-model:show="isExpanded"
      width="min(1080px, calc(100vw - 40px))"
      placement="right"
    >
      <NDrawerContent closable :title="t('workbenchTable.details')"
        ><MatchCardDetails
      /></NDrawerContent>
    </NDrawer>
    <KeepAlive v-else>
      <MatchCardDetails v-if="!puuid || isExpanded" />
    </KeepAlive>
  </div>
</template>

<script lang="ts" setup>
import { LcuOrSgpGameDetails, LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'
import { ReplayDownloadProgress } from '@shared/types/league-client/replays'
import { DraftOptions } from '@shared/shards/ongoing-game'
import { onErrorCaptured } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NDrawer, NDrawerContent } from 'naive-ui'

import MatchCardDetails from './MatchCardDetails.vue'
import MatchCardOverview from './MatchCardOverview.vue'
import { provideMatchCard } from './context'

const { t } = useTranslation()

const {
  tableLayout = false,
  summary,
  puuid,
  details = null,
  hidePrivacy = false,
  loadingDetails = false,
  replayState = null,
  canDryRunOngoingGame = false
} = defineProps<{
  tableLayout?: boolean
  summary: LcuOrSgpGameSummary
  details?: LcuOrSgpGameDetails | null
  puuid?: string
  hidePrivacy?: boolean
  loadingDetails?: boolean
  replayState?: ReplayDownloadProgress
  canDryRunOngoingGame?: boolean
}>()

const emits = defineEmits<{
  loadDetails: [gameId: number]
  downloadReplay: [gameId: number]
  watchReplay: [gameId: number]
  navigateToSummonerByPuuid: [puuid: string, setCurrent?: boolean]
  dryRunOngoingGame: [draft: DraftOptions]
}>()

const isExpanded = defineModel<boolean>('isExpanded', {
  required: false,
  default: false
})

provideMatchCard({
  isExpanded: () => isExpanded.value,
  summary: () => summary,
  puuid: () => puuid,
  details: () => details,
  hidePrivacy: () => hidePrivacy,
  loadingDetails: () => loadingDetails,
  replayState: () => replayState,
  canDryRunOngoingGame: () => canDryRunOngoingGame,

  navigateToSummonerByPuuid: (puuid: string, setCurrent?: boolean) => {
    emits('navigateToSummonerByPuuid', puuid, setCurrent)
  },
  loadReplay: (gameId: number) => {
    emits('downloadReplay', gameId)
  },
  watchReplay: (gameId: number) => {
    emits('watchReplay', gameId)
  },
  loadDetails: (gameId: number) => {
    emits('loadDetails', gameId)
  },
  dryRunOngoingGame: (draft) => {
    emits('dryRunOngoingGame', draft)
  }
})

onErrorCaptured((error) => {
  console.error(error)
})

defineExpose({
  setExpanded: (expanded: boolean) => {
    isExpanded.value = expanded
  }
})
</script>
