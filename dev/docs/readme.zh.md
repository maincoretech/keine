# Kēne

原生视觉小说引擎与编辑器，Rust 2024 / Bevy 0.19.1 / GPUI。
原生项目使用 Eiyashou `.shou`；支持 LetsGal Studio 工程与冻结的 WebGAL 兼容输入。
正式发行只使用 Hakutaku v1、WebP 图片与 Ogg Opus 音频。

## 开始

```sh
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
cargo migrate /path/to/letsgal/tday projects/tday
```

`projects/tday` 是可编辑、可运行的原生示例，剧本和 WebP/Opus 成品已入库；PNG/WAV 原稿及其他本地工程忽略。
源 LetsGal 工程独立保留；目标已存在时不要重复迁移。自动回归使用 `tests/fixtures`。
迁移先验证新工程再发布目标目录，源工程保持只读。

## 脚本性能

测试环境：**Apple M5 Pro（15 个逻辑 CPU）、24 GiB 内存、macOS 27.0.1**。
2026-10-11，0.14.3 工作区、Cargo 优化基准；单线程、输入已在内存中、10 个样本。

| 负载 | 测量结果（约） |
| --- | ---: |
| EYS 十万条文本解析 | 115 ms / 87 万 action/s |
| EYS 100001 条表达式与文本混合解析 | 132 ms / 76 万 action/s |
| tday 解析语料：14 scene / 2665 action | 2.85 ms |
| Core：赋值、条件判断、文本插值对白 | 22.5 万轮/s |
| Core：非阻塞 Comment 执行 | 1.08 亿 action/s |

执行计时排除 Program 安装；混合负载每轮含三条作者 action，立即模拟推进对白。
这些是 CPU 微基准，不含磁盘、媒体、场景/UI 更新及渲染；桌面负载未隔离。
复现方法见[测试指南](testing.md)。

| 引擎 / DSL | 脚本路径 |
| --- | --- |
| Kēne / Eiyashou | 源码转换为 typed IR；发行包加载编译后的 Program，由 Rust Core 执行 |
| [Ren'Py](https://www.renpy.org/doc/html/language_basics.html) | `.rpy` 编译为 `.rpyc`，脚本表达式使用 Python |
| [WebGAL](https://docs.openwebgal.com/en/tech/) | `.txt` 解析为可执行场景对象，舞台使用 Pixi |

Ren'Py / WebGAL 尚未做同机同负载测试；上表比较实现路径，不作为跨引擎速度排名。

## 引擎快捷键

Engine 和 Editor 的独立 Preview 使用相同键位；键盘焦点须在 Engine 窗口。

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

快进遵守设置里的“已读/全部”，默认只快进已读对白；不会代替选择分支或提交输入。
输入框、确认框和加载期间不触发剧情快捷键；失焦停止 Ctrl 自动播放。
Ctrl 参与组合键后，须松开 Ctrl 再按下才会自动播放；Alt/Cmd/Shift 组合不触发上表剧情键。
取消 A/Ctrl+A 快捷键；画面上的自动播放按钮仍可开关自动播放。右键开关仅在剧情舞台生效。
F11 只改变当前窗口，设置页的全屏选项仍可保存为偏好。

设置页“数据管理”提供“还原设置”和“清除存档”，均需确认。前者恢复默认设置，
后者清除全部手动/快速存档及其预览；这两项不清除已读记录、鉴赏解锁或全局变量。
界面与确认文案同步维护简体中文、繁体中文、日语和英语。

正常退出后记住窗口大小、位置和最大化状态，保存在项目持久化目录的
`saves/window.bin`，不随读档回滚。首次启动会按当前屏幕缩小并居中，
为标题栏和桌面面板留出空间；换屏或 DPI 改变时重新适配，屏幕移除时回到主屏。
全屏和最小化不覆盖正常窗口尺寸；Wayland 的窗口位置由合成器决定。
benchmark 指定的窗口大小不受这些偏好影响。Preview 仍使用隔离的临时数据目录。

```text
文档
├── architecture.md    职责、资源、格式与安全边界
├── language.md        当前 Eiyashou v2.0 语法
├── editor.md          Text / Blocks / Inspector / Preview
├── testing.md         开发检查、基准与验收缺口
├── release.md         打包、macOS 与 Project CI
└── compatibility.md   LetsGal 与 WebGAL 的支持边界
```

当前版本 0.15.2。macOS 是当前验收重点；Windows/Linux 的运行态验收暂缓。
编译和自动测试通过不等于 IME、DPI、音视频和运行态验收通过。

## 许可证

Kēne 原创代码和文档采用 [Defold License 1.0](../../LICENSE)。允许制作和销售商业游戏，
禁止将引擎或 Editor 本身作为游戏引擎产品商业化；项目属于源码公开软件。
第三方依赖、字体、图标、音效、美术和游戏内容保留各自许可，见 [NOTICE](../../NOTICE)。
除另有书面约定，提交给项目的贡献采用同一许可证；不要求转让版权。
