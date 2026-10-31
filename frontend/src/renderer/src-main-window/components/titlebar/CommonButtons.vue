<template>
  <div class="common-buttons" :class="{ blurred: mws.focus === 'blurred' }">
    <NTooltip :z-index="TITLE_BAR_TOOLTIP_Z_INDEX">
      <template #trigger>
        <div
          class="common-button-outer ai-assistant-trigger"
          role="button"
          tabindex="0"
          :aria-label="t('aiAssistant.title')"
          @click="openAiAssistant"
          @keydown.enter.prevent="openAiAssistant"
          @keydown.space.prevent="openAiAssistant"
        >
          <div class="common-button-inner">
            <NIcon><SparklesOutline /></NIcon>
          </div>
        </div>
      </template>
      {{ t('aiAssistant.title') }}
    </NTooltip>
    <NTooltip
      v-if="updateState.releaseInfo?.isNew || updateState.updateProgressInfo"
      :z-index="TITLE_BAR_TOOLTIP_Z_INDEX"
    >
      <template #trigger>
        <div
          class="common-button-outer"
          role="button"
          tabindex="0"
          aria-label="软件更新"
          @click="sn.showNewReleaseModal()"
          @keydown.enter.prevent="sn.showNewReleaseModal()"
          @keydown.space.prevent="sn.showNewReleaseModal()"
        >
          <NBadge dot
            ><div class="common-button-inner">
              <NIcon><ArrowUpCircleOutline /></NIcon></div
          ></NBadge>
        </div>
      </template>
      {{
        updateState.updateProgressInfo?.phase === 'waiting-for-restart'
          ? '更新已就绪，点击重启'
          : '发现新版本，点击查看'
      }}
    </NTooltip>
    <!-- notice -->
    <NPopover
      :z-index="TITLE_BAR_TOOLTIP_Z_INDEX"
      :show="shouldShowNoticeTooltip || noticeTooltipShow"
      @update:show="(v) => (noticeTooltipShow = v)"
    >
      <template #trigger>
        <div class="common-button-outer" @click="sn.showNoticeModal()">
          <NBadge dot :show="shouldShowNoticeBadge" :offset="[-4, 4]">
            <div class="common-button-inner">
              <NIcon><Notification /></NIcon>
            </div>
          </NBadge>
        </div>
      </template>

      <template v-if="sns.noticeSummary">
        <div>
          <div class="font-bold text-black/80 dark:text-white/80">
            {{ t('titlebar.actions.noticeSummary') }}
          </div>
          <div class="my-2 h-px bg-black/10 dark:bg-white/10"></div>
          <div class="mb-2 max-w-100 text-xs text-black/80 dark:text-white/80">
            {{ sns.noticeSummary }}
          </div>
          <div class="flex justify-end gap-2">
            <NButton
              size="tiny"
              @click="setRead()"
              v-if="
                aks.notice?.severity !== 'low' && sns.lastNoticeRevision !== aks.notice?.revision
              "
              >{{ t('titlebar.actions.noticeOk') }}</NButton
            >
            <NButton size="tiny" type="primary" @click="sn.showNoticeModal()">{{
              t('titlebar.actions.noticeSeeMore')
            }}</NButton>
          </div>
        </div>
      </template>
      <template v-else>
        {{ t('titlebar.actions.notice') }}
      </template>
    </NPopover>

    <!-- github -->
    <NTooltip v-if="RIFTFLOW_GITHUB" :z-index="TITLE_BAR_TOOLTIP_Z_INDEX">
      <template #trigger>
        <div class="common-button-outer" @click="handleToGithub">
          <div class="common-button-inner">
            <NIcon><LogoGithub /></NIcon>
          </div>
        </div>
      </template>
      {{ t('titlebar.actions.github') }}
    </NTooltip>

    <!-- aux window -->
    <HorizontalExpand :show="aws.settings.enabled" class="h-full">
      <NTooltip :z-index="TITLE_BAR_TOOLTIP_Z_INDEX">
        <template #trigger>
          <div class="common-button-outer" @click="handleShowAuxWindow">
            <div class="common-button-inner">
              <NIcon><Window24Filled /></NIcon>
            </div>
          </div>
        </template>
        {{ t('titlebar.actions.auxWindow') }}
      </NTooltip>
    </HorizontalExpand>

    <!-- op.gg -->
    <HorizontalExpand :show="ows.settings.enabled" class="h-full">
      <NTooltip :z-index="TITLE_BAR_TOOLTIP_Z_INDEX">
        <template #trigger>
          <div class="common-button-outer" @click="handleShowOpggWindow">
            <OpggIcon class="common-button-inner common-button-inner-img" />
          </div>
        </template>
        {{ t('titlebar.actions.opggWindow') }}
      </NTooltip>
    </HorizontalExpand>

    <!-- theme selector -->
    <NDropdown
      trigger="click"
      placement="bottom-end"
      size="small"
      :width="180"
      :z-index="TITLE_BAR_TOOLTIP_Z_INDEX"
      :options="themeDropdownOptions"
      :theme-overrides="{ fontSizeSmall: '12px', optionHeightSmall: '28px' }"
      @select="handleThemeSelect"
    >
      <div class="common-button-outer">
        <div class="common-button-inner">
          <NIcon><ColorPaletteOutline /></NIcon>
        </div>
      </div>
    </NDropdown>

    <!-- tasks -->
    <HorizontalExpand :show="bts.tasks.length !== 0" class="h-full">
      <NPopover placement="bottom-end" :z-index="TITLE_BAR_TOOLTIP_Z_INDEX" raw>
        <template #trigger>
          <div class="common-button-outer">
            <SpinningIcon
              :spinning="overallProgress !== 1"
              :count="bts.tasks.length"
              :progress="overallProgress"
              :class="{ 'all-finished': overallProgress === 1 }"
              class="common-button-inner common-button-inner-img"
            />
          </div>
        </template>
        <BackgroundTasks />
      </NPopover>
    </HorizontalExpand>
  </div>
</template>

<script setup lang="ts">
import OpggIcon from '@renderer-shared/assets/icon/OpggIcon.vue'
import SpinningIcon from '@renderer-shared/assets/icon/SpinningIcon.vue'
import HorizontalExpand from '@renderer-shared/components/HorizontalExpand.vue'
import { useInstance } from '@renderer-shared/shards'
import { useRiftFlowApiStore } from '@renderer-shared/shards/riftflow-api/store'
import { AppCommonRenderer } from '@renderer-shared/shards/app-common'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { useBackgroundTasksStore } from '@renderer-shared/shards/background-tasks/store'
import { WindowManagerRenderer } from '@renderer-shared/shards/window-manager'
import {
  useAuxWindowStore,
  useMainWindowStore,
  useOpggWindowStore
} from '@renderer-shared/shards/window-manager/store'
import { RIFTFLOW_GITHUB } from '@shared/constants/common'
import {
  AppThemeId,
  AppThemeSetting,
  BUILTIN_DARK_THEME_IDS,
  BUILTIN_LIGHT_THEME_IDS,
  getThemeColorTheme,
  isAppThemeSetting
} from '@shared/types/app-theme'
import { Checkmark, Notification } from '@vicons/carbon'
import { Window24Filled } from '@vicons/fluent'
import {
  ArrowUpCircleOutline,
  ColorPaletteOutline,
  LogoGithub,
  SparklesOutline
} from '@vicons/ionicons5'
import { useTranslation } from 'i18next-vue'
import { NBadge, NButton, NDropdown, NIcon, NPopover, NTag, NTooltip } from 'naive-ui'
import { DropdownMixedOption } from 'naive-ui/es/dropdown/src/interface'
import { computed, h, ref } from 'vue'

import { SimpleNotificationsRenderer } from '@main-window/shards/simple-notifications'
import { useSimpleNotificationsStore } from '@main-window/shards/simple-notifications/store'

import { useSelfUpdateStore } from '@renderer-shared/shards/self-update/store'
import BackgroundTasks from '../BackgroundTasks.vue'
import { useMainWindowAppContext } from '@main-window/context'

const { t } = useTranslation()
const { openAiAssistant } = useMainWindowAppContext()

const mws = useMainWindowStore()
const aws = useAuxWindowStore()
const ows = useOpggWindowStore()
const aks = useRiftFlowApiStore()
const sns = useSimpleNotificationsStore()
const updateState = useSelfUpdateStore()
const as = useAppCommonStore()
const wm = useInstance(WindowManagerRenderer)
const sn = useInstance(SimpleNotificationsRenderer)
const app = useInstance(AppCommonRenderer)

const bts = useBackgroundTasksStore()

const overallProgress = computed(() => {
  let total = 0

  for (const task of bts.tasks) {
    if (task.progress !== null) {
      total += task.progress
    } else {
      total += 1
    }
  }

  return total / bts.tasks.length
})

const TITLE_BAR_TOOLTIP_Z_INDEX = 75000

const handleShowAuxWindow = () => {
  wm.auxWindow.show()
}

const handleShowOpggWindow = () => {
  wm.opggWindow.show()
}

const handleToGithub = () => {
  window.open(RIFTFLOW_GITHUB, '_blank')
}

const themeToneLabel = (id: AppThemeId) => {
  const colorTheme = getThemeColorTheme(id)
  return t(`titlebar.actions.themeSelector.tone.${colorTheme}`)
}

const selectedThemeIcon = (key: AppThemeSetting) => {
  if (activeThemePresetKey.value !== key) {
    return undefined
  }

  return () =>
    h(NIcon, null, {
      default: () => h(Checkmark)
    })
}

const activeThemePresetKey = computed(() => {
  if (isAppThemeSetting(as.settings.theme)) {
    return as.settings.theme
  }

  return 'dark'
})

const themeToneTag = (id: AppThemeId) => {
  const colorTheme = getThemeColorTheme(id)

  return () =>
    h(
      NTag,
      {
        bordered: false,
        round: true,
        size: 'tiny',
        type: colorTheme === 'light' ? 'success' : 'info'
      },
      {
        default: () => themeToneLabel(id)
      }
    )
}

const createThemeDropdownOption = (id: AppThemeId): DropdownMixedOption => {
  return {
    key: id,
    label: t(`titlebar.actions.themeSelector.presets.${id}`, { defaultValue: id }),
    icon: selectedThemeIcon(id),
    extra: themeToneTag(id)
  }
}

const themeDropdownOptions = computed<DropdownMixedOption[]>(() => {
  return [
    {
      type: 'group',
      key: 'system',
      label: t('titlebar.actions.themeSelector.groups.system'),
      children: [
        {
          key: 'default',
          label: t('titlebar.actions.themeSelector.presets.followSystem'),
          icon: selectedThemeIcon('default')
        }
      ]
    },
    {
      type: 'group',
      key: 'bright-core',
      label: t('titlebar.actions.themeSelector.groups.brightBuiltin'),
      children: BUILTIN_LIGHT_THEME_IDS.map(createThemeDropdownOption)
    },
    {
      type: 'group',
      key: 'dark-core',
      label: t('titlebar.actions.themeSelector.groups.darkBuiltin'),
      children: BUILTIN_DARK_THEME_IDS.map(createThemeDropdownOption)
    }
  ]
})

const handleThemeSelect = (key: string | number) => {
  if (isAppThemeSetting(key)) {
    app.setTheme(key)
  }
}

const shouldShowNoticeBadge = computed(() => {
  return (
    aks.notice !== null &&
    (aks.notice.severity === 'medium' || aks.notice.severity === 'high') &&
    sns.lastNoticeRevision !== aks.notice.revision
  )
})

const noticeTooltipShow = ref(false)
const shouldShowNoticeTooltip = computed(() => {
  return (
    aks.notice !== null &&
    aks.notice.summary &&
    aks.notice.severity === 'medium' &&
    sns.lastNoticeRevision !== aks.notice.revision
  )
})

const setRead = () => {
  sns.lastNoticeRevision = aks.notice?.revision ?? null
}
</script>

<style scoped>
@reference '@renderer-shared/assets/css/tailwind.css';

.common-buttons {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: space-around;

  &.blurred {
    filter: brightness(0.8);
  }

  .common-button-outer {
    display: flex;
    justify-content: center;
    align-items: center;
    width: 32px;
    height: 100%;
    cursor: pointer;
    -webkit-app-region: no-drag;
  }

  .common-button-inner {
    padding: 4px;
    border-radius: 2px;
    transition:
      background-color 0.3s,
      color 0.3s;
    font-size: 16px;

    i {
      display: block;
    }
  }

  .common-button-inner-img {
    width: 16px;
    height: 16px;
  }
}

[data-theme='dark'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(255, 255, 255, 0.15);
      color: rgba(255, 255, 255, 1);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(255, 255, 255, 0.1);
    }

    .common-button-inner {
      color: rgba(255, 255, 255, 0.86);
    }
  }
}

[data-theme='light'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(0, 0, 0, 0.15);
      color: rgba(0, 0, 0, 1);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(0, 0, 0, 0.1);
    }

    .common-button-inner {
      color: rgba(0, 0, 0, 0.86);
    }
  }
}

[data-theme-id='dark'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(255, 255, 255, 0.15);
      color: rgba(255, 255, 255, 1);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(255, 255, 255, 0.1);
    }

    .common-button-inner {
      color: rgba(255, 255, 255, 0.86);
    }
  }
}

[data-theme-id]:not([data-theme-id='light']):not([data-theme-id='dark']) {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: color-mix(in oklch, var(--la-color-link) 18%, transparent);
      color: var(--la-color-text-themed);
    }

    .common-button-outer:active .common-button-inner {
      background-color: color-mix(in oklch, var(--la-color-link) 12%, transparent);
    }

    .common-button-inner {
      color: color-mix(in oklch, var(--la-color-text-themed) 92%, transparent);
    }
  }
}

[data-theme-id='graphite'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(105, 202, 255, 0.2);
      color: #e2edf7;
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(105, 202, 255, 0.14);
    }

    .common-button-inner {
      color: rgba(226, 237, 247, 0.9);
    }
  }
}

[data-theme-id='sakura'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(212, 90, 134, 0.18);
      color: rgba(49, 44, 52, 0.95);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(212, 90, 134, 0.12);
    }

    .common-button-inner {
      color: rgba(49, 44, 52, 0.86);
    }
  }
}

[data-theme-id='mint'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(36, 136, 115, 0.16);
      color: rgba(33, 45, 42, 0.95);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(36, 136, 115, 0.12);
    }

    .common-button-inner {
      color: rgba(33, 45, 42, 0.88);
    }
  }
}

[data-theme-id='aurora'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(193, 168, 255, 0.2);
      color: #f3f2f8;
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(193, 168, 255, 0.14);
    }

    .common-button-inner {
      color: rgba(243, 242, 248, 0.88);
    }
  }
}

[data-theme-id='butter'] {
  .common-buttons {
    .common-button-outer:hover .common-button-inner {
      background-color: rgba(194, 127, 34, 0.16);
      color: rgba(66, 57, 48, 0.95);
    }

    .common-button-outer:active .common-button-inner {
      background-color: rgba(194, 127, 34, 0.12);
    }

    .common-button-inner {
      color: rgba(66, 57, 48, 0.88);
    }
  }
}

@layer components {
  .all-finished {
    @apply text-green-700! dark:text-green-300!;
  }
}
</style>
