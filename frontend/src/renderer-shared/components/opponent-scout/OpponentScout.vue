<template>
  <div class="scout">
    <SetupInAppScope />
    <div class="scout-header" @mousedown.left="drag">
      <div class="scout-title">{{ t('opponentScout.title') }}</div>
      <NButton size="tiny" quaternary @mousedown.stop @click="toggleCollapsed">
        {{ t(collapsed ? 'opponentScout.expand' : 'opponentScout.collapse') }}
      </NButton>
      <NButton size="tiny" quaternary @mousedown.stop @click="dismiss">
        {{ t('opponentScout.close') }}
      </NButton>
    </div>
    <template v-if="!collapsed">
      <div class="scout-caption">
        {{ t('opponentScout.caption') }}
        <div v-if="active">
          {{
            t('opponentScout.rosterStatus', {
              total: players.length,
              identified: players.filter((player) => player.champion > 0).length
            })
          }}
        </div>
      </div>
      <div v-if="isAram && aramSummary && ongoing.settings.enabled && active" class="scout-aram">
        <div class="scout-aram-title">{{ t('opponentScout.aramStrength.title') }}</div>
        <div class="scout-aram-row">
          <span class="scout-aram-side">{{ t('opponentScout.aramStrength.ours') }}</span>
          <div>
            <div>{{ aramTeamText(aramSummary.own) }}</div>
            <div class="scout-muted">
              {{ aramCoverageText(aramSummary.own) }}
              <template v-if="aramSummary.own.stack > 1">
                · {{ t('opponentScout.aramStrength.stack', { size: aramSummary.own.stack }) }}
              </template>
            </div>
          </div>
        </div>
        <div class="scout-aram-row">
          <span class="scout-aram-side">{{ t('opponentScout.aramStrength.enemy') }}</span>
          <div>
            <div>{{ aramTeamText(aramSummary.enemy) }}</div>
            <div class="scout-muted">
              {{ aramCoverageText(aramSummary.enemy) }}
              <template v-if="aramSummary.enemy.stack > 1">
                · {{ t('opponentScout.aramStrength.stack', { size: aramSummary.enemy.stack }) }}
              </template>
            </div>
          </div>
        </div>
        <div
          class="scout-aram-verdict"
          :class="{ 'scout-aram-verdict--even': aramSummary.verdict === 'even' }"
        >
          {{ t(`opponentScout.aramStrength.verdict.${aramSummary.verdict}`) }}
        </div>
        <div class="scout-muted">{{ t('opponentScout.aramStrength.caution') }}</div>
      </div>
      <NAlert v-if="error" type="error" :show-icon="false">{{ error }}</NAlert>
      <div v-if="!ongoing.settings.enabled" class="scout-empty">
        {{ t('opponentScout.disabled') }}
      </div>
      <div v-else-if="!active || !players.length" class="scout-empty">
        {{ t('opponentScout.waiting') }}
      </div>
      <NScrollbar v-else class="scout-list">
        <div v-for="player in players" :key="player.id" class="scout-player">
          <div class="scout-player-header">
            <LcuImage
              class="scout-icon"
              :src="league.gameData.champions[player.champion]?.squarePortraitPath"
              :alt="
                player.champion > 0
                  ? league.gameData.championName(player.champion)
                  : t('opponentScout.waitingChampion')
              "
            />
            <div class="scout-identity">
              <div class="scout-name">
                {{
                  player.champion > 0
                    ? league.gameData.championName(player.champion)
                    : t('opponentScout.waitingChampion')
                }}<span v-if="!isAram && player.position">
                  · {{ t(`opponentScout.positions.${player.position}`) }}</span
                >
              </div>
              <div class="scout-muted scout-name">{{ player.name }}</div>
            </div>
            <div class="scout-rank">
              {{ isAram ? (player.loaded ? aramPlayerText(player.aram) : '…') : player.rank }}
            </div>
          </div>
          <div v-if="player.loading === 'loading'" class="scout-muted">
            {{ t('opponentScout.loading') }}
          </div>
          <div v-else-if="player.loading === 'error'" class="scout-retry">
            <span>{{ t('opponentScout.failed') }}</span>
            <NButton size="tiny" @click="reload(player.id)">{{ t('opponentScout.retry') }}</NButton>
          </div>
          <template v-else-if="player.summary.count">
            <div class="scout-stats">
              <span>{{ t('opponentScout.record', player.summary) }}</span>
              <span v-if="!isAram">KDA {{ player.summary.kda }}</span>
            </div>
            <div v-if="!isAram" class="scout-muted">
              {{ t('opponentScout.championRecord', player.summary) }}
            </div>
            <div class="scout-insights">
              <template v-if="!isAram">
                <div v-if="player.insights.offRole" class="scout-signal">
                  {{ t('opponentScout.offRole') }}
                </div>
                <div v-if="player.insights.main" class="scout-muted">
                  {{
                    t('opponentScout.mainPosition', {
                      position: t(`opponentScout.positions.${player.insights.main}`),
                      share: player.insights.share,
                      known: player.insights.known
                    })
                  }}
                </div>
                <div v-else class="scout-muted">{{ t('opponentScout.unknownPosition') }}</div>
                <div v-if="player.position" class="scout-muted">
                  {{
                    t('opponentScout.positionRecord', {
                      position: t(`opponentScout.positions.${player.position}`),
                      count: player.insights.roleCount,
                      wins: player.insights.roleWins,
                      losses: player.insights.roleLosses
                    })
                  }}
                </div>
              </template>
              <div
                v-for="partner in player.insights.partners"
                :key="partner.id"
                class="scout-partner"
              >
                <div>
                  {{
                    t('opponentScout.together', {
                      ...partner,
                      champion: league.gameData.championName(
                        ongoing.championSelections[partner.id] ?? 0
                      )
                    })
                  }}
                </div>
                <div>
                  {{
                    t('opponentScout.apart', {
                      count: partner.apartCount,
                      wins: partner.apartWins,
                      losses: partner.apartLosses
                    })
                  }}
                </div>
                <div v-if="partner.unknown">
                  {{ t('opponentScout.unknownRoster', { count: partner.unknown }) }}
                </div>
              </div>
              <div v-if="player.insights.partners.length" class="scout-muted">
                {{ t('opponentScout.partnerCaution') }}
              </div>
              <div v-else-if="!isAram" class="scout-muted">
                {{ t('opponentScout.noPartner') }}
              </div>
            </div>
            <div v-if="player.signals.status !== 'none'" class="scout-signal">
              {{ t(`ongoingGame.accountSignals.${player.signals.status}`) }}
              <span v-if="player.signals.status !== 'insufficient'">
                · {{ t('opponentScout.signalSample', { count: player.signals.count }) }}</span
              >
            </div>
            <div
              v-for="item in player.signals.evidence.slice(0, 2)"
              :key="item.key"
              class="scout-muted"
            >
              {{ t(`ongoingGame.accountSignals.${item.key}`, item.values) }}
            </div>
          </template>
          <div v-else class="scout-muted">
            {{ t(player.loaded ? 'opponentScout.empty' : 'opponentScout.loading') }}
          </div>
        </div>
      </NScrollbar>
      <div class="scout-footer">
        {{ t(isAram ? 'opponentScout.aramStrength.caution' : 'opponentScout.caution') }}
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NAlert, NButton, NScrollbar } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { OngoingGameRenderer } from '@renderer-shared/shards/ongoing-game'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { SetupInAppScope } from '@renderer-shared/shards/setup-in-app-scope/setup-in-app-scope-component'
import type { LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'
import LcuImage from '../LcuImage.vue'
import { analyzeAccount } from '../ongoing-game-panel/widgets/player-info-card/account-signals/analyze'
import {
  aramSample,
  aggregateAramTeam,
  aramVerdict,
  type AramSample,
  type AramTeamSummary
} from './aram'
import {
  opponentIds,
  summarize,
  scoutInsights,
  readScoutMatch,
  normalizePosition,
  type ScoutMatch
} from './summary'

const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const renderer = useInstance(OngoingGameRenderer)
const ongoing = useOngoingGameStore()
const league = useLeagueClientStore()
const app = useAppCommonStore()
const collapsed = ref(false)
const enabled = ref(false)
const error = ref('')
let shownGame = ''
let dismissedGame = ''

async function control(command: string, ...args: unknown[]) {
  try {
    const result = await ipc.call('opponent-scout-main', 'control', command, ...args)
    error.value = ''
    return result
  } catch {
    error.value = t('opponentScout.windowError')
    return undefined
  }
}
const active = computed(
  () =>
    league.isConnected &&
    ongoing.queryStage.phase === 'in-game' &&
    ['GameStart', 'InProgress', 'Reconnect'].includes(league.gameflow.phase ?? '') &&
    ongoing.queryStage.gameInfo?.gameId === league.gameflow.session?.gameData.gameId
)
const isAram = computed(() => (ongoing.queryStage.gameInfo?.queueId ?? -1) === 450)
const analysisNow = Date.now()
const gameKey = computed(() =>
  active.value
    ? `${league.auth?.region}:${league.auth?.rsoPlatformId}:${league.summoner.me?.puuid}:${ongoing.queryStage.phase === 'in-game' ? ongoing.queryStage.gameInfo.gameId : 0}`
    : ''
)
const relatedGames = computed(() => {
  const byId = new Map<number, LcuOrSgpGameSummary[]>()
  const games = [
    ...Object.values(ongoing.cachedGames),
    ...Object.values(ongoing.matchHistory).flatMap((history) => history.data)
  ]
  for (const game of games) {
    const entries = byId.get(game.gameId) ?? []
    entries.push(game)
    byId.set(game.gameId, entries)
  }
  return byId
})
const allTeamPuuids = computed(() => [...new Set(Object.values(ongoing.teams).flat())])
const scoutMatchesById = computed(() => {
  const byPlayer = new Map<string, ScoutMatch[]>()
  for (const id of allTeamPuuids.value) {
    const matches: ScoutMatch[] = []
    for (const game of ongoing.matchHistory[id]?.data ?? []) {
      try {
        const match = readScoutMatch(game, id, relatedGames.value.get(game.gameId))
        if (match) matches.push(match)
      } catch {
        /* Incomplete matches are not evidence. */
      }
    }
    byPlayer.set(id, matches)
  }
  return byPlayer
})
const players = computed(() => {
  if (!active.value) return []
  const queue = ongoing.queryStage.gameInfo?.queueId ?? -1
  return opponentIds(ongoing.teams, league.summoner.me?.puuid).map((id) => {
    const champion = ongoing.championSelections[id] ?? 0
    const history = ongoing.matchHistory[id]
    const matches = scoutMatchesById.value.get(id) ?? []
    const position =
      normalizePosition(ongoing.positionAssignments[id]?.position) ??
      normalizePosition(ongoing.positionAssignments[id]?.role?.current)
    const candidates =
      Object.values(ongoing.teams)
        .find((team) => team.includes(id))
        ?.filter((member) => member !== id) ?? []
    const summoner = ongoing.summoner[id]
    const rank =
      ongoing.rankedStats[id]?.queueMap[queue === 440 ? 'RANKED_FLEX_SR' : 'RANKED_SOLO_5x5']
    return {
      id,
      champion,
      position,
      insights: scoutInsights(matches, position, queue, candidates),
      name: app.settings.streamerMode
        ? t('opponentScout.hiddenName')
        : summoner?.gameName
          ? `${summoner.gameName}#${summoner.tagLine}`
          : summoner?.displayName || t('opponentScout.unknownName'),
      rank:
        rank && !['NONE', 'NA', 'UNRANKED', ''].includes(rank.tier)
          ? `${t(`shortTiers.${rank.tier}`, { ns: 'common' })} ${rank.division === 'NA' ? '' : rank.division} ${rank.leaguePoints} LP`
          : t('opponentScout.noRank'),
      loaded: !!history,
      loading: ongoing.matchHistoryLoadingState[id],
      summary: summarize(matches, champion, queue),
      aram: aramSample(matches, analysisNow),
      signals: analyzeAccount(
        matches,
        summoner?.summonerLevel,
        analysisNow,
        queue,
        isAram.value ? 'aram' : 'sr'
      )
    }
  })
})
const aramSummary = computed(() => {
  if (!isAram.value) return null
  const teams = Object.entries(ongoing.teams)
  if (teams.length < 2) return null
  const self = league.summoner.me?.puuid
  const ownTeam = teams.find(([, ids]) => self && ids.includes(self))?.[0]
  if (!ownTeam) return null
  const summaries = teams.map(([team, ids]) => {
    const players = ids.map((id) => {
      const sample = aramSample(scoutMatchesById.value.get(id) ?? [], analysisNow)
      const group = ongoing.mergedPremadeTeamMap[id]
      const stackSize = group
        ? ids.filter((member) => ongoing.mergedPremadeTeamMap[member] === group).length
        : 1
      return {
        id,
        games: sample.count,
        winRate: sample.winRate,
        kda: sample.kda,
        level: ongoing.summoner[id]?.summonerLevel,
        stackSize
      }
    })
    return aggregateAramTeam(team, players)
  })
  const own = summaries.find((summary) => summary.team === ownTeam)
  const enemy = summaries.find((summary) => summary.team !== ownTeam)
  if (!own || !enemy) return null
  return { own, enemy, verdict: aramVerdict(own, enemy) }
})
function aramTeamText(team: AramTeamSummary) {
  if (team.winRate == null || team.kda == null) return t('opponentScout.aramStrength.insufficient')
  return t('opponentScout.aramStrength.line', {
    count: team.games,
    winRate: Math.round(team.winRate * 100),
    kda: team.kda.toFixed(1),
    level: team.avgLevel == null ? '—' : Math.round(team.avgLevel)
  })
}
function aramCoverageText(team: AramTeamSummary) {
  return t('opponentScout.aramStrength.coverage', { sampled: team.sampled, total: team.total })
}
function aramPlayerText(sample: AramSample) {
  if (!sample.count) return t('opponentScout.aramPlayer.empty')
  return t('opponentScout.aramPlayer.line', {
    count: sample.count,
    winRate: Math.round(sample.winRate * 100),
    kda: sample.kda.toFixed(1)
  })
}

onMounted(async () => {
  enabled.value = (await control('getEnabled')) === true
})
ipc.onEventVue('opponent-scout-main', 'enabled-changed', (value: boolean) => {
  enabled.value = value
  if (value) shownGame = ''
})
watch(
  [gameKey, () => players.value.length, enabled, () => ongoing.settings.enabled],
  async ([key, count]) => {
    if (!key) {
      await control('hide')
      return
    }
    if (!enabled.value || !ongoing.settings.enabled) {
      await control('hide')
      return
    }
    if (!count || key === shownGame || key === dismissedGame) return
    shownGame = key
    collapsed.value = false
    await control('expand')
    // Re-check after asynchronous native commands so an ended game never reopens.
    if (gameKey.value === key && enabled.value && ongoing.settings.enabled && dismissedGame !== key)
      await control('show')
  },
  { immediate: true }
)

function reload(id: string) {
  renderer.reloadPlayer(id, { includes: ['matchHistory', 'rankedStats', 'summoner'] })
}
function drag() {
  void control('drag')
}
async function toggleCollapsed() {
  const next = !collapsed.value
  if ((await control(next ? 'collapse' : 'expand')) !== undefined) collapsed.value = next
}
function dismiss() {
  dismissedGame = gameKey.value
  void control('hide')
}
</script>

<style scoped>
.scout {
  height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  background: var(--la-color-bg-primary);
  color: var(--la-color-text-primary);
  border: 1px solid var(--la-border);
}
.scout-header {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 46px;
  flex-shrink: 0;
  padding: 0 12px;
  cursor: move;
  border-bottom: 1px solid var(--la-border);
}
.scout-title {
  flex: 1;
  font-weight: 700;
}
.scout-caption,
.scout-footer {
  padding: 10px 14px;
  font-size: 11px;
  color: var(--la-text-secondary);
  line-height: 1.5;
}
.scout-footer {
  margin-top: auto;
}
.scout-list {
  flex: 1;
  min-height: 0;
}
.scout-player {
  margin: 0 12px 8px;
  padding: 10px;
  border: 1px solid var(--la-border);
  border-radius: 8px;
  font-size: 12px;
  line-height: 1.6;
}
.scout-aram {
  margin: 0 12px 8px;
  padding: 10px;
  border: 1px solid var(--la-border);
  border-radius: 8px;
  font-size: 11px;
  line-height: 1.6;
}
.scout-aram-title {
  margin-bottom: 4px;
  font-weight: 700;
}
.scout-aram-row {
  display: flex;
  gap: 8px;
}
.scout-aram-row + .scout-aram-row {
  margin-top: 4px;
}
.scout-aram-side {
  min-width: 28px;
  font-weight: 600;
}
.scout-aram-verdict {
  margin-top: 4px;
  font-weight: 700;
  color: var(--la-color-warning, #d99a38);
}
.scout-aram-verdict--even {
  color: var(--la-text-secondary);
  font-weight: 600;
}
.scout-player-header,
.scout-stats,
.scout-retry {
  display: flex;
  align-items: center;
  gap: 8px;
}
.scout-player-header {
  margin-bottom: 6px;
}
.scout-icon {
  height: 32px;
  width: 32px;
  border-radius: 6px;
}
.scout-identity {
  flex: 1;
  min-width: 0;
}
.scout-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.scout-rank {
  font-size: 11px;
}
.scout-muted {
  color: var(--la-text-secondary);
  font-size: 11px;
}
.scout-stats {
  justify-content: space-between;
  font-weight: 600;
}
.scout-signal {
  color: var(--la-color-warning, #d99a38);
  font-size: 11px;
}
.scout-insights {
  margin-top: 6px;
}
.scout-partner {
  margin-top: 5px;
  font-size: 11px;
  color: var(--la-text-secondary);
}
.scout-empty {
  padding: 24px 16px;
  color: var(--la-text-secondary);
}
</style>
