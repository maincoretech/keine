# tday

Kēne 的可编辑、可运行示例工程。在仓库根目录打开：

```sh
cargo editor projects/tday
```

使用已编译的开发版：

```sh
./target/debug/editor ./projects/tday
```

`config.yaml` 指定入口 `start`；`scripts/` 保留 13 个章节，各章末尾通过 `goto(scene_id)` 接续，最后一章通过 `story.end()` 结束。资源 ID 与路径登记在 `assets.yaml`，角色和对象分别登记在 `characters.yaml`、`objects.yaml`。

仓库包含剧本、配置和 WebP/Opus 成品，克隆后无需重新迁移或转换。PNG/WAV 原稿保留在本地并忽略，不影响现有成品编辑与播放；生成缓存、存档、密钥、打包输出和其他本地工程不纳入示例。

原 LetsGal 工程迁移时缺少的 3 个音效已跳过，位置保留在 `scripts/start.shou` 的注释中；不要用重新迁移覆盖当前剧本。开水阀为单次音效，循环声音统一使用 `se(asset, id: ..., loop: true, ...)`。

本工程的剧本、背景、立绘、音乐和音效均由 **shiftz** 原创制作，版权归 shiftz，与引擎 DL1 分开；见 [版权与来源说明](LICENSE)。
