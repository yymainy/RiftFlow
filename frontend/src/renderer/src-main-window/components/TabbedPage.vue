<template>
  <div class="workspace-page">
    <header class="workspace-page__heading">
      <div class="workspace-page__identity">
        <NIcon :component="icon" />
        <h1>{{ title }}</h1>
      </div>
      <span class="workspace-page__index"
        >{{ String(currentTabIndex + 1).padStart(2, '0') }} /
        {{ String(tabs.length).padStart(2, '0') }}</span
      >
    </header>
    <nav class="workspace-sections" :aria-label="title">
      <button
        v-for="(tab, index) in tabs"
        :key="tab.key"
        type="button"
        :class="['workspace-section', { active: currentTab === tab.key }]"
        :aria-current="currentTab === tab.key ? 'page' : undefined"
        @click="currentTab = tab.key"
      >
        <NIcon :component="tab.icon" /><span>{{ tab.name }}</span
        ><small>{{ String(index + 1).padStart(2, '0') }}</small>
      </button>
    </nav>
    <section class="workspace-pane" :aria-label="activeTab?.name">
      <header class="workspace-pane__heading">
        <span class="workspace-pane__dot"></span>
        <h2>{{ activeTab?.name }}</h2>
      </header>
      <div class="workspace-pane__body">
        <KeepAlive v-if="disableTransition"
          ><component :is="currentTabComponent" :key="currentTab"
        /></KeepAlive>
        <Transition v-else name="workspace-pane-fade" mode="out-in"
          ><KeepAlive><component :is="currentTabComponent" :key="currentTab" /></KeepAlive
        ></Transition>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { NIcon } from 'naive-ui'
import {
  Component as ComponentC,
  FunctionalComponent,
  computed,
  onActivated,
  ref,
  watch
} from 'vue'
import { useRoute, useRouter } from 'vue-router'

export interface TabConfig {
  key: string
  name: string
  icon: ComponentC | FunctionalComponent
  component: ComponentC
}

const props = defineProps<{
  icon: ComponentC | FunctionalComponent
  title: string
  tabs: TabConfig[]
  disableTransition?: boolean

  /**
   * 暂时强制要求路由必须提供 route name
   */
  routeName: string
  defaultTab?: string
}>()

const currentTab = ref(props.defaultTab ?? props.tabs[0]?.key ?? '')

const currentTabComponent = computed(() => {
  const tab = props.tabs.find((t) => t.key === currentTab.value)
  return tab?.component
})

const activeTab = computed(() => props.tabs.find((tab) => tab.key === currentTab.value))
const currentTabIndex = computed(() =>
  Math.max(
    0,
    props.tabs.findIndex((tab) => tab.key === currentTab.value)
  )
)

const route = useRoute()
const router = useRouter()

onActivated(() => {
  router.replace({ name: props.routeName, params: { section: currentTab.value } })
})

watch(
  () => currentTab.value,
  (cur) => {
    router.replace({ name: props.routeName, params: { section: cur } })
  },
  { immediate: true }
)

// route to section
watch(
  [() => route.name, () => route.params.section],
  ([name, section]) => {
    if (name !== props.routeName || !section) {
      return
    }

    currentTab.value = section as string
  },
  { immediate: true }
)
</script>

<style scoped>
.workspace-page {
  height: 100%;
  min-width: 0;
  display: grid;
  grid-template-columns: 184px minmax(0, 1fr);
  grid-template-rows: 94px minmax(0, 1fr);
  padding: 0 28px 24px;
  gap: 0 24px;
  box-sizing: border-box;
}
.workspace-page__heading {
  grid-column: 1/-1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--la-border);
  margin-bottom: 20px;
}
.workspace-page__identity {
  display: flex;
  align-items: center;
  gap: 12px;
}
.workspace-page__identity .n-icon {
  font-size: 23px;
  color: var(--la-text-secondary);
}
.workspace-page h1 {
  line-height: 1.4;
  font-size: 24px;
  font-weight: 600;
  margin: 0;
  letter-spacing: -0.6px;
}
.workspace-page__index {
  color: var(--la-text-secondary);
  font:
    11px/1.4 'Segoe UI',
    sans-serif;
  font-variant-numeric: tabular-nums;
}
.workspace-sections {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-height: 0;
  overflow: auto;
  border-right: 1px solid var(--la-border);
  padding-right: 15px;
}
.workspace-section {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--la-text-secondary);
  line-height: 1.5;
  font-family: inherit;
  font-size: 12px;
  text-align: left;
  padding: 11px 10px;
  cursor: pointer;
  flex-shrink: 0;
}
.workspace-section .n-icon {
  font-size: 16px;
  flex-shrink: 0;
}
.workspace-section small {
  margin-left: auto;
  opacity: 0.45;
  font-size: 9px;
  font-variant-numeric: tabular-nums;
}
.workspace-section.active {
  background: rgb(var(--la-card-tint-rgb) / 0.08);
  color: var(--la-color-text-primary);
}
.workspace-section:hover {
  background: rgb(var(--la-card-tint-rgb) / 0.05);
}
.workspace-section:focus-visible {
  outline: 2px solid var(--la-color-link);
  outline-offset: -2px;
}
.workspace-pane {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.workspace-pane__heading {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 9px 0 19px;
  flex-shrink: 0;
}
.workspace-pane__heading h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.workspace-pane__dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--la-color-link);
}
.workspace-pane__body {
  position: relative;
  min-height: 0;
  min-width: 0;
  flex: 1;
}
.workspace-pane__body :deep(.workspace-content) {
  padding: 0 8px 28px 0;
  max-width: none;
}
.workspace-pane-fade-enter-active,
.workspace-pane-fade-leave-active {
  transition: opacity 0.12s;
}
.workspace-pane-fade-enter-from,
.workspace-pane-fade-leave-to {
  opacity: 0;
}
@media (max-width: 1100px) {
  .workspace-page {
    grid-template-columns: 160px minmax(0, 1fr);
    padding-inline: 22px;
    gap: 0 18px;
  }
  .workspace-section {
    padding: 10px 8px;
    font-size: 11px;
  }
}
@media (max-width: 900px) {
  .workspace-page {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: 80px auto minmax(0, 1fr);
    padding-inline: 18px;
  }
  .workspace-sections {
    flex-direction: row;
    overflow-x: auto;
    border-right: 0;
    border-bottom: 1px solid var(--la-border);
    padding: 0 0 10px;
    margin-bottom: 12px;
    gap: 5px;
  }
  .workspace-section {
    width: auto;
    padding: 7px 10px;
  }
  .workspace-section small {
    display: none;
  }
  .workspace-pane__heading {
    padding: 5px 0 12px;
  }
  .workspace-page h1 {
    font-size: 22px;
  }
}
</style>
