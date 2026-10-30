<template>
  <NPopover :delay="delay" :keep-alive-on-hover="keepAliveOnHover">
    <template #trigger>
      <slot />
    </template>
    <RiftFlowScorePopoverContent :score="score" :total-precision="totalPrecision" />
  </NPopover>
</template>

<script setup lang="ts">
import type { RiftFlowScore } from '@shared/data-adapter/analysis/player'
import { NPopover } from 'naive-ui'

import RiftFlowScorePopoverContent from './RiftFlowScorePopoverContent.vue'

withDefaults(
  defineProps<{
    score: RiftFlowScore
    delay?: number
    keepAliveOnHover?: boolean
    totalPrecision?: number
  }>(),
  {
    delay: 50,
    keepAliveOnHover: false,
    totalPrecision: 2
  }
)
</script>
