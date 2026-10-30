import type { RiftFlowIpcRenderer } from '../ipc'
import type { LoggerRenderer } from '../logger'
import type { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'

export const CLIENT_INSTALLATION_MAIN_NAMESPACE = 'client-installation-main'
export const CLIENT_INSTALLATION_RENDERER_NAMESPACE = 'client-installation-renderer'

export interface ClientInstallationRendererContext {
  ipc: RiftFlowIpcRenderer
  logger: LoggerRenderer
  piniaMobxUtils: PiniaMobxUtilsRenderer
}
