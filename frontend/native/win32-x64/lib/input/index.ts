import { EventEmitter } from 'node:events'

import { NativeAddonBinding } from '../addon-binding'
import type { RiftFlowInputBinding, NativeKeyState } from '../bindings'
import { KeyDefinition, VKEY_MAP, isCommonModifierKey, isModifierKey } from './definitions'

export interface KeyEvent extends KeyDefinition {
  keyCode: number
  isModifier: boolean
  isCommonModifier: boolean
  isDown: boolean
}

export type KeyState = NativeKeyState

const addon = new NativeAddonBinding<RiftFlowInputBinding>('input', () =>
  require('../../addons/riftflow-input-win64.node')
)

export function load(): void {
  addon.load()
}

export function isLoaded(): boolean {
  return addon.isLoaded()
}

export class RiftFlowNativeInput extends EventEmitter<{
  keyEvent: [definition: KeyEvent]
}> {
  private static instance: RiftFlowNativeInput

  private installed: boolean = false

  private constructor() {
    super()
  }

  public static getInstance(): RiftFlowNativeInput {
    if (!RiftFlowNativeInput.instance) {
      RiftFlowNativeInput.instance = new RiftFlowNativeInput()
    }
    return RiftFlowNativeInput.instance
  }

  get isInstalled() {
    return this.installed
  }

  install() {
    const binding = addon.get()

    if (this.installed) {
      console.warn('Input hook is already installed')
      return
    }

    try {
      binding.install()
      binding.onKeyEvent(this._handleNativeKeyEvent.bind(this))
      this.installed = true
    } catch (error) {
      binding.uninstall()
      throw error
    }
  }

  private _handleNativeKeyEvent(rawData: string): void {
    const [keyCodeRaw, state] = rawData.split(',')

    const keyCode = Number.parseInt(keyCodeRaw, 10)
    const definition = VKEY_MAP[keyCode]
    if (!definition) {
      return
    }

    const isDown = state === 'DOWN'

    const isModifier = isModifierKey(keyCode)
    const isCommonModifier = isCommonModifierKey(keyCode)

    this.emit('keyEvent', {
      ...definition,
      keyCode,
      isModifier,
      isCommonModifier,
      isDown
    })
  }

  uninstall() {
    const binding = addon.get()

    if (!this.installed) {
      console.warn('Input hook is not installed')
      return
    }

    binding.uninstall()
    this.installed = false
  }

  getKeyStates(): KeyState[] {
    return addon.get().getKeyStates()
  }

  sendKey(key: number, press: boolean): Promise<void> {
    const binding = addon.get()

    if (!Number.isInteger(key) || key < 0 || key > 255) {
      return Promise.reject(new RangeError('Virtual key code must be an integer between 0 and 255'))
    }

    return binding.sendKey(key, press)
  }

  sendString(str: string): Promise<void> {
    return addon.get().sendString(str)
  }
}

export const instance = RiftFlowNativeInput.getInstance()

export * from './definitions'
