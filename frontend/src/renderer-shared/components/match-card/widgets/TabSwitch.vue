<template>
  <div
    role="tablist"
    class="box-border flex min-h-8 items-center gap-1 rounded-md bg-(--la-card-muted-surface) p-1"
  >
    <button
      type="button"
      role="tab"
      :aria-selected="selectedTab === tab.value"
      v-for="tab of tabs"
      :key="tab.value"
      :class="[selectedTab === tab.value ? tabClass.selected : tabClass.unselected]"
      class="flex min-h-6 flex-1 cursor-pointer items-center justify-center rounded border-0 bg-transparent px-2 py-1 font-[inherit] text-xs leading-4 transition-colors focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-current"
      @click="handleTabClick(tab.value)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>

<script lang="ts" setup>
import { useWinResultTabSwitchClass } from '../utils/theme'

const { tabs = [], winResult } = defineProps<{
  tabs?: {
    label: string
    value: number | string
  }[]
  winResult?: string
}>()

const selectedTab = defineModel<number | string>('selectedTab', { required: false })

const handleTabClick = (tab: number | string) => {
  selectedTab.value = tab
}

const tabClass = useWinResultTabSwitchClass(() => winResult)
</script>

<style scoped></style>
