<img src="./src-tauri/installer/art/tomato.svg" alt="小番茄图标" width="56">

# 小番茄

小番茄是一款可以悬浮在桌面上的番茄钟。你可以自定义专注和休息时长，再把完成的专注轮次记入自己的奖励基金。

这是基于 Pomotroid 开发的分支。本 README 介绍小番茄的功能和 Windows x64 构建方式。[小番茄 Fork](https://github.com/codinglemom/pomotroid) · [上游 Pomotroid](https://github.com/Splode/pomotroid)

## 安装 Windows x64 版

目前可通过[源码构建](#从源码构建)获取安装包。预编译安装包的发布状态见[小番茄 Fork 的 Releases](https://github.com/codinglemom/pomotroid/releases)。

构建完成后双击 `src-tauri/target/release/bundle/nsis/` 下的 `小番茄_*_x64-setup.exe`，按向导完成安装，再从 Windows 开始菜单搜索「小番茄」打开。安装版会注册 Windows 桌面通知所需的应用身份。

## 快速上手

1. 点击主窗口的「开始」即可计时。初次使用默认每轮专注 25 分钟、短休息 5 分钟，每完成 4 轮安排一次 15 分钟的长休息。
2. 打开「设置 → 计时器」可以调整时长、长休息前的轮数，以及专注和休息是否自动开始。
3. 点击主窗口的「统计」，可查看今日的专注时长、本周的记录和年度热力图。

## 表盘与桌面悬浮

在「设置 → 外观 → 计时器样式」中可选 **圆环、翻页钟、辉光管、沙漏**四种表盘。

点击主窗口标题栏的「显示悬浮番茄钟」，当前计时会继续在可拖动、始终置顶的小窗中显示。圆环会在悬浮时显示为辉光管，辉光管和翻页钟保留各自的外观。沙漏用于主窗口。点击小窗的「还原主窗口」即可回到原来的表盘。

<img src="./docs/images/desktop-widget.png" alt="小番茄的辉光管悬浮窗，显示倒计时和进度条" width="440">

## 托盘、通知与提示音

- **系统托盘**。在「设置 → 系统 → 系统托盘」开启「显示在系统托盘」，再按需启用「最小化到托盘」或「关闭到托盘」。Windows 上左键点击小番茄图标可显示或隐藏主窗口，右键可以从托盘面板控制计时。
- **桌面通知**。在「设置 → 通知」开启「桌面通知」，点击「发送测试通知」即可到 Windows 通知中心确认显示情况。若看不到通知，可以在 Windows 中检查小番茄的通知权限和勿扰模式。
- **提示音**。在同一页可分别为专注、短休息和长休息选用经典、柔铃、木琴、水滴，也可选择自己的音频文件并试听。专注和休息时的滴答声可分别开关。

## 奖励基金

在「设置 → 奖励基金」保存每小时奖励金额。此后自然完成的专注轮次会按开始时的费率入账，每轮单独四舍五入到分。例如每小时设为 ¥10.00，完成 25 分钟专注可获得 ¥4.17。新费率从下一次开始的轮次生效，休息、跳过、重置和未完成的轮次均无奖励，既往专注也不会补发。

在「统计 → 奖励基金」可查看余额、每日收入和收支流水，并填写金额与用途来记录支出。每日收入按本地入账日期统计，支出从余额扣除。

## 自定义主题

可在「设置 → 外观」选择浅色、深色或跟随系统，并分别设置两种模式的主题。把自定义 JSON 文件放到应用数据目录的 `themes/` 文件夹即可加载，修改时无需重启。[自定义主题指南](./THEMES.md)

## WebSocket API

本地 WebSocket 服务默认为关闭。开启「设置 → 系统 → 集成 → WebSocket 服务器」后，可用 `ws://127.0.0.1:1314/ws` 连接计时器，适用于直播叠层或本地脚本。修改端口后用设置中的端口连接。

发送 `{"type":"getState"}` 可查询当前计时状态。服务端消息如下。

| `type`              | `payload`               | 触发时机        |
| ------------------- | ----------------------- | --------------- |
| `state`             | 计时器状态              | 响应 `getState` |
| `roundChange`       | 计时器状态              | 切换轮次        |
| `started`           | `{ "total_secs": … }`   | 轮次开始        |
| `paused`、`resumed` | `{ "elapsed_secs": … }` | 暂停或继续      |
| `reset`             | —                       | 重置计时器      |

计时器状态包括 `round_type`、`previous_round_type`、`elapsed_secs`、`total_secs`、`is_running`、`is_paused`、`work_round_number`、`work_rounds_total` 和 `session_work_count`。[协议实现](./src-tauri/src/websocket/mod.rs)

## 从源码构建

小番茄使用 Tauri 2、Rust 和 Svelte 5。在 Windows x64 上需要 Node.js 22.12 或更高版本、npm、Rust 稳定版的 x64 MSVC 工具链、Microsoft C++ Build Tools（选择「使用 C++ 的桌面开发」）和 Microsoft Edge WebView2 Runtime。[Tauri 的 Windows 环境说明](https://v2.tauri.app/start/prerequisites/#windows)

```powershell
git clone https://github.com/codinglemom/pomotroid.git
cd pomotroid
npm install
npm run tauri dev
```

在项目根目录运行下列命令生成 NSIS 安装包。

```powershell
npm run tauri build -- --bundles nsis
```

安装包位于 `src-tauri/target/release/bundle/nsis/`。开发时可用 `npm run check` 检查 Svelte 与 TypeScript，用 `cargo test --manifest-path src-tauri/Cargo.toml` 运行 Rust 测试。

## 参与贡献

小番茄的改动请向本 Fork 的 `main` 分支提交 Pull Request。项目结构、检查命令、主题和翻译的维护方式见[贡献指南](./CONTRIBUTING.md)。

## 许可证

项目采用 MIT 许可证。原始项目由 Christopher Murphy（Splode）创建。[MIT 许可证全文](./LICENSE)
