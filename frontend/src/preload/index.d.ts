import { ElectronAPI } from '@electron-toolkit/preload'

declare global {
  interface Window {
    electron: ElectronAPI
    riftflowWindowType: string
  }

  const electron: ElectronAPI
}
