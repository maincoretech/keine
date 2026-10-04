# Kēne

原生视觉小说引擎与编辑器，Rust 2024 / Bevy 0.19 / GPUI。
原生项目使用 Eiyashou `.shou`；支持 LetsGal Studio 工程与冻结的 WebGAL 兼容输入。
正式发行只使用 Hakutaku v1、WebP 图片与 Ogg Opus 音频。

## 开始

```sh
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
cargo migrate /path/to/letsgal/tday projects/tday
```

`projects/tday` 是唯一的本机原生 demo，目录与素材均忽略、不提交。
源 LetsGal 工程独立保留；目标已存在时不要重复迁移。自动回归使用 `tests/fixtures`。
迁移先验证新工程再发布目标目录，源工程保持只读。

## 引擎快捷键

Engine 和 Editor 的独立 Preview 使用相同键位；键盘焦点须在 Engine 窗口。

| 键位 | 操作 |
|---|---|
| Space / Enter | 显示完整对白，再次按下进入下一句 |
| 按住左/右 Ctrl | 持续自动播放，松开停止；沿用正常打字速度和自动播放间隔 |
| K / Ctrl+K | 开关持续快进 |
| B / Ctrl+B | 打开/关闭历史对白 |
| R / Ctrl+R | 重播当前语音 |
| 右键 / H / Ctrl+H | 隐藏/显示文本框 |
| F5 / Ctrl+Q | 快速存档（确认后执行） |
| F9 / Ctrl+L | 快速读档（确认后执行） |
| Ctrl+S / Ctrl+O | 打开存档/读档页面 |
| Esc / Ctrl+, | 剧情中打开设置；Esc 关闭当前菜单、历史或确认框 |
| Ctrl+T | 返回标题（确认后执行） |
| F11 | 切换当前窗口全屏；部分 macOS 键盘需 Fn+F11 |
| ↑ / ↓、1–9、Enter | 选择分支；数字直接选择对应项 |

快进遵守设置里的“已读/全部”，默认只快进已读对白；不会代替选择分支或提交输入。
输入框、确认框和加载期间不触发剧情快捷键；失焦停止 Ctrl 自动播放。
Ctrl 参与组合键后，须松开 Ctrl 再按下才会自动播放；Alt/Cmd/Shift 组合不触发上表剧情键。
取消 A/Ctrl+A 快捷键；画面上的自动播放按钮仍可开关自动播放。右键开关仅在剧情舞台生效。
F11 只改变当前窗口，设置页的全屏选项仍可保存为偏好。

```text
文档
├── architecture.md    职责、资源、格式与安全边界
├── language.md        当前 Eiyashou v2.0 语法
├── editor.md          Text / Blocks / Inspector / Preview
├── testing.md         开发检查、基准与验收缺口
├── release.md         打包、macOS 与 Project CI
└── compatibility.md   LetsGal 与 WebGAL 的支持边界
```

当前版本 0.12.0。macOS 是当前验收重点；Windows/Linux 的运行态验收暂缓。
编译和自动测试通过不等于 IME、DPI、音视频和运行态验收通过。
