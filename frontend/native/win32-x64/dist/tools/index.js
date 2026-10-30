"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.RiftFlowNativeTools = void 0;
const addon_binding_1 = require("../addon-binding");
const addon = new addon_binding_1.NativeAddonBinding('tools', () => require('../../addons/riftflow-tools-win64.node'));
class RiftFlowNativeTools {
    load() {
        addon.load();
    }
    isLoaded() {
        return addon.isLoaded();
    }
    fixWindowMethodA(clientZoom, config) {
        return addon.get().fixWindowMethodA(clientZoom, config);
    }
    isElevated() {
        return addon.get().isElevated();
    }
    getLeagueClientWindowPlacementInfo() {
        return addon.get().getLeagueClientWindowPlacementInfo();
    }
    getCommandLine1(pid) {
        return addon.get().getCommandLine1(pid);
    }
    getPidsByName(processName) {
        return addon.get().getPidsByName(processName);
    }
    terminateProcess(pid) {
        return addon.get().terminateProcess(pid);
    }
    isProcessForeground(pid) {
        return addon.get().isProcessForeground(pid);
    }
    isProcessRunning(pid) {
        return addon.get().isProcessRunning(pid);
    }
}
exports.RiftFlowNativeTools = RiftFlowNativeTools;
const tools = new RiftFlowNativeTools();
exports.default = tools;
