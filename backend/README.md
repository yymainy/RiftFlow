# RiftFlow 独立服务

Node.js 24，无 npm 依赖。提供与桌面程序现有协议兼容的公告、联系渠道、版本信息、四类配置读取。公告和发布信息通过管理员接口维护，写入数据目录并原子替换；配置首期从 `seed.json` 读取，管理 API 暂不修改配置。未接入授权收费、遥测统计或玩家数据上传。

本服务为新实现，不是原项目 API 服务端的拷贝。客户端已有的数据格式作为兼容协议；上游历史公告资料不等于现行 API 的服务端源码。

## 运行

设置 `PUBLIC_BASE_URL`（外部 HTTPS 地址，例如 `https://riftflow-api.yymainy.online`）、`ADMIN_TOKEN`（随机值，至少 32 字符）、`DATA_DIR`（持久目录），执行 `node server.mjs`。默认绑定 `127.0.0.1:8080`。容器内绑定 `0.0.0.0`，宿主机仅映射回环端口，经 Nginx 提供 HTTPS。

```sh
node --test server.test.mjs
docker build -t riftflow-backend:local .
```

| 功能 | 路径 |
| --- | --- |
| 健康检查 | GET `/healthz` |
| 服务发现 | GET `/bootstrap.json` |
| 最新公告 | GET `/notice/v1/latest?lang=zh-CN` |
| 联系渠道 | GET `/website/v1/contact-channels` |
| 版本信息 | GET `/releases/v1/latest?lang=zh-CN` |
| 配置 | GET `/config/v1/app/feature-gates`、`sgp/league-servers`、`sgp/supported-queues`、`auto-select/groups` |

支持 `zh-CN` 和 `en`；未发布的语言返回 404，不冒用另一语言。版本元数据不等于自动更新已启用，桌面自动替换安装仍需独立实现及验证。

## 发布公告

管理接口只允许内网／SSH 使用，不经公网 Nginx 暴露。通过 `Authorization: Bearer <ADMIN_TOKEN>` 调用 `PUT /admin/notice/zh-CN`，JSON 示例：

```json
{"severity":"medium","summary":"新版已发布","content":"## 更新内容\n修复公告显示与默认主题。"}
```

服务自动生成 `revision` 和 `updatedAt`。`low` 仅公告，`medium` 徽标提示，`high` 重要提醒；不重复使用 revision。`GET /admin/notice/zh-CN` 可回读。

另支持 `PUT /admin/contacts`（`channels` 数组）和 `PUT /admin/release/zh-CN`、`PUT /admin/release/en`。版本资料包含 `version`、`publishedAt`、`description`、`artifacts`；每个安装包必须有 HTTPS 下载地址、正整数大小、SHA256 校验值。EXE 由 Nginx 下载目录提供，不接受匿名文件上传。

备份持久数据目录及 `seed.json`。管理员密钥只保存于服务器权限受限的环境文件，绝不能放进 EXE。请求正文上限 1 MB。服务不记录 Authorization 内容。

桌面联调可设置 `RIFTFLOW_BOOTSTRAP_URL=https://riftflow-api.yymainy.online/bootstrap.json` 后启动 EXE。服务发现响应保留客户端当前要求的字段名；这不是额外依赖原服务。

## 当前部署

Ubuntu SSH 别名 `ubuntu`（从 YYM 连接），服务目录 `/opt/riftflow-backend`，容器 `riftflow-backend`，仅映射 `127.0.0.1:8091`。独立域名 `https://riftflow-api.yymainy.online`。Nginx 拦截公网 `/admin/`；配置 `/www/server/panel/vhost/nginx/riftflow-api.yymainy.online.conf`。证书由现有 acme.sh 账户签发，使用 install-cert 保存安装路径及重载命令；不在仓库中保存私钥。

服务器上发布：`sudo python3 /opt/riftflow-backend/publish.py notice/zh-CN /path/to/announcement.json`。管理密钥由该命令在服务器本地读取，不发给客户端。

`deploy.py` 负责构建容器与健康检查；`configure_domain.py` 负责当前独立域名的 HTTP 验证入口和 HTTPS 代理。首次签证后使用 `--activate`。备份文件站点配置后只移除本次添加的临时 API 路径。

## Filebrowser 副本与验证

Filebrowser 管理副本：`https://files.yymainy.online/downloads/RiftFlow/RiftFlow-1.5.1-20261029.exe`。`publish_release.py` 将文件放入 Filebrowser 现有的“公开下载 / RiftFlow”目录（宿主机 `/www/filebrowser/public/RiftFlow`，容器 `/srv/public/RiftFlow`），计算文件大小、SHA256 并向独立后端发布中英文版本元数据。不再在站点根目录创建下载目录，也不修改 Nginx。下载沿用服务器已配置的 `/downloads/` 到 Filebrowser 公开目录的映射；文件可在 Filebrowser 中增删和替换。当前桌面版自动安装更新仍禁用。

证书续期任务：`/etc/cron.d/riftflow-acme`，每日调用现有 acme.sh，证书更新后由其已保存的重载命令刷新 Nginx。

已验证：Node 发布／权限／持久化测试；Rust API 客户端 89 项测试及文档测试；服务地址 5 项测试；生产渲染构建；实际 EXE 读取独立 HTTPS 公告和联系人、语言切换；主题与公告缓存回归。

## GitHub 发布下载

源码仓库：`https://github.com/yymainy/RiftFlow`。今后用户下载使用 GitHub Releases，固定入口 `https://github.com/yymainy/RiftFlow/releases/latest/download/RiftFlow.exe`。Filebrowser 中的文件继续保留为可管理副本。

先向 GitHub Release `v<版本号>` 上传名为 `RiftFlow.exe` 的安装包，再执行 `sudo python3 /opt/riftflow-backend/publish_release.py /www/filebrowser/public/RiftFlow/RiftFlow-1.5.1-20261029.exe --version 1.5.1`。脚本下载并校验 GitHub 附件大小和 SHA256，通过后才更新后端下载地址；不会在 GitHub 附件尚未可用时发布失效链接。
