<template>
  <NPopover v-if="opponent" trigger="click" placement="bottom" :width="320">
    <template #trigger>
      <NButton
        class="mb-1"
        size="tiny"
        quaternary
        :type="result.status === 'smurf' || result.status === 'change' ? 'warning' : 'default'"
      >
        {{ t(`ongoingGame.accountSignals.${loading ? 'loading' : result.status}`) }} ·
        {{ result.count }}
      </NButton>
    </template>
    <div class="flex flex-col gap-2 text-xs">
      <div class="font-bold">{{ t(`ongoingGame.accountSignals.${result.status}`) }}</div>
      <div v-if="ongoingGame.matchHistoryLoadingState[puuid] === 'error'">
        {{ t('ongoingGame.playerCard.errorLoadingMatchHistory') }}
      </div>
      <div>
        {{
          t('ongoingGame.accountSignals.sample', {
            count: result.count,
            queue:
              result.queue === 450
                ? t('ongoingGame.accountSignals.aram')
                : result.queue === 440
                  ? t('ongoingGame.accountSignals.flex')
                  : t('ongoingGame.accountSignals.solo')
          })
        }}
      </div>
      <div>{{ t('ongoingGame.accountSignals.excluded', { count: result.excluded }) }}</div>
      <div v-if="result.status === 'smurf' || result.status === 'change'">
        {{
          t('ongoingGame.accountSignals.strength', {
            value: t(`ongoingGame.accountSignals.${result.strength}`)
          })
        }}
      </div>
      <div v-for="item in result.evidence" :key="item.key">
        • {{ t(`ongoingGame.accountSignals.${item.key}`, item.values) }}
      </div>
      <div v-if="result.status === 'insufficient'">
        {{
          t(
            result.queue === 450
              ? 'ongoingGame.accountSignals.aramMinimum'
              : 'ongoingGame.accountSignals.minimum'
          )
        }}
      </div>
      <div v-if="result.gameIds.length" class="flex flex-wrap gap-1">
        <span>{{ t('ongoingGame.accountSignals.sources') }}</span>
        <NButton
          v-for="id in result.gameIds.slice(0, 5)"
          :key="id"
          text
          size="tiny"
          @click="openMatch(id)"
          >#{{ id }}</NButton
        >
      </div>
      <div>
        {{
          t(
            result.queue === 450
              ? 'ongoingGame.accountSignals.aramCaution'
              : 'ongoingGame.accountSignals.caution'
          )
        }}
      </div>
    </div>
  </NPopover>
</template>
<script setup lang="ts">
import { computed } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NButton, NPopover } from 'naive-ui'
import { toBasicInfo } from '@shared/data-adapter/match-history/match-basic'
import { toParticipants } from '@shared/data-adapter/match-history/participants'
import { useOngoingGamePanel } from '../../../context'
import { analyzeAccount, type SignalMatch } from './analyze'

const { puuid } = defineProps<{ puuid: string }>()
const { t } = useTranslation()
const { ongoingGame, previewGame } = useOngoingGamePanel()
function openMatch(id: number) {
  const summary = ongoingGame.value.matchHistory[puuid]?.data.find((game) => game.gameId === id)
  if (summary) previewGame({ summary, details: ongoingGame.value.gameDetails[id], puuid })
}
const opponent = computed(() => {
  const self = ongoingGame.value.selfPuuid
  const teams = Object.values(ongoingGame.value.teams)
  const own = teams.find((team) => self && team.includes(self))
  return !!own && !own.includes(puuid) && teams.some((team) => team.includes(puuid))
})
const loading = computed(() => ongoingGame.value.matchHistoryLoadingState[puuid] === 'loading')
const queue = computed(() => ongoingGame.value.queryStage.gameInfo?.queueId ?? -1)
const isAram = computed(() => queue.value === 450)
const result = computed(() => {
  const matches: SignalMatch[] = []
  const history = ongoingGame.value.matchHistory[puuid]?.data || []
  for (const game of history) {
    try {
      const basic = toBasicInfo(game)
      if (isAram.value) {
        if (basic.queueId !== 450 && basic.mapId !== 12) continue
      } else if (basic.gameMode !== 'CLASSIC' || basic.mapId !== 11) {
        continue
      }
      const player = toParticipants(game, basic).find((player) => player.puuid === puuid)
      if (!player || !['win', 'loss'].includes(player.winResult)) continue
      matches.push({
        id: basic.gameId,
        at: basic.gameCreation,
        queue: basic.queueId,
        duration: basic.gameDuration,
        won: player.win,
        kills: player.kills,
        deaths: player.deaths,
        assists: player.assists,
        champion: player.championId,
        position: player.position
      })
    } catch {
      /* Incomplete summaries never become evidence. */
    }
  }
  const report = analyzeAccount(
    matches,
    ongoingGame.value.summoner[puuid]?.summonerLevel,
    Date.now(),
    queue.value,
    isAram.value ? 'aram' : 'sr'
  )
  report.excluded = history.length - report.count
  return report
})
</script>
