<template>
  <div
    class="app-sidebar"
    ref="app-sidebar"
    :class="{
      collapsed: mui.frontendSettings.sidebarCollapsed
    }"
    :style="{
      '--la-sidebar-width-collapsed': as.isMacOS ? '68px' : '52px',
      '--la-sidebar-width-expanded': as.isMacOS ? '216px' : '208px',
      '--la-sidebar-icon-height': as.isMacOS ? '40px' : '36px',
      '--la-sidebar-icon-horizontal-padding': as.isMacOS ? '8px' : '8px'
    }"
  >
    <!-- macOS 的 logo 区留给交通等 -->
    <div class="app-sidebar__head-safe-area" v-if="as.isMacOS"></div>
    <div class="app-sidebar__head" v-else>
      <div class="app-sidebar__logo">
        <NIcon class="app-sidebar__logo-icon">
          <RiftFlowLogo />
        </NIcon>
      </div>
      <div class="app-sidebar__logo-text">
        {{ t('appName', { ns: 'common' }) }}
      </div>
    </div>

    <div class="app-sidebar__section-label" v-if="!mui.frontendSettings.sidebarCollapsed">
      {{ t('desktopWorkspace.workspace') }}
    </div>

    <SidebarMenu
      class="app-sidebar__menu"
      :items="menu"
      :current="currentMenu"
      @update:current="(key) => handleMenuChange(key)"
      :is-collapsed="false"
    />

    <div class="app-sidebar__padding"></div>

    <SidebarFixed class="app-sidebar__fixed" :is-collapsed="true" />

    <!-- 一个展开和缩小的按钮 -->
    <button
      type="button"
      class="app-sidebar__expand-line"
      ref="expandLineEl"
      :aria-label="
        t(
          mui.frontendSettings.sidebarCollapsed
            ? 'desktopWorkspace.expand'
            : 'desktopWorkspace.collapse'
        )
      "
      :aria-expanded="!mui.frontendSettings.sidebarCollapsed"
      @click="toggleCollapse"
    >
      <span class="app-sidebar__expand-line-inner"></span>
    </button>
  </div>
</template>

<script setup lang="tsx">
import RiftFlowLogo from '@renderer-shared/assets/icon/RiftFlowLogo.vue'
import { useInstance } from '@renderer-shared/shards'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import { WindowManagerRenderer } from '@renderer-shared/shards/window-manager'
import { ToolFilled as ToolFilledIcon } from '@vicons/antd'
import { AiStatus as AiStatusIcon } from '@vicons/carbon'
import {
  AnimalRabbit28Filled as AnimalRabbit28FilledIcon,
  WindowMultiple20Regular as WindowMultiple20RegularIcon,
  Games24Filled as Games24FilledIcon,
  Image24Regular as Image24RegularIcon
} from '@vicons/fluent'
import { AnalyticsRound as AnalyticsRoundIcon } from '@vicons/material'
import { useTranslation } from 'i18next-vue'
import { NIcon } from 'naive-ui'
import {
  Component as ComponentC,
  computed,
  nextTick,
  ref,
  useTemplateRef,
  watch,
  watchEffect
} from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useMainWindowUiStore } from '@main-window/shards/main-window-ui/store'

import { useMainWindowAppContext } from '@main-window/context'
import SidebarFixed from './SidebarFixed.vue'
import SidebarMenu from './SidebarMenu.vue'

const { t } = useTranslation()
const { openSettingsModal } = useMainWindowAppContext()

const as = useAppCommonStore()
const ogs = useOngoingGameStore()
const mui = useMainWindowUiStore()
const lcs = useLeagueClientStore()
const wm = useInstance(WindowManagerRenderer)
const sidebarEl = useTemplateRef('app-sidebar')

if (localStorage.getItem('riftflow-desktop-layout-revision') !== '2') {
  mui.frontendSettings.sidebarCollapsed = false
  localStorage.setItem('riftflow-desktop-layout-revision', '2')
}

const MAIN_WINDOW_TRAFFIC_LIGHT_DEFAULT_POSITION = {
  x: 4,
  y: 12
}
const MAIN_WINDOW_TRAFFIC_LIGHT_EXPANDED_FALLBACK_X = 8

const renderIcon = (icon: ComponentC) => {
  const Icon = icon as any
  return () => (
    <NIcon>
      <Icon />
    </NIcon>
  )
}

const router = useRouter()
const route = useRoute()

const shouldShowOngoingGameBadge = ref(false)
const isInCombatPhase = computed(() => {
  return ogs.queryStage.phase !== 'unavailable' && ogs.queryStage.phase !== 'lobby'
})

const toggleCollapse = () => {
  mui.frontendSettings.sidebarCollapsed = !mui.frontendSettings.sidebarCollapsed
}

const getSidebarPxProperty = (propertyName: string, fallback: number) => {
  if (!sidebarEl.value) {
    return fallback
  }

  const value = Number.parseFloat(getComputedStyle(sidebarEl.value).getPropertyValue(propertyName))
  return Number.isFinite(value) ? value : fallback
}

const syncMainWindowTrafficLightPosition = () => {
  if (!as.isMacOS) {
    return
  }

  if (mui.frontendSettings.sidebarCollapsed) {
    void wm.mainWindow.setTrafficLightPosition(
      MAIN_WINDOW_TRAFFIC_LIGHT_DEFAULT_POSITION.x,
      MAIN_WINDOW_TRAFFIC_LIGHT_DEFAULT_POSITION.y
    )
    return
  }

  const x = getSidebarPxProperty(
    '--la-sidebar-icon-horizontal-padding',
    MAIN_WINDOW_TRAFFIC_LIGHT_EXPANDED_FALLBACK_X
  )

  void wm.mainWindow.setTrafficLightPosition(
    Math.round(x),
    MAIN_WINDOW_TRAFFIC_LIGHT_DEFAULT_POSITION.y
  )
}

// TODO: 临时方案：折叠状态仍由前端拥有，因此先在这里同步 macOS 交通灯位置。
watch(
  () => [as.isMacOS, mui.frontendSettings.sidebarCollapsed] as const,
  () => {
    void nextTick(syncMainWindowTrafficLightPosition)
  },
  { immediate: true, flush: 'post' }
)

watch(
  () => isInCombatPhase.value,
  (yes) => {
    if (yes && currentMenu.value !== 'ongoing-game') {
      shouldShowOngoingGameBadge.value = true
    } else {
      shouldShowOngoingGameBadge.value = false
    }
  }
)

const currentMenu = ref('match-history')
const menu = computed(() => {
  return [
    {
      key: 'player-tabs',
      icon: renderIcon(AnalyticsRoundIcon),
      name: t('navigation.sidebar.menu.match-history')
    },
    {
      key: 'ongoing-game',
      icon: renderIcon(Games24FilledIcon),
      name: t('navigation.sidebar.menu.ongoing-game'),
      inProgress: shouldShowOngoingGameBadge.value,
      isDisabled: !lcs.isConnected
    },
    {
      key: 'automation',
      icon: renderIcon(AiStatusIcon),
      name: t('navigation.sidebar.menu.automation')
    },
    {
      key: 'toolkit',
      icon: renderIcon(ToolFilledIcon),
      name: t('navigation.sidebar.menu.toolkit')
    },
    {
      key: 'window-settings',
      icon: renderIcon(WindowMultiple20RegularIcon),
      name: t('settingsAccess.windows')
    },
    {
      key: 'archive',
      icon: renderIcon(AnalyticsRoundIcon),
      name: t('archiveWorkspace.navigation')
    },
    {
      key: 'skin-assets',
      icon: renderIcon(Image24RegularIcon),
      name: t('skinAssets.title')
    },
    {
      key: 'test',
      icon: renderIcon(AnimalRabbit28FilledIcon),
      name: t('navigation.sidebar.menu.test'),
      show: mui.frontendSettings.showTestPage
    }
  ]
})

const handleMenuChange = async (val: string | undefined) => {
  if (val === 'window-settings') {
    openSettingsModal('multi-window')
    return
  }
  try {
    await router.replace({ name: val })
  } catch (error) {
    console.error('routing', error)
  }
}

watchEffect(() => {
  currentMenu.value = route.name as string

  if (route.name === 'ongoing-game') {
    shouldShowOngoingGameBadge.value = false
  }
})

watch(
  () => lcs.isConnected,
  (isConnected) => {
    if (!isConnected && route.name === 'ongoing-game') {
      router.replace({ name: 'player-tabs' })
    }
  }
)
</script>

<style scoped>
.app-sidebar {
  --la-sidebar-macos-safe-top: 42px;

  display: flex;
  flex-direction: column;
  height: 100%;
  width: var(--la-sidebar-width-expanded);
  transition: width 0.3s cubic-bezier(0, 0.9, 0.1, 1);
  position: relative;

  &.collapsed {
    width: var(--la-sidebar-width-collapsed);
  }

  .app-sidebar__head-safe-area {
    height: var(--la-sidebar-macos-safe-top);
  }

  .app-sidebar__head {
    position: relative;
    display: flex;
    align-items: center;
    padding-inline: var(--la-sidebar-icon-horizontal-padding);
    padding-block: 12px 8px;
    gap: 4px;
    overflow: hidden;
  }

  .app-sidebar__logo {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: calc(var(--la-sidebar-width-collapsed) - var(--la-sidebar-icon-horizontal-padding) * 2);
    height: var(--la-sidebar-icon-height);
    font-size: 20px;
    flex-shrink: 0;
    -webkit-app-region: drag;
  }

  .app-sidebar__logo-icon {
    color: rgba(248, 63, 111);
    filter: drop-shadow(0 0 8px rgba(248, 63, 111, 0.3));
    transition:
      filter 0.2s,
      opacity 0.2s;
  }

  .app-sidebar__logo-text {
    font-size: 14px;
    font-weight: bold;
    font-family: 'Comfortaa', sans-serif;
    text-wrap-mode: nowrap;
    transition: opacity 0.2s;

    .collapsed & {
      opacity: 0;
    }
  }

  .app-sidebar__logo-toggle {
    position: absolute;
    color: rgba(0, 0, 0, 1);
    transition:
      opacity 0.2s ease,
      color 0.2s ease,
      transform 0.2s ease;
    opacity: 0;

    &:active {
      transform: scale(0.9);
    }

    [data-theme='dark'] & {
      color: rgba(255, 255, 255, 1);
    }

    [data-theme-id]:not([data-theme-id='light']):not([data-theme-id='dark']) & {
      color: color-mix(in oklch, var(--la-color-text-themed) 92%, transparent);
    }
  }

  .app-sidebar__padding {
    flex: 1;
  }

  .app-sidebar__fixed {
    margin-bottom: var(--la-sidebar-icon-horizontal-padding);
  }

  .app-sidebar__expand-line {
    position: absolute;
    right: 0;
    top: 0;
    height: 100%;
    padding: 0 4px;
    transform: translateX(50%);
    cursor: ew-resize;

    &:hover .app-sidebar__expand-line-inner {
      background-color: #f83f6f;
    }

    .app-sidebar__expand-line-inner {
      width: 2px;
      height: 100%;
      background-color: transparent;
      transition: background-color 0.4s;
    }
  }
}

/* Desktop workspace navigation keeps the native window controls and collapse behavior. */
.app-sidebar {
  background: var(--la-sidebar-bg);
  .app-sidebar__head {
    height: 66px;
  }
  .app-sidebar__logo-icon {
    color: var(--la-color-text-primary);
    filter: none;
  }
  .app-sidebar__logo-text {
    font-family: inherit;
    font-size: 15px;
    font-weight: 600;
  }
  .app-sidebar__expand-line {
    border: 0;
    background: transparent;
    color: inherit;
  }
  .app-sidebar__expand-line-inner {
    display: block;
  }
  .app-sidebar__expand-line:focus-visible {
    outline: 2px solid var(--la-color-link);
    outline-offset: -2px;
  }
  .app-sidebar__expand-line:hover .app-sidebar__expand-line-inner {
    background: var(--la-color-link);
  }
}
.app-sidebar__section-label {
  padding: 8px 20px 12px;
  font-size: 10px;
  color: var(--la-text-secondary);
}
.app-sidebar .app-sidebar__head {
  height: 76px;
  padding-block: 12px;
}
.app-sidebar .app-sidebar__logo-text {
  font-size: 17px;
  letter-spacing: -0.4px;
}

/* Primary navigation spans the workspace; connection and settings stay at the right. */
.app-sidebar,
.app-sidebar.collapsed {
  flex-direction: row;
  align-items: center;
  width: 100%;
  height: 54px;
  flex-shrink: 0;
  box-sizing: border-box;
  padding: 0 20px;
  gap: 24px;
  border-bottom: 1px solid rgb(var(--la-card-border-rgb) / 0.12);
  background: var(--la-card-surface-95);
}
.app-sidebar .app-sidebar__head {
  height: auto;
  padding: 0;
  gap: 9px;
  flex-shrink: 0;
}
.app-sidebar .app-sidebar__logo {
  width: 28px;
  height: 28px;
  margin: 0;
}
.app-sidebar .app-sidebar__logo-icon {
  font-size: 28px;
}
.app-sidebar .app-sidebar__logo-text {
  display: block;
  opacity: 1;
  width: auto;
  font-size: 13px;
  white-space: nowrap;
}
.app-sidebar .app-sidebar__section-label,
.app-sidebar .app-sidebar__expand-line {
  display: none;
}
.app-sidebar .app-sidebar__menu {
  flex: 0 1 auto;
  min-width: 0;
}
.app-sidebar .app-sidebar__padding {
  flex: 1;
}
.app-sidebar :deep(.sidebar-menu) {
  display: flex;
  flex-direction: row;
  gap: 4px;
  padding: 0;
  width: auto;
}
.app-sidebar :deep(.sidebar-menu .menu-item) {
  width: auto;
  height: 54px;
  padding: 0 12px;
  margin: 0;
  border-radius: 0;
}
.app-sidebar :deep(.sidebar-menu .menu-item__inner) {
  height: 36px;
  gap: 7px;
  padding: 0;
}
.app-sidebar :deep(.sidebar-menu .menu-item__label) {
  display: block;
  opacity: 1;
  width: auto;
  white-space: nowrap;
  font-size: 12px;
}
.app-sidebar :deep(.sidebar-menu .menu-item__icon) {
  width: 17px;
  height: 17px;
  font-size: 17px;
}
.app-sidebar :deep(.sidebar-menu .indicator-rail) {
  display: none;
}
.app-sidebar :deep(.sidebar-menu .menu-item.active) {
  background: transparent;
  box-shadow: none;
}
/* A single underline replaces the nested sidebar selection surfaces. */
.app-sidebar :deep(.sidebar-menu .menu-item .menu-item__inner) {
  background: transparent;
  border-radius: 0;
}
.app-sidebar :deep(.sidebar-menu .menu-item::after) {
  content: '';
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 0;
  height: 2px;
  border-radius: 2px 2px 0 0;
  background: transparent;
}
.app-sidebar :deep(.sidebar-menu .menu-item.active::after) {
  background: var(--la-color-link);
}
.app-sidebar :deep(.sidebar-menu .menu-item:hover:not(:disabled)) {
  background: transparent;
}
.app-sidebar :deep(.sidebar-fixed) {
  display: flex;
  flex-direction: row;
  width: auto;
  padding: 0;
  gap: 4px;
  flex-shrink: 0;
}
.app-sidebar :deep(.sidebar-fixed .menu-item) {
  width: 36px;
  height: 36px;
  margin: 0;
  padding: 0;
  flex-shrink: 0;
}
.app-sidebar :deep(.sidebar-fixed .menu-item__inner) {
  width: 36px;
  height: 36px;
  justify-content: center;
}
.app-sidebar :deep(.sidebar-fixed .menu-item__icon),
.app-sidebar :deep(.sidebar-fixed .menu-item__custom-icon) {
  width: 24px;
  height: 24px;
}
.app-sidebar :deep(.sidebar-fixed .menu-item__label) {
  display: none;
}
.app-sidebar :deep(.sidebar-fixed .settings-entry) {
  width: auto;
  min-width: 76px;
  height: 36px;
  padding: 0 10px;
  font-size: 12px;
  flex-shrink: 0;
  white-space: nowrap;
  overflow: visible;
}
@media (max-width: 1000px) {
  .app-sidebar {
    gap: 12px;
    padding: 0 12px;
  }
  .app-sidebar .app-sidebar__logo-text {
    display: none;
  }
}
</style>
