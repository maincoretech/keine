# keine Bevy 架构设计

## 边界

工作区按职责分成三层：

```
WebGAL .txt / future language adapters
        │
        ▼
keine-loader   内容来源、格式识别、解析、notify 热重载
        │ Vec<Action>
        ▼
keine-core     可序列化 State、Action、step 状态机、配置与转场类型
        │ State
        ▼
keine          输入、ECS 同步、Bevy UI、wgpu 后处理与存档 IO
```

`core` 和 `loader` 不依赖 Bevy，可以独立测试。Bevy 主世界中的 `GameState`
直接拥有 `State`，不使用全局锁；系统顺序由 `GameSystemSet` 串联。

## 启动与项目加载

- `main.rs` 仅调用库入口 `run()`。
- `runtime/bootstrap.rs` 负责命令行项目路径、Bevy Plugin、三台相机和项目 bootstrap。
- `keine-loader` 先按配置挂载有序内容来源，再通过语言注册表稳定合并 scene。
- `runtime/asset_reader.rs` 将同一来源列表桥接成 Bevy 默认只读 overlay source。
- `ScriptWatcher` 同时拥有 notify backend 和 channel，不泄漏后台对象。
- 脚本创建、修改或删除后，`tick` 会合并事件并重新加载场景。

## 主世界系统顺序

```
Input  → tick, auto_hide_tick
Sync   → background::sync_bg, sprites::sync_sprites
Layout → resize::on_resize
Ui     → textbox/control_bar/dialog/loading systems

PostUpdate after UiSystems::Layout:
  update_blur_regions
  spawn_dialog
```

背景和立绘使用稳定 ECS 标识进行增量同步。`DesignViewport` 集中处理
1920×1080 设计空间、letterbox offset 和 Bevy world 坐标转换。

## 三相机与模糊合成

| Order | Layer | 组件 | 职责 |
|------:|------:|------|------|
| 0 | 0 | `SceneBlurCamera` | 背景、立绘、文本框背后区域模糊 |
| 1 | 1 | `UiBlurCamera` | Textbox、控制栏；Dialog 出现时对合成结果全屏模糊 |
| 2 | 2 | `DialogCamera` | 最终 Dialog，保持清晰 |

普通 UI 根节点显式使用 `UiTargetCamera` 指向 order 1 相机，Dialog 根节点显式
指向 order 2，避免 Bevy 自动选择最高 order 相机。

区域模糊在 `Core2dSystems::EarlyPostProcess` 执行；Dialog 背景模糊在
`bevy_ui_render::ui_pass` 之后、最终 upscaling 之前执行，因此普通 UI 文字也会
进入高斯采样。`count == 0` 时不提交全屏 draw call。

## 文件结构

```
src/
  main.rs              最小二进制入口
  lib.rs               库入口
  runtime.rs           系统阶段、Plugin 组合、资源类型与顺序
  runtime/
    bootstrap.rs       App 构建、项目 bootstrap、相机
    asset_reader.rs    多资源根只读覆盖桥接
    platform.rs        设计空间、窗口空间、输入与生命周期
    tick.rs            输入、热重载、文本计时、状态推进、转场生命周期
  scene.rs             ScenePlugin 入口
  scene/
    background.rs      背景增量同步
    sprites.rs         立绘增量同步、排序与变换
  storage.rs           StoragePlugin 门面与跨存储域协调
  storage/
    save.rs            原子存档与截图元数据
    settings.rs        运行设置加载与持久化
    profile.rs         profile 与退出刷新
    read_history.rs    已读历史
    gallery.rs         鉴赏解锁
  ui.rs                固定 UI 入口与系统组合
  ui/
    stage.rs + stage/  舞台 UI 门面、Textbox、控制栏与选项
    overlays.rs + overlays/ 覆盖层门面、Backlog 与 Dialog
    screens.rs + screens/ 页面门面、Title、Save/Load 与 Config
    support.rs + support/ 通用 UI 门面、字体、输入域、加载状态与音效
  render.rs            渲染插件门面
  render/
    blur.rs            两阶段可分离高斯模糊 pipeline
  assets/shaders/
    blur.wgsl          模糊 shader
```

每个领域采用“同名 `.rs` 门面 + 同名目录”：门面声明稳定接口、插件注册和系统顺序；目录中的
文件各自实现一个可独立描述的生命周期或执行机制。视觉预设不单独制造文件，除非它已经拥有
独立状态、资源或渲染阶段。

## 质量门槛

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```
