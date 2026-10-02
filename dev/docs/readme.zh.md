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
