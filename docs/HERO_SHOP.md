# 一键购买英雄

入口：工具集 → 购买英雄。读取本地已登录客户端，不支持跨区购买。

- 列出全部英雄、拥有状态、客户端当前周免状态、蓝色精萃价格与余额。
- 全选可购英雄后可一键排除周免；勾选周免时在页面与确认弹窗两处提醒。
- 只有用户在弹窗确认后才提交购买，每次仅提交一位，确认拥有状态更新后继续。停止或离开页面会停止后续项目，已提交的请求不能撤回。
- 原生端保存五分钟内存报价，绑定渲染窗口、客户端连接、账号与英雄状态。提交前重新核对价格、周免、拥有状态和余额。价格变化、账号变化、余额不足或结果不明均停止，购买 POST 不自动重试。
- 仅允许 `CHAMPION` + `IP`（蓝色精萃），数量固定为一；不支持点券、赠送、碎片升级或自动购买周免。

接口：`GET /lol-summoner/v1/current-summoner`，`GET /lol-champions/v1/inventories/{summonerId}/champions-minimal`，`GET /lol-store/v1/catalog?inventoryType=CHAMPION`，`GET /lol-inventory/v1/wallet/IP`，`POST /lol-purchase-widget/v2/purchaseItems`。LCU 为内部接口；字段缺失或不兼容时停止，不猜测价格、余额或周免名单。周免使用客户端 `freeToPlay`，拥有状态使用 `ownership.owned`，两者分别判断。

验证：在 riftflow-rs 运行 `cargo test -p riftflow-app --bin riftflow hero_shop`；按 PACKAGING.md 构建完成后，在仓库根运行 `node scripts/test-hero-shop.mjs`。后者需 OpenSSL、WebView2 和前端依赖，启动独立 APPDATA 的软件与本地 HTTPS/WSS 模拟客户端，验证真实页面、原生 IPC 与模拟交易，不连接真实账号或消耗真实货币。生成的测试证书与资料只在忽略的 tmp 目录，不进入发布包。

真实客户端如果以管理员运行，RiftFlow 也需相同权限才能读取连接信息。模拟客户端通过不等于已经验证各大区的真实商店实现；实机验收先只刷新清单与余额，实际消费由用户确认。
