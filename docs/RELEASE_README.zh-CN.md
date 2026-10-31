# RiftFlow 使用说明

解压整个发布包，运行 RiftFlow.exe。Windows 10/11 x64，需要 Microsoft Edge WebView2 Runtime。运行发布版无需安装 Node.js、Rust 或 Visual Studio。

界面、窗口入口和缓存游戏图标已编入 EXE，无需另带 renderer-dist。连接英雄联盟客户端后可查询对局；已有本地资料离线时可查看。独立于游戏的窗口在独占全屏游戏中的可见性受 Windows 显示模式限制。

点击右上角星光图标使用 AI。默认配置由服务器加密下发，不需要 ai-config.json。在助手设置中可输入自定义接口、密钥和模型，或恢复默认。自定义配置由 Windows DPAPI 保护，属于当前 Windows 用户，不适合作为共享配置文件。选人结束后的自动建议可关闭；AI 服务调用可能产生费用。

请保留随包的 LICENSE、THIRD_PARTY_NOTICES.md 和 LICENSES 目录。SHA256SUMS.txt 用于检查文件完整性，build-report.json 记录资源构建指纹；它们不是数字签名。

软件启动后自动检查新版本并提示，可在设置中手动检查或开启自动下载。下载并校验完成后，点击右上角更新图标选择“重启完成更新”，或正常退出后完成替换。更新会保留用户数据及旧 EXE 备份；下载失败可重试或取消。
