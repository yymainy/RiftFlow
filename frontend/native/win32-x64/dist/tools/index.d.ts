import type { RiftFlowToolsBinding, FixLeagueClientWindowConfig, LeagueClientWindowPlacementInfo } from '../bindings';
export type { FixLeagueClientWindowConfig, LeagueClientWindowPlacementInfo };
export declare class RiftFlowNativeTools implements RiftFlowToolsBinding {
    load(): void;
    isLoaded(): boolean;
    fixWindowMethodA(clientZoom: number, config: FixLeagueClientWindowConfig): boolean | null;
    isElevated(): boolean;
    getLeagueClientWindowPlacementInfo(): LeagueClientWindowPlacementInfo | null;
    getCommandLine1(pid: number): string;
    getPidsByName(processName: string): number[];
    terminateProcess(pid: number): boolean;
    isProcessForeground(pid: number): boolean;
    isProcessRunning(pid: number): boolean;
}
declare const tools: RiftFlowNativeTools;
export default tools;
