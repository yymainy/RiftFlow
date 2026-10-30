export class RiftFlowIpcError extends Error {
  constructor(
    message: string,
    public readonly code: string
  ) {
    super(message)
    this.name = 'RiftFlowIpcError'
  }
}

export interface IpcMainSuccessDataType<T = any> {
  success: true
  data: T
}

export interface IpcMainErrorDataType {
  success: false
  isAxiosError?: boolean
  error: any
}

export type IpcMainDataType<T = any> = IpcMainSuccessDataType<T> | IpcMainErrorDataType
