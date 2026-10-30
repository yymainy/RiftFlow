<template>
  <div
    v-if="participant && team"
    class="match-overview transition-width @container relative box-border flex w-full overflow-hidden select-none"
    :data-result="winStyleType"
  >
    <!-- main content -->
    <div class="match-overview-content">
      <div class="match-overview-main">
        <div class="match-overview-meta">
          <!-- result -->
          <div class="match-overview-result">
            <div
              :class="{
                'text-blue-600 dark:text-blue-300': winStyleType === 'win',
                'text-red-700 dark:text-red-300': winStyleType === 'loss',
                'text-black/80 dark:text-white': winStyleType === 'neutral'
              }"
              class="text-sm leading-none font-bold"
            >
              {{
                gameResultName(
                  team.subteamPlacement,
                  team.winResult,
                  team.isSurrender,
                  resources.runtime.locale
                )
              }}
            </div>
          </div>

          <!-- info line -->
          <div class="match-overview-info">
            <!-- queue name -->
            <div class="text-xs text-black dark:text-white/80">
              {{ resources.queues.name(basicInfo.queueId) }}
            </div>
            <div class="mx-1 text-xs text-black/40 dark:text-white/40">·</div>

            <!-- duration hh:mm:ss (pad 0) -->
            <!-- advanced: from -> to -->
            <div class="text-xs text-black/80 dark:text-white/60">
              {{ formatSeconds(basicInfo.gameDuration) }}
            </div>
            <div class="mx-1 text-xs text-black/60 dark:text-white/40">·</div>

            <!-- should show the specific time if hover -->
            <div class="text-xs text-black/80 dark:text-white/60" :title="gameCreationTitle">
              {{ formattedRelativeTime }}
            </div>
            <div class="mx-1 text-xs text-black/60 dark:text-white/40">·</div>
            <div class="flex-1 truncate text-xs text-black/80 dark:text-white/60">
              {{ mapName }}
            </div>
          </div>
        </div>
        <!-- 上半部分：英雄头像 + stats line -->
        <div class="match-overview-performance">
          <!-- champion icon -->
          <div class="match-overview-portrait">
            <div class="relative" :class="{ contents: !shouldShowCrown && !participant.position }">
              <ChampionIcon
                :champion-id="participant.championId"
                class="relative box-border size-11 rounded-lg border border-solid"
                :class="{
                  'border-emerald-600/60 dark:border-emerald-300/40': winStyleType === 'win',
                  'border-red-600/50 dark:border-red-300/40': winStyleType === 'loss',
                  'border-black/80 dark:border-white/80': winStyleType === 'neutral'
                }"
              />

              <!-- top1 头顶上方的皇冠 -->
              <div
                v-if="shouldShowCrown"
                class="absolute top-0 left-[calc(50%-2px)] -translate-x-1/2 -translate-y-1/2"
              >
                <NIcon class="text-orange-600 dark:text-yellow-500">
                  <Crown />
                </NIcon>
              </div>

              <!-- position -->
              <div
                v-if="participant.position"
                class="absolute right-0 bottom-0 rounded-sm bg-black/60 p-0.5 dark:bg-black/80"
              >
                <PositionIcon
                  :position="participant.position"
                  class="text-5 block! text-white/80"
                />
              </div>
            </div>
          </div>

          <!-- stats line -->
          <div class="match-overview-statistics">
            <!-- spells + runes -->
            <div v-if="displayParts.spells || displayParts.runes" class="flex gap-0.5">
              <!-- spells -->
              <div
                v-if="displayParts.spells && (participant.spells[0] || participant.spells[1])"
                class="flex flex-col gap-0.5"
              >
                <SummonerSpellDisplay :spell-id="participant.spells[0]" :size="20" />
                <SummonerSpellDisplay :spell-id="participant.spells[1]" :size="20" />
              </div>

              <!-- runes / styles -->
              <div v-if="displayParts.runes && perks" class="flex flex-col gap-0.5">
                <PerkDisplay :perk-id="perks.primaryPerkId" :size="20" />
                <PerkstyleDisplay :perkstyle-id="perks.subPerkStyleId" :size="20" />
              </div>
            </div>

            <!-- augments -->
            <div v-if="displayParts.augments" class="hidden grid-cols-3 gap-0.5 @[680px]:grid">
              <AugmentDisplay
                v-for="augment of participant.augments"
                :key="augment"
                :augment-id="augment"
                :size="20"
              />
            </div>

            <!-- spacer -->
            <div class="w-0"></div>

            <!-- KDA + DMG -->
            <NPopover :delay="300">
              <template #trigger>
                <div class="match-overview-numbers">
                  <!-- KDA -->
                  <div class="match-overview-stat">
                    <div class="flex items-center justify-center gap-0.5">
                      <div class="text-base font-bold text-black dark:text-white">
                        {{ participant.kills }}
                      </div>
                      <div class="mx-px text-xs text-black/60 dark:text-white/60">/</div>
                      <div class="text-base font-bold text-red-600 dark:text-red-300">
                        {{ participant.deaths }}
                      </div>
                      <div class="mx-px text-xs text-black/60 dark:text-white/60">/</div>
                      <div class="text-base font-bold text-black dark:text-white">
                        {{ participant.assists }}
                      </div>
                    </div>

                    <!-- KDA value -->
                    <div
                      class="flex justify-center text-xs text-yellow-700 dark:text-yellow-200"
                      v-if="
                        participant.deaths === 0 &&
                        (participant.kills > 0 || participant.assists > 0)
                      "
                    >
                      {{ t('matchCard.overview.perfect') }}
                      ({{ (participant.killParticipation * 100).toFixed(0) }}%)
                    </div>

                    <div class="flex justify-center gap-1" v-else>
                      <div class="text-xs text-black/80 dark:text-white/80">
                        {{ participant.kda.toFixed(2) }}
                      </div>
                      <div class="text-xs text-black/80 dark:text-white/80">
                        ({{ (participant.killParticipation * 100).toFixed(0) }}%)
                      </div>
                    </div>
                  </div>

                  <!-- dmg -->
                  <div class="match-overview-stat">
                    <div class="text-center text-base font-bold">
                      {{
                        (
                          (participant.totalDamageDealtToChampions /
                            (teams.teamStatMap[participant.teamIdentifier]
                              .totalDamageDealtToChampions || 1)) *
                          100
                        ).toFixed(0)
                      }}%
                    </div>

                    <div class="flex justify-center gap-1">
                      <div class="text-xs text-black/80 dark:text-white/80">
                        {{ formatExtremeNumber(participant.totalDamageDealtToChampions) }}
                      </div>
                      <div class="text-xs text-black/60 dark:text-white/60">
                        {{ t('matchCard.overview.damage') }}
                      </div>
                    </div>
                  </div>

                  <!-- cs -->
                  <div class="match-overview-stat match-overview-cs" v-if="displayParts.cs">
                    <div class="text-center text-base font-bold">
                      {{ formatExtremeNumber(participant.cs) }}
                      <span class="text-[11px] font-normal text-black/60 dark:text-white/60">{{
                        t('matchCard.overview.cs')
                      }}</span>
                    </div>

                    <div class="flex justify-center gap-1">
                      <div class="text-xs text-black/80 dark:text-white/80">
                        {{ (participant.cs / (basicInfo.gameDuration / 60)).toFixed(1) }}
                      </div>
                      <div class="text-xs text-black/60 dark:text-white/60">
                        {{ t('matchCard.overview.csPerMin') }}
                      </div>
                    </div>
                  </div>
                </div>
              </template>
              <RadarChart :puuid="puuid" />
            </NPopover>
          </div>
        </div>

        <div class="match-overview-loadout">
          <!-- items -->
          <div class="match-overview-items">
            <ItemDisplay
              v-for="item of participant.items.slice(0, 6)"
              :key="item"
              :item-id="item"
              :size="20"
            />

            <ItemDisplay :item-id="participant.items[6]" :size="20" is-trinket />
            <ItemDisplay
              v-if="participant.roleBoundItem"
              :item-id="participant.roleBoundItem"
              :size="20"
            />
          </div>

          <!-- tags line -->
          <div class="match-overview-tags">
            <ManyTags />
          </div>
        </div>
      </div>

      <!-- player list (5x5 team only) -->
      <div v-if="basicInfo.isTwoTeam" class="match-overview-roster flex gap-2">
        <!-- teams -->
        <div
          v-for="team of twoTeams"
          :key="team[0].teamIdentifier"
          class="flex min-w-0 flex-1 flex-col justify-between gap-0.5"
        >
          <!-- team players -->
          <div
            v-for="player in team"
            :key="player.puuid"
            class="group flex cursor-pointer items-center gap-1"
          >
            <!-- player champion avatar -->
            <ChampionIcon :champion-id="player.championId" class="size-4 shrink-0 rounded" />

            <!-- maybe a bot player -->
            <NIcon
              class="text-black/80 dark:text-white/80"
              v-if="!player.puuid || player.puuid === EMPTY_PUUID"
            >
              <Robot />
            </NIcon>

            <NTooltip :keep-alive-on-hover="false">
              <template #trigger>
                <div
                  class="truncate text-xs transition-colors group-hover:text-black dark:group-hover:text-white"
                  :class="{
                    'font-bold text-black/90 dark:text-white/90': player.puuid === puuid,
                    'text-black/80 dark:text-white/80': player.puuid !== puuid
                  }"
                  @click="navigateToSummonerByPuuid(player.puuid)"
                  @mousedown="handleMouseDown"
                  @mouseup="handleMouseUp($event, player.puuid)"
                >
                  {{ hidePrivacy ? resources.champions.name(player.championId) : player.gameName }}
                </div>
              </template>
              <div class="flex items-center gap-1 text-xs" v-if="!hidePrivacy">
                <span class="font-bold">{{ player.gameName }}</span>
                <span v-if="player.tagLine" class="text-white/80">#{{ player.tagLine }}</span>
              </div>
              <div class="flex items-center gap-1 text-xs" v-else>
                <span class="font-bold">{{ resources.champions.name(player.championId) }}</span>
              </div>
            </NTooltip>
          </div>

          <!-- spacer -->
          <div v-for="i in 5 - team.length" :key="i" class="h-4"></div>
        </div>
      </div>

      <div
        v-else-if="basicInfo.isCherrySubteam && !isThreePlayerCherryMode"
        class="match-overview-roster grid grid-flow-col grid-cols-2 grid-rows-2 gap-x-2"
      >
        <!-- teams -->
        <div
          v-for="team of cherryWinningTeams"
          :key="team[0].teamIdentifier"
          class="flex min-w-0 flex-col justify-center gap-1"
        >
          <!-- team players -->
          <div
            v-for="player in team"
            :key="player.puuid"
            class="group flex cursor-pointer items-center gap-1"
          >
            <!-- placement -->
            <div
              class="size-4 shrink-0 rounded-full bg-black/10 text-center text-[11px] leading-4 text-black/80 dark:bg-white/10 dark:text-white/80"
            >
              {{ player.subteamPlacement }}
            </div>

            <!-- player champion avatar -->
            <ChampionIcon :champion-id="player.championId" class="size-4 shrink-0 rounded" />

            <NIcon
              class="text-black/80 dark:text-white/80"
              v-if="!player.puuid || player.puuid === EMPTY_PUUID"
            >
              <Robot />
            </NIcon>

            <NTooltip :keep-alive-on-hover="false">
              <template #trigger>
                <div
                  class="truncate text-xs transition-colors group-hover:text-black dark:group-hover:text-white"
                  :class="{
                    'font-bold text-black/90 dark:text-white/90': player.puuid === puuid,
                    'text-black/80 dark:text-white/80': player.puuid !== puuid
                  }"
                  @click="navigateToSummonerByPuuid(player.puuid)"
                  @mousedown="handleMouseDown"
                  @mouseup="handleMouseUp($event, player.puuid)"
                >
                  {{ hidePrivacy ? resources.champions.name(player.championId) : player.gameName }}
                </div>
              </template>
              <div class="flex items-center gap-1 text-xs" v-if="!hidePrivacy">
                <span class="font-bold">{{ player.gameName }}</span>
                <span v-if="player.tagLine" class="text-white/80">#{{ player.tagLine }}</span>
              </div>
              <div class="flex items-center gap-1 text-xs" v-else>
                <span class="font-bold">{{ resources.champions.name(player.championId) }}</span>
              </div>
            </NTooltip>
          </div>
        </div>
      </div>

      <div
        v-else-if="basicInfo.isCherrySubteam"
        class="match-overview-roster grid grid-flow-col grid-cols-2 grid-rows-3 content-center gap-x-2 gap-y-1"
      >
        <!-- teams -->
        <div
          v-for="team of cherryTeams"
          :key="team[0].teamIdentifier"
          class="flex min-w-0 items-center gap-1"
        >
          <!-- placement -->
          <div
            class="size-4 shrink-0 rounded-full bg-black/10 text-center text-[11px] leading-4 text-black/80 dark:bg-white/10 dark:text-white/80"
          >
            {{ team[0].subteamPlacement }}
          </div>

          <!-- team players -->
          <NTooltip v-for="player in team" :key="player.participantId" :keep-alive-on-hover="false">
            <template #trigger>
              <div
                class="relative cursor-pointer transition-[filter] hover:brightness-110"
                @click="navigateToSummonerByPuuid(player.puuid)"
                @mousedown="handleMouseDown"
                @mouseup="handleMouseUp($event, player.puuid)"
              >
                <ChampionIcon
                  :champion-id="player.championId"
                  class="size-4 shrink-0 rounded"
                  :class="{ 'ring-1 ring-black/60 dark:ring-white/70': player.puuid === puuid }"
                />
                <NIcon
                  class="absolute -right-1 -bottom-1 text-[10px] text-black/80 dark:text-white/80"
                  v-if="!player.puuid || player.puuid === EMPTY_PUUID"
                >
                  <Robot />
                </NIcon>
              </div>
            </template>
            <div class="flex items-center gap-1 text-xs" v-if="!hidePrivacy">
              <span class="font-bold">{{ player.gameName }}</span>
              <span v-if="player.tagLine" class="text-white/80">#{{ player.tagLine }}</span>
            </div>
            <div class="flex items-center gap-1 text-xs" v-else>
              <span class="font-bold">{{ resources.champions.name(player.championId) }}</span>
            </div>
          </NTooltip>
        </div>
      </div>
    </div>

    <!-- right-end expand icon -->
    <div
      @click="$emit('toggle-expand')"
      class="match-overview-expand"
      role="button"
      tabindex="0"
      :aria-expanded="isExpanded"
      @keydown.enter.prevent="$emit('toggle-expand')"
      @keydown.space.prevent="$emit('toggle-expand')"
    >
      <NIcon
        class="text-base text-black/60 dark:text-white/60"
        :class="{ '-rotate-90': !isExpanded, 'rotate-90': isExpanded }"
      >
        <ArrowBackIosFilled />
      </NIcon>
    </div>
  </div>
</template>

<script lang="ts" setup>
import PositionIcon from '@renderer-shared/components/icons/position-icons/PositionIcon.vue'
import { useNumberFormatter } from '@renderer-shared/composables/useNumberFormatter'
import { useGameResourceProvider } from '@renderer-shared/providers/game-resource'
import { EMPTY_PUUID } from '@shared/constants/common'
import { getCherryWinningTeamCount } from '@shared/data-adapter/match-history/cherry'
import { Crown, Robot } from '@vicons/fa'
import { ArrowBackIosFilled } from '@vicons/material'
import { useIntervalFn } from '@vueuse/core'
import dayjs from 'dayjs'
import { useTranslation } from 'i18next-vue'
import { NIcon, NPopover, NTooltip } from 'naive-ui'
import { computed, ref } from 'vue'

import AugmentDisplay from '../widgets/AugmentDisplay.vue'
import ChampionIcon from '../widgets/ChampionIcon.vue'
import ItemDisplay from '../widgets/ItemDisplay.vue'
import PerkDisplay from '../widgets/PerkDisplay.vue'
import PerkstyleDisplay from '../widgets/PerkstyleDisplay.vue'
import SummonerSpellDisplay from '../widgets/SummonerSpellDisplay.vue'
import { useMatchCard } from './context'
import { useGameResultName } from './utils/text'
import { useWinResultStyleType } from './utils/theme'
import { formatSeconds } from './utils/time'
import ManyTags from './widgets/ManyTags.vue'
import RadarChart from './widgets/RadarChart.vue'

defineEmits<{
  'toggle-expand': []
}>()

const {
  puuid,
  basicInfo,
  teams,
  participants,
  isExpanded,
  hidePrivacy,
  navigateToSummonerByPuuid
} = useMatchCard()

const { formatExtremeNumber } = useNumberFormatter()

const gameResultName = useGameResultName()

const resources = useGameResourceProvider()
const { t } = useTranslation()

// 典型的 100 / 200 红蓝队方法
const twoTeams = computed(() => {
  if (!basicInfo.value.isTwoTeam) return []

  const teamIdentifiers = teams.value.teamStatsArr.map((t) => t.teamIdentifier).slice(0, 2)
  return teamIdentifiers.map((i) => {
    return participants.value.filter((s) => s.teamIdentifier === i).slice(0, 5) // 5 是战绩卡片的最大容纳量
  })
})

const cherryTeams = computed(() => {
  if (!basicInfo.value.isCherrySubteam) return []

  const teamIdentifiers = teams.value.teamStatsArr
    .toSorted((a, b) => a.subteamPlacement - b.subteamPlacement)
    .map((t) => t.teamIdentifier)

  return teamIdentifiers.map((i) => {
    return participants.value
      .filter((s) => s.teamIdentifier === i)
      .toSorted((a, b) => a.participantId - b.participantId)
  })
})

const cherryWinningTeams = computed(() => {
  if (!basicInfo.value.isCherrySubteam) return []

  const winningTeamCount = getCherryWinningTeamCount(teams.value.teamStatsArr.length)

  return cherryTeams.value
    .filter((team) => team[0]?.subteamPlacement <= winningTeamCount)
    .slice(0, winningTeamCount)
})

const isThreePlayerCherryMode = computed(() => {
  return cherryTeams.value.some((team) => team.length === 3)
})

const shouldShowCrown = computed(() => {
  return participant.value?.subteamPlacement === 1
})

// 自己相关的数据
const participant = computed(() => {
  return participants.value.find((s) => s.puuid === puuid.value)
})

const team = computed(() => {
  if (!participant.value) return null

  return teams.value.teamStatMap[participant.value.teamIdentifier]
})

const perks = computed(() => {
  if (!participant.value) return null

  const { styles } = participant.value.perks

  const primaryStyle = styles[0]
  const subStyle = styles[1]

  // no id 0, no null
  if (!primaryStyle || !subStyle || !primaryStyle.selections[0]?.perk || !subStyle.style) {
    return null
  }

  return {
    primaryPerkId: primaryStyle.selections[0].perk,
    subPerkStyleId: subStyle.style
  }
})

// UI 有些地方可以不用展示
const displayParts = computed(() => {
  return {
    spells: true,
    augments: basicInfo.value.gameMode === 'CHERRY' || basicInfo.value.gameMode === 'KIWI',
    runes: basicInfo.value.gameMode !== 'CHERRY' && basicInfo.value.gameMode !== 'KIWI',
    items: true,
    cs: basicInfo.value.gameMode === 'CLASSIC'
  }
})

const mapName = computed(() => {
  return resources.maps.name(basicInfo.value.mapId, {
    gameModeMutators: basicInfo.value.gameModeMutators
  })
})

const winStyleType = useWinResultStyleType()

const formattedRelativeTime = ref('')
const gameCreationTitle = computed(() => {
  return dayjs(basicInfo.value.gameCreation).format('YYYY-MM-DD HH:mm:ss:SSS')
})

useIntervalFn(
  () => {
    const date = dayjs(basicInfo.value.gameCreation).locale(resources.runtime.locale.toLowerCase())
    if (dayjs().diff(date, 'day', true) > 3) {
      formattedRelativeTime.value = date.format('YYYY-MM-DD HH:mm')
    } else {
      formattedRelativeTime.value = date.fromNow()
    }
  },
  60000,
  { immediateCallback: true, immediate: true }
)

const handleMouseDown = (event: MouseEvent) => {
  if (event.button === 1) {
    event.preventDefault()
  }
}

const handleMouseUp = (event: MouseEvent, puuid: string) => {
  if (event.button === 1) {
    navigateToSummonerByPuuid(puuid, false)
  }
}
</script>

<style scoped>
@import './match-card.css';
</style>

<style>
/* Workbench rows share one alignment and open their full contents in a drawer. */
.workbench-match-row .match-overview {
  height: 88px;
  border: 0;
  border-bottom: 1px solid rgb(var(--la-card-border-rgb) / 0.1);
  border-radius: 0;
  background: transparent;
}
.workbench-match-row .match-overview:hover {
  background: var(--la-card-muted-surface);
}
.workbench-match-row .match-overview::before {
  inset: 30px auto 30px 0;
  width: 2px;
}
.workbench-match-row .match-overview-content {
  display: grid;
  grid-template-columns: 160px minmax(290px, 1fr) 185px 110px;
  align-items: center;
  gap: 18px;
  padding: 12px 16px;
}
.workbench-match-row .match-overview-main {
  display: contents;
}
.workbench-match-row .match-overview-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.workbench-match-row .match-overview-info {
  flex-wrap: wrap;
  white-space: normal;
  font-size: 10px;
  max-height: 34px;
}
.workbench-match-row .match-overview-performance {
  gap: 12px;
}
.workbench-match-row .match-overview-statistics {
  gap: 10px;
}
.workbench-match-row .match-overview-numbers {
  flex: 1;
  justify-content: space-around;
  gap: 10px;
}
.workbench-match-row .match-overview-stat {
  min-width: 54px;
}
.workbench-match-row .match-overview-stat > :first-child {
  font-size: 14px;
  font-weight: 500;
}
.workbench-match-row .match-overview-loadout {
  flex-direction: column;
  align-items: flex-start;
  padding: 0;
  gap: 6px;
}
.workbench-match-row .match-overview-items {
  gap: 2px;
}
.workbench-match-row .match-overview-roster {
  width: 110px;
  height: 48px;
  padding: 0;
  border: 0;
  flex-direction: column;
  gap: 6px;
  overflow: hidden;
}
.workbench-match-row .match-overview-roster > div {
  flex-direction: row;
  gap: 3px;
}
.workbench-match-row .match-overview-roster .group > div:last-child {
  max-width: 0;
  overflow: hidden;
}
.workbench-match-row .match-overview-expand {
  flex-basis: 32px;
  border-left: 0;
}
@container (max-width: 1050px) {
  .workbench-match-row .match-overview-content {
    grid-template-columns: 140px minmax(260px, 1fr) 170px 100px;
    gap: 10px;
    padding: 10px 12px;
  }
  .workbench-match-row .match-overview-roster {
    width: 100px;
  }
}
@container (max-width: 850px) {
  .workbench-match-row .match-overview-content {
    grid-template-columns: 120px minmax(250px, 1fr) 160px;
  }
  .workbench-match-row .match-overview-roster {
    display: none;
  }
}
</style>
