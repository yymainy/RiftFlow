import { useInstance } from '@renderer-shared/shards'
import { AppCommonRenderer } from '@renderer-shared/shards/app-common'
import { SelfUpdateRenderer } from '@renderer-shared/shards/self-update'
import { useSelfUpdateStore } from '@renderer-shared/shards/self-update/store'
import { useTranslation } from 'i18next-vue'
import { useNotification } from 'naive-ui'
import { defineComponent, watch } from 'vue'

import { type SimpleNotificationsRendererContext } from './context'
import UpdateModal from './modals/UpdateModal.vue'
import WithActions from './parts/WithActions.vue'
import { useSimpleNotificationsStore } from './store'

export function registerNewReleaseModal(context: SimpleNotificationsRendererContext) {
  const Component = defineComponent({
    setup() {
      const simpleNotificationsStore = useSimpleNotificationsStore()
      const selfUpdateStore = useSelfUpdateStore()
      const selfUpdate = useInstance(SelfUpdateRenderer)
      const appCommon = useInstance(AppCommonRenderer)
      const notification = useNotification()

      const { t } = useTranslation(undefined, {
        keyPrefix: 'notifications.simple.newReleaseHints'
      })

      let lastNotifiedVersion: string | null = null

      watch(
        () => [selfUpdateStore.releaseInfo, selfUpdateStore.settings.ignoreVersion] as const,
        ([release, ignoreVersion]) => {
          if (!release || !release.isNew || ignoreVersion === release.version) {
            return
          }

          if (lastNotifiedVersion === release.version) {
            return
          }

          lastNotifiedVersion = release.version
          const inst = notification.info({
            title: () => t('title'),
            content: () => (
              <WithActions
                buttons={[
                  {
                    label: () => t('dismiss'),
                    secondary: true,
                    onClick: () => {
                      inst.destroy()
                    }
                  },
                  {
                    label: () => t('takeALook'),
                    type: 'primary',
                    onClick: () => {
                      simpleNotificationsStore.showNewReleaseModal = true
                      inst.destroy()
                    }
                  }
                ]}
              >
                <span>{t('content', { version: release.version })}</span>
              </WithActions>
            ),
            duration: 0
          })
        },
        { immediate: true }
      )

      watch(
        () => selfUpdateStore.updateProgressInfo?.phase,
        (phase, previous) => {
          if (phase === 'waiting-for-restart' && previous !== phase) {
            notification.success({
              title: '更新已准备好',
              content: '更新包校验通过。点击右上角更新图标，可选择重启完成更新。',
              duration: 0
            })
          }
        }
      )
      appCommon.onRendererLink((url) => {
        const u = new URL(url)

        if (u.pathname === '/overlays/release-modal') {
          simpleNotificationsStore.showNoticeModal = false
          simpleNotificationsStore.showNewReleaseModal = true
        }
      })

      return () => (
        <UpdateModal
          {...{
            release: selfUpdateStore.releaseInfo,
            show: simpleNotificationsStore.showNewReleaseModal,
            ignoreVersion: selfUpdateStore.settings.ignoreVersion,
            updateProgressInfo: selfUpdateStore.updateProgressInfo,
            'onUpdate:show': (value: boolean) =>
              (simpleNotificationsStore.showNewReleaseModal = value),
            onIgnoreVersion: (version: string, ignore: boolean) => {
              selfUpdate.setIgnoreVersion(ignore ? version : null)
            },
            onRestart: () => {
              void appCommon.exit()
            },
            onCancelDownload: () => {
              void selfUpdate.cancelUpdate()
            },
            onStartDownload: async () => {
              const result = (await selfUpdate.startUpdate()) as { result: string; reason?: string }
              if (result.result === 'failed')
                notification.error({
                  title: '更新下载失败',
                  content: result.reason || '请稍后重试'
                })
            }
          }}
        />
      )
    }
  })

  context.setupInAppScope.addRenderVNode(() => <Component />)
}
