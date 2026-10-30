# 当前桌面版的后端依赖

核对日期：2026-10-29。范围为 Rust/Tauri 打包 EXE 的启动、IPC 和实际渲染入口；源码中保留的 Electron 实现不等于当前 EXE 已运行该服务。本次已实现独立服务并切换默认 API 地址，详细接口与源码核对见《RiftFlow-独立后端接口与源码核对.md》。

已接入自建独立后端 `https://riftflow-api.yymainy.online`，部署于用户 Ubuntu 服务器。Rust/Tauri 仍是随 EXE 运行的本地后端。

| 功能 | 实际处理与数据来源 | 是否为自建独立后端 |
| --- | --- | --- |
| 公告、联系渠道 | 自建 API，默认 `https://riftflow-api.yymainy.online`；公告按语言启动读取并每小时刷新 | 是 |
| 功能开关、SGP 服区地址、支持队列、自动选人分组配置 | 自建 API 的 `/config/v1/*`；有内置配置及本地缓存，远程每两小时刷新，首期服务端配置只读 | 是；远程配置仍可影响本机功能和连接地址 |
| 静态资源代理 | 默认指向自建域名 `/static`；原项目资源库未迁移，未知资源返回 404 | 自有路由已切换；资源库未完成 |
| 战绩、玩家资料、段位查询 | 本地游戏客户端 LCU，及 Riot／腾讯 SGP 游戏服务 | 官方服务，不是 RiftFlow 自建后端 |
| 自动接受、选人、符文、客户端设置等操作 | 本地 Rust 逻辑调用本机 LCU／Riot Client 接口 | 本机，无独立云端 |
| OP.GG 梯队、出装等查询 | 渲染端直接使用 `https://lol-api-champion.op.gg` | 第三方服务 |
| 本地战绩档案、记忆卡、练习、设置快照 | 本机 `match-archive.db`；同步数据来自 LCU | 本机存储与计算 |
| 皮肤目录与拥有状态 | 从 LCU 读取并保存本机快照 | 本机存储，不是云端资产库 |
| 小号迹象、表现突变 | 本机使用对局页已加载的历史战绩计算 | 本机计算，不调用判定服务 |
| 个人版本简报 | 用户选择国服／国际服：国服读取腾讯官网使用的 `apps.game.qq.com/cmc/zmMcnTargetContentList` 公告列表及 `lol.qq.com` 原文；国际服读取 Riot 英文官方公告。装备名由 Riot Data Dragon 同 ID 中英文目录匹配 | 官方站点接口与 `ddragon.leagueoflegends.com`；两套来源独立缓存，无自建云端 |
| 首次使用声明与许可证 | 随程序打包的文字、本机确认设置与 LICENSE | 无远程许可服务器 |
| 更新安装 | 当前 `lifecycle_supported()` 返回 false；自建版本元数据和 HTTPS 下载地址已配置，自动替换安装尚未启用 | 当前禁用，不能算已完成的独立更新服务 |
| 激活码、付费授权、账号授权 | 尚无相关系统 | 未实现 |

API 地址可受显式设置的 RIFTFLOW_BOOTSTRAP_URL 及其配置缓存机制影响。默认不启用旧 npm 发现；打包程序已使用新配置目录验证自建域名公告。

源码中保留版本使用统计的 HTTP 方法及原 Electron 调用者；当前 Rust 应用没有调用 `post_statistics_record` 的启动任务。不能因为存在接口字符串就断言当前 EXE 正在上报，也不能据此扩展成“全软件绝对没有其他联网”的结论。

英雄图标优先使用打包资源／本地客户端；腾讯 GTIMG 接口还保留在原 Electron 补充资源模块，Storybook 的 Community Dragon 地址属于开发预览，不应算成当前桌面版的自建服务。

日志已改为 `%APPDATA%/riftflow/logs`；APPDATA 不可用时落到系统临时目录下的 `riftflow/logs`，不再回退到工作目录。软件内“打开日志目录”按实际日志文件路径定位。
