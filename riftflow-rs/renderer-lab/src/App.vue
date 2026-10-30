<template>
  <n-config-provider :theme="isDark ? darkTheme : null" :locale="naiveLocale" :date-locale="naiveLocale">
    <div class="lab" :class="{ dark: isDark }">
      <header class="lab-header">
        <div>
          <h1>{{ t('lab.title') }}</h1>
          <p class="lab-sub">{{ t('lab.subtitle') }}</p>
        </div>
        <div class="lab-actions">
          <span class="lab-caption">{{ t('lab.language') }}</span>
          <n-select v-model:value="locale" :options="localeOptions" size="small" class="w-130" />
          <span class="lab-caption">{{ t('lab.theme') }}</span>
          <n-switch v-model:value="isDark" size="small">
            <template #checked>{{ t('lab.dark') }}</template>
            <template #unchecked>{{ t('lab.light') }}</template>
          </n-switch>
          <n-button size="small" secondary :loading="loading" @click="refresh">{{ t('lab.reload') }}</n-button>
        </div>
      </header>

      <main class="lab-grid">
        <SettingsSection :title="t('contract.title')" :footer="t('contract.description')">
          <SettingsRow :label="t('contract.version')" :label-width="260">
            <span class="lab-value">{{ runtime.version || '—' }}</span>
          </SettingsRow>
          <SettingsRow :label="t('contract.connection')" :label-width="260">
            <n-tag :type="String(runtime.connection).toLowerCase() === 'connected' ? 'success' : 'warning'" size="small" round>
              {{ runtime.connection || t('contract.unknown') }}
            </n-tag>
          </SettingsRow>
          <SettingsRow :label="t('contract.phase')" :label-width="260">
            <n-tag size="small" :bordered="false">{{ runtime.phase || t('contract.notConnected') }}</n-tag>
          </SettingsRow>
          <SettingsRow :label="t('contract.summoner')" :label-width="260" no-x-padding>
            <div class="lab-summoner">
              <img v-if="runtime.portrait" :src="runtime.portrait" class="lab-portrait" alt="" />
              <span v-else class="lab-portrait lab-portrait--empty" />
              <span class="lab-value">{{ runtime.summoner || '—' }}</span>
            </div>
          </SettingsRow>
        </SettingsSection>

        <SettingsSection :title="t('settings.title')" :footer="t('settings.description')">
          <SettingsRow :label="t('settings.opacity')" :label-description="t('settings.opacityHint')" :label-width="260">
            <div class="lab-inline">
              <n-slider v-model:value="form.opacity" :min="0.3" :max="1" :step="0.05" class="w-160" :format-tooltip="percent" @update:value="write('window-manager-main/aux-window', 'opacity', $event)" />
              <read-back :state="form.opacityState" :expected="form.opacity" />
            </div>
          </SettingsRow>
          <SettingsRow :label="t('settings.timerType')" :label-width="260">
            <div class="lab-inline">
              <n-select v-model:value="form.timerType" :options="timerOptions" size="small" class="w-160" @update:value="write('window-manager-main/cd-timer-window', 'timerType', $event)" />
              <read-back :state="form.timerTypeState" :expected="form.timerType" />
            </div>
          </SettingsRow>
          <SettingsRow :label="t('settings.sendInterval')" :label-width="260">
            <div class="lab-inline">
              <n-input-number v-model:value="form.sendInterval" :min="20" :max="2000" size="small" class="w-160" @update:value="write('in-game-send-main', 'sendInterval', $event)" />
              <read-back :state="form.sendIntervalState" :expected="form.sendInterval" />
            </div>
          </SettingsRow>
          <SettingsRow :label="t('settings.opgg')" :label-description="t('settings.opggHint')" :label-width="260">
            <div class="lab-inline">
              <n-switch v-model:value="form.opggEnabled" size="small" @update:value="write('window-manager-main/opgg-window', 'enabled', $event)" />
              <read-back :state="form.opggEnabledState" :expected="form.opggEnabled" />
            </div>
          </SettingsRow>
        </SettingsSection>

        <SettingsSection :title="t('assets.title')" :footer="t('assets.description')">
          <div class="lab-assets">
            <div v-for="item in assets" :key="item.path" class="lab-asset">
              <img v-if="item.url" :src="item.url" :alt="item.name" />
              <div v-else class="lab-asset--placeholder">{{ item.error ? t('assets.failed') : t('assets.loading') }}</div>
              <span>{{ item.name }}</span>
            </div>
          </div>
        </SettingsSection>

        <SettingsSection :title="t('gallery.title')" :footer="t('gallery.description')">
          <SettingsRow :label="t('gallery.label')" :label-description="t('gallery.labelDescription')" :label-width="260">
            <div class="lab-row-gap">
              <n-button type="primary" size="small">{{ t('gallery.confirm') }}</n-button>
              <n-button size="small" secondary>{{ t('gallery.secondary') }}</n-button>
              <n-button size="small" text>{{ t('gallery.ghost') }}</n-button>
            </div>
          </SettingsRow>
          <SettingsRow :label="t('gallery.search')" :label-width="260">
            <div class="lab-inline">
              <n-input v-model:value="scratch.search" size="small" class="w-200" :placeholder="t('gallery.search')" />
              <n-select v-model:value="scratch.queue" size="small" class="w-160" :placeholder="t('gallery.queuePlaceholder')" :options="queueOptions" />
            </div>
          </SettingsRow>
          <SettingsRow :label="t('gallery.progress')" :label-width="260">
            <div class="lab-inline">
              <n-progress type="line" :percentage="68" indicator-placement="inside" class="w-240" />
              <n-checkbox v-model:checked="scratch.check">NSwitch / NCheckbox / NRadio</n-checkbox>
              <n-radio-group v-model:value="scratch.radio">
                <n-radio value="a">A</n-radio>
                <n-radio value="b">B</n-radio>
              </n-radio-group>
            </div>
          </SettingsRow>
          <SettingsRow :label="t('gallery.empty')" :label-width="260">
            <n-empty size="small" :description="t('gallery.empty')" />
          </SettingsRow>
          <SettingsRow :label="t('gallery.error')" :label-width="260" control-full-line>
            <n-alert type="error" size="small" :show-icon="true" class="w-full">
              {{ lastError || '—' }}
            </n-alert>
          </SettingsRow>
        </SettingsSection>

        <SettingsSection :title="t('log.title')" :footer="`${calls.length} · ${t('log.title')}`">
          <div class="lab-log">
            <div v-for="(line, index) in calls.slice(-14).reverse()" :key="index" class="lab-log-line" :class="{ 'lab-log-line--bad': line.startsWith('!') }">
              {{ line }}
            </div>
            <div v-if="!calls.length" class="lab-log-line">{{ t('log.empty') }}</div>
          </div>
        </SettingsSection>
      </main>
    </div>
  </n-config-provider>
</template>

<script setup>
import { computed, h, onMounted, reactive, ref, watch } from 'vue'
import {
  darkTheme,
  NAlert,
  NButton,
  NCheckbox,
  NConfigProvider,
  NEmpty,
  NInput,
  NInputNumber,
  NProgress,
  NRadio,
  NRadioGroup,
  NSelect,
  NSlider,
  NSwitch,
  NTag,
  zhCN,
  enUS,
} from 'naive-ui'
import SettingsRow from './components/SettingsRow.vue'
import SettingsSection from './components/SettingsSection.vue'
import { championIcon, initialState, invoke, lcuAsset, profileIcon } from './riftflow.js'
import { locales, setLocale, t } from './i18n.js'

const isDark = ref(true)
const locale = ref('zh-CN')
const loading = ref(false)
const calls = ref([])
const lastError = ref('')
const runtime = reactive({ version: '', connection: '', phase: '', summoner: '', portrait: '' })
const form = reactive({
  opacity: 1,
  opacityState: null,
  timerType: 'countdown',
  timerTypeState: null,
  sendInterval: 65,
  sendIntervalState: null,
  opggEnabled: true,
  opggEnabledState: null,
})
const scratch = reactive({ search: '', queue: null, check: true, radio: 'a' })
const assets = ref([])

const naiveLocale = computed(() => (locale.value === 'zh-CN' ? zhCN : enUS))
const localeOptions = locales.map((item) => ({ label: item.label, value: item.value }))
const timerOptions = computed(() => [
  { label: t('settings.timerCountdown'), value: 'countdown' },
  { label: t('settings.timerCountup'), value: 'countup' },
])
const queueOptions = [
  { label: 'RANKED_SOLO_5x5', value: 'RANKED_SOLO_5x5' },
  { label: 'RANKED_FLEX_SR', value: 'RANKED_FLEX_SR' },
  { label: 'URF', value: 'URF' },
]

// A tiny inline component so every control can show what the main process actually
// stored, instead of only what the slider is holding locally.
const ReadBack = {
  name: 'ReadBack',
  props: ['state', 'expected'],
  setup(props) {
    return () => {
      if (!props.state) return h('span', { class: 'lab-readback' }, ' ')
      if (props.state.pending) return h('span', { class: 'lab-readback' }, '…')
      const ok = JSON.stringify(props.state.value) === JSON.stringify(props.expected)
      return h('span', { class: ['lab-readback', ok ? 'lab-readback--ok' : 'lab-readback--bad'] }, `${ok ? '✓' : '✗'} ${format(props.state.value)}`)
    }
  },
}
const readBack = ReadBack

function format(value) {
  if (value === null || value === undefined) return 'null'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

const percent = (value) => `${Math.round(value * 100)}%`

function note(line) {
  calls.value.push(line)
}

watch([locale], () => setLocale(locale.value))

async function refresh() {
  loading.value = true
  try {
    const info = await invoke('app-common-main', 'getRuntimeInfo')
    runtime.version = (info && (info.version || info.appVersion)) || (await invoke('app-common-main', 'getVersion'))
    note(`app-common-main:getRuntimeInfo ✓`)
  } catch (error) {
    runtime.version = ''
    lastError.value = String(error.message || error)
    note(`! app-common-main:getRuntimeInfo ${lastError.value}`)
  }

  try {
    const surface = await initialState('league-client-main', 'summoner', ['me'])
    const me = surface.me
    const name = me && (me.gameName || me.summoner)
    const tag = me && (me.tagLine || me.acctUserId)
    runtime.summoner = name ? `${name}${tag ? '#' + tag : ''}` : ''
    note(`league-client-main summoner/me ✓ ${name || '(empty)'}`)
    if (me && me.profileIconId) runtime.portrait = await lcuAsset(profileIcon(me.profileIconId))
  } catch (error) {
    runtime.summoner = ''
    lastError.value = String(error.message || error)
    note(`! league-client-main summoner/me ${lastError.value}`)
  }

  try {
    const gameflow = await initialState('league-client-main', 'gameflow', ['phase'])
    const connection = await initialState('league-client-main', 'state', ['connectionState'])
    runtime.phase = gameflow.phase || ''
    runtime.connection = connection.connectionState || ''
    note(`gameflow phase=${runtime.phase || '-'} connection=${runtime.connection || '-'}`)
  } catch (error) {
    lastError.value = String(error.message || error)
    note(`! gameflow ${lastError.value}`)
  }

  for (const [key, namespace, path] of [
    ['opacity', 'window-manager-main/aux-window', 'opacity'],
    ['timerType', 'window-manager-main/cd-timer-window', 'timerType'],
    ['sendInterval', 'in-game-send-main', 'sendInterval'],
    ['opggEnabled', 'window-manager-main/opgg-window', 'enabled'],
  ]) {
    try {
      const settings = await initialState(namespace, 'settings', [path])
      form[key] = settings[path] ?? form[key]
      form[`${key}State`] = { value: settings[path] }
      note(`${namespace} ${path} = ${format(settings[path])}`)
    } catch (error) {
      form[`${key}State`] = { value: null }
      lastError.value = String(error.message || error)
      note(`! ${namespace}:${path} ${lastError.value}`)
    }
  }

  loading.value = false
}

async function write(namespace, key, value) {
  // `enabled` is stored on the opgg namespace but lives under its own form key
  const [formKey, slot] =
    key === 'enabled' ? ['opggEnabled', 'opggEnabledState'] : [key, `${key}State`]

  form[formKey] = value
  form[slot] = { pending: true }

  try {
    await invoke('setting-factory-main', 'set', namespace, key, value)
    const back = await initialState(namespace, 'settings', [key])
    form[slot] = { value: back[key] }
    note(`set ${namespace}/${key} = ${format(value)} → ${format(back[key])}`)
  } catch (error) {
    form[slot] = { value: null }
    lastError.value = String(error.message || error)
    note(`! set ${namespace}/${key} ${lastError.value}`)
  }
}

async function loadAssets() {
  const wanted = [
    { name: '266', path: championIcon(266) },
    { name: '222', path: championIcon(222) },
    { name: '157', path: championIcon(157) },
    { name: '876', path: championIcon(876) },
  ]

  assets.value = wanted.map((item) => ({ ...item, url: '', error: false }))

  await Promise.all(
    wanted.map(async (item, index) => {
      try {
        assets.value[index].url = await lcuAsset(item.path)
      } catch (error) {
        assets.value[index].error = true
        note(`! asset ${item.path} ${error.message || error}`)
      }
    }),
  )
}

onMounted(async () => {
  setLocale(locale.value)
  await refresh()
  loadAssets()
})
</script>

<style>
.lab {
  min-height: 100%;
  padding: 16px 20px 28px;
  background: #f6f7f9;
  color: rgb(0 0 0 / 85%);
}

.lab.dark {
  background: #16181c;
  color: rgb(255 255 255 / 88%);
}

.lab-header {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: flex-end;
  justify-content: space-between;
  padding: 0 12px 14px;
}

.lab-header h1 {
  margin: 0;
  font-size: 17px;
  line-height: 24px;
  font-weight: 700;
}

.lab-sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: rgb(0 0 0 / 55%);
}

.dark .lab-sub {
  color: rgb(255 255 255 / 55%);
}

.lab-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.lab-caption {
  font-size: 12px;
  opacity: 0.65;
}

.lab-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(460px, 1fr));
  gap: 18px 22px;
  align-items: start;
}

.lab-value {
  font-size: 13px;
  font-variant-numeric: tabular-nums;
}

.lab-inline {
  display: flex;
  gap: 10px;
  align-items: center;
}

.lab-row-gap {
  display: flex;
  gap: 8px;
  align-items: center;
}

.lab-readback {
  min-width: 68px;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  opacity: 0.7;
}

.lab-readback--ok {
  color: #18a058;
}

.lab-readback--bad {
  color: #d03050;
}

.lab-summoner {
  display: flex;
  gap: 8px;
  align-items: center;
}

.lab-portrait {
  width: 28px;
  height: 28px;
  border-radius: 50%;
}

.lab-portrait--empty {
  background: rgb(0 0 0 / 8%);
}

.dark .lab-portrait--empty {
  background: rgb(255 255 255 / 10%);
}

.lab-assets {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  padding: 12px;
}

.lab-asset {
  display: flex;
  width: 62px;
  flex-direction: column;
  gap: 4px;
  align-items: center;
  font-size: 11px;
  opacity: 0.8;
}

.lab-asset img {
  width: 56px;
  height: 56px;
  border-radius: 6px;
  object-fit: cover;
}

.lab-asset--placeholder {
  display: grid;
  width: 56px;
  height: 56px;
  place-items: center;
  border-radius: 6px;
  background: rgb(0 0 0 / 6%);
  font-size: 11px;
  text-align: center;
}

.dark .lab-asset--placeholder {
  background: rgb(255 255 255 / 8%);
}

.lab-log {
  max-height: 220px;
  padding: 10px 12px;
  overflow: auto;
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 11px;
  line-height: 1.7;
}

.lab-log-line {
  white-space: pre-wrap;
  opacity: 0.8;
}

.lab-log-line--bad {
  color: #d03050;
}

.w-130 { width: 130px; }
.w-160 { width: 160px; }
.w-200 { width: 200px; }
.w-240 { width: 240px; }
.w-full { width: 100%; }
</style>
