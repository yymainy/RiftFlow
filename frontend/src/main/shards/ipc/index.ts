import { IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'
import type { IpcMainInvokeEvent, WebContents } from 'electron'

import { RIFTFLOW_IPC_MAIN_NAMESPACE } from './context'
import { RiftFlowIpcInvocationRouter } from './invocation-router'

export {
  RiftFlowIpcError,
  type IpcMainDataType,
  type IpcMainErrorDataType,
  type IpcMainSuccessDataType
} from './types'

/**
 * RiftFlow 的 IPC 主进程实现
 */
@Shard(RiftFlowIpcMain.id)
export class RiftFlowIpcMain implements IRiftFlowShardInitDispose {
  static id = RIFTFLOW_IPC_MAIN_NAMESPACE

  private readonly _router = new RiftFlowIpcInvocationRouter()

  async onInit() {
    this._router.attach()
  }

  async onDispose() {
    this._router.detach()
  }

  /**
   * 发送到所有已注册的渲染进程, 事件名使用 kebab-case
   */
  sendEvent(namespace: string, eventName: string, ...args: any[]) {
    this._router.sendEvent(namespace, eventName, ...args)
  }

  sendEventToWebContents(w: number, namespace: string, eventName: string, ...args: any[]): void
  sendEventToWebContents(w: WebContents, namespace: string, eventName: string, ...args: any[]): void
  sendEventToWebContents(
    w: WebContents | number,
    namespace: string,
    eventName: string,
    ...args: any[]
  ) {
    if (typeof w === 'number') {
      this._router.sendEventToWebContents(w, namespace, eventName, ...args)
    } else {
      this._router.sendEventToWebContents(w, namespace, eventName, ...args)
    }
  }

  /**
   * 处理来自渲染进程的调用, 方法名使用 camelCase
   * @param cb
   */
  onCall(
    namespace: string,
    fnName: string,
    cb: (event: IpcMainInvokeEvent, ...args: any[]) => Promise<any> | any
  ) {
    this._router.onCall(namespace, fnName, cb)
  }

  /**
   * 获取已注册的渲染进程 ID 列表
   */
  getRegisteredRendererIds() {
    return this._router.getRegisteredRendererIds()
  }
}
