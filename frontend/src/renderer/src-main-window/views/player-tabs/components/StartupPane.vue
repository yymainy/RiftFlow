<template>
  <NScrollbar class="startup-scroll">
    <div class="startup-workspace">
      <div class="startup-caption">
        <span>{{ t('appName', { ns: 'common' }) }}</span>
        <span>v{{ as.version }}</span>
      </div>
      <header class="startup-heading">
        <div>
          <div class="startup-heading__title">{{ t('navigation.sidebar.menu.match-history') }}</div>
          <p>{{ t('desktopWorkspace.startupDescription') }}</p>
        </div>
        <div class="startup-heading__actions">
          <span
            class="connection-state"
            :class="{ 'connection-state--active': lcs.isConnected }"
            role="status"
          >
            <i aria-hidden="true"></i>
            {{
              t(
                lcs.isConnected
                  ? 'desktopWorkspace.connected'
                  : lcs.connectingClient
                    ? 'desktopWorkspace.connecting'
                    : 'desktopWorkspace.notConnected'
              )
            }}
          </span>
          <NButton v-if="showNewVersionBadge" size="small" secondary @click="handleShowUpdateModal">
            {{ t('startup.launcher.newVersionAvailable') }}
          </NButton>
        </div>
      </header>

      <div class="startup-sections">
        <section class="startup-panel">
          <div class="startup-panel__heading">
            <NIcon><PlugConnected24Filled /></NIcon>
            <span>{{ t('desktopWorkspace.clients') }}</span>
          </div>
          <p class="startup-panel__description">{{ t('desktopWorkspace.clientsDescription') }}</p>
          <div class="startup-entries">
            <button
              v-if="lcs.summoner.me"
              type="button"
              class="startup-entry"
              @click="handleOpenSelfTab"
            >
              <LcuImage
                :src="profileIconUri(lcs.summoner.me.profileIconId)"
                class="startup-entry__image"
              />
              <span class="startup-entry__content">
                <span class="startup-entry__name">{{ lcs.summoner.me.gameName }}</span>
                <span class="startup-entry__detail"
                  >#{{ lcs.summoner.me.tagLine }} ·
                  {{ t(`sgpServers.${sgps.availability.sgpServerId}`, { ns: 'common' }) }}</span
                >
              </span>
              <NIcon class="startup-entry__arrow"><ChevronRight20Filled /></NIcon>
            </button>
            <template v-if="!lcs.isConnected">
              <button
                v-for="(client, index) of otherClients"
                :key="client.pid"
                type="button"
                class="startup-entry"
                @click="handleConnect(client)"
              >
                <LcuImage
                  :src="lcps.connectableClientExtraInfo[client.pid]?.profileIcon"
                  class="startup-entry__image"
                />
                <span class="startup-entry__content">
                  <span class="startup-entry__name">{{ otherClientName(client, index) }}</span>
                  <span class="startup-entry__detail"
                    >{{ otherClientTagLine(client) }} ·
                    {{
                      t(`sgpServers.${getSgpServerId(client.region, client.rsoPlatformId)}`, {
                        ns: 'common'
                      })
                    }}
                    · {{ client.pid }}</span
                  >
                </span>
                <NSpin v-if="lcs.connectingClient?.pid === client.pid" :size="14" />
                <NIcon v-else class="startup-entry__arrow"><PlugConnected24Filled /></NIcon>
              </button>
            </template>
            <div
              v-if="!lcs.summoner.me && (lcs.isConnected || !otherClients.length)"
              class="startup-empty"
            >
              <NIcon class="startup-empty__icon"><PlugConnected24Filled /></NIcon>
              <span>{{
                t(lcs.isConnected ? 'playerTabs.profile.loading' : 'desktopWorkspace.noClients')
              }}</span>
            </div>
          </div>
        </section>

        <section class="startup-panel">
          <div class="startup-panel__heading">
            <NIcon><RiftFlowLogo /></NIcon>
            <span>{{ t('desktopWorkspace.launchers') }}</span>
          </div>
          <p class="startup-panel__description">{{ t('desktopWorkspace.launchersDescription') }}</p>
          <div class="startup-entries">
            <button
              v-for="item of launchItems"
              :key="item.path"
              type="button"
              class="startup-entry"
              @click="item.launch"
            >
              <img :src="item.imgUrl" class="startup-entry__image" alt="" />
              <span class="startup-entry__content">
                <span class="startup-entry__name">{{ item.name }}</span>
                <NEllipsis class="startup-entry__detail" :tooltip="{ placement: 'bottom' }">
                  <template #tooltip
                    ><span class="font-mono text-xs">{{ item.path }}</span></template
                  >
                  {{ item.path }}
                </NEllipsis>
              </span>
              <NIcon class="startup-entry__arrow"><ChevronRight20Filled /></NIcon>
            </button>
            <div v-if="!launchItems.length" class="startup-empty">
              <NIcon class="startup-empty__icon"><RiftFlowLogo /></NIcon>
              <span>{{ t('desktopWorkspace.noLaunchers') }}</span>
            </div>
          </div>
        </section>
      </div>
    </div>
  </NScrollbar>
</template>

<script setup lang="ts">
import leagueIco from '@renderer-shared/assets/ico/league.ico'
import riotClient from '@renderer-shared/assets/ico/riotclient.ico'
import weGameIco from '@renderer-shared/assets/ico/wegame.ico'
import RiftFlowLogo from '@renderer-shared/assets/icon/RiftFlowLogo.vue'
import LcuImage from '@renderer-shared/components/LcuImage.vue'
import { useStreamerModeMaskedText } from '@renderer-shared/composables/useStreamerModeMaskedText'
import { useInstance } from '@renderer-shared/shards'
import { useAppCommonStore } from '@renderer-shared/shards/app-common/store'
import { ClientInstallationRenderer } from '@renderer-shared/shards/client-installation'
import { useClientInstallationStore } from '@renderer-shared/shards/client-installation/store'
import { LeagueClientRenderer } from '@renderer-shared/shards/league-client'
import { useLeagueClientUxStore } from '@renderer-shared/shards/league-client-ux/store'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { profileIconUri } from '@renderer-shared/shards/league-client/game-data-assets'
import { useSelfUpdateStore } from '@renderer-shared/shards/self-update/store'
import { useSgpStore } from '@renderer-shared/shards/sgp/store'
import { UxCommandLine } from '@shared/shards/league-client-ux'
import { getSgpServerId } from '@shared/utils/sgp'
import { ChevronRight20Filled, PlugConnected24Filled } from '@vicons/fluent'
import { useTranslation } from 'i18next-vue'
import { NButton, NEllipsis, NIcon, NScrollbar, NSpin, useMessage } from 'naive-ui'
import { computed } from 'vue'

import { useLeagueClientPeekStore } from '@main-window/shards/league-client-peek/store'
import { PlayerTabsRenderer } from '@main-window/shards/player-tabs'
import { SimpleNotificationsRenderer } from '@main-window/shards/simple-notifications'

const { t } = useTranslation()

const as = useAppCommonStore()
const cis = useClientInstallationStore()
const lcs = useLeagueClientStore()
const lcuxs = useLeagueClientUxStore()
const lcps = useLeagueClientPeekStore()
const sgps = useSgpStore()

const sus = useSelfUpdateStore()

const lc = useInstance(LeagueClientRenderer)
const ci = useInstance(ClientInstallationRenderer)
const pt = useInstance(PlayerTabsRenderer)
const sn = useInstance(SimpleNotificationsRenderer)

const message = useMessage()
const { masked, summonerName: streamerSummonerName } = useStreamerModeMaskedText()

const { navigateToTabByPuuid } = pt.useNavigateToTab()

const showNewVersionBadge = computed(() => {
  const release = sus.releaseInfo
  if (!release?.isNew || !release.isUpdateSupported) {
    return false
  }

  if (sus.settings.ignoreVersion === release.version) {
    return false
  }

  return true
})

const handleShowUpdateModal = () => {
  sn.showNewReleaseModal()
}

const handleOpenSelfTab = () => {
  if (lcs.summoner.me) {
    navigateToTabByPuuid(lcs.summoner.me.puuid)
  }
}

const launch = async (fn: () => Promise<any>, name: string) => {
  try {
    await fn()
    message.success(t('startup.launcher.successMessage', { name }))
  } catch (error) {
    message.error(t('startup.launcher.failedMessage', { name, reason: (error as any).message }))
  }
}

const launchItems = computed(() => {
  const arr: {
    name: string
    imgUrl: string
    path: string
    launch: () => any | Promise<any>
  }[] = []

  if (cis.tclsExecutablePath) {
    arr.push({
      name: t('startup.launcher.tcls'),
      imgUrl: leagueIco,
      path: cis.tclsExecutablePath,
      launch: () => launch(ci.launchTencentTcls.bind(ci), t('startup.launcher.tcls'))
    })
  }

  if (cis.weGameLauncherExecutablePath) {
    arr.push({
      name: t('startup.launcher.weGame'),
      imgUrl: weGameIco,
      path: cis.weGameLauncherExecutablePath,
      launch: () => launch(ci.launchWeGameLeagueOfLegends.bind(ci), t('startup.launcher.weGame'))
    })
  }

  if (cis.officialRiotClientExecutablePath) {
    arr.push({
      name: t('startup.launcher.riotClient'),
      imgUrl: riotClient,
      path: cis.officialRiotClientExecutablePath,
      launch: () => launch(ci.launchDefaultRiotClient.bind(ci), t('startup.launcher.riotClient'))
    })
  }

  return arr
})

const otherClients = computed(() => {
  return lcuxs.launchedClients.filter((c) => c.pid !== lcs.auth?.pid)
})

const otherClientName = (client: UxCommandLine, index: number) => {
  const summoner = lcps.connectableClientExtraInfo[client.pid]?.summoner
  const name = summoner?.gameName || summoner?.displayName || '—'
  const seed = summoner?.gameName || summoner?.puuid || String(client.pid)
  return masked(name, () => streamerSummonerName(seed, index))
}

const otherClientTagLine = (client: UxCommandLine) => {
  const summoner = lcps.connectableClientExtraInfo[client.pid]?.summoner
  return masked(summoner ? `#${summoner.tagLine}` : '—', '#####')
}

const handleConnect = (cmd: UxCommandLine) => {
  if (lcs.connectingClient?.pid === cmd.pid) {
    lc.disconnect()
    return
  }

  lc.connect(cmd)
}
</script>

<style scoped>
.startup-scroll {
  height: 100%;
}
.startup-workspace {
  box-sizing: border-box;
  width: 100%;
  max-width: 1240px;
  padding: 24px 28px 40px;
  margin: 0 auto;
  color: var(--la-color-text-primary);
}
.startup-caption {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 10px;
  line-height: 18px;
  color: var(--la-text-secondary);
}
.startup-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 18px;
  padding: 26px 0 28px;
  border-bottom: 1px solid var(--la-border);
  margin-bottom: 26px;
}
.startup-heading__title {
  font-size: 25px;
  font-weight: 600;
  line-height: 1.3;
  letter-spacing: -0.6px;
}
.startup-heading p {
  margin: 9px 0 0;
  color: var(--la-text-secondary);
  font-size: 12px;
  line-height: 1.7;
}
.startup-heading__actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.connection-state {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 1px solid var(--la-border);
  border-radius: 5px;
  padding: 5px 9px;
  color: var(--la-text-secondary);
  font-size: 10px;
  white-space: nowrap;
}
.connection-state i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
}
.connection-state--active {
  color: var(--la-color-link);
}
.startup-sections {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 22px;
}
.startup-panel {
  min-width: 0;
  padding: 20px;
  border: 1px solid var(--la-border);
  border-radius: 8px;
  background: var(--la-surface);
}
.startup-panel__heading {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 12px;
  font-weight: 600;
}
.startup-panel__heading > .n-icon {
  color: var(--la-text-secondary);
  font-size: 17px;
}
.startup-panel__description {
  margin: 8px 0 20px;
  min-height: 32px;
  color: var(--la-text-secondary);
  font-size: 11px;
  line-height: 1.6;
}
.startup-entries {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.startup-entry {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  min-height: 64px;
  box-sizing: border-box;
  margin: 0;
  padding: 12px;
  border: 1px solid var(--la-border);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background-color 0.15s;
}
.startup-entry:hover {
  background: rgb(var(--la-card-tint-rgb) / 0.05);
}
.startup-entry:active {
  background: rgb(var(--la-card-tint-rgb) / 0.08);
}
.startup-entry:focus-visible {
  outline: 2px solid var(--la-color-link);
  outline-offset: 2px;
}
.startup-entry:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.startup-entry__image {
  display: block;
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  object-fit: contain;
  border-radius: 6px;
}
.startup-entry__content {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 4px;
}
.startup-entry__name {
  display: block;
  color: var(--la-color-text-primary);
  font-size: 12px;
  font-weight: 500;
  overflow-wrap: anywhere;
}
.startup-entry__detail {
  display: block;
  color: var(--la-text-secondary);
  font-size: 10px;
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.startup-entry__arrow {
  flex-shrink: 0;
  font-size: 15px;
  color: var(--la-text-secondary);
}
.startup-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  min-height: 160px;
  border: 1px dashed var(--la-border);
  border-radius: 6px;
  color: var(--la-text-secondary);
  font-size: 11px;
}
.startup-empty__icon {
  font-size: 24px;
  opacity: 0.5;
}
@media (max-width: 900px) {
  .startup-sections {
    grid-template-columns: minmax(0, 1fr);
  }
  .startup-workspace {
    padding: 20px;
  }
  .startup-panel__description {
    min-height: 0;
  }
}
</style>
