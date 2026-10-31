<template><div class="ai-response markdown-body" v-html="renderAiMarkdown(text)"></div></template>
<script setup lang="ts">
import { renderAiMarkdown } from './markdown'
defineProps<{ text: string }>()
</script>
<style scoped>
.ai-response {
  font-size: 14px;
  line-height: 1.8;
  overflow-wrap: anywhere;
  background: transparent;
  color: inherit;
}
.ai-response :deep(p) {
  margin: 0 0 12px;
}
.ai-response :deep(ul),
.ai-response :deep(ol) {
  margin: 8px 0 12px;
  padding-left: 22px;
}
.ai-response :deep(h1),
.ai-response :deep(h2),
.ai-response :deep(h3) {
  font-size: 15px;
  margin: 16px 0 8px;
}
.ai-response :deep(pre) {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.ai-response :deep(table) {
  display: block;
  overflow-x: auto;
}
</style>
