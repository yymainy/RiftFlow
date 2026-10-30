# 峡序 · RiftFlow 品牌接入

使用用户选定的首版图标：灰绿色的角形 R 与暖白色交错路线，深灰圆角背景。

## 已替换

- 中英产品名、侧栏、面包屑、启动页、关于、窗口标题、对局消息中的品牌前缀。
- RiftFlowLogo、RiftFlowLogoSanta、RiftFlowSpan 组件及引用。
- Tailwind 的 riftflow 配色标识，替换原界面品牌色标识。
- Windows 可执行文件产品名、应用标识、托盘名称和图标、favicon、Electron 打包名称与图标引用。
- 8 档 PNG 与多尺寸 ICO，源 SVG 保存在 src/renderer-shared/assets/brand。

## 兼容边界

英雄 Akali / 阿卡丽属于游戏数据，保留。原有 riftflow:// 协议、IPC/shard 命名空间、部分算法/API 数据键（如 riftflowScore）、工作区目录和 Rust 依赖 crate 名属于内部兼容接口，未全局搜索替换。配置目录 riftflow 与 RiftFlow.db 沿用，避免遗失已有设置和收藏。上游 GitHub、服务端地址、作者 Hanxven 与 MIT 归属保留，并在关于及原始声明中明确为上游来源。外部依赖的 updater 资源名保留。

## 验证

五窗口生产渲染构建与 Rust 桌面构建通过。现有应用标题状态测试通过；14 个主页面入口、8 个设置分区、5 窗口加载通过，0 JavaScript 异常。Windows 原生 LoadImage 接口确认 16/32/256px ICO 可用，exe 元数据 ProductName / FileDescription 均为 RiftFlow。全量类型检查与真实对局验证限制沿用前一轮说明，不声称已完成。

可重建图标：python scripts/generate-riftflow-icons.py（使用本机 Playwright + Edge）。桌面构建：cargo build --target-dir target/verify -p riftflow-app --bin riftflow。

运行报告：riftflow-rs/tmp/riftflow-ui-smoke.json。原图标备份：riftflow-rs/tmp/branding-before-riftflow.zip。

更新命令相关 Rust 测试：5 通过、0 失败。已安装并运行 riftflow-rs/RiftFlow.exe，原 target/debug/riftflow-app.exe 与 RiftFlow-Codex.exe 启动路径也已同步新版。实装进程验证 document.title=RiftFlow、侧栏=峡序 · RiftFlow、新图标 viewBox=0 0 128 128；截图 ui-preview/branding/riftflow/riftflow-installed.png。原程序备份 tmp/before-riftflow/riftflow-app.exe。
