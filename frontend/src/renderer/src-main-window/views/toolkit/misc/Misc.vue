<template>
  <div class="h-full w-full">
    <NScrollbar class="relative h-full max-w-full">
      <div class="workspace-content flex flex-col">
        <SummonerProfile />
        <ChatAvailability />
        <FakeRanked />
        <ChatStatusMessage />
        <GameView />
      </div>
    </NScrollbar>
  </div>
</template>

<script setup lang="ts">
import { NScrollbar } from 'naive-ui'

import ChatAvailability from './ChatAvailability.vue'
import ChatStatusMessage from './ChatStatusMessage.vue'
import FakeRanked from './FakeRanked.vue'
import GameView from './GameView.vue'
import SummonerProfile from './SummonerProfile.vue'
</script>
