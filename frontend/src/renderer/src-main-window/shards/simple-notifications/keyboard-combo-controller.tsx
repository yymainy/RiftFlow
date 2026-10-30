import RiftFlowSpan from '@renderer-shared/components/RiftFlowSpan.vue'
import { useKeyboardCombo } from '@renderer-shared/composables/useKeyboardCombo'
import { useMessage } from 'naive-ui'

import { useSimpleNotificationsStore } from './store'

export function watchSpecialKeyboardCombo() {
  const message = useMessage()
  const simpleNotificationsStore = useSimpleNotificationsStore()

  useKeyboardCombo('RIFTFLOW', {
    onFinish: () => {
      if (simpleNotificationsStore.showDeclarationModal) {
        simpleNotificationsStore.showDeclarationModal = false
      } else {
        message.success(() => <RiftFlowSpan bold />)
      }
    },
    requireSameEl: true,
    caseSensitive: false,
    timeout: 250
  })
}
