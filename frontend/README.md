# RiftFlow 界面

这是 RiftFlow 的 Vue 界面与保留的 Electron 实现。当前发布版本使用 Rust/Tauri 本地后端，项目介绍与构建步骤见 [根目录说明](../README.md)。

## 开发与测试

```sh
node .yarn/releases/yarn-4.9.1.cjs install --immutable
node scripts/build-desktop-renderer.mjs
node node_modules/vitest/vitest.mjs run --maxWorkers=1
```

## 来源与许可

桌面基础代码来自 [Hanxven](https://github.com/Hanxven) 的公开项目，保留 [原作者 MIT 许可证和版权声明](../LICENSES/MIT-original.txt)。新增内容的免费使用与禁止未经授权销售条款见 [RiftFlow 软件许可协议](LICENSE)，适用范围及第三方例外见 [第三方声明](../THIRD_PARTY_NOTICES.md)。项目不再整体标为 MIT 或开源软件。游戏及第三方资源的权利归各权利人所有。

## 参考项目

| Project                                                                                            | Description                                                                          |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| [Pengu Loader](https://github.com/PenguLoader/PenguLoader)                                         | The ultimate JavaScript plugin loader, build your unmatched LoL Client.              |
| [League of Legends LCU and Riot Client API Docs](https://github.com/KebsCS/lcu-and-riotclient-api) | League of Legends LCU and Riot Client API Docs                                       |
| [Community Dragon](https://www.communitydragon.org/documentation/assets)                           | Resource management and asset documentation reference.                               |
| [Seraphine](https://github.com/Zzaphkiel/Seraphine)                                                | Provided integration approaches and insights into combining multiple tools.          |
| [fix-lcu-window](https://github.com/LeagueTavern/fix-lcu-window)                                   | Resolved the issue with abnormal window size of the League of Legends client.        |
| [Joi](https://github.com/watchingfun/Joi)                                                          | A League of Legends assistant. tool                                                  |
| [vscode-league-respawn-timer](https://github.com/Coooookies/vscode-league-respawn-timer)           | An extension to display League of Legends player respawn time in Visual Studio Code. |
| [LeaguePrank](https://github.com/LeagueTavern/LeaguePrank)                                         | Provided inspiration for playful and humorous features.                              |

