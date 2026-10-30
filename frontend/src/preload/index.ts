import { electronAPI } from '@electron-toolkit/preload'
import { contextBridge } from 'electron'

const riftflowWindowType = process.argv
  .find((arg) => arg.startsWith('--riftflow-window-type='))
  ?.split('=')[1]

if (process.contextIsolated) {
  try {
    contextBridge.exposeInMainWorld('electron', electronAPI)
    contextBridge.exposeInMainWorld('riftflowWindowType', riftflowWindowType)
  } catch (error) {
    console.error('preload', error)
  }
} else {
  // @ts-ignore (define in dts)
  window.electron = electronAPI
  // @ts-ignore (define in dts)
  window.riftflowWindowType = riftflowWindowType
}
