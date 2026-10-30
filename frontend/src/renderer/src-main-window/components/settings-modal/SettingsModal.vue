<template>
  <NModal
    transform-origin="center"
    size="small"
    preset="card"
    v-model:show="show"
    class="settings-workspace"
  >
    <template #header>
      <div class="settings-window-title">
        <NIcon :component="Settings16FilledIcon" />
        <span>{{ t('settings.modal.title') }}</span>
      </div>
    </template>
    <div class="settings-shell">
      <nav class="settings-navigation" :aria-label="t('settings.modal.title')">
        <NScrollbar class="settings-navigation-scroll">
          <div class="settings-navigation-items">
            <div class="settings-access-caption">{{ t('settingsAccess.shortcuts') }}</div>
            <NButton
              quaternary
              block
              class="settings-shortcut"
              @click="
                openTarget({ section: 'multi-window', key: 'settings.multiWindow.auxWindow.title' })
              "
              >{{ t('settingsAccess.windows') }} ↗</NButton
            >
            <NButton quaternary block class="settings-shortcut" @click="openAutomation"
              >{{ t('navigation.sidebar.menu.automation') }} ↗</NButton
            >
            <div class="settings-access-caption">{{ t('settingsAccess.allSettings') }}</div>
            <NButton
              v-for="(section, index) in sections"
              :key="section.key"
              quaternary
              block
              class="settings-navigation-item"
              :class="{ 'is-active': tabName === section.key }"
              :aria-current="tabName === section.key ? 'page' : undefined"
              @click="tabName = section.key"
            >
              <div class="settings-navigation-label">
                <NIcon :component="section.icon" />
                <span>{{ section.title }}</span>
                <span class="settings-navigation-number">{{
                  String(index + 1).padStart(2, '0')
                }}</span>
              </div>
            </NButton>
          </div>
        </NScrollbar>
      </nav>
      <section class="settings-content" :aria-label="activeSection.title">
        <div class="settings-search-bar">
          <NInput
            v-model:value="searchQuery"
            clearable
            :placeholder="t('settingsAccess.searchPlaceholder')"
            :aria-label="t('settingsAccess.searchPlaceholder')"
          />
        </div>
        <NScrollbar v-if="searchQuery.trim()" class="settings-search-results">
          <div class="settings-search-summary">
            {{ t('settingsAccess.results', { count: searchResults.length }) }}
          </div>
          <NButton
            v-for="result in searchResults"
            :key="result.section + result.key"
            quaternary
            block
            class="settings-search-result"
            @click="openTarget(result)"
          >
            <span class="settings-search-result-label">{{ result.label }}</span>
            <span class="settings-search-result-section">{{ result.sectionTitle }} ↗</span>
          </NButton>
          <div v-if="!searchResults.length" class="settings-search-summary">
            {{ t('settingsAccess.noResults') }}
          </div>
        </NScrollbar>
        <header v-show="!searchQuery.trim()" class="settings-content-heading">
          <NIcon :component="activeSection.icon" />
          <h2>{{ activeSection.title }}</h2>
        </header>
        <div v-show="!searchQuery.trim()" class="settings-content-body">
          <KeepAlive>
            <component :is="activeSection.component" :key="activeSection.key" />
          </KeepAlive>
        </div>
      </section>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { ToolFilled as ToolFilledIcon } from '@vicons/antd'
import { Debug as DebugIcon, Layers as LayersIcon } from '@vicons/carbon'
import {
  Games24Filled as Games24FilledIcon,
  Settings16Filled as Settings16FilledIcon,
  Storage24Filled as Storage24FilledIcon,
  WindowMultiple20Regular as WindowMultiple20RegularIcon
} from '@vicons/fluent'
import { InfoSharp as InfoSharpIcon } from '@vicons/material'
import { useTranslation } from 'i18next-vue'
import { NButton, NIcon, NInput, NModal, NScrollbar } from 'naive-ui'
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { settingsSearchIndex } from './settings-search-index'

import AboutPane from './AboutPane.vue'
import AppSettings from './AppSettings.vue'
import DebugSettings from './DebugSettings.vue'
import MatchHistorySettings from './MatchHistorySettings.vue'
import MiscSettings from './MiscSettings.vue'
import MultiWindowSettings from './MultiWindowSettings.vue'
import OngoingGameSettings from './OngoingGameSettings.vue'
import StorageSettings from './storage-settings/StorageSettings.vue'

const { t } = useTranslation()
const router = useRouter()
const searchQuery = ref('')
const show = defineModel<boolean>('show', { default: false })
const tabName = defineModel<string>('tabName', { default: 'basic' })

const sections = computed(() => [
  {
    key: 'basic',
    title: t('settings.app.title'),
    icon: Settings16FilledIcon,
    component: AppSettings
  },
  {
    key: 'player-tabs',
    title: t('settings.matchHistory.title'),
    icon: LayersIcon,
    component: MatchHistorySettings
  },
  {
    key: 'ongoing-game',
    title: t('settings.ongoingGame.title'),
    icon: Games24FilledIcon,
    component: OngoingGameSettings
  },
  {
    key: 'multi-window',
    title: t('settings.multiWindow.title'),
    icon: WindowMultiple20RegularIcon,
    component: MultiWindowSettings
  },
  {
    key: 'storage',
    title: t('settings.storage.title'),
    icon: Storage24FilledIcon,
    component: StorageSettings
  },
  { key: 'misc', title: t('settings.misc.title'), icon: ToolFilledIcon, component: MiscSettings },
  { key: 'debug', title: t('settings.debug.title'), icon: DebugIcon, component: DebugSettings },
  { key: 'about', title: t('settings.about.title'), icon: InfoSharpIcon, component: AboutPane }
])
const activeSection = computed(
  () => sections.value.find((section) => section.key === tabName.value) ?? sections.value[0]
)

const searchResults = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase()
  if (!query) return []
  const targets = [
    ...sections.value.map((s) => ({ section: s.key, key: '', label: s.title, groupKey: '' })),
    ...settingsSearchIndex.map((s) => ({ ...s, label: t(s.key) }))
  ]
  return targets
    .map((target) => ({
      ...target,
      sectionTitle:
        target.section === 'automation'
          ? t('navigation.sidebar.menu.automation')
          : [
              target.section === 'multi-window' ? t('settingsAccess.windows') : '',
              sections.value.find((s) => s.key === target.section)?.title,
              'groupKey' in target && target.groupKey ? t(target.groupKey) : ''
            ]
              .filter(Boolean)
              .join(' / ')
    }))
    .filter((target) =>
      `${target.label} ${target.sectionTitle}`.toLocaleLowerCase().includes(query)
    )
})

const openAutomation = async () => {
  show.value = false
  await router.replace('/automation/auto-gameflow')
}

const openTarget = async (target: {
  section: string
  key: string
  route?: string
  groupKey?: string
}) => {
  const label = target.key ? t(target.key) : ''
  if (target.route) {
    show.value = false
    await router.replace(target.route)
  } else {
    tabName.value = target.section
  }
  searchQuery.value = ''
  await nextTick()
  // Wait for the route transition before locating an automation section.
  const locate = (remaining: number) => {
    let scope = target.route
      ? document.querySelector('.app-frame__right-content')
      : document.querySelector('.settings-content-body')
    if (scope && target.groupKey) {
      const groupLabel = t(target.groupKey)
      const heading = [...scope.querySelectorAll('.settings-section-title')].find(
        (el) => el.textContent?.trim() === groupLabel.trim()
      )
      scope = heading?.closest('.settings-section') ?? scope
    }
    const labels = [
      ...(scope?.querySelectorAll<HTMLElement>(
        '.settings-row-label-main, .settings-section-title'
      ) ?? [])
    ]
    const match = label ? labels.find((el) => el.textContent?.trim() === label.trim()) : null
    if (!match && label && remaining > 0) {
      window.setTimeout(() => locate(remaining - 1), 80)
      return
    }
    const element = match?.closest<HTMLElement>('.settings-row, .settings-section') ?? match
    if (!element) return
    document
      .querySelectorAll('.settings-search-target')
      .forEach((el) => el.classList.remove('settings-search-target'))
    element.classList.add('settings-search-target')
    element.scrollIntoView({ block: 'center', behavior: 'smooth' })
    window.setTimeout(() => element.classList.remove('settings-search-target'), 2500)
  }
  locate(5)
}
watch(show, () => {
  searchQuery.value = ''
})
</script>

<style src="./settings-workspace.css"></style>
