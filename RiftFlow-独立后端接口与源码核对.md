# 独立后端接口与源码核对

核对日期：2026-10-29。以当前 Rust/Tauri EXE 的启动代码和请求实现为依据。

本次已实现独立服务，源码在 `backend/`，部署于 Ubuntu 的 `/opt/riftflow-backend`，域名为 `https://riftflow-api.yymainy.online`。原项目 API 服务端源码没有在当前工作区找到；公开组织仓库中也未找到现行 API 后端，不能据此断言它一定不开源。

## 原始代码在哪里

- 上游作者为 [Hanxven](https://github.com/Hanxven)。工作区保留的客户端请求定义在 `frontend/src/shared/shards/riftflow-api/types.ts`，调用器在 `frontend/src/shared/http-api-axios-helper/riftflow/`。
- 当前 EXE 的 Rust 调用器：`riftflow-rs/crates/riftflow-api/src/client.rs`，运行与刷新策略在同目录的 `loops.rs`、`state.rs`，启动入口在 `riftflow-rs/crates/riftflow-app/src/riftflow_api_main.rs`。这是客户端代码，不是云端 API 服务。
- 核对时参考过上游历史公告与配置资料；它们属于内容资料，不是现行 API 服务端实现。
- 本次独立后端是按现有客户端数据格式新写的 `backend/server.mjs`；自有发布入口在 `backend/publish.py`。没有复制原作者的云端服务器实现。

## 哪些数据从独立后端获取

| 功能 | API | 当前实现 |
| --- | --- | --- |
| 软件公告 | `/notice/v1/latest?lang=zh-CN` 或 `en` | 已实现，支持 Markdown、级别、摘要、revision、发布时间 |
| 联系渠道 | `/website/v1/contact-channels` | 已实现，默认空列表，不保留原作者联系方式 |
| 功能开关 | `/config/v1/app/feature-gates` | 已实现读取，自有配置 |
| SGP 服区端点 | `/config/v1/sgp/league-servers` | 已实现读取，起始数据取自程序已有内置配置 |
| SGP 支持队列 | `/config/v1/sgp/supported-queues` | 已实现读取，同上 |
| 自动选人分组 | `/config/v1/auto-select/groups` | 已实现读取，同上 |
| 版本信息 | `/releases/v1/latest?lang=zh-CN` 或 `en` | 已实现并发布当前中英文版本资料；未发布时返回明确 404 |
| 服务发现 | `/bootstrap.json` | 已实现，保留当前客户端要求的响应字段 |
| 静态资源 | `/static/*` | 本期没有迁移原项目资源库；未知资源返回 404，不回退原服务 |

公告与联系人每小时刷新，远程配置每两小时刷新。配置仍有内置数据与本机缓存，服务不可用时不阻止本地功能使用。首期配置只读，更新需维护 `seed.json` 并重新部署；公告、联系渠道和版本信息支持管理员发布，无需重打 EXE。

## 不经过这个后端的功能

战绩、玩家资料、段位来自 LCU／Riot 或腾讯 SGP；皮肤资产来自本机 LCU；对手表现迹象在本机计算；个人档案保存在本机 SQLite。OP.GG 数据仍来自 OP.GG。游戏版本简报仍读取腾讯／Riot 官方公告及 Data Dragon，不是软件自身的公告服务。

首次使用声明与开源许可证随程序打包，没有远程授权许可。收费授权、激活码、账号系统、云端战绩同步未实现。自动更新安装当前仍禁用，发布版本信息不等于已启用自动替换 EXE。

当前 Rust 启动流程未调用旧版本统计上报接口，本次服务也不实现玩家数据上传或版本统计写入。

## 维护

管理员 API 不通过公网 Nginx 暴露。用 SSH 登录后执行 `sudo python3 /opt/riftflow-backend/publish.py notice/zh-CN /path/to/announcement.json`。管理员密钥留在服务器 `/opt/riftflow-backend/service.env`，不会放进桌面 EXE。

部署和发布说明见 `backend/README.md`。独立 HTTPS 虚拟主机不再依赖文件站点的临时 `/riftflow/` 路径。

当前 EXE 下载：`https://files.yymainy.online/downloads/RiftFlow/RiftFlow-1.5.1-20261029.exe`，大小 40363008 字节，SHA256 `67c58aec157d8301a48711a41d7d3e81cae724942b2eee892f740cb16f9b9189`。公告 API 与文件下载使用独立域名路径，文件管理入口保留。

下载文件已移至 Filebrowser 的“公开下载 / RiftFlow”。原站点根目录的 EXE 已移除。后续发布脚本只使用 Filebrowser 管理目录。YYM 桌面上的三份中转 Python 脚本已清理，服务器正式维护脚本保留在 `/opt/riftflow-backend`。
