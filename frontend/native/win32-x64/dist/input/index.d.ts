import { EventEmitter } from 'node:events';
import type { NativeKeyState } from '../bindings';
import { KeyDefinition } from './definitions';
export interface KeyEvent extends KeyDefinition {
    keyCode: number;
    isModifier: boolean;
    isCommonModifier: boolean;
    isDown: boolean;
}
export type KeyState = NativeKeyState;
export declare function load(): void;
export declare function isLoaded(): boolean;
export declare class RiftFlowNativeInput extends EventEmitter<{
    keyEvent: [definition: KeyEvent];
}> {
    private static instance;
    private installed;
    private constructor();
    static getInstance(): RiftFlowNativeInput;
    get isInstalled(): boolean;
    install(): void;
    private _handleNativeKeyEvent;
    uninstall(): void;
    getKeyStates(): KeyState[];
    sendKey(key: number, press: boolean): Promise<void>;
    sendString(str: string): Promise<void>;
}
export declare const instance: RiftFlowNativeInput;
export * from './definitions';
