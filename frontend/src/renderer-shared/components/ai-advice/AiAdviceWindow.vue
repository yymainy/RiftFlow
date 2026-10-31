<template>
  <div class="advice-frame">
    <SetupInAppScope />
    <div
      class="flex h-12 shrink-0 cursor-move items-center gap-2 border-b border-black/10 px-4 dark:border-white/10"
      @mousedown.left="control('drag')"
    >
      <span class="flex-1 font-semibold">{{ t('aiAssistant.preGameTitle') }}</span>
      <NButton size="tiny" quaternary @mousedown.stop @click="dismiss">{{
        t('opponentScout.close')
      }}</NButton>
    </div>
    <div class="shrink-0 px-4 pt-3 text-xs opacity-60">{{ t('aiAssistant.preGameCaption') }}</div>
    <NScrollbar class="min-h-0 flex-1">
      <div class="p-4">
        <div v-if="busy" class="flex items-center gap-2 py-6">
          <NSpin :size="18" /><span>{{ t('aiAssistant.adviceLoading') }}</span>
        </div>
        <NAlert v-if="error" type="warning" class="mb-3">{{ error }}</NAlert>
        <AiResponse v-if="answer" :text="answer" />
      </div>
    </NScrollbar>
    <div
      class="flex shrink-0 items-center gap-3 border-t border-black/10 p-3 text-xs dark:border-white/10"
    >
      <NButton v-if="busy" size="tiny" @click="stop">{{ t('aiAssistant.stop') }}</NButton>
      <NButton v-else-if="error" size="tiny" :disabled="!gameKey" @click="start(gameKey, true)">{{
        t('aiAssistant.retry')
      }}</NButton>
      <span class="opacity-60">{{ t('aiAssistant.adviceCaution') }}</span>
    </div>
  </div>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import { NAlert, NButton, NScrollbar, NSpin } from 'naive-ui'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import { SetupInAppScope } from '@renderer-shared/shards/setup-in-app-scope/setup-in-app-scope-component'
import { readScoutMatch } from '../opponent-scout/summary'
import { AdviceLifecycle } from './lifecycle'
import AiResponse from './AiResponse.vue'
const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const league = useLeagueClientStore(),
  ongoing = useOngoingGameStore()
const answer = ref(''),
  error = ref(''),
  busy = ref(false),
  enabled = ref(false)
const lifecycle = new AdviceLifecycle()
let timer: ReturnType<typeof setTimeout> | undefined
let requestId = '',
  startingKey = '',
  version = 0,
  detectedAt = 0,
  previousKey = ''
const gameKey = computed(() =>
  league.isConnected &&
  !ongoing.draft &&
  ongoing.settings.enabled &&
  ongoing.queryStage.phase === 'in-game' &&
  ['GameStart', 'InProgress', 'Reconnect'].includes(league.gameflow.phase || '') &&
  ongoing.queryStage.gameInfo.gameId === league.gameflow.session?.gameData.gameId &&
  league.summoner.me?.puuid
    ? `${league.summoner.me.puuid}:${ongoing.queryStage.gameInfo.gameId}`
    : ''
)
const ids = computed(() => [...new Set(Object.values(ongoing.teams).flat())])
const signature = computed(() =>
  ids.value.map((id) => `${id}:${ongoing.championSelections[id] || 0}`).join('|')
)
const selfChampion = computed(
  () => ongoing.championSelections[league.summoner.me?.puuid || ''] || 0
)
const completeRoster = computed(
  () =>
    ids.value.length >= 10 &&
    Object.keys(ongoing.teams).length >= 2 &&
    ids.value.every((id) => ongoing.championSelections[id] > 0)
)
async function control(command: string, ...args: unknown[]) {
  return ipc.call('ai-advice-main', 'control', command, ...args)
}
function clearTimer() {
  if (timer) clearTimeout(timer)
  timer = undefined
}
async function stop() {
  const id = requestId
  requestId = ''
  if (id) await ipc.call('ai-assistant-main', 'cancel', id).catch(() => {})
}
function reset() {
  version++
  startingKey = ''
  clearTimer()
  void stop()
  busy.value = false
  answer.value = ''
  error.value = ''
  void control('hide').catch(() => {})
}
watch(
  [gameKey, signature, enabled],
  ([key]) => {
    if (key !== previousKey) {
      reset()
      previousKey = key
      detectedAt = Date.now()
    }
    clearTimer()
    if (!enabled.value) {
      reset()
      return
    }
    if (
      !key ||
      !selfChampion.value ||
      !ids.value.includes(league.summoner.me?.puuid || '') ||
      !lifecycle.canStart(key) ||
      startingKey === key
    )
      return
    const delay = Math.max(0, detectedAt + (completeRoster.value ? 2000 : 10000) - Date.now())
    timer = setTimeout(() => {
      timer = undefined
      void start(key)
    }, delay)
  },
  { immediate: true }
)
function context() {
  const self = league.summoner.me?.puuid
  const ownTeam = Object.entries(ongoing.teams).find(
    ([, members]) => self && members.includes(self)
  )?.[0]
  return {
    task: '选人结束后的本局建议',
    gameId: league.gameflow.session?.gameData.gameId,
    queueId: ongoing.queryStage.gameInfo?.queueId,
    phase: league.gameflow.phase,
    identified: ids.value.length,
    completeRoster: completeRoster.value,
    versionKnowledge: '未提供实时版本数值和时间线。未识别到的玩家与战绩不能猜测。',
    players: Object.entries(ongoing.teams).flatMap(([team, members]) =>
      members.map((id, index) => {
        const matches = (ongoing.matchHistory[id]?.data || [])
          .flatMap((game) => {
            try {
              const match = readScoutMatch(game, id)
              return match ? [match] : []
            } catch {
              return []
            }
          })
          .filter((m) => m.queue === ongoing.queryStage.gameInfo?.queueId && m.duration >= 300)
          .sort((a, b) => b.at - a.at)
          .slice(0, 10)
        const champion = ongoing.championSelections[id]
        return {
          player: id === self ? '自己' : `${team === ownTeam ? '队友' : '对手'}${index + 1}`,
          team: team === ownTeam ? '己方' : '对方',
          champion: champion ? league.gameData.championName(champion) : null,
          position: ongoing.positionAssignments[id]?.position || null,
          recent: {
            games: matches.length,
            wins: matches.filter((m) => m.won).length,
            champions: matches.map((m) => ({
              champion: league.gameData.championName(m.champion),
              position: m.position,
              won: m.won
            }))
          }
        }
      })
    )
  }
}
async function start(key: string, manual = false) {
  if (
    !key ||
    key !== gameKey.value ||
    busy.value ||
    startingKey === key ||
    lifecycle.isDismissed(key) ||
    (!manual && (!enabled.value || !lifecycle.canStart(key)))
  )
    return
  startingKey = key
  const currentVersion = version
  try {
    const status = await ipc.call<{ configured: boolean }>('ai-assistant-main', 'status')
    if (
      !status.configured ||
      currentVersion !== version ||
      key !== gameKey.value ||
      lifecycle.isDismissed(key)
    )
      return
    if (!manual && !lifecycle.start(key)) return
    const snapshot = context()
    answer.value = ''
    error.value = ''
    busy.value = true
    requestId = crypto.randomUUID()
    const id = requestId
    await control('show')
    if (currentVersion !== version || key !== gameKey.value || lifecycle.isDismissed(key)) return
    const result = await ipc.call<{ text: string }>('ai-assistant-main', 'generate', {
      id,
      question:
        '阵容已确认。请为“自己”给出简短实用的本局建议，约300字：1.这局总体怎么打；2.对线或配合时优先注意谁；3.当前英雄的两个行动建议。用小标题和短句，不要复述全部战绩，不猜测缺失玩家。没有当前版本资料时不给确定数值和出装结论。',
      context: snapshot
    })
    if (currentVersion === version && key === gameKey.value && !lifecycle.isDismissed(key))
      answer.value = result.text
  } catch (reason) {
    if (currentVersion === version && key === gameKey.value && !lifecycle.isDismissed(key))
      error.value =
        typeof reason === 'object' && reason !== null && 'message' in reason
          ? String(reason.message)
          : t('aiAssistant.requestError')
  } finally {
    if (currentVersion === version) {
      startingKey = ''
      requestId = ''
      busy.value = false
    }
  }
}
function dismiss() {
  lifecycle.dismiss(gameKey.value)
  reset()
}
onMounted(async () => {
  try {
    enabled.value = (await control('getAuto')) as boolean
  } catch {}
})
ipc.onEventVue('ai-advice-main', 'auto-advice-changed', (value: boolean) => (enabled.value = value))
onBeforeUnmount(reset)
</script>
<style scoped>
.advice-frame {
  height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  border: 1px solid var(--la-border);
  background: var(--la-color-bg-primary);
  color: var(--la-color-text-primary);
}
</style>
