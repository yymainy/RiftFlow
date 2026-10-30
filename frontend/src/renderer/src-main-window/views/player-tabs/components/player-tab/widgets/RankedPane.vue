<template>
  <div v-if="shouldRender" class="ranked-strip">
    <div v-if="isCrossRegion" class="ranked-strip__unavailable">
      <span>{{ t('playerTabs.ranked.crossRegion', 'Cross Region') }}</span>
      <span>{{ t('playerTabs.ranked.unavailable', 'Unavailable') }}</span>
    </div>
    <template v-else>
      <div v-if="isLoading && !displayedRankedEntries.length" class="ranked-strip__unavailable">
        {{ t('playerTabs.profile.loading') }}
      </div>
      <div v-for="entry in displayedRankedEntries" :key="entry.queueType" class="ranked-compact">
        <img
          class="ranked-compact__emblem"
          :src="rankedImageMap[getCurrentTier(entry)] || rankedImageMap['UNRANKED']"
          alt=""
        />
        <div class="ranked-compact__content">
          <div class="ranked-compact__queue">
            {{
              t(`queueTypes.${entry.queueType}`, { defaultValue: entry.queueType, ns: 'common' })
            }}
            <span v-if="hasEntryTopRecord(entry)" class="ranked-compact__top">{{
              formatEntryTopRecord(entry)
            }}</span>
          </div>
          <div class="ranked-compact__tier">{{ formatTier(entry) }}</div>
          <div class="ranked-compact__record">{{ formatEntryRecord(entry) }}</div>
          <div class="ranked-compact__highest">
            <span>{{ t('playerTabs.ranked.highest') }}</span>
            <img
              v-if="getHighestTier(entry) && rankedMedalMap[getHighestTier(entry)]"
              :src="rankedMedalMap[getHighestTier(entry)]"
              alt=""
            />
            <span>{{ formatHighestTier(entry) }}</span>
          </div>
        </div>
      </div>
      <NButton
        v-if="displayedRankedEntries.length > 1"
        class="ranked-strip__more"
        :title="t('playerTabs.profile.rankedMore')"
        :aria-label="t('playerTabs.profile.rankedMore')"
        size="tiny"
        quaternary
        @click="isShowingRankedModal = true"
      >
        <template #icon
          ><NIcon><MoreHorizFilled /></NIcon
        ></template>
      </NButton>
    </template>
  </div>

  <NModal v-model:show="isShowingRankedModal">
    <div class="ranked-details-modal">
      <div class="mb-4 grid grid-cols-2 gap-4">
        <div
          v-for="entry in displayedRankedEntries"
          :key="entry.queueType"
          class="relative flex h-27 w-60 items-center justify-center rounded-md border border-solid border-black/10 bg-(--la-card-surface-90) dark:border-white/10"
        >
          <!-- Queue Type Label -->
          <div
            class="absolute top-0 left-0 flex max-w-full items-center gap-1.5 px-2 py-1 text-xs text-gray-500 dark:text-gray-400"
          >
            <span class="shrink-0">
              {{
                t(`queueTypes.${entry.queueType}`, {
                  defaultValue: entry.queueType,
                  ns: 'common'
                })
              }}
            </span>
            <span
              v-if="hasEntryTopRecord(entry)"
              class="rounded-md bg-black/8 px-1.5 py-px text-[10px] leading-4 text-gray-600 dark:bg-white/10 dark:text-gray-300"
            >
              {{ formatEntryTopRecord(entry) }}
            </span>
          </div>

          <!-- Main Content -->
          <div class="relative top-1 flex w-full items-center justify-center gap-2">
            <!-- Image Container -->
            <div class="relative h-12 w-16">
              <img
                class="absolute top-1/2 left-1/2 h-[144%] w-[144%] -translate-x-1/2 -translate-y-1/2 object-contain"
                :src="rankedImageMap[getCurrentTier(entry)] || rankedImageMap['UNRANKED']"
              />
            </div>

            <!-- Info -->
            <div class="flex min-w-16 flex-col">
              <span class="text-base font-bold text-gray-900 dark:text-gray-100">{{
                formatTier(entry)
              }}</span>
              <span class="text-xs text-gray-500 dark:text-gray-400">
                {{ formatEntryRecord(entry) }}
              </span>

              <!-- Historical Highest -->
              <div
                class="flex items-center text-[10px] text-gray-500 dark:text-gray-300"
                :class="{
                  'text-gray-400 dark:text-gray-500':
                    !entry.highestTier || entry.highestTier === 'NA'
                }"
              >
                <span class="mr-0.5">{{ t('playerTabs.ranked.highest') }}</span>
                <div class="flex items-center">
                  <img
                    v-if="getHighestTier(entry) && rankedMedalMap[getHighestTier(entry)]"
                    :src="rankedMedalMap[getHighestTier(entry)]"
                    class="mr-0.5 h-4 w-4"
                  />
                  <span>{{ formatHighestTier(entry) }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <RankedTable v-if="rankedStats" :ranked-stats="rankedStats" />
    </div>
  </NModal>
</template>

<script setup lang="ts">
import RankedBronze from '@renderer-shared/assets/ranked-icons-large/bronze.png'
import RankedChallenger from '@renderer-shared/assets/ranked-icons-large/challenger.png'
import RankedDiamond from '@renderer-shared/assets/ranked-icons-large/diamond.png'
import RankedEmerald from '@renderer-shared/assets/ranked-icons-large/emerald.png'
import RankedGold from '@renderer-shared/assets/ranked-icons-large/gold.png'
import RankedGrandmaster from '@renderer-shared/assets/ranked-icons-large/grandmaster.png'
import RankedIron from '@renderer-shared/assets/ranked-icons-large/iron.png'
import RankedMaster from '@renderer-shared/assets/ranked-icons-large/master.png'
import RankedPlatinum from '@renderer-shared/assets/ranked-icons-large/platinum.png'
import RankedSilver from '@renderer-shared/assets/ranked-icons-large/silver.png'
import RankedNone from '@renderer-shared/assets/ranked-icons-large/unranked.png'
import BronzeMedal from '@renderer-shared/assets/ranked-icons/bronze.png'
import ChallengerMedal from '@renderer-shared/assets/ranked-icons/challenger.png'
import DiamondMedal from '@renderer-shared/assets/ranked-icons/diamond.png'
import EmeraldMedal from '@renderer-shared/assets/ranked-icons/emerald.png'
import GoldMedal from '@renderer-shared/assets/ranked-icons/gold.png'
import GrandmasterMedal from '@renderer-shared/assets/ranked-icons/grandmaster.png'
import IronMedal from '@renderer-shared/assets/ranked-icons/iron.png'
import MasterMedal from '@renderer-shared/assets/ranked-icons/master.png'
import PlatinumMedal from '@renderer-shared/assets/ranked-icons/platinum.png'
import SilverMedal from '@renderer-shared/assets/ranked-icons/silver.png'
import RankedTable from '@renderer-shared/components/RankedTable.vue'
import { RankedEntry, RankedStats } from '@shared/types/league-client/ranked'
import { MoreHorizFilled } from '@vicons/material'
import { useTranslation } from 'i18next-vue'
import { NButton, NIcon, NModal } from 'naive-ui'
import { computed, ref } from 'vue'

import { usePlayerTab } from '../context'
import { useRankedStats } from '../data/ranked-stats'

const { isCrossRegion } = usePlayerTab()

const { t } = useTranslation()
const isShowingRankedModal = ref(false)

const { rankedStats, isLoading } = useRankedStats()

// 只显示单双排和灵活组排
const DISPLAY_QUEUE_TYPES: Array<keyof RankedStats['queueMap']> = [
  'RANKED_SOLO_5x5',
  'RANKED_FLEX_SR'
]

const displayedRankedEntries = computed(() => {
  if (!rankedStats.value) return []

  return DISPLAY_QUEUE_TYPES.map((queueType) => {
    return (
      rankedStats.value?.queueMap[queueType] ||
      rankedStats.value?.queues.find((entry) => entry.queueType === queueType)
    )
  }).filter((entry): entry is RankedEntry => Boolean(entry))
})

const rankedImageMap: Record<string, string> = {
  UNRANKED: RankedNone,
  IRON: RankedIron,
  BRONZE: RankedBronze,
  SILVER: RankedSilver,
  GOLD: RankedGold,
  EMERALD: RankedEmerald,
  PLATINUM: RankedPlatinum,
  DIAMOND: RankedDiamond,
  MASTER: RankedMaster,
  GRANDMASTER: RankedGrandmaster,
  CHALLENGER: RankedChallenger
}

const rankedMedalMap: Record<string, string> = {
  IRON: IronMedal,
  BRONZE: BronzeMedal,
  SILVER: SilverMedal,
  GOLD: GoldMedal,
  PLATINUM: PlatinumMedal,
  EMERALD: EmeraldMedal,
  DIAMOND: DiamondMedal,
  MASTER: MasterMedal,
  GRANDMASTER: GrandmasterMedal,
  CHALLENGER: ChallengerMedal
}

const shouldRender = computed(() => {
  if (isCrossRegion.value) {
    return true
  }
  return displayedRankedEntries.value.length > 0 || isLoading.value
})

const isUnrankedTier = (tier: string | undefined | null) => {
  return !tier || tier === 'NA' || tier === 'NONE'
}

const isRankedEntry = (entry: Partial<RankedEntry>) => {
  return !isUnrankedTier(entry.tier)
}

const getCurrentTier = (entry: Partial<RankedEntry>) => {
  return isUnrankedTier(entry.tier) ? 'UNRANKED' : entry.tier!
}

const getHighestTier = (entry: Partial<RankedEntry>) => {
  return isUnrankedTier(entry.highestTier) ? '' : entry.highestTier!
}

const formatEntryRecord = (entry: Partial<RankedEntry>) => {
  if (isRankedEntry(entry)) {
    const losses = entry.losses ? ` ${entry.losses} ${t('playerTabs.ranked.lose')}` : ''

    return `${entry.wins} ${t('playerTabs.ranked.win')}${losses} ${entry.leaguePoints} LP`
  }

  return '—'
}

const hasEntryTopRecord = (entry: Partial<RankedEntry>) => {
  return isRankedEntry(entry) && typeof entry.losses === 'number' && entry.losses !== 0
}

const formatEntryTopRecord = (entry: Partial<RankedEntry>) => {
  const wins = entry.wins ?? 0
  const losses = entry.losses ?? 0
  const total = wins + losses
  const winRate = total > 0 ? ((wins / total) * 100).toFixed(1) : '0.0'

  return `${t('playerTabs.ranked.winRate')} ${winRate}%`
}

const formatTier = (entry: Partial<RankedEntry>) => {
  if (!entry) return ''

  const rawTier = entry.tier

  if (isUnrankedTier(rawTier)) {
    return t('playerTabs.ranked.unranked', 'unranked')
  }

  const tier = t(`tiers.${rawTier}`, {
    defaultValue: rawTier,
    ns: 'common'
  })

  const division = entry.division

  if (isUnrankedTier(division)) {
    return tier
  }

  return `${tier} ${division}`
}

const formatHighestTier = (entry: Partial<RankedEntry>) => {
  if (!entry) return ''

  if (isUnrankedTier(entry.highestTier)) {
    return t('playerTabs.ranked.unranked', 'unranked')
  }

  const tier = t(`tiers.${entry.highestTier}`, {
    defaultValue: entry.highestTier,
    ns: 'common'
  })

  const division = entry.highestDivision

  if (isUnrankedTier(division)) {
    return tier
  }

  return `${tier} ${division}`
}
</script>

<style scoped>
.ranked-strip {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}
.ranked-compact {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 155px;
  padding-left: 16px;
  border-left: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
}
.ranked-compact__emblem {
  width: 43px;
  height: 51px;
  object-fit: contain;
  flex-shrink: 0;
}
.ranked-compact__content {
  min-width: 0;
}
.ranked-compact__queue {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  color: var(--la-text-secondary);
  font-size: 9px;
  line-height: 15px;
}
.ranked-compact__top {
  border-radius: 3px;
  background: rgb(var(--la-card-tint-rgb) / 0.07);
  padding: 0 3px;
}
.ranked-compact__tier {
  color: var(--la-color-text-primary);
  font-size: 12px;
  font-weight: 600;
  line-height: 20px;
}
.ranked-compact__record {
  color: var(--la-text-secondary);
  font-size: 10px;
  line-height: 16px;
}
.ranked-compact__highest {
  display: flex;
  align-items: center;
  gap: 3px;
  color: var(--la-text-secondary);
  font-size: 9px;
  line-height: 15px;
}
.ranked-compact__highest img {
  width: 12px;
  height: 12px;
  object-fit: contain;
}
.ranked-strip__more {
  align-self: center;
  flex-shrink: 0;
}
.ranked-strip__unavailable {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  color: var(--la-text-secondary);
}
.ranked-details-modal {
  box-sizing: border-box;
  max-width: calc(100vw - 32px);
  max-height: 88vh;
  overflow: auto;
  padding: 20px;
  border: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  border-radius: 8px;
  background: var(--la-card-surface-95);
}
@media (max-width: 1000px) {
  .ranked-strip {
    gap: 20px;
    flex-wrap: wrap;
  }
  .ranked-compact {
    padding-left: 0;
    padding-right: 20px;
    border-left: 0;
    border-right: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  }
}
</style>
