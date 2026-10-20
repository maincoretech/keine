![Kēne — 原生视觉小说引擎](../../src/assets/branding/keine-banner.png)

Kēne 是一款视觉小说引擎，附带原生桌面编辑器。使用 Eiyashou（`.shou`）编写剧本，
可以直接改文本，也可以用积木视图和属性面板编辑，再通过 Engine 预览游戏。
这几种编辑方式共用同一份剧本。

引擎和编辑器使用 Rust、Bevy 与 GPUI。游戏以 Hakutaku v2 打包，图片使用 WebP，
音频使用 Ogg Opus。支持导入 LetsGal Studio 工程；WebGAL 仅支持[兼容文档](compatibility.md)中列出的功能。

[English](../../README.md)

## 下载

- [Editor](https://github.com/maincoretech/keine/releases/tag/editor-latest)：附带配套的预览 Engine，`main` 的桌面 CI 通过后更新。
- [tday 试玩](https://github.com/maincoretech/keine/releases/tag/tday-latest)：可直接运行的示例游戏，通过 **Build tday** 手动发布新构建。

打包自己的游戏，请参阅[发布指南](release.md)。

## 构建与运行

先安装[开发环境](testing.md#开发环境)中列出的原生依赖。仓库固定使用 Rust 1.99.0，引擎使用 Bevy 0.19.1。

```sh
cargo build --locked -p keine --bin keine --features video-native,video-ffmpeg
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
```

使用 Editor 预览前，需要先编译 Engine。编辑器会在独立窗口中启动它，不会自动替你编译。
上述命令启用了 FFmpeg；macOS 优先使用系统原生视频后端。

`projects/tday/` 是已完成的示例项目，想尝试修改时请先复制一份。
`tests/fixtures/` 中放的是小型测试项目。转换已有工程的方式见[迁移命令](release.md#cli)；
编辑器中的兼容 JSON 为只读，转换会保留原工程。

## 脚本性能

测试于 2026 年 10 月 16 日，环境为 **Apple M5 Pro（15 个逻辑 CPU）、24 GiB 内存、macOS 27.0.1**。
使用的是加入解析器优化的 0.16.3，相关改动已在 0.16.4 发布；这些数据尚未在 0.16.7 上重测。

在 Criterion 测试中，优化后解析十万条文本的耗时从 **85.26 降至 75.09 ms**，
解析 tday 剧本中 2626 条指令的耗时从 **2.27 降至 2.13 ms**。

下表在同一台机器上运行各语言的等价脚本，均为单线程，输入已放入内存，每项取三轮测试的中位数。

| 引擎 / 脚本实现 | 解析一万条文本 | 解析一万条赋值 | 执行一万次加一 |
| --- | ---: | ---: | ---: |
| Kēne / EYS，优化后 0.16.3 | 5.98 ms | 4.40 ms | 0.65 ms |
| TyranoScript `c8dbfd4`，V8 / Node 24.22 | 2.50 ms | 7.01 ms | 0.77 ms |
| Suika3 26.07.13，C tag 路径 | 1.06 ms | 1.77 ms | 16.49 ms |
| Ren’Py 8.5.3，SDK Python 3.12 | 81.65 ms | 63.23 ms | 3.74 ms |

这里只计脚本解析与执行的耗时，不包含文件加载、画面、音频等游戏功能。
各解析器做的检查也不同，因此不能据此判断哪个引擎启动更快、游戏更流畅。
Suika3 的 NoctLang/JIT 未参与测试。完整条件、限制、原始数据和复现命令见
[测试记录](testing.md#0163-解析优化与四引擎脚本对照)。

## 引擎快捷键

游戏和编辑器的预览窗口使用同一套快捷键。使用时请先选中 Engine 窗口。

| 键位 | 操作 |
|---|---|
| Space / Enter | 显示完整对白，再次按下进入下一句 |
| 按住左/右 Ctrl | 持续自动播放，松开停止；沿用正常打字速度和自动播放间隔 |
| K / Ctrl+K | 开关持续快进 |
| B / Ctrl+B | 打开/关闭历史对白 |
| R / Ctrl+R | 重播当前语音 |
| 右键 | 舞台隐藏/显示文本框；其他页面同 Esc 返回，确认框取消；标题页询问退出 |
| H / Ctrl+H | 舞台隐藏/显示文本框 |
| F5 / Ctrl+Q | 快速存档（确认后执行） |
| F9 / Ctrl+L | 快速读档（确认后执行） |
| Ctrl+S / Ctrl+O | 打开存档/读档页面 |
| Esc / Ctrl+, | 剧情中打开设置；Esc 关闭当前菜单、历史或确认框 |
| Ctrl+T | 返回标题（确认后执行） |
| F11 | 切换当前窗口全屏；部分 macOS 键盘需 Fn+F11 |
| Delete | 存读档页删除鼠标指向的存档（确认后执行） |
| ↑ / ↓、1–9、Enter | 选择分支；数字直接选择对应项 |

快进默认只跳过已读对白，可在设置中改为全部对白；分支选择和文字输入仍需手动完成。
输入框、确认框和加载期间不会响应剧情快捷键。切到其他窗口会停止按住 Ctrl 的自动播放；
使用 Ctrl 组合键后，须松开再按下 Ctrl 才会开始自动播放。
Alt、Cmd、Shift 组合不会触发这些剧情快捷键，自动播放也可通过画面上的按钮切换。

设置中的“还原设置”和“清除存档”都需要确认。清除存档会删除手动存档、快速存档及其预览图，
但会保留已读记录、鉴赏解锁和全局变量；还原设置同样不会清除这些进度。
游戏界面支持简体中文、繁体中文、日语和英语。

正常退出后会记住窗口大小、位置和最大化状态；读档不会改变它们。
F11 只切换当前窗口的全屏状态，要保存全屏偏好请使用设置页。
Preview 使用独立的临时数据目录。

## 文档

- [架构](architecture.md)
- [Eiyashou v2.0](language.md)
- [Editor](editor.md)
- [开发与测试](testing.md)
- [发布](release.md)
- [Android](android.md)
- [工程兼容](compatibility.md)

当前版本 **0.16.7**。近期主要在 macOS 上进行人工测试，Windows 和 Linux 尚待新一轮运行测试。
CI 通过后，仍需在设备上检查输入、显示和音视频播放。

## 许可证

Kēne 的原创代码和文档采用 [Defold License 1.0](../../LICENSE)。
可以用它制作和销售游戏，但许可证限制将引擎或 Editor 本身作为游戏引擎产品商业化。

第三方库和素材各有自己的许可证，见 [NOTICE](../../NOTICE)。
tday 的剧本和素材归 shiftz 所有，见[示例许可](../../projects/tday/LICENSE)。
除另有约定，贡献采用与 Kēne 相同的许可证，不要求转让版权。
