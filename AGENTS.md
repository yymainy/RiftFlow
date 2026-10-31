# 项目协作记录

- Windows 桌面发布主线是 Rust/Tauri + Vue，保留的 Electron 构建不是当前发布入口。
- 打包前先读 [docs/PACKAGING.md](docs/PACKAGING.md)。统一入口为 `scripts/package-desktop.ps1`；只校验用 `-CheckOnly`。
- 前端构建、资源校验、Rust 编译必须顺序执行。不得在 Vite 清理/重建 renderer-dist 时并行编译 Tauri，也不要覆盖正在运行的 EXE。
- 默认 AI 配置来自服务器加密接口，客户端不需要旧 ai-config.json。密钥、DPAPI 文件、用户数据、服务端私密配置不进入发布包、文档或日志。
- 本地打包不等于授权发布、上传、部署或修改线上服务；这些操作按当次用户指令执行。
