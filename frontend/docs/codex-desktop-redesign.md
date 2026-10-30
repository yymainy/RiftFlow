# Codex 桌面风格改版交付说明

## 实施范围

本次修改真实桌面程序的 Vue 渲染层，沿用原有组件、数据源、状态和操作逻辑，不使用演示数据替换业务页面。布局可调整，原有内容与功能保留。

覆盖五个窗口：主窗口、辅助窗口、OP.GG 窗口、进行中对局窗口、冷却计时窗口，以及相关弹窗。主窗口包含战绩与对局页面、4 个自动化子页、8 个工具箱子页和设置中的 8 个页签。工具箱的战利品页仍遵循原有开发环境可见条件。

## 视觉规则

`dark` 与 `graphite` 使用统一的深灰工作台配色：主背景约 `#202020`、面板约 `#252525`、细边线约 `#343434`，配合紧凑间距、6–8px 圆角、清晰的主次文字和低饱和绿色强调。页面通过现有主题变量适配，其余主题选项保留。

## 构建接入

在 `RiftFlow` 中运行 `node scripts/build-desktop-renderer.mjs`，生成 `../riftflow-rs/renderer-build` 中的五个原版渲染入口。确认构建成功后，将生成内容复制到 `riftflow-rs/renderer-dist`，再于 `riftflow-rs` 执行 `cargo build`，由 Rust/Tauri 桌面宿主加载更新后的渲染资源。

此路径只构建渲染层，不依赖 Electron 主进程、preload 或原生插件二进制。当前工作目录没有 Git 元数据，因此不能提供 Git 提交或基于 Git 的变更统计。

## 验证与待验

2026-10-27 已验证：

- 五窗口生产渲染构建通过（最终构建约 16 秒）；只有原有大包体积提示。
- Rust/Tauri 开发构建通过；原有未使用符号等编译警告保留。
- 各 agent 对负责范围内共 188 个 Vue 组件完成模板编译检查；主窗口也已通过生产构建及实际启动。
- 新增 Codex 主题通过 TypeScript 独立严格类型检查。
- 独立配置的真实桌面实例：14 个主窗口路由、8 个设置页签、5 个窗口加载，0 JavaScript 异常；1000×700 无整页横向溢出。
- 明暗切换背景色正确。侧栏已使用原生按钮并提供焦点样式；隐藏窗口的合成键盘输入未触发导航，因此不声称键盘运行验证通过。
- 全量 vue-tsc 曾因本机可用内存不足（约 200 MB）而持续换页，已中止，不能视为全量类型检查通过。

详细运行记录：`riftflow-rs/tmp/codex-ui-smoke.json`。实际桌面截图：`ui-preview/desktop-automation.png`、`desktop-toolkit.png`、`desktop-settings.png`。测试实例已退出，未关闭用户的原实例。

可运行开发构建：`riftflow-rs/RiftFlow-Codex.exe`。运行前退出旧版，避免共用 WebView2 配置目录。原渲染资源备份在 `riftflow-rs/tmp/codex-original-renderer`。本说明不代表全部业务功能已完成实战验证。

仍需在相应真实游戏状态下手动检查：

- 客户端断开、连接、登录后，页面状态与操作权限是否正确。
- 实际战绩加载、玩家切换、搜索筛选、翻页和详情弹窗。
- 组队、匹配确认、英雄选择及游戏结束时的自动化操作与配置持久化。
- 工具箱操作、奖励领取、好友工具和局内发送，需要对应账号数据、权限及游戏阶段。
- 进行中对局、辅助窗口、OP.GG 与冷却计时窗口，需要实际对局、网络数据及快捷键触发。
- 不同窗口尺寸、系统缩放比例下的滚动、长文本、弹窗和主题切换。

## 第二轮：按设计稿重排真实界面

本轮在原版业务组件基础上更换页面结构：

- 全局侧栏首次进入新版时展开；60px 顶栏加入面包屑和常驻搜索入口，仍使用原搜索与隐私提醒逻辑。
- 未连接首屏取消居中大 Logo，改为页面标题、真实连接状态、客户端与启动入口双面板。
- 玩家页取消固定双档宽度，改为流式 max1240 布局、57px 头像、紧凑段位信息条、独立筛选分页工具栏、244px 左分析栏和右战绩列表。窄窗分析可折叠，原内容保留。
- 战绩卡重排为结果元信息、英雄/技能/符文/数据、装备标签三条扫描线；双方阵容独立分隔。
- 自动操作与工具集改为页标题、子功能导航、右侧内容区；窄窗子导航转为横向可滚动。
- 设置改为左侧8项导航和右侧内容，保留 show/tabName、原8业务组件、存储二级页和相关弹窗。

五窗口渲染构建、Rust构建通过。新版独立实例再次验证14路由、8设置分区、5窗口加载及1000×700无整页横向溢出，0 JavaScript异常。当前未连接客户端，真实战绩与实战状态仍需验证。全量类型检查限制同上，不声称通过。

发现用户原先仍运行 `target/debug/riftflow-app.exe` 旧二进制，本轮已备份至 `riftflow-rs/tmp/before-layout-v2/riftflow-app.exe`，将新版覆盖到原路径并重启，文件哈希与验证构建一致。也同步更新 `riftflow-rs/RiftFlow-Codex.exe`。已从原路径运行实例确认：dark主题、展开侧栏、新顶栏和新首屏实际加载。实装截图：`ui-preview/desktop-installed-v2.png`。

## RiftFlow data workspace (2026-10-27)

Primary navigation now spans the top of the main window. Player statistics occupy a horizontal metric strip, followed by aligned match rows. Full match details open in a right drawer; the existing match context continues to handle detail loading, replay, player navigation and simulation actions. Supplemental player modules remain available through the summary toggle below the toolbar. Shared automation, toolkit and settings screens use the same top-level navigation.

The match row layout is opt-in (`tableLayout`); embedded match previews retain their existing presentation. Internal IPC, database fields and game data are unchanged.

### Settings discoverability

Main navigation uses text/icon color feedback on hover and one underline for selection. Window settings have a named top navigation entry, and the general settings button retains its visible text. The settings dialog includes shortcuts to window management and automation, plus localized search across existing setting labels and automation sections. Search results retain their section/group context and navigate to the original control, briefly highlighting it. Searching the user-facing window label also matches the upstream small-window terminology. Search does not modify setting values.
