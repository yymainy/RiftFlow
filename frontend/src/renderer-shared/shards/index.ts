import { RiftFlowManager, Constructor } from '@shared/riftflow-shard'
import { App, getCurrentInstance } from 'vue'

import { LoggerRenderer } from './logger'

declare module 'vue' {
  export interface ComponentCustomProperties {
    $riftflowManager: RiftFlowManager
  }
}

declare global {
  interface Window {
    riftflowManager: RiftFlowManager
  }
}

/**
 * Vue 版本插件工厂, 特别地, 日志工具被内置到支持中
 * @returns
 */
export function createManager() {
  const riftflowManager = new RiftFlowManager()

  return {
    install: (app: App) => {
      app.config.globalProperties.$riftflowManager = riftflowManager
      app.config.errorHandler = (err, _instance, info) => {
        const logger = riftflowManager.getInstance('logger-renderer') as LoggerRenderer
        logger?.error('Vue', err, info)
      }

      window.riftflowManager = riftflowManager
    },
    setup: () => riftflowManager.setup(),
    getInstance: (id: string) => riftflowManager.getInstance(id),
    use: (shard: Constructor, config?: object) => riftflowManager.use(shard, config)
  }
}

/**
 * 批判的使用
 * @param id
 * @returns
 */
export function useInstance(id: string): any
export function useInstance<T extends new (...args: any[]) => any>(Ctor: T): InstanceType<T>
export function useInstance(idOrCtor: any): any {
  const ctx = getCurrentInstance()

  if (!ctx) {
    throw new Error('useInstance must be called within a setup function')
  }

  if (!ctx.appContext.config.globalProperties.$riftflowManager) {
    throw new Error('RiftFlowManager not found in app context')
  }

  const ins = ctx.appContext.config.globalProperties.$riftflowManager.getInstance(idOrCtor)

  if (!ins) {
    throw new Error(`Shard with id (or constructor) "${idOrCtor}" not instantiated`)
  }

  return ins
}
