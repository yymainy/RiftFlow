<template>
  <div class="h-full w-full">
    <NScrollbar class="relative h-full max-w-full">
      <div class="workspace-content flex flex-col">
        <LobbyTool />
        <StrawberryTool />
      </div>
    </NScrollbar>
  </div>
</template>

<script setup lang="ts">
import { NScrollbar } from 'naive-ui'

import LobbyTool from './LobbyTool.vue'
import StrawberryTool from './StrawberryTool.vue'
</script>
