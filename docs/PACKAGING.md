# Windows 桌面打包手册

本文件是当前 Rust/Tauri 桌面版的打包入口说明。最后核对：2026-10-31。根目录 AGENTS.md 会引导后续维护先阅读这里。

## 一条命令

在仓库根目录的 PowerShell 执行：

```powershell
# 只检查已有构建，不重新编译、不发布
.\scripts\package-desktop.ps1 -CheckOnly

# 正常构建并整理发布包
.\scripts\package-desktop.ps1

# 主动更新游戏图标，再构建；需要网络
.\scripts\package-desktop.ps1 -RefreshIcons
```

输出为 `dist/RiftFlow-<版本>-<时间>/` 及同名 ZIP，每次创建新目录，不覆盖旧包或正在运行的程序。脚本不会提交 Git、上传 Release、发布公告、部署后端或修改服务器。

当前主线是 **Rust/Tauri + Vue**。不要用保留的 Electron `npm run build:win` 代替这个流程。Rust 最终二进制为 `riftflow-rs/target/release/riftflow.exe`。

## 环境与依赖

- Windows x64，Node.js 24 或以上、Python 3、Rust stable/MSVC、Visual Studio C++ 构建工具、Windows SDK。
- 首次安装前端依赖使用仓库固定的 Yarn：`node .yarn/releases/yarn-4.9.1.cjs install --immutable`，工作目录为 frontend。已有 node_modules 但依赖或锁文件变化时，也要手动重跑这一条，不能以“目录存在”判断依赖已更新。
- Cargo 不在 PATH 时，脚本会寻找 `$env:USERPROFILE\.cargo\bin\cargo.exe`。无需改系统环境变量。
- 运行 EXE 的用户需要 WebView2 Runtime，不需要 Node/Rust/MSVC 开发工具。
- 本机 Rust 工具链可能没装 rustfmt；它不是打包依赖，不要因此改用另一套桌面构建。

## 固定顺序及原因

1. 校验三处版本一致：frontend/package.json、riftflow-rs/Cargo.toml 的 workspace.package、Tauri 配置。
2. 准备图标缓存：frontend/resources/game-data/manifest.json 与 Rust assets/game-icon-map.json 必须对应同一版本、同一映射。首次或主动更新时运行 `python scripts/prepare-game-icons.py`；缓存完整时可以离线使用。
3. 运行 Vue 类型检查。
4. 用 `frontend/scripts/build-desktop-renderer.mjs ../riftflow-rs/renderer-dist` 构建界面，工作目录为 frontend。
5. 校验全部入口、HTML/JS/CSS 依赖、图标及版本，计算资源指纹。
6. 再运行 `cargo build --locked --release -p riftflow-app --bin riftflow`，工作目录为 riftflow-rs。
7. 再次校验资源指纹，确认编译过程中未被其他构建改动，然后整理包和 SHA256。

**前端构建与 Rust 编译不能并行。** Vite 会清空输出目录；Tauri 编译时把该目录嵌入 EXE。两者重叠会产生缺资源或混入旧资源的程序。统一脚本会锁住其他同入口的打包任务；手工运行的 Vite/Cargo 不受这个锁控制，因此也不要同时启动。

Tauri 的 frontendDist 在 `riftflow-rs/crates/riftflow-app/tauri.conf.json`，当前指向 `../../renderer-dist`。不要再使用 renderer-build 后增量 Copy-Item 的旧流程，那会留下失效文件。

## 必须存在的资源

窗口入口由 Vite 配置读取，Rust 中引用的 `*-window.html` 也会交叉校验。当前有七个：

| 入口 | 用途 |
| --- | --- |
| main-window.html | 主窗口 |
| aux-window.html | Mini |
| opgg-window.html | OP.GG 窗口 |
| ongoing-game-window.html | 独立对局窗口 |
| opponent-scout-window.html | 对手速览 |
| ai-advice-window.html | 选人结束后的 AI 建议悬浮窗 |
| cd-timer-window.html | 冷却计时窗口 |

新增窗口时，要同时处理 Vite entry、原生创建/WindowSpec、实际 Vue 入口分支及必要的窗口控制；仅创建 HTML 文件不够。可选开发用 lab.html 不属于生产入口清单。

当前图标基线是 Data Dragon 16.21.1、1137 个唯一文件；数量会随后续版本改变，以 manifest 为准，不能只检查总数。校验脚本会核对英雄、装备、召唤师技能和符文四类，检查图像文件头，逐个比较构建目录中的文件内容。英雄源使用腾讯英雄图片，其余当前由 Data Dragon 准备；资源版权见第三方声明。

**先由 Vite 生成页面，再复制 game-data。** 不能先复制进输出目录再运行 Vite，否则 emptyOutDir 会删掉缓存；build-desktop-renderer.mjs 已按正确顺序完成这一点。

Rust 另外编入的资源包括 assets/champion-summary.json、assets/game-icon-map.json、assets/riftflow-builtin.json、assets/RIFTFLOW_ICON.ico 及 Tauri 图标。缺少这些会编译失败，不要临时替换成空 JSON 或空标记文件。

## 发布包内容与排除项

统一脚本仅复制：RiftFlow.exe、LICENSE、THIRD_PARTY_NOTICES.md、LICENSES/ 和用户运行说明，随后生成 build-report.json、SHA256SUMS.txt、ZIP。

- 界面和图标已编入 EXE，运行包不需要另带 renderer-dist、node_modules、Cargo target、源码或 Electron 插件。
- 不包含 ai-config.json。默认 AI 配置从服务器 HTTPS 接口经 X25519/HKDF-SHA256/AES-256-GCM 下发，只在 Rust 内存短期缓存；已删除旧文件及兼容逻辑。
- 自定义配置在用户数据目录用 DPAPI 保存，不放进发布包。不得打包 ai-provider.json、service.env、*.dpapi、数据库、用户档案、日志、测试截图、测试配置、PDB 或调试源码映射。
- 不把服务器默认地址、真实模型、密钥写进 README、源码常量、截图或测试输出。
- 配置下发的加密不改变模型上游地址；上游 HTTP/HTTPS 是另一条链路。具体见 riftflow-rs/AI配置说明.md。

build-report 记录版本、图标版本和数量、资源字节数与指纹、源码提交及是否有未提交改动。SHA256SUMS 覆盖包内文件，但不包含自己，也不等于数字签名。不要只用 EXE 大小判断资源齐全。

## 出包后的实际检查

静态校验不能替代运行验证。新包应从自己的目录运行，不覆盖或强行结束用户正在使用的旧进程。

1. 主窗口、Mini、对局和速览入口能打开，离线英雄/装备/技能/符文图标能显示。
2. 对局页两队各五人的合成数据可显示；战绩到达时、切走再返回时，无需鼠标滚动触发渲染。真实人数不足要显示实际状态，不捏造第五人。
3. Mini 和 AI 建议关闭只影响本次，不被同一局重复自动打开。
4. AI 从右上角星光图标打开弹窗，不占导航栏；hover 与旁边按钮一致。普通问答不默认附带战绩。
5. AI 配置留空使用默认；表单保存自定义、保留未修改的密钥、恢复默认正常。默认配置不回显给页面。测试用合成密钥，不调用真实模型消耗额度。
6. 自动建议不在选人尚未结束时触发；每局一次，临时重连不重复；切局取消旧请求并忽略迟到响应；关闭开关后不请求。
7. 加密配置拒绝认证标签篡改、过期响应和其他请求的重放。线上冒烟只获取配置即可验证加密接口，无需向上游模型发送真实问题。

自动测试按改动范围选择：frontend 的 ai-advice/advice.test.ts、ai-assistant/context.test.ts；Rust 的 ai_assistant、ai_configuration、对局计算/战绩/流水线测试；backend 的 `npm test`。只改本地打包文档不必重跑整套游戏功能测试。

隔离运行验证可以使用单独 APPDATA 和 `RIFTFLOW_AUTO_CONNECT=0`。只有调试测试才设置 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 的本地调试端口，不要写进发布配置。只清理自己创建的测试进程，不终止用户进程。tmp/ 下历史脚本被 Git 忽略，不是可依赖的正式构建入口或长期测试资产。

## 后端与发布边界

桌面包和 Node 服务分别构建。仅改前端时不部署服务器；仅运行此打包脚本不会改线上。

后端是 Node 24，Dockerfile 必须包含 server.mjs、ai-config.mjs、seed.json 和 package.json；漏 ai-config.mjs 会导致服务启动失败。服务端私密 AI 配置通过数据卷提供，不能 COPY 进镜像或 seed。服务端代码改动先跑 `npm test`，部署要保留原服务和配置备份，验证 healthz、公告/版本接口及加密配置接口；不要以 healthz 正常代替验证实际新接口。

若当次获准公开发布，再进行版本号、标签、Release、DOWNLOADS.md 和公告工作。固定下载资产名为 RiftFlow.exe；本地有 ZIP 不代表已上传。不要把历史临时 EXE 的文件名当作自动更新版本号。

## 自动更新检查

自动更新实现及发布元数据约定见 [AUTO_UPDATE.md](AUTO_UPDATE.md)。安装助手复制自当前 EXE，不另打包 updater.exe。出包后运行 `node scripts/test-desktop-update.mjs` 验证独立测试目录中的替换、备份与启动；发布更新元数据使用包内 EXE 的大小和 SHA256，不使用 ZIP 的散列。
