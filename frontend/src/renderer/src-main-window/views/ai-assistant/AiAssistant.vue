<template>
  <NScrollbar class="h-full">
    <div class="mx-auto flex max-w-240 flex-col gap-4 p-5">
      <div class="flex items-center justify-between gap-3">
        <div class="flex gap-2">
          <NButton
            v-for="tab in ['chat', 'notes', 'settings']"
            :key="tab"
            :type="pane === tab ? 'primary' : 'default'"
            size="small"
            @click="pane = tab"
            >{{ t('aiAssistant.panes.' + tab) }}</NButton
          >
        </div>
        <NTag size="small" :type="status?.configured ? 'success' : 'warning'">{{
          t(status?.configured ? 'aiAssistant.connected' : 'aiAssistant.unconfigured')
        }}</NTag>
      </div>
      <NAlert v-if="error" type="error" closable @close="error = ''">{{ error }}</NAlert>
      <template v-if="pane === 'chat'">
        <div>
          <div class="text-xl font-semibold">{{ t('aiAssistant.welcome') }}</div>
          <div class="mt-1 text-sm opacity-60">{{ t('aiAssistant.simpleIntro') }}</div>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <NButton
            v-for="value in quickModes"
            :key="value"
            :disabled="busy"
            :type="mode === value ? 'primary' : 'default'"
            @click="chooseMode(value)"
            >{{ t('aiAssistant.quick.' + value) }}</NButton
          >
        </div>
        <div class="flex flex-wrap items-center gap-3">
          <NSelect
            :value="mode"
            :options="AI_MODES.map((value) => ({ value, label: t('aiAssistant.modes.' + value) }))"
            class="w-45"
            :disabled="busy"
            @update:value="chooseMode"
          />
          <div
            v-if="mode === 'chat' || mode === 'knowledge'"
            class="flex items-center gap-2 text-xs"
          >
            <NSwitch v-model:value="includeContext" size="small" :disabled="busy" />{{
              t('aiAssistant.attachData')
            }}
          </div>
        </div>
        <div
          v-if="includeContext"
          class="rounded-lg border border-black/10 p-3 dark:border-white/10"
        >
          <div class="text-sm">{{ contextSummary }}</div>
          <NSelect
            v-if="mode === 'beginner'"
            v-model:value="championId"
            :options="championOptions"
            clearable
            filterable
            class="mt-2"
            :placeholder="t('aiAssistant.champion')"
            :disabled="busy"
          />
          <NSelect
            v-if="mode === 'review'"
            v-model:value="matchId"
            :options="matchOptions"
            class="mt-2"
            :placeholder="t('aiAssistant.match')"
            :disabled="busy"
          />
          <NButton text size="tiny" class="mt-2" @click="showSources = !showSources">{{
            t('aiAssistant.adjustData')
          }}</NButton>
          <div v-if="showSources" class="mt-2 flex flex-col gap-2">
            <NSelect
              v-model:value="owner"
              :options="ownerOptions"
              :placeholder="t('aiAssistant.player')"
              :disabled="busy"
            />
            <NSelect
              v-if="mode !== 'beginner'"
              v-model:value="championId"
              :options="championOptions"
              clearable
              filterable
              :placeholder="t('aiAssistant.champion')"
              :disabled="busy"
            />
            <div class="text-xs opacity-60">{{ t('aiAssistant.dataHint') }}</div>
            <NButton text size="tiny" @click="showContext = !showContext">{{
              t('aiAssistant.preview')
            }}</NButton>
            <pre
              v-if="showContext"
              class="max-h-50 overflow-auto text-xs break-all whitespace-pre-wrap"
              >{{ JSON.stringify(makeContext(), null, 2) }}</pre>
          </div>
        </div>
        <div v-else class="text-xs opacity-60">{{ t('aiAssistant.questionOnly') }}</div>
        <NInput
          v-model:value="question"
          type="textarea"
          :maxlength="6000"
          show-count
          :autosize="{ minRows: 3, maxRows: 6 }"
          :placeholder="t('aiAssistant.simpleQuestion')"
          :disabled="busy"
          @keydown.ctrl.enter.prevent="generate"
        />
        <div class="flex flex-wrap items-center gap-3">
          <NButton
            type="primary"
            :loading="busy"
            :disabled="refreshing || !question.trim()"
            @click="generate"
            >{{ t('aiAssistant.send') }}</NButton
          >
          <NButton v-if="busy" @click="cancel">{{ t('aiAssistant.stop') }}</NButton>
          <NButton v-if="answer && !busy" size="small" @click="newTopic">{{
            t('aiAssistant.newConversation')
          }}</NButton>
          <NButton v-if="!status?.configured" text @click="pane = 'settings'">{{
            t('aiAssistant.configure')
          }}</NButton>
        </div>
        <NCard v-if="answer" size="small" :title="t('aiAssistant.answer')">
          <AiResponse :text="answer" />
          <NAlert v-if="truncated" type="warning" class="mt-3">{{
            t('aiAssistant.truncated')
          }}</NAlert>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <NButton size="small" @click="copyAnswer">{{ t('aiAssistant.copy') }}</NButton
            ><NButton size="small" :disabled="savingNote" @click="saveAnswer">{{
              t('aiAssistant.saveAnswer')
            }}</NButton
            ><span class="text-xs opacity-50">{{ answerScope }}</span>
          </div>
        </NCard>
      </template>
      <template v-else-if="pane === 'notes'">
        <div class="text-sm opacity-65">{{ t('aiAssistant.simpleNoteHint') }}</div>
        <NSelect
          v-model:value="championId"
          :options="championOptions"
          clearable
          filterable
          :placeholder="t('aiAssistant.champion')"
        />
        <NInput
          v-model:value="noteTitle"
          :maxlength="100"
          :placeholder="t('aiAssistant.noteTitle')"
        />
        <NInput
          v-model:value="noteBody"
          type="textarea"
          :maxlength="5000"
          :autosize="{ minRows: 3, maxRows: 6 }"
          :placeholder="t('aiAssistant.noteBody')"
        />
        <div>
          <NButton
            :loading="savingNote"
            :disabled="!noteTitle.trim() || !noteBody.trim()"
            @click="saveNote"
            >{{ t('aiAssistant.saveNote') }}</NButton
          >
        </div>
        <NCard v-for="note in relatedNotes" :key="note.id" size="small" :title="note.title">
          <div class="text-xs opacity-60">
            {{ note.championId ? championName(note.championId) : t('aiAssistant.generalNote') }}
          </div>
          <div class="mt-2 max-h-45 overflow-auto break-words whitespace-pre-wrap">
            {{ note.body }}
          </div>
          <NButton
            text
            size="tiny"
            class="mt-2"
            :disabled="savingNote"
            @click="deleteNote(note.id)"
            >{{ t('aiAssistant.delete') }}</NButton
          >
        </NCard>
      </template>
      <template v-else>
        <AiAutoAdviceSettings />
        <NCard size="small" :title="t('aiAssistant.configure')">
          <NTag size="small">{{
            t(
              configForm.source === 'custom'
                ? 'aiAssistant.customSource'
                : 'aiAssistant.defaultSource'
            )
          }}</NTag>
          <div class="my-3 text-sm opacity-70">{{ t('aiAssistant.serverConfigHint') }}</div>
          <div class="flex flex-col gap-2">
            <NInput
              v-model:value="configForm.url"
              :disabled="busy || savingConfig"
              :placeholder="t('aiAssistant.urlInput')"
              autocomplete="off"
            />
            <NInput
              v-model:value="configForm.key"
              :disabled="busy || savingConfig"
              type="password"
              show-password-on="mousedown"
              :placeholder="
                t(configForm.hasKey ? 'aiAssistant.keyRetained' : 'aiAssistant.keyInput')
              "
              autocomplete="new-password"
            />
            <NInput
              v-model:value="configForm.model"
              :disabled="busy || savingConfig"
              :placeholder="t('aiAssistant.modelInput')"
              autocomplete="off"
            />
          </div>
          <div class="mt-3 flex flex-wrap gap-2">
            <NButton
              type="primary"
              :loading="savingConfig"
              :disabled="busy"
              @click="saveConfiguration"
              >{{ t('aiAssistant.saveConfig') }}</NButton
            >
            <NButton :disabled="busy || savingConfig" @click="resetConfiguration">{{
              t('aiAssistant.restoreDefault')
            }}</NButton>
            <NButton :loading="refreshing" :disabled="busy || savingConfig" @click="refresh">{{
              t('aiAssistant.refresh')
            }}</NButton>
          </div>
          <div v-if="status?.error" class="mt-3 text-xs" role="alert">{{ status.error }}</div>
        </NCard>
        <div class="text-xs opacity-60">{{ t('aiAssistant.sendHint') }}</div>
      </template>
    </div>
  </NScrollbar>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useTranslation } from 'i18next-vue'
import {
  NAlert,
  NButton,
  NCard,
  NInput,
  NScrollbar,
  NSelect,
  NTag,
  NSwitch,
  useMessage
} from 'naive-ui'
import AiResponse from '@renderer-shared/components/ai-advice/AiResponse.vue'
import AiAutoAdviceSettings from '@renderer-shared/components/ai-advice/AiAutoAdviceSettings.vue'
import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import { useLeagueClientStore } from '@renderer-shared/shards/league-client/store'
import { useOngoingGameStore } from '@renderer-shared/shards/ongoing-game/store'
import { useArchiveApi, type ArchivedMatch } from '../archive/archive-api'
import { archiveErrorMessage } from '../archive/archive-error'
import {
  AI_MODES,
  MODE_PROMPTS,
  archiveMatches,
  deduplicateMatches,
  matchStats,
  visibleMatches,
  weeklySamples,
  type AiMode,
  type AiNote
} from './context'

const { t } = useTranslation()
const ipc = useInstance(RiftFlowIpcRenderer)
const league = useLeagueClientStore()
const ongoing = useOngoingGameStore()
const archive = useArchiveApi()
const message = useMessage()
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const pane = ref('chat')
const includeContext = ref(false)
const showSources = ref(false)
const quickModes: AiMode[] = ['lineup', 'history', 'beginner', 'fun']
const dialogue = ref<{ question: string; answer: string }[]>([])
const status = ref<{ configured: boolean; path: string; model: string; error: string | null }>()
const configForm = ref({ source: 'default', url: '', key: '', model: '', hasKey: false })
const savingConfig = ref(false)
const records = ref<ArchivedMatch[]>([])
const notes = ref<AiNote[]>([])
const mode = ref<AiMode>('chat')
const question = ref('')
const answer = ref('')
const answerScope = ref('')
const truncated = ref(false)
const busy = ref(false)
const refreshing = ref(false)
const error = ref('')
const showContext = ref(false)
const owner = ref<string | null>(null)
const championId = ref<number | null>(null)
const matchId = ref<string | null>(null)
const noteTitle = ref('')
const noteBody = ref('')
const savingNote = ref(false)
const championCatalog = ref<{ id: number; name: string }[]>([])
let requestId = ''
let disposed = false
const championName = (id: number) =>
  league.gameData.champions[id]?.name ||
  championCatalog.value.find((c) => c.id === id)?.name ||
  String(id)
const championOptions = computed(() =>
  championCatalog.value.map((c) => ({ value: c.id, label: championName(c.id) }))
)
const ownerOptions = computed(() => {
  const values = new Map<string, string>()
  if (league.summoner.me)
    values.set(league.summoner.me.puuid, league.summoner.me.gameName || t('aiAssistant.self'))
  for (const record of records.value)
    values.set(record.owner, record.playerName || t('aiAssistant.player'))
  Object.keys(ongoing.matchHistory).forEach((id, index) =>
    values.set(id, ongoing.summoner[id]?.gameName || `${t('aiAssistant.player')} ${index + 1}`)
  )
  return [...values].map(([value, label]) => ({ value, label }))
})
const matches = computed(() =>
  owner.value
    ? deduplicateMatches([
        ...archiveMatches(records.value, owner.value),
        ...visibleMatches(ongoing.matchHistory[owner.value]?.data || [], owner.value, championName)
      ])
    : []
)
const matchOptions = computed(() =>
  matches.value.map((m) => ({
    value: m.id,
    label: `${new Date(m.at).toLocaleString()} · ${m.champion} · ${m.kills}/${m.deaths}/${m.assists}`
  }))
)
const roster = computed(() => {
  if (ongoing.queryStage.phase === 'unavailable') return []
  return Object.entries(ongoing.teams).flatMap(([team, ids]) =>
    ids.map((id, index) => {
      const alias = id === league.summoner.me?.puuid ? '自己' : `${team} 玩家${index + 1}`
      const history = visibleMatches(ongoing.matchHistory[id]?.data || [], id, championName).slice(
        0,
        20
      )
      return {
        player: alias,
        team,
        champion: ongoing.championSelections[id]
          ? championName(ongoing.championSelections[id])
          : null,
        position: ongoing.positionAssignments[id]?.position || null,
        stats: matchStats(history),
        recent: history
      }
    })
  )
})
const relatedNotes = computed(() => {
  const champions = new Set(Object.values(ongoing.championSelections))
  if (championId.value) champions.add(championId.value)
  return notes.value
    .filter((n) => !n.championId || champions.has(n.championId))
    .sort((a, b) => b.updatedAt - a.updatedAt)
})
const contextSummary = computed(() => {
  if (mode.value === 'lineup') return t('aiAssistant.lineupScope', { count: roster.value.length })
  if (mode.value === 'beginner')
    return championId.value
      ? t('aiAssistant.heroScope', { champion: championName(championId.value) })
      : t('aiAssistant.selectChampion')
  if (mode.value === 'notes')
    return t('aiAssistant.notesScope', { count: relatedNotes.value.length })
  if (mode.value === 'review') return t('aiAssistant.reviewScope')
  return t('aiAssistant.historyScope', {
    player:
      ownerOptions.value.find((o) => o.value === owner.value)?.label || t('aiAssistant.player'),
    count: matchStats(matches.value).games
  })
})
watch(
  ownerOptions,
  (options) => {
    if (!options.some((o) => o.value === owner.value)) owner.value = options[0]?.value || null
  },
  { immediate: true }
)
watch(matches, (values) => {
  if (!values.some((m) => m.id === matchId.value)) matchId.value = values[0]?.id || null
})
function chooseMode(value: AiMode) {
  mode.value = value
  question.value = t('aiAssistant.starters.' + value)
  includeContext.value = !['chat', 'knowledge'].includes(value)
  clearConversation()
}
function clearConversation() {
  answer.value = ''
  answerScope.value = ''
  dialogue.value = []
  truncated.value = false
}
function newTopic() {
  clearConversation()
  question.value = ''
  mode.value = 'chat'
  includeContext.value = false
}
function makeContext() {
  const previousDialogue = ['chat', 'knowledge'].includes(mode.value)
    ? dialogue.value.slice(-3)
    : []
  if (!includeContext.value) return { task: mode.value, previousDialogue }
  const personalModes: AiMode[] = ['chat', 'knowledge', 'history', 'weekly', 'review', 'fun']
  const personalMatches =
    mode.value === 'review' ? matches.value.filter((m) => m.id === matchId.value) : matches.value
  return {
    generatedAt: new Date().toISOString(),
    previousDialogue,
    task: mode.value,
    selectedPlayer: owner.value === league.summoner.me?.puuid ? '自己' : '所选玩家',
    selectedChampion: championId.value ? championName(championId.value) : null,
    coverage: '仅已加载的可见记录；不保证完整，不同队列不能直接横比。个人记录按玩家分离。',
    personal: personalModes.includes(mode.value)
      ? {
          stats: matchStats(personalMatches),
          matches: personalMatches
        }
      : null,
    weekly: mode.value === 'weekly' ? weeklySamples(matches.value, Date.now()) : null,
    currentGame: ['chat', 'knowledge', 'lineup', 'notes'].includes(mode.value)
      ? {
          phase: league.gameflow.phase,
          gameId: league.gameflow.session?.gameData.gameId,
          players: roster.value
        }
      : null,
    personalNotes:
      mode.value === 'notes'
        ? relatedNotes.value.map((n) => ({
            title: n.title,
            body: n.body,
            champion: n.championId ? championName(n.championId) : null
          }))
        : [],
    versionKnowledge: '未提供实时版本规则或时间线'
  }
}
async function refresh() {
  refreshing.value = true
  error.value = ''
  const results = await Promise.allSettled([
    loadConfiguration(),
    ipc
      .call<typeof status.value>('ai-assistant-main', 'status')
      .then((value) => (status.value = value)),
    ipc.call<AiNote[]>('ai-assistant-main', 'notes').then((value) => (notes.value = value)),
    ipc
      .call<{ id: number; name: string }[]>('riftflow-archive-main', 'champions')
      .then((value) => (championCatalog.value = value)),
    Promise.all([1, 2, 3, 4].map((page) => archive.list({ page }))).then(
      (pages) => (records.value = pages.flatMap((page) => page.records))
    )
  ])
  const failed = results.find((result) => result.status === 'rejected')
  if (failed?.status === 'rejected')
    error.value = archiveErrorMessage(failed.reason, t('aiAssistant.loadError'))
  refreshing.value = false
}
async function generate() {
  if (busy.value) return
  if (!question.value.trim() || !props.active) return
  if (mode.value === 'lineup' && !roster.value.length) {
    error.value = t('aiAssistant.noLineup')
    return
  }
  if (mode.value === 'beginner' && !championId.value) {
    error.value = t('aiAssistant.selectChampion')
    return
  }
  if (mode.value === 'review' && !matchId.value) {
    error.value = t('aiAssistant.selectMatch')
    return
  }
  busy.value = true
  error.value = ''
  requestId = crypto.randomUUID()
  try {
    const snapshot = makeContext()
    const submittedQuestion = question.value
    const requestQuestion = MODE_PROMPTS[mode.value]
      ? `${MODE_PROMPTS[mode.value]}\n\n用户关注：${submittedQuestion}`
      : submittedQuestion
    const submittedAt = new Date().toISOString()
    const result = await ipc.call<{ text: string; model: string; truncated: boolean }>(
      'ai-assistant-main',
      'generate',
      { id: requestId, question: requestQuestion, context: snapshot }
    )
    if (disposed) return
    answer.value = result.text
    dialogue.value = [
      ...dialogue.value,
      { question: submittedQuestion, answer: result.text.slice(0, 8000) }
    ].slice(-3)
    truncated.value = result.truncated
    answerScope.value = `${new Date(submittedAt).toLocaleString()} · ${t('aiAssistant.modes.' + mode.value)}`
    status.value = await ipc.call('ai-assistant-main', 'status')
  } catch (reason) {
    if (!disposed) error.value = archiveErrorMessage(reason, t('aiAssistant.requestError'))
  } finally {
    busy.value = false
    requestId = ''
  }
}
async function loadConfiguration() {
  const value = await ipc.call<{ source: string; url: string; model: string; hasKey: boolean }>(
    'ai-assistant-main',
    'configView'
  )
  configForm.value = { ...value, key: '' }
}
async function saveConfiguration() {
  if (
    !configForm.value.url.trim() &&
    !configForm.value.model.trim() &&
    !configForm.value.key.trim()
  ) {
    await resetConfiguration()
    return
  }
  savingConfig.value = true
  error.value = ''
  try {
    await ipc.call('ai-assistant-main', 'saveConfig', {
      url: configForm.value.url.trim(),
      api_key: configForm.value.key,
      model: configForm.value.model.trim()
    })
    configForm.value.key = ''
    await loadConfiguration()
    status.value = await ipc.call('ai-assistant-main', 'status')
    message.success(t('aiAssistant.configSaved'))
  } catch (reason) {
    error.value = archiveErrorMessage(reason, t('aiAssistant.saveError'))
  } finally {
    savingConfig.value = false
  }
}
async function resetConfiguration() {
  savingConfig.value = true
  error.value = ''
  try {
    await ipc.call('ai-assistant-main', 'resetConfig')
    await loadConfiguration()
    status.value = await ipc.call('ai-assistant-main', 'status')
    message.success(t('aiAssistant.defaultRestored'))
  } catch (reason) {
    error.value = archiveErrorMessage(reason, t('aiAssistant.saveError'))
  } finally {
    savingConfig.value = false
  }
}
async function cancel() {
  if (requestId) await ipc.call('ai-assistant-main', 'cancel', requestId).catch(() => {})
}
async function persistNotes(next: AiNote[]) {
  savingNote.value = true
  try {
    notes.value = await ipc.call('ai-assistant-main', 'saveNotes', next)
    return true
  } catch (reason) {
    error.value = archiveErrorMessage(reason, t('aiAssistant.saveError'))
    return false
  } finally {
    savingNote.value = false
  }
}
async function saveNote() {
  if (
    await persistNotes([
      ...notes.value,
      {
        id: crypto.randomUUID(),
        title: noteTitle.value.trim(),
        body: noteBody.value.trim(),
        championId: championId.value || 0,
        updatedAt: Date.now()
      }
    ])
  ) {
    noteTitle.value = ''
    noteBody.value = ''
  }
}
async function deleteNote(id: string) {
  await persistNotes(notes.value.filter((n) => n.id !== id))
}
async function saveAnswer() {
  await persistNotes([
    ...notes.value,
    {
      id: crypto.randomUUID(),
      title: t('aiAssistant.modes.' + mode.value),
      body: answer.value.slice(0, 5000),
      championId: championId.value || 0,
      updatedAt: Date.now()
    }
  ])
}
async function copyAnswer() {
  try {
    await navigator.clipboard.writeText(answer.value)
    message.success(t('aiAssistant.copied'))
  } catch {
    error.value = t('aiAssistant.copyError')
  }
}
onMounted(refresh)
watch(
  () => props.active,
  (value) => {
    if (!value) void cancel()
  }
)
onBeforeUnmount(() => {
  disposed = true
  void cancel()
})
</script>
