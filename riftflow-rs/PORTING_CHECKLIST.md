# RiftFlow → Rust 移植清单

后端（Electron main + preload + native addon）改写为 Rust，渲染层 77,493 行 Vue 不动。
本清单是逐条可核对的验收面：勾掉一项，代表 Rust 侧已实现 **且有测试或真机验证**。

## 交接：本轮结束时的真实状态（2026-10-27，turn 18/20）

工作区是干净可构建的：`cargo test --workspace` **569 passed / 0 failed**，`target/debug/riftflow-app.exe` 已是最新代码（07:56）并已在用户的真实 profile 上运行。**目标未达成，不要当成完成。**

已经坐实的（不是"写了"，是"验过"）：

- 契约面：router 注册 191 个方法 / 25 个命名空间；与 Electron 源按 `(namespace, method)` 对拍 **missing = 0**（`python tmp/contract_namespaces.py`，Rust 侧来自 `RIFTFLOW_DUMP_IPC_REGISTRY` 的运行时 dump）。全 crate 无任何 "not ported" 应答。
- 运行面：19 个真实面板带线上数据渲染，0 JS 异常、0 `[ipc:err]`、0 `[ipc:reject]`；UI 可达的 168 个方法里 **144 个被真实调用过并读回效果**（63 按钮 + 40 勾选框 + 89 次窗口调用 + saved-player 全回路 + 自动化/设置面）。重跑测量：`python tmp/coverage_audit.py`。
- 本轮修掉的真缺陷：`update*PresetOptions` 静默丢弃非法补丁（现在按原版形状抛 `ZodError`）、`fixWindowMethodA` 的 `NativeNotSupported` 桩（现在是 addon 算法的移植 + 未提权时与原版一致的静默 no-op）、shell 层最后五个 `NotImplemented`（日志文件、`openLogsDir`、剪贴板、`exit`、提权重启）、`checkUpdates` 恒失败、`riot-client` 协议臂。

剩下的三堆，按"要什么"分：

1. **只要用户点头就能关掉的**：`client-installation-main` 四个 launch（会真拉起 WeGame/TCLS）、`saved-player-main` 导入导出（要人手点原生文件对话框）、每窗口的 `close`/`toggleDevtools`/`setIgnoreMouseEvents`/`downloadUrl`、`uninstallApp`、`rebuildWmi`。工具已就位：照 `tmp/drive_settings_cluster.js` 的"调用 + 读回"形状加条目即可。
2. **必须有一局对局才能关掉的**：cd-timer `gameTime` 读数、对局内 SendInput 实战、客户端窗口可见时的贴边、选人阶段 auto-select、提权下 `fixWindowMethodA` 的真实缩放。另加键盘 hook 投递——需要先有非提权前台（对照组 `crates/riftflow-native/examples/hook_watcher.exe` 能一次判掉是不是输入根本没进系统）。
3. **另一个量级的**：第二阶段同栈 1:1 重写 renderer（cd-timer → ongoing-game → aux → opgg → main）。现在跑的还是原版打包的 renderer。第一步是用 `ipc-contract.json` 生成 typed IPC mirror，再逐窗替换、逐窗截图比对。

复现命令（都不依赖前台窗口）：`cargo test --workspace`；起实例
`APPDATA="C:\Users\Schmoll\Desktop\Refit\riftflow-rs\tmp\verify-profile" WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9223 --remote-allow-origins=*" RIFTFLOW_IPC_TRACE=1 ./target/verify/debug/riftflow-app.exe`
（**必须换 APPDATA**，否则与已运行实例抢 `EBWebView` 会 `0x8007139F` 建不出窗口）；
驱动 `MSYS2_ARG_CONV_EXCL='*' python tmp/cdp_sweep.py` / `tmp/cdp_click_all.py` / `tmp/cdp_toggle_sweep.py` / `tmp/drive_windows.py <脚本.js>`。


## 已定架构决策

| 决策 | 取值 | 依据 |
|---|---|---|
| Rust 承担的角色 | Tauri 2 核心替换 Electron main + preload | 渲染层对 Electron 的耦合只有 `preload/index.ts` 20 行与 `call-service.ts:11` 一处调用点 |
| 渲染层 | 原样复用，靠 `window.electron.ipcRenderer` 兼容垫片接入 | 目标是"一模一样" |
| IPC 线格式 | 完全保持：`riftflowCall(ns, fn, ...args)` → `{success,data}` / `{success:false,error,isAxiosError?}`；事件走 `riftflow-event`，载荷 `[ns, eventName, ...args]` | `invocation-router.ts:31-47,109` |
| 未知方法 | 传输层 reject（不是错误信封） | `invocation-router.ts:83` 在标准化之外 throw |
| 提权策略 | 普通权限启动 + 可选"以管理员重启"，能力按 `NATIVE_SUPPORT.available` 优雅降级 | 原项目语义；且本机真机验证坐实客户端由 WeGame 提权启动 |
| 状态同步 | 用 `riftflow-state` 的 prop 存储取代 MobX `propSync`，事件名 `update-state-prop/<ns>:<stateId>` | `mobx-utils/index.ts:110,149` |

## 里程碑状态

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M1 | radix / shard 运行时 / IPC 契约 / 状态同步 / 存储与设置 | ✅ 106 测试全绿，0 warning |
| M2 | `riftflow-lcu` 连接层 + Tauri 壳 + 渲染层垫片 + 5 窗口 | ✅ 连接层与 IPC 穿越均真机验证；**5 个窗口尚未全部接线**（当前只起 main 窗口） |
| M3 | 选锁 / 自动接受 / 复活计时端到端 | 🔨 真 Vue 界面已在 Rust 后端上完整启动（截图确认），IPC 零拒绝、零 JS 报错；功能 RPC 与其余 4 窗口待接 |
| M4 | 123 个 IPC 方法 + 33 组事件 + SGP/OP.GG/Riot 全量 | 🔨 已交付 `riftflow://` 代理（LCU HTTP 全量走真数据）+ window-manager 19 方法 + ongoing-game 4 方法 + renderer-debug 2 方法 |
| M5 | SendInput 局内发消息、窗口修正、托盘热键、自更新换轨 | ⬜ |

### M2 壳层真机验证（`RIFTFLOW_IPC_TRACE=1 ./target/debug/riftflow-app.exe`）

渲染层 `window.electron.ipcRenderer` 由 shim.rs 注入脚本模拟，Vue 代码零改动：

| 验证点 | 结果 |
|---|---|
| `invoke('riftflowRendererRegister','register')` | `{"success":true}`，无 `data` 键，与原实现一致 |
| `invoke('riftflowCall','app-common-main','getVersion')` | `{success:true,data:"1.5.1"}` |
| 未知方法 | promise **reject**，消息 `No function "x" in namespace "y"` ✓ 契约保持 |
| `subscribeLcuEndpoint('/**')` | 返回 `__1` |
| 自动连接 | 普通权限下 `[shell] connecting to pid=14264 port=52611`，凭据来自原生读取 |
| 渲染层 → Rust → 真 LCU | `经 IPC 的 LCU HTTP: 200 ok` |
| Rust → 渲染层推送 | 4 帧真实 LCU 事件进入 webview（`/lol-regalia/...`、`/lol-hovercard/...` 等 4 个 uri） |

诊断脚手架（一次性 bridge 自检、`shell_ping`、page-load 日志）已拆除，仅保留 `RIFTFLOW_IPC_TRACE` 与 `RIFTFLOW_LCU_TRACE` 两个环境开关。

Tauri 2 + `tauri-plugin-global-shortcut` + `webview2-com` 已在本机 MSVC 工具链下 53 秒编译通过，依赖已缓存。

### M3 真界面启动验证（同一命令，`RIFTFLOW_IPC_TRACE=1`）

真 `renderer-dist`（未改一行 Vue）在 Rust 后端上渲染出主窗口：无边框深色标题栏、侧栏 5 项（Match History / Ongoing Game / Automation / Toolkit / Settings）、版本号 `v1.5.1` 取自 `app-common-main/getVersion`；日志中 **0 条 `js-error` / `js-reject` / IPC 拒绝**，LCU 好友列表 `200 (242786 bytes)` 为真数据。

| 关键决定 | 依据 |
|---|---|
| `riftflow://` 代理改为 **IPC 命令** `riftflow_protocol_fetch`（protocol.rs），而不是只靠 Tauri 自定义协议 | 实测：同一 URL 在 document-start 能被 WebView2 交给协议处理器，页面 `complete` 之后同类请求全部 `Failed to fetch` 且 Rust 侧无入口日志。因此 fetch/XHR 流量走命令通道，`<img>`/CSS url 走同一命令取字节转 `blob:`；协议处理器仍保留（导航与早期子资源可用） |
| 渲染层垫片改写 `fetch` / `XMLHttpRequest.open` / DOM 属性（`src`/`href`/`srcset`/`poster`/`style`/`xlink:href`）+ MutationObserver | 保持 `baseURL:'riftflow://league-client'`、`LcuImage src` 等原样，不改应用代码 |
| `respawn-timer-main/state.info` 必须是对象而非 `null` | `SidebarFixed.vue:5` 在 setup 里读 `rts.info.isDead`，`null` 直接崩掉主窗口；据此逐个回查了占位值形状 |
| 窗口默认 `show:false` + ready 后再显示、无边框、1500×860/min 840×600 | 逐窗口抄自 `*-window/window.ts` 的 `browserWindowOptions`；`close` 语义按 `handleClose`（隐藏而非关闭，`closeAction` 决定） |
| `opacity` / `contentProtection` / `downloadUrl` / `repositionToAlignLeagueClientUx` / `sendInGame` 显式返回未实现 | Tauri 核心无等价调用或需 M5 原生层，宁可报错也不假装成功 |

### M3 真数据接入（`lc_state.rs`）

点击"没反应"的根因不是输入，而是**没有 player tab**：`player-tabs` 以 `sgpServerId:puuid` 为 key，二者当时都是占位值，主窗口只能停在 splash。接入后截图确认：tab 标题 `TENCENT_HN1:15e2…`、左下召唤人 `锁骨能养小金鱼 #43532`、段位卡 / Queue 下拉 / 分页 / Page Filter 全部渲染，头像 `profile-icons/29.jpg` 经代理返回 200（96612 字节）。

| 接入项 | 来源 |
|---|---|
| `league-client-main` 的 summoner/gameflow/lobby/champSelect/matchmaking/entitlements/leagueSession 子状态 | 连接后一次性 GET + 事件推送（`_stateInitializer` + `events.on`） |
| `gameData` 九个 Record | `/lol-game-data/assets/v1/*.json` 数组按 `id` 建索引；`gameModeMutators` 按 **`MapId`**、`queues` 只保留第一个 id=0（原实现如此） |
| `sgp-main/state.availability`、`supportedQueues` | `getSgpServerId()`（TENCENT → `TENCENT_<platform>`，其余走别名表）+ `builtin.ts` 的 26 个服务器配置，已抽成 `assets/riftflow-builtin.json` 由 `include_str!` 编入 |
| 断连清理 | 子状态回到 `null` / `{}`，availability 归零（`_onLcuNotConnected`） |

**占位值形状教训**：`supportedQueues` 我按配置对象声明，实际是 computed 数组 → `n.supportedQueues.map is not a function`；据此把 13 个子状态的默认值逐个对回 `lc-state/*.ts` 的字段声明（Record 用 `{}`、可空对象用 `null`，只有真数组才用 `[]`）。

### M2 连接层真机验证（`cargo run -p riftflow-lcu --example live_connect`）

凭据取自 `riftflow/RiftFlow.db` 的 `lastConnectedClient`（非提权可读），走的是**移植后的生产路径**
（`ProductionDialer` → reqwest/rustls → tokio-tungstenite → radix → IPC 帧）：

| 验证点 | 结果 |
|---|---|
| HTTP ping + `current-summoner` + `gameflow-phase` | 200，`region=TENCENT` |
| wss 订阅 `OnJsonApiEvent` | 收到空文本 ack（len=0）并被容忍 |
| 真实事件帧 | 4 帧 `[8,"OnJsonApiEvent",{uri,…}]` 经 `/**` 路由命中 |
| IPC 转发帧 | `['league-client-main','extra-lcu-event','__3',payload,params]` 逐字正确 |
| 断连 | 状态回到 `Disconnected` |

`RIFTFLOW_LCU_TRACE=1` 可打印每一帧原文，用于区分"客户端安静"与"读线程没跑"。


## 数据可信度标注

- **自验**：我用脚本从源码机械抽取，并与独立统计核对到 123 条完全一致。
- **子 agent 报告（待逐条回查）**：SGP/Riot/OP.GG 端点表、native addon 函数表、设置注册表。按本项目既有规矩，实现每一项前先在源码里复核路径与行号。

---

## 一、IPC 方法面（自验：123 条注册点 / 21 个命名空间）

命名空间由所在 shard 目录推导为 `<dir>-main`，已与各 `context.ts` 常量核对。
`window-manager-main` 的注册实际落在子命名空间 `window-manager-main/<suffix>`，
suffix ∈ `main-window | aux-window | opgg-window | cd-timer-window | ongoing-game-window`，
其中 `base-riftflow-window.ts` 的 17 个方法**每个窗口实例各注册一次**（Rust 侧需 5 个实例）。

### 每命名空间方法数

| namespace | 方法数 |
|---|---|
| `riftflow-protocol-main` | 1 |
| `app-common-main` | 7 |
| `auto-champ-config-main` | 2 |
| `auto-gameflow-main` | 3 |
| `auto-misc-main` | 2 |
| `auto-select-main` | 3 |
| `client-installation-main` | 4 |
| `game-client-main` | 3 |
| `in-game-send-main` | 22 |
| `keyboard-shortcuts-main` | 5 |
| `league-client-main` | 8 |
| `league-client-ux-main` | 2 |
| `logger-factory-main` | 3 |
| `mobx-utils-main` | 1 |
| `ongoing-game-main` | 6 |
| `renderer-debug-main` | 2 |
| `riot-client-main` | 1 |
| `saved-player-main` | 13 |
| `self-update-main` | 6 |
| `setting-factory-main` | 6 |
| `window-manager-main` | 23 |

### 全量方法表

| 状态 | namespace | 方法 | 源位置 |
|---|---|---|---|
| - [x] | `riftflow-protocol-main` | `cancelProxyRequest` | shards/riftflow-protocol/index.ts:32 |
| - [ ] | `app-common-main` | `exit` | shards/app-common/ipc-handlers.ts:39 |
| - [ ] | `app-common-main` | `getRuntimeInfo` | shards/app-common/ipc-handlers.ts:35 |
| - [ ] | `app-common-main` | `getVersion` | shards/app-common/ipc-handlers.ts:23 |
| - [ ] | `app-common-main` | `openUserDataDir` | shards/app-common/ipc-handlers.ts:27 |
| - [ ] | `app-common-main` | `readClipboardText` | shards/app-common/ipc-handlers.ts:31 |
| - [ ] | `app-common-main` | `relaunchAsAdministrator` | shards/app-common/ipc-handlers.ts:19 |
| - [ ] | `app-common-main` | `setDisableHardwareAcceleration` | shards/app-common/ipc-handlers.ts:15 |
| - [ ] | `auto-champ-config-main` | `updateRunes` | shards/auto-champ-config/ipc-handlers.ts:14 |
| - [ ] | `auto-champ-config-main` | `updateSummonerSpells` | shards/auto-champ-config/ipc-handlers.ts:22 |
| - [x] | `auto-gameflow-main` | `cancelAutoAccept` | shards/auto-gameflow/ipc-handlers.ts:15 |
| - [x] | `auto-gameflow-main` | `cancelAutoMatchmaking` | shards/auto-gameflow/ipc-handlers.ts:19 |
| - [x] | `auto-gameflow-main` | `setFriendsToBeInvited` | shards/auto-gameflow/ipc-handlers.ts:23 |
| - [ ] | `auto-misc-main` | `applyRankedStatus` | shards/auto-misc/ipc-handlers.ts:19 |
| - [ ] | `auto-misc-main` | `applyStatusMessage` | shards/auto-misc/ipc-handlers.ts:15 |
| - [ ] | `auto-select-main` | `setBanConfig` | shards/auto-select/ipc-handlers.ts:21 |
| - [ ] | `auto-select-main` | `setPickConfig` | shards/auto-select/ipc-handlers.ts:12 |
| - [ ] | `auto-select-main` | `setTemporarilyDisabled` | shards/auto-select/ipc-handlers.ts:30 |
| - [ ] | `client-installation-main` | `launchDefaultRiotClient` | shards/client-installation/ipc-handlers.ts:19 |
| - [ ] | `client-installation-main` | `launchTencentTcls` | shards/client-installation/ipc-handlers.ts:11 |
| - [ ] | `client-installation-main` | `launchWeGame` | shards/client-installation/ipc-handlers.ts:15 |
| - [ ] | `client-installation-main` | `launchWeGameLeagueOfLegends` | shards/client-installation/ipc-handlers.ts:23 |
| - [ ] | `game-client-main` | `getSettingsFileReadonlyOrWritable` | shards/game-client/ipc-handlers.ts:25 |
| - [ ] | `game-client-main` | `setSettingsFileReadonlyOrWritable` | shards/game-client/ipc-handlers.ts:17 |
| - [ ] | `game-client-main` | `terminateGameClient` | shards/game-client/ipc-handlers.ts:13 |
| - [ ] | `in-game-send-main` | `clearPresetSelections` | shards/in-game-send/ipc-handlers.ts:175 |
| - [ ] | `in-game-send-main` | `createFixedTextPresetItem` | shards/in-game-send/ipc-handlers.ts:143 |
| - [ ] | `in-game-send-main` | `deleteFixedTextPresetItem` | shards/in-game-send/ipc-handlers.ts:155 |
| - [ ] | `in-game-send-main` | `generateJunglePresetLines` | shards/in-game-send/ipc-handlers.ts:51 |
| - [ ] | `in-game-send-main` | `generatePremadePresetLines` | shards/in-game-send/ipc-handlers.ts:59 |
| - [ ] | `in-game-send-main` | `generateRatingPresetLines` | shards/in-game-send/ipc-handlers.ts:43 |
| - [ ] | `in-game-send-main` | `moveFixedTextPresetItem` | shards/in-game-send/ipc-handlers.ts:159 |
| - [ ] | `in-game-send-main` | `sendFixedTextPreset` | shards/in-game-send/ipc-handlers.ts:91 |
| - [ ] | `in-game-send-main` | `sendJunglePreset` | shards/in-game-send/ipc-handlers.ts:75 |
| - [ ] | `in-game-send-main` | `sendLines` | shards/in-game-send/ipc-handlers.ts:39 |
| - [ ] | `in-game-send-main` | `sendPremadePreset` | shards/in-game-send/ipc-handlers.ts:83 |
| - [ ] | `in-game-send-main` | `sendRatingPreset` | shards/in-game-send/ipc-handlers.ts:67 |
| - [ ] | `in-game-send-main` | `setJunglePresetOptions` | shards/in-game-send/ipc-handlers.ts:103 |
| - [ ] | `in-game-send-main` | `setJunglePuuids` | shards/in-game-send/ipc-handlers.ts:167 |
| - [ ] | `in-game-send-main` | `setPremadeIndices` | shards/in-game-send/ipc-handlers.ts:171 |
| - [ ] | `in-game-send-main` | `setPremadePresetOptions` | shards/in-game-send/ipc-handlers.ts:111 |
| - [ ] | `in-game-send-main` | `setRatingPresetOptions` | shards/in-game-send/ipc-handlers.ts:95 |
| - [ ] | `in-game-send-main` | `setRatingPuuids` | shards/in-game-send/ipc-handlers.ts:35 |
| - [ ] | `in-game-send-main` | `updateFixedTextPresetItem` | shards/in-game-send/ipc-handlers.ts:147 |
| - [ ] | `in-game-send-main` | `updateJunglePresetOptions` | shards/in-game-send/ipc-handlers.ts:127 |
| - [ ] | `in-game-send-main` | `updatePremadePresetOptions` | shards/in-game-send/ipc-handlers.ts:135 |
| - [ ] | `in-game-send-main` | `updateRatingPresetOptions` | shards/in-game-send/ipc-handlers.ts:119 |
| - [ ] | `keyboard-shortcuts-main` | `_getInternalVars` | shards/keyboard-shortcuts/ipc-handlers.ts:35 |
| - [ ] | `keyboard-shortcuts-main` | `getDebugState` | shards/keyboard-shortcuts/ipc-handlers.ts:20 |
| - [ ] | `keyboard-shortcuts-main` | `getRegistration` | shards/keyboard-shortcuts/ipc-handlers.ts:13 |
| - [ ] | `keyboard-shortcuts-main` | `getRegistrationByTargetId` | shards/keyboard-shortcuts/ipc-handlers.ts:28 |
| - [ ] | `keyboard-shortcuts-main` | `setDebugStatefulShortcut` | shards/keyboard-shortcuts/ipc-handlers.ts:24 |
| - [ ] | `league-client-main` | `connect` | shards/league-client/ipc-handlers.ts:16 |
| - [ ] | `league-client-main` | `disconnect` | shards/league-client/ipc-handlers.ts:20 |
| - [ ] | `league-client-main` | `fixWindowMethodA` | shards/league-client/ipc-handlers.ts:32 |
| - [ ] | `league-client-main` | `http-request` | shards/league-client/ipc-handlers.ts:12 |
| - [ ] | `league-client-main` | `peekClient` | shards/league-client/ipc-handlers.ts:44 |
| - [ ] | `league-client-main` | `subscribeLcuEndpoint` | shards/league-client/ipc-handlers.ts:36 |
| - [ ] | `league-client-main` | `unsubscribeLcuEndpoint` | shards/league-client/ipc-handlers.ts:40 |
| - [ ] | `league-client-main` | `writeItemSetsToDisk` | shards/league-client/ipc-handlers.ts:24 |
| - [ ] | `league-client-ux-main` | `rebuildWmi` | shards/league-client-ux/ipc-handlers.ts:14 |
| - [ ] | `league-client-ux-main` | `update` | shards/league-client-ux/ipc-handlers.ts:13 |
| - [ ] | `logger-factory-main` | `log` | shards/logger-factory/ipc-handlers.ts:9 |
| - [ ] | `logger-factory-main` | `openLogsDir` | shards/logger-factory/ipc-handlers.ts:33 |
| - [ ] | `logger-factory-main` | `setLogLevel` | shards/logger-factory/ipc-handlers.ts:28 |
| - [ ] | `mobx-utils-main` | `subscribeAndGetInitialState` | shards/mobx-utils/ipc-handlers.ts:9 |
| - [x] | `ongoing-game-main` | `clearDraft` | shards/ongoing-game/ipc-handlers.ts:57 |
| - [x] | `ongoing-game-main` | `getAll` | shards/ongoing-game/ipc-handlers.ts:25 |
| - [ ] | `ongoing-game-main` | `reload` | shards/ongoing-game/ipc-handlers.ts:61 |
| - [ ] | `ongoing-game-main` | `reloadPlayer` | shards/ongoing-game/ipc-handlers.ts:65 |
| - [x] | `ongoing-game-main` | `setDraft` | shards/ongoing-game/ipc-handlers.ts:53 |
| - [x] | `ongoing-game-main` | `setMatchHistoryTagParams` | shards/ongoing-game/ipc-handlers.ts:45 |
| - [x] | `renderer-debug-main` | `setLogAllLcuEvents` | shards/renderer-debug/ipc-handlers.ts:13 |
| - [x] | `renderer-debug-main` | `setSendAllNativeLcuEvents` | shards/renderer-debug/ipc-handlers.ts:9 |
| - [ ] | `riot-client-main` | `http-request` | shards/riot-client/ipc-handlers.ts:11 |
| - [ ] | `saved-player-main` | `deleteEncounteredGame` | shards/saved-player/ipc-handlers.ts:54 |
| - [ ] | `saved-player-main` | `deleteSavedPlayer` | shards/saved-player/ipc-handlers.ts:46 |
| - [ ] | `saved-player-main` | `exportTaggedPlayersToJsonFile` | shards/saved-player/ipc-handlers.ts:82 |
| - [ ] | `saved-player-main` | `getAllPlayerTags` | shards/saved-player/ipc-handlers.ts:74 |
| - [ ] | `saved-player-main` | `getPlayerTags` | shards/saved-player/ipc-handlers.ts:62 |
| - [ ] | `saved-player-main` | `importTaggedPlayersFromJsonFile` | shards/saved-player/ipc-handlers.ts:96 |
| - [ ] | `saved-player-main` | `queryAllSavedPlayers` | shards/saved-player/ipc-handlers.ts:70 |
| - [ ] | `saved-player-main` | `queryEncounteredGames` | shards/saved-player/ipc-handlers.ts:50 |
| - [ ] | `saved-player-main` | `querySavedPlayer` | shards/saved-player/ipc-handlers.ts:30 |
| - [ ] | `saved-player-main` | `querySavedPlayerWithGames` | shards/saved-player/ipc-handlers.ts:34 |
| - [ ] | `saved-player-main` | `saveEncounteredGame` | shards/saved-player/ipc-handlers.ts:58 |
| - [ ] | `saved-player-main` | `saveSavedPlayer` | shards/saved-player/ipc-handlers.ts:42 |
| - [ ] | `saved-player-main` | `updatePlayerTag` | shards/saved-player/ipc-handlers.ts:66 |
| - [ ] | `self-update-main` | `cancelUpdate` | shards/self-update/ipc-handlers.ts:60 |
| - [ ] | `self-update-main` | `checkUpdates` | shards/self-update/ipc-handlers.ts:23 |
| - [ ] | `self-update-main` | `forceStartUpdate` | shards/self-update/ipc-handlers.ts:45 |
| - [ ] | `self-update-main` | `openNewUpdatesDir` | shards/self-update/ipc-handlers.ts:68 |
| - [ ] | `self-update-main` | `startUpdate` | shards/self-update/ipc-handlers.ts:31 |
| - [ ] | `self-update-main` | `uninstallApp` | shards/self-update/ipc-handlers.ts:77 |
| - [ ] | `setting-factory-main` | `exportSettingsToJsonFile` | shards/setting-factory/ipc-handlers.ts:46 |
| - [ ] | `setting-factory-main` | `get` | shards/setting-factory/ipc-handlers.ts:30 |
| - [ ] | `setting-factory-main` | `getByPrefix` | shards/setting-factory/ipc-handlers.ts:34 |
| - [ ] | `setting-factory-main` | `importSettingsFromJsonFile` | shards/setting-factory/ipc-handlers.ts:60 |
| - [ ] | `setting-factory-main` | `removeByPrefix` | shards/setting-factory/ipc-handlers.ts:38 |
| - [ ] | `setting-factory-main` | `set` | shards/setting-factory/ipc-handlers.ts:22 |
| - [x] | `window-manager-main` | `close` | shards/window-manager/base-riftflow-window.ts:221 |
| - [x] | `window-manager-main` | `closeMainWindow` | shards/window-manager/main-window/window.ts:105 |
| - [x] | `window-manager-main` | `closeMainWindowForce` | shards/window-manager/main-window/window.ts:110 |
| - [ ] | `window-manager-main` | `downloadUrl` | shards/window-manager/base-riftflow-window.ts:249 |
| - [x] | `window-manager-main` | `getPosition` | shards/window-manager/base-riftflow-window.ts:201 |
| - [x] | `window-manager-main` | `getSize` | shards/window-manager/base-riftflow-window.ts:193 |
| - [x] | `window-manager-main` | `hide` | shards/window-manager/base-riftflow-window.ts:233 |
| - [x] | `window-manager-main` | `maximize` | shards/window-manager/base-riftflow-window.ts:209 |
| - [x] | `window-manager-main` | `minimize` | shards/window-manager/base-riftflow-window.ts:205 |
| - [ ] | `window-manager-main` | `repositionToAlignLeagueClientUx` | shards/window-manager/aux-window/window.ts:159 |
| - [ ] | `window-manager-main` | `repositionToAlignLeagueClientUx` | shards/window-manager/opgg-window/window.ts:100 |
| - [x] | `window-manager-main` | `repositionWindowIfInvisible` | shards/window-manager/base-riftflow-window.ts:245 |
| - [x] | `window-manager-main` | `resetPosition` | shards/window-manager/base-riftflow-window.ts:241 |
| - [x] | `window-manager-main` | `restore` | shards/window-manager/base-riftflow-window.ts:217 |
| - [ ] | `window-manager-main` | `sendInGame` | shards/window-manager/cd-timer-window/windows.ts:233 |
| - [x] | `window-manager-main` | `setIgnoreMouseEvents` | shards/window-manager/base-riftflow-window.ts:254 |
| - [x] | `window-manager-main` | `setPosition` | shards/window-manager/base-riftflow-window.ts:197 |
| - [x] | `window-manager-main` | `setSize` | shards/window-manager/base-riftflow-window.ts:189 |
| - [x] | `window-manager-main` | `setTitle` | shards/window-manager/base-riftflow-window.ts:229 |
| - [x] | `window-manager-main` | `setTrafficLightPosition` | shards/window-manager/main-window/window.ts:114 |
| - [x] | `window-manager-main` | `show` | shards/window-manager/base-riftflow-window.ts:237 |
| - [x] | `window-manager-main` | `toggleDevtools` | shards/window-manager/base-riftflow-window.ts:225 |
| - [x] | `window-manager-main` | `unmaximize` | shards/window-manager/base-riftflow-window.ts:213 |

---

## 二、事件面（子 agent 报告，待逐条回查）

`sendEvent` / `sendEventToWebContents` 共 73 处调用点、33 组命名空间+事件名。重点项：

- `mobx-utils-main` → `update-state-prop/<ns>:<stateId>`，载荷 `[path, value, {action, raw}]`，**只推给已订阅的渲染进程**（懒订阅模型必须保留，否则每次状态变动都会广播到 5 个窗口）。
- `league-client-main` → `extra-lcu-event` `[subId, data, params]`；`error-connecting` `[message]`；`error-sync-data` `[endpointLabel]`（29 处调用点）。
- `ongoing-game-main` → `summoner-loaded/removed`、`ranked-stats-loaded/removed`、`saved-info-loaded/removed`、`champion-mastery-loaded/removed`、`game-details-loaded/removed`、`match-history-loaded/removed`、`additional-game-loaded`、`clear`。
- `keyboard-shortcuts-main` → `shortcut`、`last-active-shortcut`，载荷 `ShortcutDetails`。
- `self-update-main` → `error-download-update`、`error-cancel-update`、`cancel-download-update`。
- `app-common-main` → `renderer-link`、`second-instance`、`second-instance-deep-link`、`show-about-riftflow`、`show-settings`。
- `auto-champ-config-main` / `auto-gameflow-main` / `auto-misc-main` → 各自的 `error-*` 项。
- `window-manager-main/main-window` → `close-asking`。
- `renderer-debug-main` → `lcu-event`。

## 三、SGP 与远端 API（子 agent 报告，待逐条回查）

本机真机确认：`region=TENCENT`、`rsoPlatformId=HN1` → `sgpServerId=TENCENT_HN1`，
内置配置里 Tencent 项 `matchHistory === common`，叠加 `preferredLolSource` 默认 `'sgp'`
→ `ongoing-game` 的 `state.apiShouldUse` 判定为 **`sgp`**。所以 SGP 是这个环境的主数据路径，优先级高于 LCU 数据类接口。

必须复刻的机制（不是端点清单本身）：

1. 三个魔法请求头驱动路由：`x-riftflow-sgp-server-id`（跨区覆盖）、`x-riftflow-token-type`（`entitlements` → matchHistory base，`league-session` → common base）、`x-riftflow-force-stream-collect`（把流式 body 预缓冲并去掉 `Transfer-Encoding`）。发送前必须全部剥离。
2. URL 占位符 `@riftflow:sgpServerSubId@` 由 `regionPathParam` 或 `TENCENT_` 后缀推导的 subId 填充；主机名只来自 league-servers 配置，绝不从 ID 拼。
3. 跨区门禁：仅当当前与目标 `isTencent` 同为真、或目标即当前时才允许（渲染层 `useSgpServerQuery`）。
4. `league-servers` / `supported-queues` 每 2h 拉取，`updatedAt` 严格更新才生效，落盘 JSON 缓存；`supportedQueues` 会把本地当前 queueId **前置**到列表。
5. token 由 LCU 侧持有并靠 WS 事件刷新（`/entitlements/v1/token`、`/lol-league-session/v1/league-session-token`，失败 5s 重试）；SGP 层自身不刷新，缺 token 直接抛。
6. 无传输层响应缓存；缓存只存在于消费方（LRU 256，键 `sgp:{gameId}` / `lcu:{gameId}`）。

## 四、Win32 原生能力（子 agent 报告，待逐条回查）

`native/win32-x64` 两个 Node addon 用 `windows` crate 重写。关键 API 面：

- input：`SetWindowsHookEx(WH_KEYBOARD_LL)` + 专职消息泵线程（`PeekMessage(WM_USER)` 预热队列后 `GetMessage` 循环）、`SendInput` 文本注入走 `KEYEVENTF_UNICODE` 每码元 2 条 INPUT 一次提交、按键走 `MapVirtualKey(MAPVK_VK_TO_VSC)`、256 槽按键状态表、`ThreadSafeFunction` 回调；`install()` 同步等待钩子就绪。
- tools：`FindWindowW(L"RCLIENT", L"League of Legends")` + `FindWindowExW(… L"CefBrowserWindow")`、`SetWindowPos(…, SWP_NOZORDER)` 两次（frame 居中 + 子窗口铺满）、`GetSystemMetrics(SM_CXSCREEN/SM_CYSCREEN)`、`GetWindowPlacement` + `GetDpiForWindow` 归一化、`NtQueryInformationProcess(60)` 两段式读命令行、`CreateToolhelp32Snapshot` 枚举、`TerminateProcess`、`GetForegroundWindow`+`GetWindowThreadProcessId`、`AllocateAndInitializeSid`+`CheckTokenMembership` 判管理员组。
- **`fixWindowMethodA` 与 placement 处于不同坐标空间**（前者物理×zoom，后者逻辑÷DPI），下游 `window-position-service.ts:212` 用 `width<200 && height<50` 兜住最小化，移植时别"顺手统一"。
- 语义陷阱：`getCommandLine1` 失败返回哨兵字符串 `"Failed to retrieve command line."` 而非抛错；`getPidsByName` 是精确字节匹配；tools addon 加载失败 = 启动致命（`app.exit(10001)`），input addon 失败 = 可降级。
- `resources/magic/magic.win32-x64.node` 无源码，算法未知，**无法从本仓库移植**（选隐玩家 puuid 去混淆）——需要单独决策。

## 五、持久化与设置（自验部分已覆盖）

- 表：`Metadata` / `Settings` / `SavedPlayers`（4 列复合主键）/ `EncounteredGames`；schema 版本存在 `Metadata` 行 `version`，不是 `PRAGMA user_version`。
- ⚠️ 已踩并已修的坑：TypeORM 的 `json` 列在 SQLite 落到 **NUMERIC affinity**，数字值存成 INTEGER 单元格；按字符串读会失败并被误判为"库损坏"→ 触发备份重建 → 用户设置清空。见 `riftflow-store::cell_to_json_text`。
- 设置行键是扁平 `${namespace}/${key}`；`null` 写为 DELETE；所有写入 1000ms 去抖，退出前必须 flush（先 flush 设置、后销毁连接）。
- 前缀扫描的边界是 `>= ns/prefix/` 且 `< ns/prefix0`，依赖 BINARY collation，照抄别"修正"。
- 版本阶梯 0→v10→v15，单事务、失败重建重试仅一次；`version > 15` 容忍并只置 `usingHigherVersionDb`（渲染层弹警告）。

## 六、真机已验证事实（国服 / WeGame）

| 事实 | 影响 |
|---|---|
| `/riotclient/auth-token` 200、`/lol-gameflow/v1/gameflow-phase` 200（`"None"`）、wss 订阅 `OnJsonApiEvent` 收到 type=8 帧 | 连接层与健康探针按此实现 |
| `/version`、`/lcu-version` → 404 `Invalid function` | 不能用作探针 |
| `/lol-summoner/v1/current-summoner` 无 `psvLevel`，`accountId` 是 JSON **数字** | serde 结构体必须数字容忍，否则运行时反序列化失败 |
| 客户端由 WeGame 启动，但**普通权限即可读到它的命令行**（`NtQueryInformationProcess(60)` 实测成功，`is_elevated=false` 下解析出 port 52611 / TENCENT / HN1） | 自动连接**不需要**提权；被挡住的是 PowerShell/CIM 路径（返回空 CommandLine），这才是 `useWmi` + `rebuild_WMI.bat` 存在的原因 |

| 无 UAC 验证通道：读 `%APPDATA%\riftflow\RiftFlow.db` 的 `league-client-main/lastConnectedClient` | 后续集成测试沿用此法拿真 token |


---

## 附录 B、48 个同步状态面

机械抽取自各 shard 的 `propSync` 调用。抽取过程中发现两类极易漏掉的声明形式：prop 列表可以是**裸字符串**（如 `logger-factory-main:state` 的 `logLevel`），prop id 也可以不是 `state`/`settings`（如 `extra-assets-main:gtimg`、`league-client-main` 的 13 个 LCU 子状态）。

| 状态 | 命名空间:id | prop 路径 |
|---|---|---|
| - [ ] | `riftflow-api-main:state` | featureGates,notice,contactChannels |
| - [ ] | `app-common-main:settings` | showFreeSoftwareDeclaration,locale,theme,httpProxy,streamerMode,streamerModeUseRiftFlowStyledName,preferredLolSource |
| - [ ] | `app-common-main:state` | isElevated,platform,disableHardwareAcceleration,baseConfig,startupDeepLink,isRunInTempDir,nativeSupport |
| - [ ] | `auto-champ-config-main:settings` | enabled,runesV2,summonerSpells |
| - [ ] | `auto-gameflow-main:settings` | autoAcceptDelaySeconds,autoAcceptEnabled,autoHonorEnabled,autoHonorStrategy,autoMatchmakingDelaySeconds,autoMatchmakingEnabled,autoMatchmakingMinimumMembers,autoMatchmakingRematchFixedDuration,autoMatchmakingRematchStrategy,autoMatchmakingWaitForInvitees,autoSkipLeaderEnabled,playAgainEnabled,autoHandleInvitationsEnabled,autoReconnectEnabled,autoMatchmakingMaximumMatchDuration,invitationHandlingStrategies,rejectInvitationWhenAway,autoSendARAMTeamSideEnabled,autoSendARAMTeamSideVisibleToTeam |
| - [ ] | `auto-gameflow-main:state` | willAcceptAt,willSearchMatch,willSearchMatchAt,activityStartStatus,friendsToBeInvited |
| - [ ] | `auto-misc-main:settings` | [SPREAD] |
| - [ ] | `auto-select-main:settings` | pickConfig,banConfig |
| - [ ] | `auto-select-main:state` | groups,temporarilyDisabled,delayedBan,delayedPick,delayedBenchSwap,delayedChampionSwap,expectedPicks,expectedBans,expectedSwaps,activeGroupConfigId |
| - [ ] | `client-installation-main:state` | leagueClientExecutablePaths,tencentInstallationPath,weGameExecutablePath,officialRiotClientExecutablePath,tclsExecutablePath,weGameLauncherExecutablePath,detectedLiveStreamingClients |
| - [ ] | `extra-assets-main:gtimg` | heroList,kiwiAugments |
| - [ ] | `extra-assets-main:opgg` | aramBalance |
| - [ ] | `game-client-main:settings` | terminateGameClientWithShortcut,terminateShortcut |
| - [ ] | `in-game-send-main:settings` | sendInterval,cancelShortcut,ratingPresetOptions,junglePresetOptions,premadePresetOptions,fixedTextPresetItems |
| - [ ] | `in-game-send-main:state` | ratingPuuids,junglePuuids,premadeIndices |
| - [ ] | `league-client-main:champSelect` | gridChampions |
| - [ ] | `league-client-main:champSelect` | session,currentPickableChampionIds,currentBannableChampionIds,disabledChampionIds,currentChampion,ongoingChampionSwap,skinSelectorInfo |
| - [ ] | `league-client-main:chat` | me,participants.postGame,participants.customGame,participants.championSelect,conversations.postGame,conversations.customGame,conversations.championSelect |
| - [ ] | `league-client-main:entitlements` | token |
| - [ ] | `league-client-main:gameData` | champions,items,perks,perkstyles,queues,summonerSpells,augments,gameModeMutators,maps |
| - [ ] | `league-client-main:gameflow` | phase,session |
| - [ ] | `league-client-main:honor` | ballot |
| - [ ] | `league-client-main:initialization` | progress |
| - [ ] | `league-client-main:leagueSession` | token |
| - [ ] | `league-client-main:lobby` | lobby,receivedInvitations |
| - [ ] | `league-client-main:lobbyTeamBuilder` | champSelect.subsetChampionList |
| - [ ] | `league-client-main:login` | loginQueueState |
| - [ ] | `league-client-main:matchmaking` | readyCheck,search |
| - [ ] | `league-client-main:settings` | autoConnect |
| - [ ] | `league-client-main:state` | auth,connectionState,connectingClient |
| - [ ] | `league-client-main:summoner` | me,profile |
| - [ ] | `league-client-ux-main:settings` | useWmi |
| - [ ] | `league-client-ux-main:state` | launchedClients,hasClientButNoCommandLine |
| - [ ] | `logger-factory-main:state` | logLevel |
| - [ ] | `ongoing-game-main:additional` | additional |
| - [ ] | `ongoing-game-main:settings` | concurrency,enabled,matchHistoryLoadCount,matchHistoryTagPreference,gameDetailsLoadCount,orderPlayerBy,showChampionUsage,showMatchHistoryItemBorder,showJunglePathing,showJunglePathingForAllPlayers,autoRouteWhenGameStarts,playerCardTags,queryInLobbyPhase,premadeTeamInferMatchCountThreshold |
| - [ ] | `ongoing-game-main:state` | championSelections,positionAssignments,analysis,queryStage,teams,isInEog,matchHistoryLoadingState,summonerLoadingState,savedInfoLoadingState,rankedStatsLoadingState,championMasteryLoadingState,teamParticipantGroups,matchHistoryTagParams,draft,additional,mergedPremadeTeamMap,inferredPremadeTeams |
| - [ ] | `renderer-debug-main:state` | sendAllNativeLcuEvents,logAllLcuEvents |
| - [ ] | `respawn-timer-main:settings` | enabled |
| - [ ] | `respawn-timer-main:state` | info |
| - [ ] | `self-update-main:settings` | autoCheckUpdates,autoDownloadUpdates,ignoreVersion |
| - [ ] | `self-update-main:state` | isUpdateSupportedOnCurrentPlatform,isCheckingUpdates,releaseInfo,updateProgressInfo,lastUpdateSucceeded |
| - [ ] | `sgp-main:state` | availability,isTokenReady,leagueServers,supportedQueues,connectionSuccessesCounted,connectionFailuresCounted |
| - [ ] | `storage-main:state` | usingHigherVersionDb |
| - [ ] | `window-manager-main:settings` | backgroundMaterial,contentProtection |
| - [ ] | `window-manager-main:settings` | opacity,pinned [SPREAD] |
| - [ ] | `window-manager-main:state` | focus,ready,status,show,trackedBounds [SPREAD] |
| - [ ] | `window-manager-main:state` | supportsMica,downloadTasks |

## 七、设置地基（所有功能开关的前提）

| 事实 | 说明 |
|---|---|
| `settings_registry.rs` 接管 `*:settings` 面 | 注册 `SettingsService`（校验器 + 默认值 + `applyToState`）→ propSync → 变更时同时回推渲染层并广播给功能模块；`state_bootstrap` 通过 `OWNED_NAMESPACES` 让位，避免同一 prop 声明两次（`prop_sync` 会 bail） |
| **真 bug：设置从来不落库** | `DelayedTaskScheduler` 只记录待写，没人按时间排空；`setting-factory-main/set` 之后重启即丢失。已加 `flush_due(now)`（带 `due_at` 的 1000 ms 窗口）+ 1 s tick 调用 + 退出时 `flush_all`，并补 `flush_due_writes_only_after_the_debounce_window` 回归测试 |
| 真机验证 | 打开 Automation → Auto-Accept 开关 ON → 日志出现 `update-state-prop/auto-gameflow-main:settings` → 杀进程重启后仍为 ON（落库 + `applyToState` 双通），随后已恢复用户原值 OFF |

### auto-gameflow-main 现状

已移植：自动接受（含 `autoAcceptDelaySeconds`、非 ReadyCheck 取消、Accepted/Declined 事件取消）、再来一局（WaitingForStats 10s / PreEndOfGame 3.25s / EndOfGame 1.575s）、自动重连（Reconnect 阶段 10s）、自动匹配（`activityStartStatus` 全部 7 个分支照抄 + 队伍频道倒计时/取消提示，中英文案取自 `main.yaml`）、取消排队策略（fixed-duration / estimated-duration，含 `penaltyTime` 基线）。

实现方式：一个任务持有全部定时器，LCU 事件与设置变更走同一通道，网络调用一律 await —— 取代原来的 mobx `reaction`，取消路径不会留下野定时器。

未接：邀请处理、自动荣誉、跳过队长、ARAM 分路发送（`invitation-controller.ts` / `honor-controller.ts` / `aram-team-side-controller.ts`）。

### auto-gameflow-main 剩余四块（已补）

| 块 | 规则来源 | 要点 |
|---|---|---|
| 自动处理邀请 | `invitation-controller.ts:_watchAutoHandleInvitation` | 只处理 `Pending && canAcceptInvitation`；策略取 `strategies[inviteGameType]` → `strategies['<DEFAULT>']` → `ignore`；稳定排序 accept > decline > 其它，每轮只动第一个；`rejectInvitationWhenAway && chat.me.availability==='away'` 时直接跳过 |
| 转让房主 | 同文件 `_watchAutoSkipLeader` | 排除自己（`summonerId`）与观战；ready 优先，否则 notReady；`promote` 前若存在队伍频道先发提示 |
| 自动邀请好友 | 同文件 `_watchAutoInvitation` | 订阅 `/lol-chat/v1/friends/:id`（radix 占位符）；`Delete` 事件移出待发列表；仅当 `availability==='chat'`、`localMember.allowedInviteOthers`、在待发列表中且尚未在房间时才发 `[{toSummonerId}]`，无论成败最后都移出列表；离开房间清空列表 |
| 自动点赞 | `honor-controller.ts` | 有 ballot 且带 gameId 就先取消"再来一局"（与开关无关，原实现如此）；三批候选人 = 房间内己方 → 房间外己方 → 房间外敌方，每批只补前一批剩余票数，等权不放回抽样；`POST /lol-honor/v1/honor` 逐个 + `POST /lol-honor/v1/ballot` 提交，失败发 `error-auto-honor` |
| ARAM 分路播报 | `aram-team-side-controller.ts` | 仅 `benchEnabled` 且 `map.gameMode ∈ {ARAM, KIWI}`；team 由 `localPlayerCellId` 在 `myTeam` 里反查，只接受 1/2；TW2 区域用 `-tw` 文案；`visibleToTeam` 决定是否加 `[RiftFlow] ` 前缀以及消息类型（`chat` vs `celebration`） |

**两处上游细节的处置**（都写在代码注释里）：
- `randomInt(min,max)` 上界默认**开区间**，原 `_watchAutoSkipLeader` 传的是 `randomInt(0, len-1)`，因此**永远选不到最后一个候选人**。这里按 `[0, len)` 均匀取，属有意修正。
- 点赞分类 `AUTO_GAMEFLOW_HONOR_CATEGORY` 只有 `HEART` 一项，`randomInt(0, 1)` 恒为 0，所以直接固定 `HEART`，行为一致。

新增 `rng.rs`：splitmix64（时钟+pid 播种），只为"随机挑个队友"服务，避免为此引入 rand 依赖扩大产物体积；含分布与不放回抽样的单测。

验证：121 测试全绿；启动后无 panic、无越权动作 —— 不在房间/选人阶段时四块逻辑都没有发出任何 LCU 写请求（日志中无 `队伍频道`/`点赞`/`邀请` 行），这正是守卫正确的证据。真正的端到端（进房间收邀请、结算点赞、ARAM 开局播报）需要一局真实对局，留待下次联机验证。

---

## 八、auto-select-main（自动选锁，已移植）

| 块 | 规则来源 | 要点 |
|---|---|---|
| 计算态 | `computed-state.ts` → 新 crate `riftflow-auto-select` | `correctedTimer`（用 `internalNowInEpochMs` 校准本地时钟）、`phaseCalibratedDelayMs`、`scopedBenchChampions`、`currentActions/activeAction/firstUnfinishedPickAction`、`getMove` 十种 move、`getActiveGroupConfig`（SGP 服务器 + isCustom + gameMode/queueTypes 通配）、`expectedPicks/Swaps/Bans` 的 14 种状态；13 个单测 |
| 延迟 ban/pick | `ban-pick-controller.ts` | 三种策略（just-show / show-and-lock-in / lock-in-immediately）的 `completed` 判定、`show-and-lock-in` 在 complete 阶段把 targetOffset 翻倍、just-show 一旦已选即停手、lock-in-immediately 忽略 空选→亮出→锁定 的过渡态、`startAt` 跨重排保留 |
| 备战席换位 | `bench-controller.ts` | `benchChampionCooldown`（英雄首次上榜时间戳，重排保留、重新上榜刷新）、`benchSwapAccumulatedDelaySeconds` 倒计时可**为负**（原实现即立即触发），再与 `correctedTimer.remainingMs` 取小 |
| 英雄交易 | `trade-controller.ts` | 只在**首次收到** `state==='RECEIVED'` 时记基准时间（trade 没有服务端基准可用）；不在自己列表里 → 拒绝且**不做非负钳制**（照抄原式），在列表里则按 `benchSelectFirstAvailableChampion` 决定接受/拒绝并钳制；FINALIZATION 阶段受剩余时间约束 |
| 执行器 | `action-executor.ts` | `PATCH /lol-champ-select/v1/session/actions/{id}`（ban/pick/intent 三种 body）、`POST .../bench/swap/{id}`、`POST .../champion-swaps/{id}/accept\|decline`；失败发 `error-*` 文案 |
| 局内提示 | `local-message-service.ts` | `celebration` 类型只本地可见；没有 champSelect 频道时先积压，`chatId` 出现后按序补发；离开选人清空积压并把 `temporarilyDisabled` 归零 |
| 配置填充 | `config-manager.ts` | 启动时为每个 group 补齐空 pick/ban 配置并落库（渲染层据此渲染标签页） |
| IPC | `ipc-handlers.ts` | `setPickConfig` / `setBanConfig`（`_.mergeWith` 语义：英雄 id 数组整体替换、对象逐键合并）+ `setTemporarilyDisabled` |

### 顺带修掉的两个真 bug

1. **`Set` 类型 prop 到不了渲染层。** Electron 的结构化克隆能直接传 `Set`，JSON 不行；渲染层 `lcs.champSelect.currentPickableChampionIds.has(...)` 是**方法调用**，给数组就抛错。现在 `riftflow-state` 按 prop 打标记（`__riftflowSet__`），shim 在 `subscribeAndGetInitialState` 与 `update-state-prop` 两个入口还原成真正的 `Set`。第一版只补了事件入口，CDP 实测初始订阅路径拿到的仍是 `{}`，因为 `riftflow_call` 返回的是 `{success,data}` 信封 —— 已修。
2. **断连清理把可空对象写成了 `{}`。** `clear()` 对 `champSelect.session`、`gameflow.session`、`lobby` 等写 `{}`，而它们声明为可空对象；`hydrate` 遇 404 又直接跳过，于是"没有选人局"被表示成一个**空对象 session**，`in_champ_select` 变真。现在统一走 `default_value(state, path)`，且 404 与 `clear()` 用同一张表（补了 `the_synced_props_clear_to_their_declared_shape` 测试，逐条校验 SYNCED 的默认形状）。

### 真机验证（国服在线客户端，CDP 驱动真实窗口）

- `subscribeAndGetInitialState('league-client-main','champSelect')`：三个 id 列表 `instanceof Set === true`、`.has` 可用；`gridChampions` 255 条已灌入；无选人局时 `session === null`、`ongoingChampionSwap === null`。
- 事件推送路径注入 `{__riftflowSet__:[1,2,3]}` → 渲染层收到 `Set(size=3)`。
- 页面 `#/automation/auto-select` 实际渲染：9 个模式标签（图标走 `riftflow://` 资源代理）、Auto Pick / Auto Ban 两个子页、Expected Champions 6 行、Show Intent / Ignore Intent / Lock Strategy 三选一 / Delay(s) / 备战席专属项，数值全部来自 Rust 侧空配置默认值（`2.9`、`show-and-lock-in`），不是占位。
- **真实鼠标点击** Enable 开关 → 后端 `pickConfig.aram.enabled === true` → 杀进程重启后仍为 `true`（落库 + `applyToState`），随后已恢复用户原值（`enabled:false, delaySeconds:0, champions:[]`）。
- `setPickConfig` 合并语义实测：只改 `aram`，`ranked` 等 9 组原样；数组整体替换而非合并。
- 全程日志中 `[auto-select]` 行数为 0 —— 不在选人阶段时没有任何越权的 LCU 写请求。

**仍未端到端验证**：真实选人阶段内的 ban/pick/换位/交易执行（需要开一局并打开开关），以及 `getUserTakeover`（原实现里也没有）。

---

## 九、saved-player / auto-misc / auto-champ-config（已移植并真机验证）

| 块 | 规则来源 | 要点 |
|---|---|---|
| saved-player-main 13 方法 | `saved-player/index.ts` + `ipc-handlers.ts` | SQL 层落在 `riftflow-store/src/players.rs`（复合主键 upsert、`save` 只写调用方给出的列、`remove` 返回被删行、分页 total、ranked 的 `ranked-default` 回落列表）；三条校验错误文案逐字照抄 |
| 标签导入导出 | `tagged-players-file-service.ts` | `type` / `databaseVersion` / 字段形状三重校验，500 条一批事务写入 |
| setting-factory 余项 | `ipc-handlers.ts` + `settings-json-file-service.ts` | `removeByPrefix`、`exportSettingsToJsonFile` / `importSettingsFromJsonFile`（导入后 `restart()`：先 `flush_all` 再重启，因为 `std::process::exit` 走不到 `RunEvent::Exit`） |
| 原生文件对话框 | Electron `dialog.show*Dialog` | `riftflow-native/src/lib.rs` 直接用 shell32 的 `IFileOpenDialog` / `IFileSaveDialog`（`CoCreateInstance` + `COMDLG_FILTERSPEC`），不引入新依赖，产物体积不变 |
| auto-misc-main | `auto-reply-controller.ts` + `login-automation-controller.ts` | 自动回复（`type==='chat'` 且非自己、away 门控）、离线状态纠正（`offline → away/chat` 才改回）、`chat.me` 出现后 2 s 稳定期一次性应用签名/段位（手动调用会打断自动流程，与原实现一致）；apex 段位不发 `rankedLeagueDivision` |
| auto-champ-config-main | `auto-config-controller.ts` | CLASSIC+RANKED 用 `ranked-<position>` 且**符文**可回落 `ranked-default`（技能不回落）；有自定义页配额就新建页，否则改写第一页；`currentpage` 的请求体是**裸 id**；`[RiftFlow] 英雄 - 位置` 页名；进入选人频道时播报已配置英雄列表（去重、最多 16 个） |

### 顺带修掉的代理层根因（诊断 agent 给出证据）

`readBody` 不读 `ReadableStream`。渲染层 axios 用 `adapter: 'fetch'`，其 fetch adapter 会把 body 包成 `new Request(url, {body, duplex:'half'})`，于是 **每一个 POST 的 body 都是流** —— 之前一律被丢弃。这一个根因同时解释了：
- `POST /lol-replays/v2/metadata/{id}/create` → 400（零字节）；
- `POST riftflow://sgp/challenges-client/v2/all-player-data/` → 415（`content-type: application/json` 已转发、body 为空，网关拒绝；不是 SGP 规则错，`plan()` 与原版逐条一致）。

修法是 shim 里加一个 `getReader()` 排空分支。另外 `Proxied::from_raw` 现在会剔除 `content-encoding` 并按解码后的真实长度回填 `content-length`（reqwest 的 gzip 已经解过码，原样转发会让调用方二次解码或截断）。

诊断 agent 还记下两处未修的差异：`cancelProxyRequest` 恒返回 false（放弃的请求仍会占满 30 s 超时），以及 `riot-client` / `riftflow-api` / `riftflow-static` 三个域名仍回 500。

### 真机验证（国服在线客户端 + CDP）

- saved-player 全链路：`saveSavedPlayer` → `querySavedPlayer` → `getAllPlayerTags(total=1)` → `querySavedPlayerWithGames`（嵌套 `encounteredGames` + `tags[].markedBySelf`）→ `saveEncounteredGame`（自增 id）→ `queryEncounteredGames` → `updatePlayerTag` → `deleteEncounteredGame(affected=1)` → `deleteSavedPlayer`（返回被删行）→ 再查为 `null`。时间戳为 ISO-8601。
- 三条校验错误逐条命中原文案；`removeByPrefix` 在不存在的命名空间上返回 0（未触碰真实设置）。
- `updateRunes` / `updateSummonerSpells` 写入后设置面为 `{"222":{"ranked-default":{...}}}`，形状正确。
- `applyStatusMessage` 用**玩家当前签名原值回写**（等于无操作）验证调用链返回成功，未改动用户资料。
- 验证数据已全部清理：`runesV2` / `summonerSpells` 回到 `{}`，`queryAllSavedPlayers.count === 0`。
- 重启后 `not forwarded` 计数 0，400/415 不再出现。

## 十、接线轮：把 agent 产出的模块接进运行的 app

这轮的瓶颈不是写代码而是接线——四个 agent 交付了能编译的模块，但没有任何一个被 `main.rs` 调用。逐个接上并验证：

- **`riftflow-ongoing-game`**：加入 workspace 成员 + `riftflow-app` 依赖（该 crate 自己 157 个测试随 `cargo test --workspace` 跑）。`crates/riftflow-app/src/ongoing_game_main.rs`（1218 行）在 `main.rs` 里 `register(...)` + `start(...)`。删掉旧的 `ongoing_game.rs` 桩——它对 `getAll` 返回七个空对象，看起来像功能正常，实际是假数据。启动日志里 `ongoing-game-main` 从 placeholder 名单消失，trace 显示渲染进程真实调用了 `ongoing-game-main:getAll` 且整个启动过程渲染器零 `console.error`。
- **`riftflow-api`**：同样入 workspace；新增 `crates/riftflow-app/src/riftflow_api_main.rs`（build / start / proxy 三件事）。启动证据：`Loaded bootstrap generation 2`、`updated riftflow-api-main:state#notice, ...#contactChannels for en`。渲染进程经冻结的 IPC 读到的都是真数据：`featureGates{gates,updatedAt}`、`notice{content,contentType,language,revision,severity}`、`contactChannels{channels,updatedAt}`、`sgp-main` 的 `leagueServers`（26 个区）、`supportedQueues`（17）、`auto-select-main.groups`（10）。
- **两个协议域接上**：`riftflow://riftflow-api/config/v1/app/feature-gates` → 200 带上游 JSON；未知路径 → 404 `{"error":{"code":"UnknownRiftFlowApiPath"}}`（端点表在起作用，不是兜底瞎猜）。
- **`riot-client` 协议域**：之前 500 `riot-client proxy is not ported yet` 一直伪装成"SGP 夜间不可用"。`RiotClientService` 其实早就存在，只是协议臂是桩；接上后 `POST riftflow://riot-client/player-account/lookup/v1/namesets-for-puuids` 不再进 `[protocol:fail]`，渲染进程的 `PlayerTab AxiosError 500` 归零。
- **`in-game-send-main` 设置面**：`settings_registry::in_game_send()` 用 TS 的默认值注册。此前 `ratingPresetOptions` 被声明成 `{}`，`#/toolkit/in-game-send` 挂载即抛 3 个 `TypeError: Cannot read properties of undefined (reading 'friendly'/'enemy'/'all')`；现在页面节点数从 559 升到 860 且零异常。**教训**：record/对象型 prop 必须照 `createDefault*` 填真实形状，`{}` 不是"保守占位"而是崩溃源。
- **`supportedQueues` 推导补正**：TS 里 `SgpState.supportedQueues` 是 computed getter（把当前对局队列前置）。原来 `lc_state::sync_supported_queues` 只读内置表，而 riftflow-api 会把远端文档原样写进同一个 prop，两边互相覆盖。现在推导读远端文档（`FETCHED_SUPPORTED_QUEUES`，由 `run_config_tasks` 新增的 `on_refresh` 回调喂入），单测 `supported_queues_follow_the_fetched_doc_and_prepend_the_live_queue` 锁住三种情况。
- **协议代理加了失败日志**：`[protocol:fail] 方法 url -> 状态 响应前 200 字`，只在 `RIFTFLOW_IPC_TRACE` 下输出。之前 ≥400 的代理响应完全静默，才让 riot-client 的桩被误判成外部故障。
- **全路由回归**（17 个路由，`tmp/cdp_sweep.mjs` + node_repl 自写的 CDP 客户端）：除 `/favicon.ico` 外零失败请求、零 Vue 异常。favicon 的 500 来自 Tauri 资源处理器对缺失文件的行为；已把原项目的 `resources/LA_ICON.ico` 复制为 `renderer-dist/favicon.ico` 并重新编译（只新增文件，不改动既有 bundle）。修复后一次整页导航里 **没有任何 ≥400 响应**，但 favicon 自身那次请求没被重新发出（命中缓存），所以"它现在返回 200"这一点还没有直接证据。

`cargo test --workspace`：534 passed / 0 failed。

仍缺（诚实清单）：`in-game-send-main` 的 8+ 个 IPC 方法与原生按键注入（`riftflow-native` 还没有 `SendInput` 那层，crate 侧 `InGameKeyboard` 是留好的缝）、`self-update-main`、`game-client-main`（含 `gameTime`，依赖客户端 2999 端口）、`client-installation-main`/`renderer-debug-main` 的方法面、窗口对齐，以及真实对局下的 ongoing-game / auto-select 端到端验证。

### 托盘已补上（原来的空档会让"最小化到托盘"变成死路）

`crates/riftflow-app/src/tray.rs` port 了 `tray/{index,tray-menu-controller,tray-state-watcher}.ts`：图标 `assets/LA_ICON.ico`（tauri 加了 `image-ico` feature）、tooltip `RiftFlow`、左键恢复主窗口、右键弹菜单，菜单项与 `shared/i18n/{zh-CN,en}/main.yaml` 的 `tray.*` 原文一致，enabled 规则照抄 `[settings.enabled, state.ready]`。菜单不在变化时重建（`tray::refresh_if_changed` 挂在 shell 已有的轮询里，六个被监视的标志没动就不碰原生菜单）。

证据：UIA 枚举 Windows"显示隐藏的图标"面板，我们的进程在跑时里面有两个 `RiftFlow` 按钮（另一个是用户仍在运行的原版 RiftFlow），`taskkill` 后只剩一个——说明我们的托盘项是真的注册进去了。**没验的**：左键恢复与菜单项点击本身没有真的点过，因为两个按钮同名、无法程序化区分。

### 关键判定：`nativeSupport.nativeInput` 不能改成"可用"

热键诊断 agent 建议把 `app_common.rs` 的 `nativeInput: requirement(false, true)` 改成"hook 活着即可用 + requiresElevation:false"，理由是全局热键在非提权进程里已跑通。**不采纳**，原实现是：

```ts
// RiftFlow/src/main/native/index.ts:54
NATIVE_SUPPORT.nativeInput.available =
  isWindows && isElevated && Boolean(nativeInput?.isLoaded()) && Boolean(nativeInput?.instance.isInstalled)
```

这一个标志同时描述"监听键盘"和"向游戏内注入按键"。我们的监听确实不需要提权（实测 `WH_KEYBOARD_LL` 非提权可用），但注入还没实现——置真就是让 in-game-send 的按钮承诺一件代码没做的事。等 `riftflow-native` 补上 `SendInput`，这个标志应由注入能力驱动，而不是由 hook 存活驱动。

热键侧的修复本身已进主干且是对的：去掉 `is_elevated()` 门禁；`KEY_STATES` 由 `const` 数组改 `static`（此前 hook 写一份、读取另一份，组合键的修饰符全被丢掉），实测 `LeftControl+LeftShift+F10` 现在带 `keyCodes=[162,160,121]` 推到渲染进程。遗留问题：6 次运行里有 4 次，进程内的 hook 要等其他进程也装上 hook 才开始回调，独立探针复现不出来——仍未解决。

## 十一、in-game-send 接线完成，但注入仍是坏的（已复现）

`crates/riftflow-app/src/in_game_send_main.rs`（1957 行）提供 **22 个方法，与 TS 逐个对齐、无多无少**（`tmp` 里第一版 diff 脚报"缺 9 个"是我的正则没覆盖 `("sendRatingPreset", PresetKind::Rating)` 这种元组注册，重测后 MISSING/EXTRA 都是 none）。`in-game-send-main` 已从 placeholder 名单移除，启动零渲染器报错。

真机驱动（CDP 打 `riftflow_call`）：

| 调用 | 返回 | 判定 |
| --- | --- | --- |
| `setRatingPuuids([假 puuid])` | `null`，props 仍 `[]` | 正确——TS 的 `_filterCurrentPuuids` 会按当前对局玩家过滤，没对局就是空 |
| `generateRatingPresetLines('friendly')` | `[]` | 正确——没有选手可描述 |
| `sendLines([...])` | `false` + 日志 `不发送: 当前阶段不可发送` | 正确拒绝，走的是阶段判断而不是"假装成功" |

**原生注入没做完，且现在是坏的。** `crates/riftflow-native/src/input.rs` 用 `SendInput` 实现了 `send_key` / `send_text`（UTF-16 单元，能处理 CJK），`injection_allowed` 按 UIPI 真实语义判断（只有"目标提权而自己没有"才不可用），三个单测通过。但真机复现（scratch 记事本 + Ctrl+S 读文件）：

```
输入 Ab9Zk  →  记事本实际得到 kkkkkkkkkk   (5 个字符变 10 个，全是最后一个码元)
输入 RiftFlow测试123 → 3333333333             (同上，全 '3')
```

两个结论：每个字符进了两次（`KEYEVENTF_UNICODE|KEYEVENTF_KEYUP` 那条没被当成抬起），且投递的码元固定成最后一个。Win11 记事本走 TSF，不排除是它把注入的 unicode 事件合并了；用自建经典 `EDIT` 控件做对照的探针我写砸了（windows-rs 0.58 的参数类型反复猜错，已删掉那个 example，不再耗时间）。**在此之前 `nativeSupport.nativeInput` 必须继续报 `available:false`**，`in_game_send_main` 里 `SendAction::InGame` 分支也只记日志不改状态，所以 UI 上没有任何按钮在承诺做不到的事。

复现脚本留在 `tmp/verify_input5.ps1`（开一个唯一命名的 scratch 记事本、校验前台、跑 `cargo run -p riftflow-native --example input_probe`、Ctrl+S 后读文件）。

**注入其实是对的，那次"坏掉"是测试错了。** 两条独立证据：
- `tmp/…/inject_observer.rs`：自建 `WH_KEYBOARD_LL` 钩子看自己的注入事件，`"Ab9Z"` → 8 个事件、down 序列 `[65,98,57,90]` 完全对上，`vk=0xE7` + `LLKHF_INJECTED` 正是 `KEYEVENTF_UNICODE` 事件的形状。
- Win11 记事本会**恢复上次未保存的会话**，所以第二次运行的 buffer 里混着上一轮的字符，看起来像"全变成最后一个字符"。用唯一文件名 + 每次清空后，`Ab9Zk` 和 `德莱文123` 都逐字命中。

## 十二、窗口对齐与可用性标志

- `crates/riftflow-native/src/placement.rs`：`getLeagueClientWindowPlacementInfo` 的移植（`FindWindowW("RCLIENT", "League of Legends")` + `GetWindowPlacement` + `GetDpiForWindow` 换算成 DIP，字段与 addon 返回的对象一致）。
- `windows.rs::position_with_snap` / `align_to_league_client_ux`：照抄 `getPositionWithSnap` 的四角公式与 work area 夹取，接进 `repositionToAlignLeagueClientUx`（原来是一个 `NativeNotSupported` 业务错误）。三个单测钉住四角结果与夹取。
- `nativeSupport`：`nativeInput.available` 现在按 UIPI 真值报（本机 `isElevated=false` 仍为 `available:true`、`requiresElevation:false`），`getLeagueClientWindowPlacement.available = isWindows`，与 `native/index.ts:61` 一致。
- 真机跑通到"跳过"分支：`LeagueClientUx.exe` 在场时探针读到 136x39，正是原注释里最小化的尺寸，于是按原语义跳过对齐并写了一行日志。**真正的贴边只有把客户端窗口还原后才能验**，这一步留给下一次。
- 这一轮把 `align_to_league_client_ux` 里所有静默 return 都改成带原因的日志——之前那次"返回 ok 但什么都没发生"就是因为没有任何输出可查。
- `cd-timer-window:sendInGame` 从写死的 `NativeInputNotSupported` 换成了原实现的三道门（可用性 → `AlreadySending` 闩 → 前台判断）加 Enter·文本·Enter（20ms / 65ms 与原值一致）。真机：`{"code":"GameClientNotForeground","message":"game client is not foreground"}` —— 前两道门已过，拦下它的是"现在没有对局"，与原版行为一致而不是假失败。`in-game-send` 的 `SendAction::InGame` 分支同样换成了真的 `run_in_game_send`（`spawn_blocking` + 共享的 `cancel_requested` 作为 `cancelled()`）。

`cargo test --workspace`：555 passed / 0 failed。

## 十三、方法面完整性 + 真检查更新

`tmp/gap_report.py`（结果写进 `ipc-contract.json`，取代之前那份不可靠的启发式产物）：把 `RiftFlow/src/main/shards/*` 里所有 `ipc.onCall(ns, 'method')` 与 Rust 侧所有 `on_call(NS, "method")`（含 `for (name, ..) in [...]` 这种成组注册）对拍。

- **TS 119 个方法，Rust 侧 119 个，missing = 0。** 注意三个前提，缺一个这个数字就是假的：必须扫**所有 crate**（`getAll`/`reloadPlayer` 等注册在 `crates/riftflow-ongoing-game/src/pipeline.rs`，`peekClient`/`fixWindowMethodA`/`rebuildWmi` 在 `crates/riftflow-lcu/src/shard.rs` 的成组表里），必须识别元组循环，必须只取注册位里的字符串。
- 全路由真机扫描（15 条路由 + 3 个窗口）配合新加的 `[ipc:err]` / `[ipc:reject]` 两条 trace：整轮 **0 个 envelope 错误、0 个未知方法拒绝**。之前 `Reply(success=false)` 在日志里完全隐形，这就是"方法存在但一直失败"能藏住的原因。

**self-update 不再是假失败**：原实现里 `shouldRunSelfUpdateLifecycle()` 只是 `win32 && x64`（本机为真），所以之前的 `checkUpdates` 恒返回 `failed` 是一个真缺陷而不是忠实移植。现在：

- `launchers.rs::resolve_release_info` / `is_supported_artifact` 照抄 `release-info.ts`（win32+x64 且 `application/x-7z-compressed`，或 `application/octet-stream` + `.7z` 文件名），`is_newer` 用 riftflow-api 的 semver；三个单测覆盖工件筛选、新旧比较、版本不可解析。
- `checkUpdates` 真去打远端：`refresh_latest_release(locale)` → 计算 → 发布 `self-update-main:state#releaseInfo` → 按原语义返回 `new-updates` / `no-updates` / `failed`，`isCheckingUpdates` 包住整个过程。
- 真机：`checkUpdates -> {result:"no-updates"}`，`releaseInfo` 里是上游真实的 `version:"1.5.1"`、`artifact.fileName:"riftflow-win-x64.7z"`、`isNew:false`、`isUpdateSupported:true`。

还没做（诚实清单，第十四节已划掉一部分）：cd-timer 的 `gameTime`（2999 live-client API）、热键 hook 在应用进程内偶发不回调那个问题。`uninstallApp` 与 `relaunchAsAdministrator` 是真实现但没有真机跑过（前者会删掉安装目录，后者会弹 UAC），只验到编译与参数拼装。

`cargo test --workspace`：558 passed / 0 failed。

## 十四、shell 层最后五个 NotImplemented 清零 + 真机有客户端的一轮

`handlers.rs` 里 `pending()` 这个辅助函数已经删掉——现在整个 `crates/riftflow-app` 里没有任何方法回答"还没移植"了（`grep -rn "NotImplemented\|not ported yet" crates/` 只剩注释）。

**日志文件**（`crates/riftflow-app/src/logger.rs`，新）：原来 `logger-factory-main:log` 只在 `RIFTFLOW_RENDERER_LOG` 打开时往 stderr 转一份，应用根本没有日志文件，`openLogsDir` 因此只能报"未实现"。现在照 `src/main/logger/index.ts` 移植：目录取 `<exe 同级>/logs`（不是 userData，和 Electron 版一致），文件名 `LA_YYYYMMDD_HHmmssSSS.log`，行格式 `[YYYY-MM-DD HH:mm:ss:SSS] [ns] [level] msg`，级别过滤按 winston 的 error<warn<info<debug，`LogMessageFormatter` 的错误对象（有 `message`/`stack`）与两空格 JSON 也照抄。时间戳用 `civil_from_days` + `GetTimeZoneInformation`，没有引入 chrono。四个单测钉住级别门限、未知级别回落到 info、错误对象格式、以及文件名时间戳形状。

真机（`tmp/verify_shell_ops.py`，自己写的 CDP 客户端，因为这机器上没有 node）：

- 渲染进程 `log('shell-probe','info', 'hello from the renderer', {nested:{a:1}}, {message:'boom',stack:'at thing'})` → 文件里出现
  `[2026-10-27 06:07:56:046] [shell-probe] [info] hello from the renderer {\n  "nested": …\n} boom\nat thing`，同一次调用里那条 `debug` 被门限挡掉，没进文件。
- `setLogLevel('debug')` → 同步属性 `logger-factory-main:state#logLevel` 从 `info` 变 `debug`，`base-config.json` 写入 `logLevel`，并且按原实现的顺序先打一行 `Setting log level to debug` 再改级别（两行都在文件里）。
- `openLogsDir` → 新起一个 explorer 窗口，标题 `logs - 文件资源管理器`（用 `tmp/close_logs_explorer.ps1` 枚举窗口确认后关掉，只关我开的那个）。

**其余四个**：`readClipboardText` 直接接 `riftflow_native::clipboard::read_text()`（真机返回 `RIFTFLOW_CLIPBOARD_PROBE_2026`，与 Electron 一样把"剪贴板没有文本"当成空字符串而不是报错）；`exit` = `flush_all` + 进程退出（真机调用后 `Get-Process riftflow-app` 为空）；`setDisableHardwareAcceleration` 保留原实现的两个提前 return，然后写 `base-config.json` + 重启——Electron 的 `app.disableHardwareAcceleration()` 在 WebView2 侧只有 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 这一条路，所以 `app_common::apply_disable_gpu` 负责把这个参数拼进去/摘出来并保留其它参数。真机三轮：同值调用 PID 不变（31316→31316，提前 return 生效）；改成 true → 旧进程退出、新 PID 24332、`base-config.json` 出现 `"disableHardwareAcceleration":true`、新实例打印 `已禁用硬件加速，WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223 --remote-allow-origins=* --disable-gpu`（说明启动时真的按 base config 应用了，且没有吃掉调试参数）；再改回 false → 新 PID 14172、配置回到 false、那一行不再出现。

**自更新节律**：`self_update::start` 在启动时先读 `NEW_VERSION_FLAG` 再按 `autoCheckUpdates` 打一次远端，之后每 4 小时一次。真机证据是"不点任何按钮"的冷启动：新实例里 `self-update-main:state#releaseInfo` 已经是 `{version:"1.5.1", currentVersion:"1.5.1", isNew:false, isUpdateSupported:true, artifact.sha256:…}`，而这一轮脚本没有调用过 `checkUpdates`；`lastUpdateSucceeded` 仍为 `null`（没有待确认的新版本，符合原语义）。`PLACEHOLDER_NAMESPACES` 因此去掉 `self-update-main`，只剩 sgp / client-installation / game-client / renderer-debug 四个，而且这四个的方法都是真的，只是还没逐条做行为验证。

**客户端在场的一轮**（LeagueClientUx 起来之后）：`#/player-tabs` 真数据渲染——召唤师卡（头像、等级 1、`烈焰sddf#43532`）、单双排/灵活组排两张段位卡、一条真实对局（`Aborted`、自选自定义、01:13、2 days ago、Gold 徽章、英雄与召唤师技能图标）、Assets 44 英雄 / 44 皮肤、12 个 `<img>` 全部加载成功、0 个坏图；公告弹窗是真远端内容（4 个 QQ 群 + Telegram），用 CDP 真点了 `I have read` 才关掉。启动阶段唯一的失败仍是连接建立之前的 3 个 `current-summoner` 503，连上之后没有新增失败。

### 十四·补 之前那份路由扫描有一半是假路由（重要更正）

`TabbedPage.vue` 把 `route.params.section` 直接赋给 `currentTab`，所以 `/toolkit/:section?` 只认 `Toolkit.vue` 里声明的 key。之前清单里用的 `#/toolkit/saved-players`、`#/toolkit/replay-downloader`、`#/toolkit/hextech`、`#/toolkit/notes`、`#/toolkit/spender`、`#/automation/auto-misc`、`#/automation/respawn-timer` **在原版里同样是空白面板**——它们不是路由，是页内功能名。也就是说"15 条路由 0 报错"当时只覆盖到了真正的 4 个面板，其余是"空页面没有报错"。`tmp/cdp_sweep.py` 换成真实 tab key 之后重扫 19 个面板，全部有内容且 0 异常：

- toolkit：client 383 / in-game-send 710 / in-process 341 / lobby 352 / misc 554 / claim-tools 469 / friend-tools 395 个节点（`loot-tools` 只有 DEV 构建才注册，生产包里是空的，与原版一致）。
- automation：auto-gameflow 569 / auto-select 603（9 张英雄图全加载）/ auto-champ-config 321 / misc 348。
- test：markdown 583（1 张外链示例图坏图，离线正常）/ self-update 446 / **game-data 12417 个节点、863 张图 0 坏**（真机 game-data 全量渲染）/ match-history-filters 308 / keyboard-shortcuts 342。

### 十四·补2 提权重启是真跑通的（也是唯一一个我关不掉的实例）

`#/test/keyboard-shortcuts` 上的"提权重启"按钮真点了一次：日志 `[ipc] riftflow_call app-common-main:relaunchAsAdministrator args=0` → 旧进程退出 → 新 PID 起来，而且 `taskkill /PID <新> /F` 从普通权限的 shell 返回 **Access is denied**，这就是"它真的是提权进程"的证据（同机器上普通进程是可以被杀掉的）。桌面截图里提权实例的界面正常渲染真数据。副作用要说清楚：**这个提权实例我从这里关不掉**（UIPI 正是我们自己移植的那条限制），只能由用户从托盘退出。

`relaunch_as_administrator` 用的是 `powershell Start-Process -Verb RunAs -ErrorAction Stop`（原版是打包的 `elevate.exe`，这个二进制不在 Rust 包里；`-ErrorAction Stop` 是为了让"用户点了取消"变成非零退出码，从而走 `Err` 分支——否则 PowerShell 在非交互下吞掉这个错误返回 0，我们就会像原版语义里不该退出的时候退出）。两点没查清、下次接着看：调用之后大约一分钟还有一个 `powershell.exe`（PID 9136）活着，我把它杀了，但没有证据说明它是这次提权的宿主还是之前 `Set-Clipboard` 的残留；以及提权起来的实例**没有**监听 9223（`child.wait()` 返回前旧实例已经让出了端口，怀疑是绑定竞争或提权后的 WebView2 起了另一个 browser process），所以那一轮之后我没法再用 CDP 驱动它。

`cargo test --workspace`：567 passed / 0 failed。

## 十五、四个"占位"命名空间全部真验过，占位表清空；cd-timer 的 gameTime 落地

### cd-timer 的 gameTime

原实现是 `_updateGameStats`（`cd-timer-window/windows.ts:269`）：`shouldUseCdTimer` 变 true 时立刻打一次 `GET https://127.0.0.1:2999/liveclientdata/gamestats`，之后每 `GAME_STATS_POLL_INTERVAL = 4000` 一次；失败 `setGameTime(null)`；变 false 时 `clearInterval` 并把 gameTime 置 null。现在 `aux_windows.rs` 里是同一套：`reveal_now` 起、`conceal_now` 停，停的时候置回 null，取不到就报错并写 null，端口复用 `respawn_timer::LIVE_CLIENT_DATA_PORT`（同一个 live-client data server）。停止用 `AtomicBool` 而不是 `JoinHandle::abort()`——`tauri::async_runtime::spawn` 返回的是 tauri 自己的 JoinHandle，没有 abort，而且让轮询把手上那次请求做完再退出更干净。一个单测钉住 `gameTime` 只接受真数字（字符串、缺字段、非对象都是 null）。**没有真机验证**：2999 只有在对局进行中才存在。

### 四个命名空间的行为验证（`tmp/verify_shell_ops.py`）

- `client-installation-main:state` 是这台机器上真实探测出来的：`tencentInstallationPath = D:\WeGameApps\LOL`、`tclsExecutablePath = D:\WeGameApps\LOL\Launcher\Client.exe`、`weGameLauncherExecutablePath = ...\WeGameLauncher\launcher.exe`、`leagueClientExecutablePaths` 两条（`d:/wegameapps/lol/...` 与 `d:/wegameapps/英雄联盟（含经典模式）/...`）。`weGameExecutablePath` 与 `officialRiotClientExecutablePath` 是 null——这不是漏了：原版靠 `HKCU\wegame\DefaultIcon` 和 `HKCU\Software\Tencent\LOL` 两个键找，`reg query` 确认这台机器两个键都不存在，所以原版在这台机器上同样是 null。
- `game-client-main` 同步的是 **settings** 面（`index.ts:186` propSync `'settings'`），不是 state——我第一次探 `:state` 拿到 `{}` 是探针错，不是缺陷。真值：`{terminateGameClientWithShortcut:false, terminateShortcut:null}`。`getSettingsFileReadonlyOrWritable` 在真实的 PersistedSettings.json 上跑了一次完整来回：`writable → set readonly → readonly → set 回 writable → writable`。`terminateGameClient` 故意没调（它会杀掉游戏进程）。
- `sgp-main:state#availability` 是真数据：`{region:TENCENT, rsoPlatform:HN1, sgpServerId:TENCENT_HN1, serversSupported:{common,matchHistory}=https://hn1-k8s-sgp.lol.qq.com:21019}`，`connectionSuccessesCounted:4 / failures:0`。
- `renderer-debug-main` 两个开关真写进同步面：`setSendAllNativeLcuEvents(true)` / `setLogAllLcuEvents(true)` 后属性为 true，再关回 false。第一次跑失败报 `enabled should be a boolean`——那是我的探针把 `true` 当字符串发了，处理器的严格校验是对的。

`PLACEHOLDER_NAMESPACES` 因此清空（连同 `self-update-main`，上一轮已验），启动日志改成只在非空时打印。

### 两个环境事实（都会再遇到）

1. **两个实例不能共用 WebView2 的 `EBWebView` 目录**：第二个实例的 webview 创建直接失败 `HRESULT(0x8007139F)`（"组或资源的状态不是执行请求操作的正确状态"），窗口建不出来，CDP 也就不监听。这解释了上一轮"提权实例起来后 9223 没监听"——不是提权的问题，是旧实例退出与新实例绑定之间的目录争用。绕开办法：给验证实例换一个 `APPDATA`（`tmp/verify-profile`），它就有自己的 `EBWebView` 和自己的库，能与提权实例并跑，CDP 正常。
2. **国服客户端"收到托盘"时不是最小化**：`RCLIENT` 窗口 `IsIconic=false` 而 rect 是 `0,0,136,39`，`ShowWindow(SW_RESTORE)` 也改变不了。`is_usable_as_align_target` 按尺寸跳过是对的（原版同规则），但意味着贴边对齐只有把客户端界面真正显示出来才验得到。

### 重扫

19 个真实面板全过、0 异常，内容比第十四节那轮多得多（每页文本 3000-5000 字符，`#/test/game-data` 12650 节点 / 863 图 0 坏）——上一轮的公告弹窗遮住了页面。唯一的失败是上游自己的：`GET riftflow://league-client/lol-game-data/assets/content/src/LeagueClient/GameModeAssets/Cherry/img/game-select-icon-hover.png` 由 LeagueClient 返回 `400 RPC_ERROR`（它自己缺这个文件），我们只是把状态原样透传。

`cargo test --workspace`：568 passed / 0 failed。

## 十六、把每个按钮真点一遍：63 次点击，0 个 IPC 错误

面板能画出来不等于按钮能用。`tmp/cdp_click_all.py` 走的是后者：进每个真实面板，把 `button / .n-button / [role=button]` 去重后逐个 `click()`，按标签跳过会退出、杀进程、卸载、要 UAC 的那几个（退出/exit/quit/卸载/uninstall/终止/terminate/提权/重启/restart/relaunch/删除/delete/清除/clear/移除/remove/关闭/close），点完之后从页面侧收 `Runtime.exceptionThrown`，从 shell 侧看 `[ipc:err]` / `[ipc:reject]`。

结果：**15 个面板、63 个按钮、0 个 JS 异常、0 个 `[ipc:err]`、0 个 `[ipc:reject]`**（`#/toolkit/loot-tools` 不在表里，生产包不注册这个 tab；`#/test/markdown` 没点，页面只有示例渲染）。

这一轮顺带把自更新的下发路径按到底了：测试页的"强制触发自动更新"真的把 88,579,402 字节的上游工件拉了下来（`tmp/verify-profile/riftflow/NewUpdates/riftflow-win-x64.7z`）、把 `riftflow-updater.exe` 复制进 `%TEMP%` 并登记了 pending；随后 `cancelUpdate` 返回 `{result:"ok"}`，`NewUpdates` 变空、temp 里的更新器消失、`updateProgressInfo` 回到 null——所以退出时不会有任何东西去覆盖 `target/verify/debug/riftflow-app.exe`（该文件时间戳仍是 06:42，未被替换）。

两个踩坑记录，都是工具的问题不是应用的问题：

- 下载 88MB 期间 `Runtime.evaluate` 会超过 60s 不返回，脚本误判"应用挂了"并 `launch_app()` 起了第二个实例；第二个实例因为 `EBWebView` 被第一个占用而 webview 创建失败（第十五节那条），变成一个没有窗口的僵尸进程，已 `taskkill` 掉。**结论：点下载类按钮前要把 evaluate 超时放宽，或者只点不等待。**
- 探针的 `DENY` 是按标签文本匹配的，英文界面下中文词不生效——所以列表里中英都要有。

仍然只有代码证据、没有真机证据的：托盘左键与右键菜单项（UIA 驱动提权/隐藏图标面板太容易留下脏状态，这轮没做）、gameTime 的真机读数、对局内键盘注入、窗口贴边。

## 十七、设置写入路径：40 个勾选框点完，17 项改动全部活过重启

面板上的"开关"在这个应用里其实是 `.n-checkbox`（`#/toolkit/in-game-send` 有 13 个 `.n-checkbox`、0 个 `.n-switch`），所以 `tmp/cdp_toggle_sweep.py` 的选择器要同时含 `.n-switch / [role=switch] / .n-checkbox / [role=checkbox]`。三步验证：

1. 点之前把 13 个命名空间的 `*:settings` 面全部快照（83 个属性）；逐面板点完 40 个勾选框；再快照 → **17 个属性真的变了**（`auto-gameflow-main` 9 项、`auto-misc-main` 5 项、`auto-select-main#pickConfig`、`in-game-send-main#ratingPresetOptions`、`auto-champ-config-main#enabled` 等），0 个 JS 异常，0 个 `[ipc:err]` / `[ipc:reject]`。嵌套对象属性（`pickConfig`、`ratingPresetOptions`）也在写，说明不只是标量通路。
2. 用 `app-common-main:exit` 正常退出（顺带再验一次 exit 与退出前的 `flush_all`），重启实例，再快照 → **与重启前 0 差异**，17 项改动全部来自 SQLite。
3. `claim-tools` 3 个、`friend-tools` 1 个点了没翻转：它们不是设置项（选择类勾选框，没有对应 settings 属性），不算失败。

一个工具教训：**JS 探针字符串里不能写 `//` 注释**——整段表达式是一行，`//` 会把它后面所有内容注释掉，`evaluate` 报语法错误，而 `.get("value") or []` 会把这个错误变成"什么都没找到"，于是整整一轮扫描白跑（第一次跑就是 40→0）。现在脚本会把 `error` 打出来。

这轮跑在隔离 profile（`tmp/verify-profile`）上，被翻转的是那个临时档案的设置，用户真实配置没动；实例已关，机器上只剩用户自己那个提权实例。

## 十八、"热键 hook 偶发不回调"查下来是测试环境的假象；favicon 的 500 确认已修

### hook 那条

`RIFTFLOW_HOTKEY_TRACE=1` 起实例（日志确认 `keyboard hook thread pumped its first message`，即钩子线程的消息泵在跑），然后用 `SendKeys` 连发六次 `qwe`：应用侧 **0 条 `hook callback`**。看起来像复现了那个"偶发不回调"。

于是写了对照组 `crates/riftflow-native/examples/hook_watcher.rs`——另一个进程里一个自己装的 `WH_KEYBOARD_LL` + `GetMessageW` 泵，跑 7 秒打印每个回调。同一时间发同样的六次 `SendKeys`：**对照组也看到 0 个回调**。

也就是说按键根本没进入系统输入流：当时的前台窗口是那个**提权**的 RiftFlow 实例，medium IL 的 PowerShell 注入被 UIPI 丢掉（正是我们自己移植的那条规则，`input_probe` 也报了 `foreground=Elevated injectionAllowed=false`）。所以"4/6 次要别的进程也装钩子才收到"这个旧观察不能算已证实的应用缺陷，它是同一个混淆因子的另一种表现。

要把这条真正验掉，需要一个非提权的前台窗口：要么用户把那个提权实例退出，要么由用户手按一个物理键（那时对照组和应用侧应该同时看到）。在此之前这条只能记成"未证实的缺陷 + 未验证的通过"，不能记成已修好。

### favicon

`fetch('/favicon.ico', {cache:'no-store'})` → `200 image/vnd.microsoft.icon` 699,556 字节。之前"仍然是 500，只是被 Chromium 缓存遮住"的怀疑用 `no-store` 排掉了，`renderer-dist/favicon.ico`（从 `assets/LA_ICON.ico` 复制）确实是被 Tauri 的资源处理器正常服务的那一个。已知缺陷清单里这一条可以划掉。

## 十九、契约升级：从"方法名对得上"到"命名空间也对得上"（前端重写的前置件）

计划里承诺给前端的第 (2) 条之前只做到了一半：`tmp/gap_report.py` 比的是**方法名**，看不见"名字对但注册在错命名空间下"这种会让按钮直接 `unknown method` 的错法。现在有两个东西：

- **运行时的权威注册表**：`riftflow-ipc` 加了 `registered_calls()`，`main.rs` 在 `RIFTFLOW_DUMP_IPC_REGISTRY=<路径>` 时把 router 真实的 `(namespace, method)` 全量写成 JSON。本次实测 **191 个方法 / 25 个命名空间**，含五个窗口命名空间各自一份（`window-manager-main/{main,aux,opgg,ongoing-game,cd-timer}-window`，17-20 个方法不等，`main-window` 多 `closeMainWindow`/`closeMainWindowForce`，`aux`/`opgg` 多 `repositionToAlignLeagueClientUx`，`cd-timer` 多 `sendInGame`）。静态 grep 之所以不够，正是这些按窗口成组注册的部分。
- **`tmp/contract_namespaces.py`**：TS 侧解析 `onCall(ns, 'm')`（含 `BaseRiftFlowWindow` 的 `${namespace}/${suffix}` 规则：基类方法属于每个窗口，子类方法只属于自己），Rust 侧优先读上面的 dump，然后按 `(ns, method)` 对拍。结果：**TS 24 个命名空间 188 个方法，missing = 0**；Rust 多出 3 个（`keyboard-shortcuts-main:_getInternalVars`、`league-client-main:http-request`、`riot-client-main:http-request`），都是有意的额外入口，不是漂移。

`ipc-contract.json` 现在是 `{ts, rust, missingFromRust, rustOnly, unresolvedRust}`，前端重写阶段可以直接拿它生成 typed mirror 层。两个已知边界要说清楚：dump 是启动第一圈打的，这次它已经覆盖到五个窗口命名空间（cd-timer 与 ongoing-game 的注册在启动时就完成了），但**如果某个窗口的注册真的推迟到对局中，这份 dump 会漏**——要绝对保险就在对局里再打一次；TS 侧仍靠正则，改注册形状时要一起改脚本。

## 二十、调用覆盖率审计：从"方法存在"到"UI 真调用过"

`tmp/coverage_audit.py` 把三个集合叠起来：router 自己 dump 的 191 个注册方法、历次扫描日志里真实出现过的 `[ipc] riftflow_call ns:method`、以及**出厂 renderer 包里是否以字符串形式引用过这个方法名**（esbuild 把调用参数压成反引号模板串，只找引号会一个都找不到——第一次跑就是 191 全判成"renderer 用不到"）。

第一轮的真相：**191 注册 / 168 renderer 可达 / 只有 29 个真被 UI 调用过**。之前几轮（路由扫描、63 个按钮、40 个勾选框）加起来只覆盖了两成多，"所有功能都能用"远还没到能说出口的程度。

第二轮补的是最大的一块：`window-manager-main/<窗口>` 这 89 个按窗口注册的方法，之前没有任何扫描碰过（它们由标题栏和托盘驱动，不在页面里）。`tmp/drive_windows.py` 对五个窗口逐个跑 `getSize → setSize(620,480) → getSize → getPosition → setPosition(+24) → getPosition → setTitle → minimize → restore → maximize → unmaximize → hide → show → resetPosition`，并且**每项都用同一个 IPC 读回效果**：五个窗口全部 `setSize`/`setPosition` 成功且 `getSize` 从 `[1500,860]/[340,420]/[530,720]/[1300,840]/[144,226]` 变成 `[620,480]`、坐标也真的移动了。有意跳过的：`close` / `closeMainWindow(Force)`（会把窗口或应用带走）、`toggleDevtools`（再开一个 webview）、`setIgnoreMouseEvents`（会把窗口变成点击穿透）、`sendInGame`（要对局）。

跑完覆盖率变成 **89 / 168 被真实调用过**（29 → 89）。

三条要记下来的：

- `setSize` / `setPosition` 的线格式是**位置参数** `(width, height, animate?)` / `(x, y, animate?)`（`base-riftflow-window.ts:189/197`），第一次驱动我传了对象，六个 FAIL 全是探针的错而不是应用的错——应用正确地按参数校验拒绝了。
- `ongoing-game-window` 与 `cd-timer-window` 在驱动时不存在（前一轮的勾选框扫描把它们 `enabled` 翻成了 false），所有调用按"窗口不存在"失败；用 `setting-factory-main:set` 打开后窗口建出来，全部方法就通了。也就是说这两扇窗的注册与生命周期是跟着 `enabled` 走的，符合原设计。
- **一个待查的不一致**：`hide → show(true)` 之后 `window-manager-main/aux-window:state#show` 仍然是 `false`（其余四个窗口都从 false 翻到 true）。aux 是 overlay，它的 `show` 属性可能本来就记的是"用户可见"而不是"调用过 show"，但这条没查清，记在这里而不是当作已验证。

剩下的 79 个未驱动方法集中在 `in-game-send-main`（预设条目的增删改与发送，需要多步交互）、`saved-player-main`（标签、导入导出）、`league-client-main`（connect/disconnect/writeItemSetsToDisk）、`client-installation-main` 的四个启动按钮（会真拉起外部程序）以及 `ongoing-game-main` 的选人/对局期方法（要有对局）。

## 二十一、驱动 in-game-send 整块：找到一个真缺陷（补丁被静默丢弃）

`tmp/drive_in_game_send.js` 把 22 个 in-game-send 方法按"调用 + 读回"跑了一遍：`createFixedTextPresetItem` ×2（拿到两个 UUID 条目）→ `updateFixedTextPresetItem`（`{title:'压路',content:'中路miss',shortcut:'Ctrl+1'}` 写进去并读得回来）→ `moveFixedTextPresetItem(id,'up')` 返回 true 且顺序真的变成 `["", "压路"]` → `sendFixedTextPreset` 返回 `false`（没对局，符合三道门）→ 两个 delete 都 true、列表回到空。选项侧：`updateRatingPresetOptions` 的 `{kda:true,targetShortcuts:{friendly:'F1'}}` 合并进设置、`setRatingPresetOptions` 整体替换把 `winRate` 从 false 翻成 true、传一个缺全部字段的对象被 `ZodError` 拒绝并列出缺哪些键。覆盖率从 89 升到 **107 / 168**。

两个"看起来像 bug 其实是忠实移植"的，都去原实现对过：

- `setRatingPuuids/setJunglePuuids/setPremadeIndices` 调用成功但状态读回来是空——原实现 `preset-selection-controller.ts:40` 就是 `state.setRatingPuuids(this._filterCurrentPuuids(puuids))`，没有对局就没有可选选手，过滤后必然为空。
- `generate*PresetLines` 返回 `[]` 同理（选手集是空的）。

**一个真缺陷，已修**：`updatePremadePresetOptions({nameDisplayStrategy:'preferSummonerName'})` 原来返回成功、设置不变、也没有任何错误——因为 `parse_patch` 把解析失败的补丁"按空补丁处理"。原版走的是 `_updatePresetOptions → _setPresetOptions` 的 zod `parse`，非法枚举值是要抛错的。现在 `parse_patch` 返回 `Result`，三个 `update*PresetOptions` 调用点用 `?` 传播，错误按原版的形状报成 `ZodError`。真机复验：合法值 `preferName` 写入生效；非法枚举被拒并回显 `unknown variant \`preferSummonerName\`, expected one of \`preferName\`, \`preferChampionName\`, \`championNameWithName\``，且设置保持为上一个合法值；`{dragonControl:'yes'}` 这种类型错也被拒。`cargo test --workspace` 568 passed / 0 failed。

剩下的未驱动块：`saved-player-main` 12 个（标签增删、导入导出会弹原生文件对话框）、`league-client-main` 6 个（connect/disconnect/writeItemSetsToDisk/订阅原生事件）、`ongoing-game-main` 5 个（要有对局）、`client-installation-main` 4 个 launch（会真拉起外部程序，需要用户同意）、`in-game-send-main` 还剩 `setJunglePresetOptions` / `setPremadePresetOptions` 两个整体写入没点。

## 二十三、fixWindowMethodA 从"报不支持"变成真移植；saved-player 与 league-client 驱动完

### 又一个"方法存在但一直报不支持"的：fixWindowMethodA

`riftflow-lcu/src/shard.rs` 里它原来固定返回 `NativeNotSupported` 业务错误。原实现是 `league-client/index.ts:673`：先看 `NATIVE_SUPPORT.adjustLeagueClientWindowSize.available`（`native/index.ts:64` 的 `isWindows && isElevated`），不可用就 **返回 undefined 连 zoom 都不读**；可用则 `GET /riotclient/zoom-scale`，再调 addon 的 `tools.fixWindowMethodA(zoom, config)`。addon 的算法在 `native/win32-x64/src/tools/tools.cc:110`：`baseWidth/baseHeight` 默认 1280/720，`zoom == -1` 抛 "Invalid original zoom"，`FindWindowW("RCLIENT","League of Legends")` 与 `FindWindowExW(parent, "CefBrowserWindow")` 任一找不到抛 "Can't find LeagueClient window"，然后 `width = (int)(base*zoom)`、主窗口 `SetWindowPos(..., (screenW-width)/2, (screenH-height)/2, width, height, SWP_NOZORDER)`、CEF 子窗口 `SetWindowPos(0,0,width,height)`，返回 true。

移植成三块：`riftflow-native/src/placement.rs` 的 `scaled_window_size`（纯函数，钉住缩放截断、居中、以及 -1 的报错）+ `adjust_league_client_window_size`（同样的窗口查找与两次 `SetWindowPos`）；`riftflow-lcu` 侧因为 `riftflow-native` 在它之上（加依赖会成环），改成**注入**：`shard::set_window_adjuster(fn(f64, Option<(i32,i32)>) -> Result<bool,String>)`，由 `main.rs` 在 `cfg!(windows) && is_elevated()` 时注入——注入与否就是原版的 `available` 标志，所以未提权时行为和原版一样是静默 no-op。`app_common.rs` 的 `nativeSupport.adjustLeagueClientWindowSize` 也从写死的 `requirement(false, true)` 改成 `requirement(win32 && elevated, win32)`。

真机（未提权的验证实例）：`fixWindowMethodA -> {success:true, data:null}`（不再是错误），`nativeSupport` 读回 `{available:false, availableOnCurrentPlatform:true, requiresElevation:true}`。**提权路径下的真实缩放没有真机证据**（需要一次 UAC），只有纯函数单测与窗口 API 的形状。

顺带：`crates/riftflow-lcu/tests/connection_lifecycle.rs` 里那条钉住旧桩行为的测试（`..._reports_the_missing_native_bridge`，断言 `NativeNotSupported`）改成了断言新的忠实契约；`target/debug/examples/edit_roundtrip.pdb` 被锁会让整个 workspace 测试链接失败（LNK1285），删掉陈旧产物即可恢复——那个 example 本身不是移植的一部分。

### saved-player / league-client 驱动

`tmp/drive_saved_player.js`：`saveSavedPlayer → querySavedPlayer → querySavedPlayerWithGames → updatePlayerTag → getPlayerTags → saveEncounteredGame → queryEncounteredGames → deleteEncounteredGame → deleteSavedPlayer → queryAllSavedPlayers` 全部成功，中间读回都对（标签改完能查到、`deleteEncounteredGame` 返回 `affected`）。`getAllPlayerTags` / `queryAllSavedPlayers` 返回的是带 `count/page/pageSize/data` 的分页对象，与原版一致。

`league-client-main`：`subscribeLcuEndpoint` 成功并返回一个订阅 id、`unsubscribeLcuEndpoint` 返回 false——因为我传的是 URI 而不是那个订阅 id（原版签名是 `unsubscribeLcuEndpoint(subscriptionId)`），是探针的错；`writeItemSetsToDisk({}, false)` 返回 `{written}`、`disconnect` 之后自动重连的轮询在十几秒内把连接恢复了（`http-request` 重新成功）。`peekClient` 与 `connect` 我按无参调用被 "auth is required" 拒绝——这两个的参数是凭据对象，是探针的错不是应用的错，正确的驱动留待下次（渲染进程是从 `getSavedCredentials` 拿到 auth 再传的）。

覆盖率：**126 / 168**（上一轮 107）。剩下 42 个的构成：`window-manager-main/*` 里被有意跳过的 `close`/`closeMainWindow`/`toggleDevtools`/`setIgnoreMouseEvents` 等（跳过是因为会把窗口或应用带走、或把窗口变成点击穿透）、`client-installation-main` 的四个 launch（会真拉起外部程序）、`saved-player-main` 的导入导出（要人手点原生文件对话框）、`ongoing-game-main` 与 `in-game-send` 的发送类（要有对局）。

`cargo test --workspace`：569 passed / 0 failed。

## 二十四、自动化 / 设置面驱动完，覆盖率 144/168；两处"看着不对"都查成忠实移植

`tmp/drive_settings_cluster.js` 一轮跑完并读回效果：`auto-select-main:setTemporarilyDisabled(true)` 之后 `state#temporarilyDisabled` 读回 true（再关回 false）；`setBanConfig({aram:{...}})` 之后 `setting-factory-main:get('auto-select-main','banConfig')` 读回 `aram/bot/cherry/custom` 四个键；`auto-champ-config-main:updateSummonerSpells` / `updateRunes`、`auto-gameflow-main:cancelAutoAccept` / `cancelAutoMatchmaking` / `setFriendsToBeInvited`、`auto-misc-main:applyStatusMessage`、`ongoing-game-main:setMatchHistoryTagParams` / `clearDraft` / `reload`、`keyboard-shortcuts-main:getDebugState`、`league-client-ux-main:update`、`riftflow-protocol-main:cancelProxyRequest`（不存在的 id → false）全部成功。两处失败是探针参数错：`getRegistration` 要一个字符串 id、`reloadPlayer` 明确回 "reloadPlayer needs a puuid"（这正是该有的校验）。

两处一开始被我当成缺陷、查原实现后确认是忠实移植的：

- `setting-factory-main:getByPrefix('auto-select-main','ban')` 返回空、`removeByPrefix(...,'friendsToBeInvited')` 返回 0。原因不是 bug：原版 `_getStorageKeyPrefixBounds`（`setting-factory/index.ts:215`）算的是 `lowerBound = ns/keyPrefix/`、`upperBound = lowerBound.slice(0,-1)+'0'`，也就是**前缀是"键路径段"而不是字符串前缀**——`ban` 只会匹配 `ban/...`，不会匹配 `banConfig`。我们的 `prefix_bounds` 与它逐字符一致，所以行为相同。
- `setFriendsToBeInvited` 写完之后按 *settings* 去读是 null。原版 `friendsToBeInvited` 就在 `auto-gameflow/state.ts:143`，是状态属性不是设置项，我们写的也正是 `state`，一致。

覆盖率：**144 / 168 被真实调用过**（`reachable but never driven` 还剩 38 个，其中 25 个是窗口面有意不点的）。有意不点的：每窗口各一份的 `close`/`toggleDevtools`/`setIgnoreMouseEvents`/`downloadUrl`（会把窗口关掉、再开一个 devtools webview、或让窗口变点击穿透）、`closeMainWindow(Force)`、`sendInGame`（要有对局）、`client-installation-main` 四个 launch（会真拉起 WeGame/TCLS）、`saved-player-main` 的导入导出（要人手点原生文件对话框）、`self-update-main:uninstallApp`（会删安装）、`league-client-ux-main:rebuildWmi`（会重建 WMI 仓库）。这些不是"没实现"，是"需要用户同意或有破坏性"，清单里逐条列明。

## 二十五、用户报的真缺陷：选人期窗口反复抢焦点、关不掉（已修）

现象（用户口述）：进房间后弹出"自动接受"那个窗口，它永远在前台，点别的应用时它会反复跳回前台，导致别的应用用不了；关掉它又立刻弹出来，只有回 LOL 客户端退出房间才行。

根因在 `aux_windows.rs` 的 watcher：它是**每 500ms 轮询**的，而 `opgg()` / `aux()` / `ongoing_game()` 里"应该显示"的分支每个 tick 都会调 `self.reveal(suffix, false)`，`reveal` 又走 `show_or_restore(..., inactive=false)` → `show()` + `set_focus()`。于是选人阶段窗口每秒被重新置顶并抢两次焦点；用户点关闭时 `handle_close` 按原版语义只是隐藏，state#show 变 false，下一个 tick 判定"应该显示"就再弹出来——看起来就是"关不掉 + 一直跳"。

原版不会这样，因为它本来就是事件驱动的：`BaseRiftFlowWindow.showOrRestore(inactive)` 的第一行是 `if (!this.state.show)`，只有状态从"没显示"变"显示"时才真去 `show()`/`showInactive()`；已经显示着的时候再调一次是空操作。

修法就是把那条守卫搬过来：`Watcher::reveal` 开头加 `if self.controls.shown(suffix) { return; }`。这样轮询重复投递同一个"该显示"的决定时不再重复 show+focus，而 `conceal` 之后（state#show=false）该显示时仍然会正常弹一次。`inactive` 参数保持原样（原版这些窗口第一次确实是 `show()` 而不是 `showInactive()`，抢一次焦点是对的，反复抢才是 bug）。

`cargo test --workspace` 569 passed / 0 failed。这条是用户报的，也是"144/168 方法都调用成功"覆盖不到的一类问题：**方法都对、行为在时间上错**。所以后续驱动不能只看单次调用成功，还要看 watcher 的重复投递语义。

## 二十七、对局期真机结果：gameTime 成了；对局内发送被 UIPI 拦住，原因查明

用户手开一局自定义（人机）对局后实测：

- **2999 在国服是可用的**，不需要任何"允许直播数据"开关：`League of Legends.exe` 直接 `LISTENING 127.0.0.1:2999`，`/liveclientdata/gamestats` 连续返回 `gameTime` 172.26 → 213.40 递增。之前"国服可能不支持"的猜测是错的。
- **我们的 `gameTime` 属性真的被填上了**：`window-manager-main/cd-timer-window:state#gameTime = 285.70`，同时 `show=true`、`ready=true`、`supportedGameModes=8`。cd-timer 的倒计时模式从此有时钟可用；对局还没开始时它按原语义打 `ECONNREFUSED` 并写一行拒绝原因，连上之后自动恢复。
- **键盘 hook 正常**：日志里出现真实回调 `hook callback vk=50 up`、`vk=13 down/up`、`vk=164 down/up`。任务 #20 那条"偶发不回调"确认是提权前台导致的观测假象，关掉。
- **`sendInGame` 的三道门都在正确位置拒绝**：90 次重试的结果是 62 × `GameClientNotForeground` + 20 × `NativeInputNotSupported`，0 次成功。原因不是代码坏，而是**这台机器上游戏进程是提权启动的**（WeGame 提权拉起 `League of Legends.exe`），medium IL 的应用向提权窗口注入必然被 UIPI 拒绝；游戏不在前台时第二道门先拒，游戏在前台时第一道门（`probe()` 看到前台提权）拒。这正是原版需要"以管理员身份重启"的原因。
- 于是用 `Start-Process -Verb RunAs` 把应用提权重启（用户接受 UAC，新实例确认是提权进程：普通权限 `taskkill` 回 `Access is denied`）。**提权实例不监听 9223**，所以我无法再用 CDP 驱动它，"对局内发送成功"这一步只能由用户在游戏里手点（cd-timer 浮窗或工具集 In-Game Send 的发送按钮）来确认。

顺带记一条实现上的不一致（还没改）：`send_in_game` 的第一道门用 `riftflow_native::input::probe()`（按当前前台窗口实时判 UIPI），而窗口存在性和 `nativeSupport` 用 `hotkeys::input_available()`（启动时算的 `NATIVE_SUPPORT.nativeInput.available`）。原版两处都用后者。差别是"前台恰好是提权窗口"时我们会多拒一次并回 `NativeInputNotSupported`。要不要对齐取决于我们是否愿意比原版更诚实地拒绝——目前倾向于保留实时判断但把错误码分得更细，而不是退回静默失败。

## 二十八、cd-timer 置顶（选项 2）+ 快捷键绑定：改完并真机跑通

用户选的方案 2（"给 cd-timer 窗口挂 `WS_EX_TOOLWINDOW | WS_EX_TOPMOST`，检测到 `League of Legends.exe` 抢前台时再补一次置顶"）落地在三处：

- `riftflow-native/src/placement.rs::raise_overlay()`：raw FFI（`GetWindowLongPtrW`/`SetWindowLongPtrW`/`SetWindowPos`）写 `WS_EX_TOPMOST|WS_EX_TOOLWINDOW` 并清掉互斥的 `WS_EX_APPWINDOW`，再用 `HWND_TOPMOST + NOMOVE|NOSIZE|NOACTIVATE` 重新占位。纯位运算拆成 `overlay_ex_style()` 便于单测（`the_overlay_style_pair_replaces_the_app_window_bit`）。原版 `setAlwaysOnTop(true,'screen-saver',1)` 的 level/relativeLevel 只在 macOS 有效，Windows 上等价物就是这一对 ex-style + 重新入带。
- `riftflow-app/src/windows.rs::set_topmost(window, suffix, pinned)`：**只有 spec 里 `skip_taskbar: true` 的窗口（cd-timer / ongoing-game）走原生通道**，其余（main / aux / opgg）保持 `set_always_on_top`，否则主窗口会掉出 Alt+Tab。三处原 `set_always_on_top` 调用点（`apply_window_settings`、`WindowControls::apply_overlay`、`Watcher::apply_pinned`）全部改走它。
- `aux_windows.rs::Watcher::reassert_on_game_foreground()`：`Applied` 新增 `game_foreground` 记忆位，只在"游戏拿到前台"这条**边沿**上补一次 `apply_overlay`，不是每个 500ms tick 都刷——重复刷就是上一轮那个抢焦点 bug 的形状。

快捷键绑定这条查清了原机制，**和直觉相反**：`ShortcutSelector.vue` 里显示的按键不是 DOM keydown 采的，`preventFn` 只挡 Ctrl+Shift+R / Ctrl+Shift+I / Alt+F4 三个；真正喂数据的是主进程 hook → `_ipc.sendEvent('keyboard-shortcuts-main','shortcut', details)` → 渲染层 `kbd.onShortcut`。所以"按下没反应"只可能是这条链断在某一环，而当时最可疑的一环是**能力位与事实不符**：`nativeSupport.nativeInput.available` 旧实现取 `riftflow_native::input::probe().0`（=启动那一刻"当前前台能不能注入"），而 `hotkeys` 那条监听链用的是 `input_available()`（hook 是否在线）。前台恰好是提权窗口时按钮是灰的，按钮亮着时又可能后面没人监听。现在 `available = win32 && hook 在线`，`requiresElevation` 保持 true（决定 unavailable 时显示哪句提示），并把 `hotkeys::start()` 从 `app_common::publish()` 之后挪到之前——否则启动快照到的 hook 状态永远是 false。

真机结果（非提权实例 + `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`，`RIFTFLOW_HOTKEY_TRACE=1 RIFTFLOW_IPC_TRACE=1`，profile 用 `tmp/verify-profile`）：

- 注入用 `tmp/inject_keys.ps1`（SendInput）+ `tmp/drive_shortcut_binding.py`（CDP 里挂 `riftflow-event` 监听并回读 DOM）。**踩坑两条**：后台进程 `SetForegroundWindow` 会被前台锁拒绝（先注一次 Alt 才有权利）；`HoldMs=40` 的按键 hook 收不到，120–150ms 稳定收到（日志 `hook callback vk=68 down/up` ×N）。
- 全链打通：`hook callback` → `send event shortcut id="D"` → `[ipc-push]` → 渲染层 `__shortcutHits` 里出现 `{name:'shortcut', id:'D', keyCodes:[68], pressed:true}`；`#/toolkit/in-game-send` 上 Select 按钮 `disabled:false`，点开标题 "Shortcut Recording" 的卡窗，按 D 之后卡窗正文变成 `Shortcut Recording / D / Cancel / Clear / Confirm`，点 Confirm 后主进程日志出现 `Register shortcut D for target in-game-send-main/preset/rating/friendly (last-active)`——绑定真的写进去了（随后已按原值 F1 复原，并把 cd-timer 的 `enabled` 设回 false）。
- 置顶改造的可离线部分也验了：`setting-factory-main:set('window-manager-main/cd-timer-window','enabled',true)` 之后窗口建出来，`tmp/read_window_style.ps1` 读到 cd-timer `topmost=True toolwindow=True appwindow=False`，作为对照的 opgg 窗口仍是 `toolwindow=False appwindow=True`。

还缺的一块：`reassert_on_game_foreground` 那条边沿需要真的 `League of Legends.exe` 在前台才能观测，得等一局自定义；日志锚点是 `[cd-timer-window] 游戏抢到前台，重新置顶`。

## 二十九、快捷键"没用"的真机定性：三种不同的"没反应"，只有一种是我们的

用户报"快捷键好像还是没用 / 点确认之后还显示未设置"。用带 trace 的实例（`RIFTFLOW_HOTKEY_TRACE=1 RIFTFLOW_IPC_TRACE=1` + 9223 CDP，profile 用用户真实档）把三种情况分开量了一遍：

1. **F7 / F8 根本没进 Windows。** 整份 hook 日志里 F 区（vk 112–123）只出现过 `vk=120`（我注入的 F9），用户实体按的 F7、F8 一条回调都没有；同期字母/数字/空格/Enter/Ctrl 全都有。这台机器的功能键区在不按 Fn 时被键盘固件吃掉，应用侧无从记录。**观测手段**：`grep -oE "hook callback vk=(11[2-9]|12[0-3]) "`。
2. **Enter 记上了但显示"此快捷键不可用"。** 原版 `DISABLED_KEYS = [VK_CODE_F22, 13]`（`keyboard-shortcuts/context.ts`），我们逐字移植在 `hotkeys.rs:39`，所以 Enter 是保留键、"确认"按设计禁用。用户把这读成"绑不上"。
3. **组合键是好的。** 注入 `Ctrl(162)+Alt(164)+K(75)` 和弦之后：`send event shortcut id="LeftControl+LeftAlt+K" keyCodes=[162,164,75]` → 卡窗正文 `快捷键设定 LeftControl + LeftAlt + K` → 点确认 → `Register shortcut LeftControl+LeftAlt+K for target in-game-send-main/preset/rating/friendly (last-active)` → 那一行从"未设置"变成色块。测完已把 friendly/enemy/all 全部写回 null。

顺带纠正两条我自己的错判：

- **"注入到 RiftFlow 自己窗口的按键不过钩子"是注入假象，不是缺陷。** 前台是 RiftFlow 时 `SendInput`/`keybd_event` 的键能进页面（DOM 探针读到 `k'k`）但钩子 0 回调；把前台换成记事本/Qoder 再注入，回调正常。实体键不受影响。所以"注入键没回调"不能当证据用。
- **`Ctrl+Shift+J` 弹 DevTools 是 debug 构建的 WebView2 行为**，不是功能缺陷：wry 在建窗口时 `settings.SetAreDevToolsEnabled(attributes.devtools)`，而 `attributes.devtools` 在 debug 是 true、release 是 false（`wry-0.57.1/src/lib.rs:873/875`）。第一次按被 DevTools 抢走、第二次只记到 `J`、第三次记全三个键，就是 DevTools 窗口抢焦点把修饰键的 down 错开了一拍。

**这条顺带挖出一个 release 编译阻塞**（真缺陷，已修）：tauri 的 `open_devtools/close_devtools/is_devtools_open` 都带 `#[cfg(any(debug_assertions, feature = "devtools"))]`，而 `windows.rs:398` 的 `toggleDevtools` 无条件调它们，工作区 tauri 只开了 `["tray-icon","image-ico"]` → `cargo check --release` 报 6 个 `no method named open_devtools`。修法：`Cargo.toml` 加 `"devtools"`，并在窗口构建里加 `.devtools(true)`（否则 release 下 `AreDevToolsEnabled=false`，`OpenDevToolsWindow()` 静默失败，"开发者工具"按钮变成空按钮）。代价：正式包里 F12 / Ctrl+Shift+J 也能开 DevTools，原版是靠 `ShortcutSelector.preventFn` 在渲染层挡掉其中三个组合。

`hotkeys.rs` 的 `log()`/`trace()` 原来只走 `eprintln!`，**完全不进 `logger.rs` 的文件**，`target/debug/logs/LA_*.log` 是 0 字节——这次想拿按键时间戳判断"修饰键 down 丢失是不是钩子超时"时直接扑空。已改：`log()` 委托 `crate::logger::log(NAMESPACE, Level::Info, …)`，真机确认文件里现在有 `[2026-10-27 14:10:50:638] [keyboard-shortcuts-main] [info] hook callback vk=75 down` 和同一条 `send event shortcut id="LeftControl+LeftAlt+K"`。这条不是洁癖：提权实例不监听 9223，日志文件是唯一的观测口。

## 三十、真缺陷：钩子看不见发给本进程的按键，绑定框在 RiftFlow 前台时永远记不上键

用户实测结论（比我任何自测都准）："RiftFlow 在前台就失败，RiftFlow 不在最前面就能识别"。这条之前被我误判成"硬件不吃 F7/F8 + 用户按错窗口"，因为我的注入测试恰好只能在**别的进程**前台时成功。

三方向量在同一实例上量出来的事实（DOM 探针 + `RIFTFLOW_HOTKEY_TRACE` + shim `riftflow-event` 监听）：

- 用户实体按 J，页面收到 `keydown KeyJ:74` + `keyup`，**钩子 0 条 `vk=74` 回调**，同期他在聊天框打的 N/Y/Space/Enter 全部有回调 → 钩子是活的，只是**收不到目标窗口属于本进程的按键**。
- 把钩子改装到 Tauri 主线程（窗口所有线程，`RIFTFLOW_HOOK_ON_MAIN` 实验分支）：**同样收不到**（用户再测仍"没反应"，日志里 J 依旧 0 条），但记事本前台时能收到 → 该分支已删除，不是解。
- 原版 `input.cc` 的钩子结构与我们逐行同构（`nCode == HC_ACTION` 守卫、自建线程 + `PeekMessage` 建队列、`GetModuleHandle(nullptr)`、`dwThreadId=0`），所以这不是移植走样，是**这条机制在 WebView2 窗口上根本不成立**，必须换输入源。

解法：把渲染层的按键流接进同一条管线，钩子仍然优先。

- `shim.rs`：capture 阶段 `keydown`/`keyup` → `riftflow_key_event`，并用 `KeyboardEvent.location` 把 16/17/18 换成**带左右侧**的 160/161/162/163/164/165（钩子报的就是侧别码；不换的话和弦会被 `process_common_modifier_key` 当成"通用修饰键按下不处理"，只剩主键——这正是用户看到的"第二次按只绑到 J"）；`event.repeat` 丢弃；`blur` 时把还按着的键补发 key-up，避免 alt-tab 丢掉 key-up 后修饰键卡死。
- `main.rs`：`#[tauri::command] riftflow_key_event(key_code, is_down)` → `hotkeys::instance().process_external_key_event(...)`。
- `hotkeys.rs`：`process_key_event` 拆成 `process_key_event_from(event, reconcile)`；外部源 **跳过 `_reconcilePressedStateFromNative`**（那个补偿步骤按钩子的 `KEY_STATES` 清状态，而钩子从没记过这些键，一跑就把 DOM 的修饰键抹掉）。钩子 proc 里新增 `HOOK_SEEN_MS[512]`（`GetTickCount64`，一次原子写），外部事件若在 60 ms 内钩子已报过同一个 (vk, 方向) 就丢弃 → 钩子能看到时它是唯一源，注册过的快捷键不可能一次按键触发两遍。

真机验证（合成事件走完整链，不依赖 OS 注入）：`window.dispatchEvent(new KeyboardEvent('keydown',{keyCode:162}))` + 164 + 75 + 三个 keyup → 日志 `send event shortcut id="LeftControl+LeftAlt+K" keyCodes=[162, 164, 75] available=true`，卡窗正文变成 `Shortcut Recording / LeftControl + LeftAlt + K`。`cargo test -p riftflow-app` 92 passed。

**教训（写死在流程里）**：验证输入链路时，"注入成功"必须先证明**目标窗口真的收到了键**（页面 `document.hasFocus()` + DOM 事件计数），否则测的是另一个进程。这一轮我因为 `SetForegroundWindow` 之后 WebView 没有键盘焦点（`hasFocus=false`、DOM 0 事件）而把"钩子正常"当成了结论，方向反了整整两轮。

## 三十一、秒退在国服根本打不出去：`/login/dodge` 这台客户端不认，改用 `/lol-gameflow/v1/session/dodge`

现象（用户）：人机模式点「立即秒退」没反应、也不报错。

原版实现是 aux 窗口 `ChampSelectOperations.vue` 的 `dodgeLoop()`：5 个并发 worker 死循环 `await lc.api.login.dodge()`，`catch {}` 吞掉一切，直到 `lcs.champSelect.session` 变 null 才 break。也就是说**只要那个请求一直失败，UI 就是"点了没反应也没报错"**，与我们观察到的完全一致。

用 `crates/riftflow-app/examples/lcu_endpoint_probe.rs`（本轮新增，`cargo run -p riftflow-app --example lcu_endpoint_probe -- GET /path`，凭据从 `LeagueClientUx` 命令行现读，不走 `lastConnectedClient` 那行旧数据）对真机量了一遍：

| URI | 本机答案 | 含义 |
|---|---|---|
| `GET /lol-gameflow/v1/gameflow-phase` | 200 `"Lobby"` | 连接与鉴权正常 |
| `GET /lol-champ-select/v1/session` | 404 `RPC_ERROR "No active delegate"` | 路由存在，只是当前没进选人 |
| `POST /login/dodge` | 404 `RESOURCE_NOT_FOUND "Invalid URI format"` | **国服客户端根本没有这条路由** |
| `GET /lol-gameflow/v1/session/dodge` | 405 `WRONG_METHOD` | **这条路由存在且只收 POST** ← 秒退的真实入口 |
| `GET /health`、`/available-callbacks`、`/lcu-version` | 404 `Invalid function` | 与既往记录一致，别拿它们当健康探针 |

关键区分：`Invalid URI format` 是路由器的"我不认识这个 URI"，`No active delegate` 是真 handler 说"现在没这个会话"——只有前者才该改道。

修法（`protocol.rs`）：渲染层字节不动，替换放在代理里。新增 `LCU_URI_FALLBACKS` 表 + `unknown_uri()` + `uri_fallback()`，`proxy_lcu` 收到 `404 Invalid URI format` 且命中表时，用备用 URI 重发一次并 `logger::warn` 记一行「本机客户端不识别，改用 … 重试」。全局客户端不会命中（它的 `/login/dodge` 是 2xx），所以零回归。两条单测：`the_cn_dodge_path_is_substituted`、`only_the_router_s_own_unknown_uri_retries`（94 passed）。

同轮把代理的失败也变成无条件落盘：`[protocol:fail]` 原来要 `RIFTFLOW_IPC_TRACE=1` 且只 `eprintln!`，而提权实例既没有 CDP 也没有控制台，等于永远看不到；现在 ≥400 一律 `logger::log("riftflow-protocol-main", Warn, method uri -> status + 200 字响应体)`。

**没做也刻意没做的事**：没有替用户真发一次 `POST /lol-gameflow/v1/session/dodge`。当前是 Lobby 阶段，最坏情况会给账号记一次秒退惩罚，不可逆；正确验证方式是在真的选人阶段由用户点「立即秒退」，然后读日志里那行重试。

关于用户看到的「999s」：那是 aux 房间面板 `等待秒退计时器 {{seconds}} s`（`Lounge.vue` 的 `penaltyTime` ← `matchmaking-controller.ts:110` 读 `lowPriorityData.penaltyTime`），是客户端自己给的秒退惩罚排队时间，反复秒退后确实会有几百秒——不是我们编出来的数字，也不是这条 bug 的一部分。
