<template>
  <div class="player-profile">
    <div class="player-profile__identity">
      <div class="player-profile__avatar">
        <LcuImage
          class="player-profile__image"
          :src="summoner ? profileIconUri(summoner.profileIconId) : undefined"
        />
        <span v-if="summoner" class="player-profile__level">{{ summoner.level }}</span>
      </div>
      <StreamerModeMaskedText>
        <template #masked>
          <div class="player-profile__names">
            <div class="player-profile__name">{{ maskedName }}</div>
            <div class="player-profile__tag">{{ maskedTagLine }}</div>
          </div>
        </template>
        <div class="player-profile__names">
          <CopyableText
            class="player-profile__name"
            :text="summoner ? `${summoner.gameName}#${summoner.tagLine}` : '—'"
          >
            {{ summoner?.gameName || '—' }}
          </CopyableText>
          <div class="player-profile__tag">{{ summoner ? `#${summoner.tagLine}` : '—' }}</div>
        </div>
      </StreamerModeMaskedText>
    </div>
    <div class="player-profile__ranked"><RankedPane /></div>

    <!-- buttons -->
    <div class="player-profile__actions">
      <!-- tag edit -->
      <NPopover
        v-if="!isSelfTab && !isCrossRegion"
        v-model:show="isTagEditPopoverShowing"
        trigger="click"
        placement="bottom-end"
      >
        <template #trigger>
          <NButton secondary class="size-8!" :title="t('playerTabs.profile.tagPlayer')">
            <template #icon>
              <NIcon><Edit20Filled /></NIcon>
            </template>
          </NButton>
        </template>

        <PlayerTagEditPanel
          :puuid="puuid"
          :summoner="summoner"
          @cancel="isTagEditPopoverShowing = false"
          @saved="handleTagSaved"
        />
      </NPopover>

      <!-- refresh -->
      <NButton
        secondary
        class="size-8!"
        :title="t('playerTabs.profile.refreshPage')"
        :loading="isSomethingLoading"
        @click="refresh"
      >
        <template #icon>
          <NIcon><RefreshSharp /></NIcon>
        </template>
      </NButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import CopyableText from '@renderer-shared/components/CopyableText.vue'
import LcuImage from '@renderer-shared/components/LcuImage.vue'
import { PlayerTagEditPanel } from '@renderer-shared/components/player-tag-edit'
import StreamerModeMaskedText from '@renderer-shared/components/StreamerModeMaskedText.vue'
import { useStreamerModeMaskedText } from '@renderer-shared/composables/useStreamerModeMaskedText'
import { profileIconUri } from '@renderer-shared/shards/league-client/game-data-assets'
import { Edit20Filled } from '@vicons/fluent'
import { RefreshSharp } from '@vicons/ionicons5'
import { NButton, NIcon, NPopover } from 'naive-ui'
import { useTranslation } from 'i18next-vue'
import { computed, ref } from 'vue'

import { usePlayerTab } from '../context'
import { useSummoner } from '../data/summoner'
import { useTags } from '../data/tags'
import { useRefresh } from '../utils/refresh'
import RankedPane from './RankedPane.vue'

const { t } = useTranslation()
const { puuid, isSelfTab, isCrossRegion } = usePlayerTab()
const { summoner } = useSummoner()
const { loadTags } = useTags()

const { masked, summonerName: streamerSummonerName } = useStreamerModeMaskedText()

const { refresh, isSomethingLoading } = useRefresh()

const isTagEditPopoverShowing = ref(false)

const maskedName = computed(() => {
  const seed = summoner.value?.gameName || summoner.value?.puuid || puuid.value
  return streamerSummonerName(seed, 0)
})

const maskedTagLine = computed(() => masked(summoner.value?.tagLine || '—', '#####'))

const handleTagSaved = async () => {
  isTagEditPopoverShowing.value = false
  await loadTags()
}
</script>

<style scoped>
.player-profile {
  display: flex;
  align-items: center;
  gap: 24px;
  flex-wrap: wrap;
}
.player-profile__identity {
  display: flex;
  align-items: center;
  gap: 15px;
  flex: 1 1 200px;
  min-width: 0;
}
.player-profile__avatar {
  position: relative;
  width: 57px;
  height: 57px;
  flex-shrink: 0;
}
.player-profile__image {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: 11px;
  border: 1px solid rgb(var(--la-card-border-rgb) / 0.16);
  box-sizing: border-box;
}
.player-profile__level {
  position: absolute;
  bottom: -5px;
  left: 50%;
  transform: translateX(-50%);
  border: 1px solid rgb(var(--la-card-border-rgb) / 0.18);
  border-radius: 4px;
  background: var(--la-card-surface-95);
  padding: 0 6px;
  font-size: 9px;
  line-height: 15px;
  color: var(--la-text-secondary);
}
.player-profile__names {
  min-width: 0;
}
.player-profile__name {
  display: block;
  max-width: 100%;
  overflow-wrap: anywhere;
  color: var(--la-color-text-primary);
  font-size: 23px;
  font-weight: 600;
  line-height: 1.3;
  letter-spacing: -0.5px;
}
.player-profile__tag {
  margin-top: 5px;
  font-size: 11px;
  color: var(--la-text-secondary);
}
.player-profile__ranked {
  margin-left: auto;
}
.player-profile__actions {
  display: flex;
  flex-shrink: 0;
  gap: 6px;
}
@media (max-width: 1000px) {
  .player-profile {
    gap: 18px;
  }
  .player-profile__identity {
    flex-basis: calc(100% - 110px);
  }
  .player-profile__actions {
    order: 1;
    margin-left: auto;
  }
  .player-profile__ranked {
    order: 2;
    flex-basis: 100%;
    margin-left: 0;
  }
}
</style>
