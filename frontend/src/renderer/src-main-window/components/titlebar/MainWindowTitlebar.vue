<template>
  <div
    class="app-titlebar"
    :class="[
      { 'should-show-bottom-border': shouldShowBottomBorder },
      as.isMacOS ? 'pl-[calc(8px+var(--la-mac-titlebar-safe-left))]' : 'pl-2'
    ]"
  >
    <div class="workspace-breadcrumb">
      <span>{{ t('appName', { ns: 'common' }) }}</span
      ><span class="workspace-breadcrumb__slash">/</span><b>{{ workspaceTitle }}</b>
    </div>
    <div class="shard-area">
      <OngoingGameTitle v-if="$route.name === 'ongoing-game'" class="workspace-live-controls" />
      <PlayerTabsTitle />
    </div>

    <div class="divider" :class="{ invisible: !shouldShowDivider }" />
    <CommonButtons />

    <div class="w-1" v-if="as.isMacOS"></div>

    <!-- no duplicate traffic buttons on macOS xD -->
    <template v-if="!as.isMacOS">
      <div class="divider" />
      <TrafficButtons />
    </template>
  </div>
</template>

<script setup lang="ts">
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import { computed } from 'vue'
import { useTranslation } from 'i18next-vue'
import { useRoute } from 'vue-router'

import { usePlayerTabsStore } from '@main-window/shards/player-tabs/store'

import CommonButtons from './CommonButtons.vue'
import OngoingGameTitle from './OngoingGameTitle.vue'
import PlayerTabsTitle from './PlayerTabsTitle.vue'
import TrafficButtons from './TrafficButtons.vue'

const route = useRoute()
const { t } = useTranslation()
const workspaceTitle = computed(() =>
  route.name === 'archive'
    ? t('archiveWorkspace.navigation')
    : route.name === 'skin-assets'
      ? t('skinAssets.title')
      : t(
          'navigation.sidebar.menu.' +
            ({
              'player-tabs': 'match-history',
              'ongoing-game': 'ongoing-game',
              automation: 'automation',
              toolkit: 'toolkit',
              test: 'test'
            }[String(route.name)] || 'match-history')
        )
)

const lcs = useLeagueClientStore()
const ogs = useOngoingGameStore()
const pts = usePlayerTabsStore()
const as = useAppCommonStore()

const shouldShowDivider = computed(() => {
  switch (route.name) {
    case 'player-tabs':
      return lcs.isConnected && pts.tabs.length

    case 'ongoing-game':
      const isCsSpectateWait =
        lcs.champSelect.session &&
        lcs.champSelect.session.isSpectating &&
        Object.values(ogs.teams).flat().length === 0

      return ogs.queryStage.phase !== 'unavailable' && !isCsSpectateWait
    default:
      return false
  }
})

const shouldShowBottomBorder = computed(() => {
  switch (route.name) {
    case 'player-tabs':
      return lcs.isConnected && pts.tabs.length
    case 'ongoing-game':
      return ogs.queryStage.phase !== 'unavailable'
    default:
      return false
  }
})
</script>

<style scoped>
.app-titlebar {
  display: flex;
  position: relative;
  height: var(--la-titlebar-height);
  align-items: center;
  -webkit-app-region: drag;
  z-index: 1000000;

  &.should-show-bottom-border {
    border-bottom: 1px solid rgba(0, 0, 0, 0.1);

    [data-theme='dark'] & {
      border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    }
  }

  .mica & {
    backdrop-filter: none;
  }
}

.shard-area {
  height: 100%;
  width: 0;
  flex: 1;
}

.app-name {
  padding: 0 4px;
  font-family: 'Comfortaa', sans-serif;
  font-weight: bold;
  margin-left: 8px;
}

[data-theme='dark'] {
  .app-name {
    color: rgba(255, 255, 255, 0.8);
  }
}

[data-theme='light'] {
  .app-name {
    color: rgba(0, 0, 0, 0.8);
  }
}

.divider {
  width: 1px;
  height: 40%;
  box-sizing: border-box;
  margin: 0 8px;
  background-color: rgba(255, 255, 255, 0.15);

  &.invisible {
    visibility: hidden;
  }
}

[data-theme='dark'] {
  .divider {
    background-color: rgba(255, 255, 255, 0.15);
  }
}

[data-theme='light'] {
  .divider {
    background-color: rgba(0, 0, 0, 0.15);
  }
}

[data-theme-id]:not([data-theme-id='light']):not([data-theme-id='dark']) {
  .app-name {
    color: color-mix(in oklch, var(--la-color-text-themed) 84%, transparent);
  }

  .app-titlebar {
    &.should-show-bottom-border {
      border-bottom-color: rgb(var(--la-card-border-rgb) / 0.22);
    }
  }

  .divider {
    background-color: rgb(var(--la-card-border-rgb) / 0.3);
  }
}

.app-titlebar {
  border-bottom: 1px solid var(--la-border);
  background: var(--la-color-bg-primary);
}

.app-titlebar {
  padding-left: 25px;
  padding-right: 6px;
  gap: 8px;
}
.workspace-breadcrumb {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11px;
  color: var(--la-text-secondary);
  white-space: nowrap;
  padding-right: 12px;
}
.workspace-breadcrumb b {
  color: var(--la-color-text-primary);
  font-weight: 500;
}
.workspace-breadcrumb__slash {
  opacity: 0.4;
}
.shard-area {
  min-width: 0;
  display: flex;
  align-items: center;
}
.shard-area > :last-child {
  flex: 1;
  min-width: 0;
}
.workspace-live-controls {
  flex: 0 1 auto;
}
@media (max-width: 1100px) {
  .app-titlebar {
    padding-left: 16px;
  }
  .workspace-breadcrumb > span:first-child,
  .workspace-breadcrumb__slash {
    display: none;
  }
  .workspace-breadcrumb {
    padding-right: 0;
  }
}
</style>
