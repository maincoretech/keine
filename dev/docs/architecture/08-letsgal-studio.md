# LetsGal Studio 1.9.1 原生同步

keine 把 LetsGal 当作一种开放的编辑器工程格式，而不是运行宿主。Studio 与 keine 是两个
独立进程；同步只通过工程目录中的开放 JSON 完成，不安装扩展、不注入 DOM、不修改 ASAR，
也不启动本机 HTTP/TCP 服务。

> **术语说明：** 下文的“调试位置”是 Studio 当前选中的 fragment 与剧情 block，不是鼠标
> 光标。Studio 在 `.studio/state.json` 中将 block 序号命名为 `cursorBlockIndex`，因此内部接口
> 仍保留 `cursor` 字样。工程配置中的 `cursor` 才是鼠标指针外观；keine 不实现个性化指针，
> 始终使用操作系统默认鼠标指针。

```text
LetsGal Studio 1.9
  ├─ project.json
  ├─ chapters/*.json
  ├─ extensions/avg.internal.default-shell/ui/dialogue-box.json
  ├─ characters.json / scenes.json / project.variables.json
  ├─ assets/.manifest.json + assets/**
  └─ .studio/state.json
              │ notify recursive watch + 200 ms debug-position fallback poll
              v
LetsGalProjectAdapter -> typed Program + config + initial variables + selected-block position
              │
              v
keine studio session -> deterministic replay -> native 1920x1080 preview
```

## 唯一启动方式

```bash
cd /Users/shiftz/dev/keine
cargo studio-sync '/absolute/path/to/LetsGal project'
```

`studio-sync` 与 `studio-dev` 都展开为 `keine studio <project>`。它们不会安装任何东西；换一台
机器只需 keine 源码/二进制和 LetsGal 工程目录。普通 `cargo dev`、发行二进制与
`build_app_with_loader` 不会进入 Studio 同步模式。

## 同步合同

| Studio 数据 | keine 结果 | 更新方式 |
|---|---|---|
| project、chapter、character、scene | 中性 config、Program、Action | 保存后完整临时编译，成功才替换 |
| asset manifest | hash/逻辑路径别名和资源类型 | 保存后重建 config |
| assets 文件 | Bevy asset handle | FS watcher 原位热重载 |
| project variables | slot/shared 默认值 | 每次确定性重放重新注入 |
| character attributes | `<character-id>.<attribute>` 默认值 | 每次确定性重放重新注入 |
| chapterFolders / chapterTreeOrder | 1.9.1 虚拟目录展开后的章节执行顺序 | project 保存后重编译 |
| dialogueBehavior | 打字间隔、字符淡入和 10 种文字出现效果 | 对话框样式保存后刷新 config |
| `.studio/state.json` | fragment UUID + 一基 source step | 选择 block 后立即重放 |

Studio 的 block index 是零基；loader 转成一基 `SourceSpan.line`。一个 block 可编译成多个 Action，
runtime 以所有 `line <= selected_step` 的 Action 为目标，因此不会把复合 block 截断一半。

## 生命周期

1. `studio` 打开工程并一次编译全部有效 fragment；
2. 从 `.studio/state.json` 读取当前 fragment/block；
3. 从 keine 项目入口确定性重放到目标，恢复其此前背景、角色、变量、音频和镜头状态；
4. watcher 常驻工程根目录；只改调试位置时不重编译，内容保存时先重编译再重新定位；
5. 每 200 ms 校验一次小型调试位置 JSON，去重后只在 fragment/block 真正改变时重放，
   用于兜底系统文件通知合并或丢失；
6. Studio 原子写 JSON 期间若读到临时不完整内容，runtime 最多跨 8 帧重试，不阻塞渲染线程；
7. 关闭 keine 窗口即结束同步，不依赖 Studio 心跳，也不会因 Studio 失焦而退出。

同步会话把 Studio 作为唯一调试控制面。keine 仍更新动画、视频和打字机，但忽略自身的剧情
推进/自动/快进输入，避免两个进程各走一步后状态分叉。选择另一个 Studio block 会从干净的
项目默认变量重新重放，不继承前一次预览产生的临时变量。
同步会话不写快速存档、profile、已读历史或图鉴；SAVE/LOAD/CONFIG 可打开检查布局，
但会改动持久数据的按钮在该会话中不执行。

## 完整性与边界

- 1.9.1 当前内置的 34 种 runtime block 必须全部编译为 typed core Action；未知内置类型报错；
- 1.9.1 的 `chapterTreeOrder` 为章节执行顺序主来源：根章节按 tree entry 排列，目录 entry 按
  对应 `chapterFolders[].chapterIds` 展开；缺少新字段的旧项目继续回退 `chapterOrder`；
- 默认壳 `dialogue-box.json` 的 `text_speed`、`char_fade_in_duration`、
  `text_reveal_effect` 与四个 reveal 参数进入 adapter-neutral config；字符动画由原生 Bevy UI
  执行，`blur` 在当前 2D UI 后端以同节奏的柔和透明度聚焦近似，不引入 Studio runtime；
- 新增 `stageAnimation` 编译为 adapter-neutral 共享舞台时间轴：camera、character、scene layer
  共用真实时间时钟，支持关键帧、循环、倍率、等待，以及 camera/scene/particle/shake/audio
  事件；
- 1.9.0 的 `waitForInput` 降级为 core 的显式玩家确认等待，不借用定时器，也不在低帧率下自动
  越过；
- 角色全局/表情级 `heightRatio` 进入通用 `SpriteLayout::ViewportHeight`；皮肤表和角色属性
  降级为通用变量驱动的图片选择，不把 Studio 角色对象带进渲染器；
- 跨章节 `callFragment` 继续使用 core scene call stack；adapter 仅负责解析稳定 fragment id；
- 1.8.0 起新增的相机时间轴属性全部进入 core `PostProcessEffect` 并由 GPU 材质直接采样，adapter
  不保留 Studio 私有运行对象；
- 第三方游戏扩展 block 默认只能保留为通用 host capability，不能伪装为已原生实现；
- 已明确建模的 `shiftz.backspace/backspace-to`（兼容旧
  `maincore.backspace-to/backspace-to`）是例外：adapter 只把 `source`/`keep` 校验并降级为
  adapter-neutral `RetractDialogue`，实际按用户打字速度缩放并限制在 6–12 字/秒逐字反向播放；
  独立点击等待、存档恢复和 Studio 确定性重放全部由 core/runtime 实现；keine 仍不加载或
  执行 Studio 扩展 bundle；
- adapter 只读源工程；不启动 watcher、窗口或进程，实际生命周期归 loader/runtime；
- core、渲染器和 UI 不导入 LetsGal model；卸下 adapter 后引擎仍独立运行；
- Studio 原版“运行”按钮与 Player 不受 keine 控制，两者不能同时作为同一调试会话的状态源；
- 本方案明确不支持 Studio 扩展、内嵌预览或反向操控 Studio UI。
- 个性化鼠标指针不属于剧情同步合同；项目的 `cursor` 外观配置被忽略并安全回退为系统指针。

Windows、macOS 与 Linux 使用同一个 `notify::RecommendedWatcher` 合同；差异只在系统文件通知
后端。逻辑资源路径统一为 `/`，Windows 路径分隔符不会进入 Program 或 Bevy asset key。
