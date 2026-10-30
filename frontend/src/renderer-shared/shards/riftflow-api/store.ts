import type {
  RiftFlowContactChannels,
  RiftFlowFeatureGateSnapshot,
  RiftFlowNotice
} from '@shared/shards/riftflow-api'
import { defineStore } from 'pinia'
import { ref, shallowRef } from 'vue'

export const useRiftFlowApiStore = defineStore('shard:riftflow-api-renderer', () => {
  const featureGates = shallowRef<RiftFlowFeatureGateSnapshot | null>(null)
  const notice = ref<RiftFlowNotice | null>(null)
  const contactChannels = ref<RiftFlowContactChannels | null>(null)

  return {
    featureGates,
    notice,
    contactChannels
  }
})
