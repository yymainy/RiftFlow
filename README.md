# RiftFlow

RiftFlow 是一款 Windows 英雄联盟桌面工具，提供对局分析、客户端自动操作、个人战绩档案、皮肤资产管理和版本简报。

当前桌面版使用 **Rust / Tauri 2 + Vue 3 / TypeScript**。公告、联系渠道、远程配置和版本信息由独立 Node.js 服务提供。

## 主要功能

| 功能           | 说明                                                                                       |
| -------------- | ------------------------------------------------------------------------------------------ |
| 对局信息       | 查看玩家战绩、对局数据和英雄资料，提供辅助窗口与 OP.GG 数据视图。                          |
| 对手历史分析   | 根据可获取的历史对局提供账号表现线索；结果属于启发式分析，不能证明对方是代练、陪玩或小号。 |
| 客户端自动操作 | 提供选人、回复、游戏流程等可配置操作，具体能力取决于客户端状态及接口可用性。               |
| 个人战绩档案   | 同步可获取的战绩到本地，支持搜索、筛选和查看同步状态。                                     |
| 皮肤资产       | 按英雄分类查看已拥有与未拥有的皮肤，支持筛选和 CSV 导出；永久拥有与限时体验分别处理。      |
| 个人工具       | 对线记忆卡、练习目标、大乱斗装备方案批量配置和游戏设置快照。                               |
| 个人版本简报   | 可选择国服或国际服公告来源，按关注英雄及装备筛选更新内容。                                 |
| AI 助手        | 使用自己配置的模型进行战绩总结、阵容建议、成长周报、单场复盘、英雄上手和笔记整理。       |
| 界面与设置     | 支持多种主题，默认跟随系统，提供托盘、快捷键和关闭行为设置。                               |

## 下载

最新发布版本为 **v1.6.3**，安装包通过 GitHub Releases 提供：

- 固定下载入口：<https://github.com/yymainy/RiftFlow/releases/latest/download/RiftFlow.exe>
- 全部版本与发布说明：<https://github.com/yymainy/RiftFlow/releases>
- 文件大小、SHA256 与历史版本：见 [DOWNLOADS.md](DOWNLOADS.md)

v1.0.0 是仓库公开后的首个发行版本；当前发布版本为 v1.6.3，请以 Release 页面的最新版本为准。1.6.0 及以上可在软件内检查更新；更早版本请退出旧版后手动运行新版。从 1.6.0–1.6.2 升级时，如果弹窗里的“重启完成更新”没有反应，请改用右上角更新图标中的“立即重启”，或手动运行新版；1.6.3 起已修复。

1. 下载 `RiftFlow.exe` 后直接运行，无需安装；是否可单独移动 EXE，以对应版本的发布说明为准。
2. 启动并登录英雄联盟客户端，等待软件显示已连接。战绩同步、皮肤资产同步等功能需要相应客户端接口可用。
3. 在设置中选择主题、快捷键及关闭行为，再按需启用自动操作。

未连接时不能同步账号资产；已有本地快照仍可查看。新账号显示 0 项已拥有皮肤可能是正常结果，不能仅凭数量判断同步失败。

## 系统要求

AI 默认配置由服务器通过 HTTPS、X25519 和 AES-256-GCM 加密下发，不再需要本地 JSON。点击右上角星光图标打开助手，可在设置中填写自己的 URL、密钥、模型，或恢复默认；自定义配置使用 Windows DPAPI 保护。设置中也可启停选人结束后的自动建议。普通问答默认不附带战绩，数据分析可展开查看资料。详细说明见 [AI 配置说明](riftflow-rs/AI配置说明.md)。

- Windows 10 或 Windows 11（64 位）。
- Microsoft Edge WebView2 Runtime；Windows 11 通常已自带，Windows 10 若缺失需先安装。
- 可登录的英雄联盟客户端；国服与国际服均可使用，但可用功能随服区和客户端版本不同。
- 可访问 GitHub Releases 与 RiftFlow 独立服务的网络。离线时只能查看已有的本地快照。

运行发布版不需要安装 Node.js、Rust 或 Visual Studio 构建工具，这些只用于从源码构建。

## 数据来源与限制

- 客户端连接使用本机 LCU 等接口。国服、国际服及不同版本的接口、数据范围和可用功能可能不同。
- 历史战绩受上游返回范围限制，“同步全部”表示同步当前能获取的记录，不保证覆盖账号全部历史。
- 版本简报分别使用腾讯国服与 Riot 国际服公告；国际服的发布时间、上线时间和数值不应直接视为国服结论。
- 皮肤快照对应同步时登录的账号；限时体验不计为永久拥有。客户端目录与实际可购买商品不一定一致。
- 独立服务提供公告、联系渠道、配置及版本信息，未接入玩家档案或皮肤资产上传。
- Windows x64 支持自动检查更新并提示，确认后下载并校验 EXE，选择重启或正常退出后完成替换。

本工具不是 Riot Games 或腾讯的官方产品。使用前请确认相关游戏规则及用户协议，软件不保证账号安全或所有环境下的兼容性。

## 常见问题

- **启动后一直显示未连接客户端？** 先启动并登录英雄联盟客户端。部分功能还要求进入英雄选择或对局，客户端未就绪时相关按钮可能不可用。
- **新账号显示 0 项已拥有皮肤？** 统计只计入永久拥有；租用、限时体验以及默认/经典模型不计入。客户端目录与商城在售列表也可能不同。
- **“同步全部”为什么没有覆盖账号全部历史？** 该操作只同步上游当前能返回的记录，不代表完整历史。
- **可以自动安装更新吗？** Windows x64 可以。默认先提示，下载完成后由用户选择重启；设置中可开启自动下载。详见 [自动更新](docs/AUTO_UPDATE.md)。
- **国服和国际服的结果为什么不同？** 两服的接口、公告来源和数据范围不同；国际服的发布时间与数值不应直接当作国服结论。

## 目录

- `riftflow-rs/`：当前桌面版 Rust/Tauri 后端。
- `frontend/`：Vue 界面及保留的 Electron 实现。
- `backend/`：软件公告、联系渠道、远程配置和版本信息的独立服务。
- 根目录的功能说明文档：`RiftFlow-皮肤资产说明.md`、`RiftFlow-账号迹象说明.md`、`RiftFlow-个人工具与同步修复说明.md`。

## 构建桌面版

需要 Windows、Node.js 24、Yarn 4、Rust 稳定版及 MSVC 工具链、Visual Studio C++ 构建工具和 WebView2 Runtime。

在仓库根目录打开 PowerShell：

```powershell
.\scripts\package-desktop.ps1 -CheckOnly  # 检查已有界面资源
.\scripts\package-desktop.ps1             # 构建并整理完整发布包
```

完整流程和常见遗漏记录在 [打包手册](docs/PACKAGING.md)。脚本按顺序执行前端构建、窗口与图标校验、Rust 编译，并在 `dist/` 生成附带许可、资源构建记录和 SHA256 的目录与 ZIP；不会发布或部署。首次没有构建产物时可直接运行正常构建；需要更新图标时加 `-RefreshIcons`。不要在 Vite 清空输出目录期间并行编译 Tauri。

历史 Electron 原生插件与旧更新器二进制不再随源码提供。当前 Rust/Tauri 发布入口不使用这些插件；更新助手由当前程序复制生成，不需要另带旧更新器二进制。

## 验证

在仓库根目录执行对应命令：

```powershell
# Rust 桌面程序编译检查
cd riftflow-rs
cargo check -p riftflow-app
cd ..

# 前端测试
cd frontend
node node_modules/vitest/vitest.mjs run --maxWorkers=1
cd ..

# 独立服务测试
node --test backend/server.test.mjs
```

`frontend/native/*.test.ts` 需要先用 `npm run build:native:win` 构建 Electron 原生插件（需要 MSVC）；未构建时这两个测试文件会失败。当前 Rust/Tauri 桌面版不依赖这些插件。

## 发布

桌面版版本号以 `frontend/package.json`、`riftflow-rs/Cargo.toml` 和 `riftflow-rs/crates/riftflow-app/tauri.conf.json` 为准，发布前应保持一致。发布流程：

1. 按上文构建 `riftflow.exe`，确认程序内显示的版本号与新版本一致。
2. 创建并推送 `v<版本号>` 标签。
3. 在对应 GitHub Release 中把可执行文件以 `RiftFlow.exe` 文件名上传；`https://github.com/yymainy/RiftFlow/releases/latest/download/RiftFlow.exe` 为固定下载入口。
4. 更新 [DOWNLOADS.md](DOWNLOADS.md) 中的版本、大小和 SHA256。

自动更新元数据需提供 Windows x64 EXE 的真实大小、SHA256 与不可变 HTTPS 地址，详见 [自动更新发布约定](docs/AUTO_UPDATE.md)。

## 独立后端

使用 Node.js 24，无 npm 运行依赖。配置、API 和部署方式见 [backend/README.md](backend/README.md)。管理员密钥仅保存在部署环境，不应写入客户端或提交到 Git。

源码不应包含客户端凭据、服务器管理密钥或本机日志；测试夹具中的玩家名、Riot ID、PUUID、召唤师 ID 和账号 ID 已替换为合成值。

## 问题反馈

可以在 [GitHub Issues](https://github.com/yymainy/RiftFlow/issues) 提交问题。反馈时请附软件版本、Windows 版本、国服或国际服、客户端连接状态、复现步骤及经过脱敏的错误提示。不要上传客户端令牌、管理员密钥或完整个人档案。

## 来源与许可

```text
Copyright (c) 2026 Hanxven
Copyright (c) 2026 yymainy
```

桌面基础代码来自 [Hanxven](https://github.com/Hanxven) 的公开项目，保留原作者的 [MIT 许可证与版权声明](LICENSES/MIT-original.txt)。维护者有权授权的新增内容采用 [RiftFlow 软件许可协议](LICENSE)：允许个人和商业场景免费使用，禁止未经书面授权倒卖、收费分发或打包销售；第三方内容的原有许可权利不受影响。该许可含销售限制，项目不再整体标为 MIT 或开源软件。详见 [第三方声明](THIRD_PARTY_NOTICES.md)。游戏及第三方资源的权利属于相应权利人。

发布桌面版时，应将 `LICENSE`、`THIRD_PARTY_NOTICES.md`、`LICENSES/` 及适用的依赖许可随软件一起提供；仅运行 Cargo 构建不会自动把这些文档复制到 EXE 所在目录。
