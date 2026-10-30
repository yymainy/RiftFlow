<template>
  <div class="relative h-full">
    <NScrollbar x-scrollable :theme-overrides="{ width: '8px' }" ref="scrollbarEl">
      <div class="player-workspace" :class="{ 'player-workspace--compact': isSmallSize }">
        <div class="workspace-caption">
          <span>{{ t('navigation.sidebar.menu.match-history') }}</span>
          <span class="workspace-caption__detail">{{ t('appName', { ns: 'common' }) }}</span>
        </div>

        <PlayerTabHeader class="workspace-profile" />

        <div ref="stickySentinelLeftSideEl" class="h-0"></div>
        <div ref="stickySentinelRightSideEl" class="h-0"></div>
        <SummaryPane class="workbench-overview" />
        <div class="workspace-toolbar">
          <span class="workspace-toolbar__title">{{
            t('navigation.sidebar.menu.match-history')
          }}</span>
          <MatchHistoryPagination horizontal class="workspace-toolbar__controls" />
          <NButton
            size="small"
            quaternary
            :aria-expanded="showAnalysis"
            :aria-controls="`player-analysis-${id}`"
            @click="showAnalysis = !showAnalysis"
          >
            {{ t('playerTabs.summary.title') }}
            <span class="ml-2" aria-hidden="true">{{ showAnalysis ? '−' : '+' }}</span>
          </NButton>
        </div>

        <div class="workspace-columns">
          <section
            class="workspace-matches workbench-table"
            :aria-label="t('navigation.sidebar.menu.match-history')"
          >
            <MatchHistoryList />
          </section>
          <aside
            :id="`player-analysis-${id}`"
            v-show="showAnalysis"
            class="workspace-analysis"
            :aria-label="t('playerTabs.summary.title')"
          >
            <div class="workspace-analysis__heading">{{ t('playerTabs.summary.title') }}</div>
            <div class="workspace-analysis__modules">
              <div class="analysis-module analysis-tags"><NormalTagBlock /></div>
              <ChampionMasteryPane class="analysis-module" />
              <RecentlyPlayers class="analysis-module" side="ally" />
              <RecentlyPlayers class="analysis-module" side="enemy" />
              <PlayerChallenges class="analysis-module" />
              <EncounteredGames class="analysis-module" />
            </div>
          </aside>
        </div>
      </div>
    </NScrollbar>

    <CollectModeProgress />

    <div
      :class="{
        'pointer-events-auto opacity-80': shouldShowScrollToTopButton,
        'pointer-events-none opacity-0': !shouldShowScrollToTopButton
      }"
      class="absolute! right-8 bottom-8 z-10 transition-opacity hover:opacity-100"
    >
      <NButton size="large" type="primary" circle :focusable="false" @click="scrollToTop">
        <NIcon>
          <ArrowUp20Regular />
        </NIcon>
      </NButton>
    </div>

    <ConnectedMatchPreviewer
      v-model:show="showPreviewModal"
      :game-id="previewingGame.gameId"
      :source="previewingGame.source"
      :puuid="previewingGame.puuid"
      :summary="previewingGame.summary"
      :details="previewingGame.details"
      :hide-privacy="as.settings.streamerMode"
      :can-dry-run-ongoing-game="canDryRunOngoingGame"
      @navigate-to-summoner-by-puuid="(puuid) => navigateToTabByPuuid(puuid)"
      @dry-run-ongoing-game="handleDryRunOngoingGame"
    />

    <!-- 这个组件不会生成 DOM，但用来保证全局状态同步 -->
    <GlobalStateTracker />
  </div>
</template>

<script setup lang="ts">
import ConnectedMatchPreviewer from '@renderer-shared/components/match-preview/ConnectedMatchPreviewer.vue'
import {
  type MatchPreviewPayload,
  type MatchPreviewState,
  toMatchPreviewState
} from '@renderer-shared/components/match-preview'
import { useActivated } from '@renderer-shared/composables/useActivated'
import { useInstance } from '@renderer-shared/shards'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { OngoingGameRenderer } from '@renderer-shared/shards/ongoing-game'
import { useSgpStore } from '@renderer-shared/shards/sgp/store'
import { DraftOptions } from '@shared/shards/ongoing-game'
import { ArrowUp20Regular } from '@vicons/fluent'
import { useElementVisibility, useTimeoutFn } from '@vueuse/core'
import { NButton, NIcon, NScrollbar } from 'naive-ui'
import { useTranslation } from 'i18next-vue'
import { computed, ref, shallowRef, useTemplateRef, watchEffect } from 'vue'
import { useRouter } from 'vue-router'

import { useMainWindowAppContext } from '@main-window/context'
import { PlayerTabsRenderer } from '@main-window/shards/player-tabs'
import { usePlayerTabsStore } from '@main-window/shards/player-tabs/store'

import GlobalStateTracker from './GlobalStateTracker'
import { SMALL_SIZE_THRESHOLD } from './constants'
import { providePlayerTab } from './context'
import { useFreezeValue } from './utils/freeze'
import ChampionMasteryPane from './widgets/ChampionMasteryPane.vue'
import EncounteredGames from './widgets/EncounteredGames.vue'
import MatchHistoryList from './widgets/MatchHistoryList.vue'
import MatchHistoryPagination from './widgets/match-history-pagination'
import NormalTagBlock from './widgets/NormalTagBlock.vue'
import PlayerChallenges from './widgets/PlayerChallenges.vue'
import PlayerTabHeader from './widgets/PlayerTabHeader.vue'
import RecentlyPlayers from './widgets/RecentlyPlayers.vue'
import SummaryPane from './widgets/SummaryPane.vue'
import CollectModeProgress from './widgets/match-history-filters/CollectModeProgress.vue'

const { id, puuid, sgpServerId } = defineProps<{
  id: string
  puuid: string
  sgpServerId: string
}>()

const { t } = useTranslation()
const showAnalysis = ref(false)

const pt = useInstance(PlayerTabsRenderer)
const og = useInstance(OngoingGameRenderer)
const router = useRouter()

const lcs = useLeagueClientStore()
const as = useAppCommonStore()
const pts = usePlayerTabsStore()
const sgps = useSgpStore()

const { navigateToTabByPuuid } = pt.useNavigateToTab()

const { contentWidth } = useMainWindowAppContext()

const isSmallSize = computed(() => contentWidth.value < SMALL_SIZE_THRESHOLD)

const isCurrentTab = computed(() => {
  return pts.currentTabId === id
})

const isActivated = useActivated()

const isInvisible = computed(() => {
  return !isCurrentTab.value || !isActivated.value
})

const scrollbarEl = useTemplateRef('scrollbarEl')
const stickySentinelLeftSideEl = useTemplateRef('stickySentinelLeftSideEl')
const stickySentinelRightSideEl = useTemplateRef('stickySentinelRightSideEl')
const isSentinelVisibleLeftSide = useElementVisibility(stickySentinelLeftSideEl, {
  initialValue: true
})
const isSentinelVisibleRightSide = useElementVisibility(stickySentinelRightSideEl, {
  initialValue: true
})

const {
  value: frozenVisibleLeftSide,
  freeze: freezeLeftSide,
  unfreeze: unfreezeLeftSide
} = useFreezeValue(isSentinelVisibleLeftSide)
const {
  value: frozenVisibleRightSide,
  freeze: freezeRightSide,
  unfreeze: unfreezeRightSide
} = useFreezeValue(isSentinelVisibleRightSide)

const shouldShowScrollToTopButton = computed(() => {
  if (isSmallSize.value) {
    return !frozenVisibleRightSide.value
  }

  return !frozenVisibleLeftSide.value
})

const showPreviewModal = ref(false)
const previewingGame = shallowRef<MatchPreviewState>({
  gameId: 0,
  source: 'sgp'
})

const handlePreviewGame = (payload: MatchPreviewPayload) => {
  previewingGame.value = toMatchPreviewState(
    payload,
    as.settings.preferredLolSource,
    lcs.summoner.me?.puuid
  )
  showPreviewModal.value = true
}

// The analysis draft relies on local-region data loaded by ongoing-game.
const canDryRunOngoingGame = computed(() => sgpServerId === sgps.availability.sgpServerId)

const handleDryRunOngoingGame = async (draft: DraftOptions) => {
  if (!canDryRunOngoingGame.value) {
    return
  }

  await og.setDraft(draft)
  await router.replace({ name: 'ongoing-game' })
}

const scrollToTop = () => {
  scrollbarEl.value?.scrollTo({ top: 0, behavior: 'smooth' })
}

// 一个粗糙的解决闪烁问题的方式
// 我们假设浏览器在 50ms 内可以完成异步的 intersection observer 的回调
const { start, stop } = useTimeoutFn(() => {
  unfreezeLeftSide()
  unfreezeRightSide()
}, 50)

watchEffect(() => {
  if (isInvisible.value) {
    stop()
    freezeLeftSide()
    freezeRightSide()
  } else {
    start()
  }
})

providePlayerTab({
  id: () => id,
  puuid: () => puuid,
  sgpServerId: () => sgpServerId,
  isCurrentTab,
  isSmallSize,
  previewGame: handlePreviewGame
})
</script>

<style scoped>
.player-workspace {
  box-sizing: border-box;
  width: 100%;
  max-width: 1240px;
  margin: 0 auto;
  padding: 22px 28px 32px;
  color: var(--la-color-text-primary);
}
.workspace-caption {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  color: var(--la-text-secondary);
  font-size: 11px;
  line-height: 18px;
}
.workspace-caption__detail {
  opacity: 0.65;
  font-size: 10px;
}
.workspace-profile {
  padding: 22px 0 24px;
}
.workspace-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  min-height: 46px;
  padding: 6px 0 12px;
  margin-bottom: 22px;
  border-bottom: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
}
.workspace-toolbar__title {
  font-size: 12px;
  font-weight: 600;
  margin-right: auto;
}
.workspace-toolbar__controls {
  max-width: 100%;
}
.workspace-columns {
  display: grid;
  grid-template-columns: 244px minmax(0, 1fr);
  gap: 24px;
  align-items: start;
}
.workspace-analysis {
  min-width: 0;
  border-right: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  padding-right: 22px;
}
.workspace-analysis__heading {
  font-size: 10px;
  color: var(--la-text-secondary);
  margin-bottom: 16px;
}
.workspace-analysis__modules {
  display: flex;
  flex-direction: column;
  gap: 0;
}
.workspace-analysis__modules :deep(.analysis-module) {
  box-sizing: border-box;
  width: 100%;
  padding: 0 0 18px;
  margin: 0 0 18px;
  border: 0;
  border-bottom: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  border-radius: 0;
  background: transparent;
}
.workspace-analysis__modules :deep(.analysis-module:last-child) {
  border-bottom: 0;
  margin-bottom: 0;
}
.workspace-analysis__modules :deep(.text-base.font-bold) {
  font-size: 12px;
  font-weight: 600;
}
.workspace-analysis__modules :deep(.analysis-tags:empty) {
  display: none;
}
.workspace-analysis__modules :deep(.analysis-tags > div) {
  padding: 10px;
  margin-bottom: 8px;
  background: var(--la-card-muted-surface);
  color: var(--la-color-text-primary);
  border-radius: 6px;
}
.workspace-analysis__modules :deep(.analysis-tags .text-white) {
  color: var(--la-color-text-primary);
}
.workspace-matches {
  min-width: 0;
}
.player-workspace--compact {
  padding: 18px 20px 28px;
}
.player-workspace--compact .workspace-columns {
  grid-template-columns: minmax(0, 1fr);
  gap: 22px;
}
.player-workspace--compact .workspace-analysis {
  border-right: 0;
  border-bottom: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  padding: 0 0 4px;
}
.player-workspace--compact .workspace-analysis__modules {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: 0 24px;
}
.player-workspace--compact .workspace-analysis__heading {
  display: none;
}
.player-workspace--compact .workspace-toolbar {
  margin-bottom: 18px;
}

.player-workspace,
.player-workspace--compact {
  max-width: 1600px;
  padding: 18px 28px 32px;
}
.workspace-caption {
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.workspace-profile {
  padding: 14px 0 18px;
}
.workspace-toolbar,
.player-workspace--compact .workspace-toolbar {
  margin: 12px 0 0;
  padding: 8px 0;
  border-bottom: 0;
}
.workspace-columns,
.player-workspace--compact .workspace-columns {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.workspace-matches {
  width: 100%;
}
.workspace-analysis,
.player-workspace--compact .workspace-analysis {
  width: 100%;
  box-sizing: border-box;
  padding: 20px 0 0;
  border: 0;
  border-top: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
}
.workspace-analysis__modules,
.player-workspace--compact .workspace-analysis__modules {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 20px;
}
.workbench-overview {
  margin-bottom: 4px;
}
</style>
