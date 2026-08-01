# keine ECS 架构（历史参考）

> **注意：本文件描述的是早期自研 ECS 方案，现已改为使用 Bevy 0.19 完整 ECS。**
> 当前架构见 [05-bevy-architecture.md](05-bevy-architecture.md)。

## 核心原则

借鉴 Bevy 的 Plugin + ECS + Schedule 架构思想，但不引入 Bevy 整个依赖链。
自研约 500 行的瘦 ECS，只覆盖 VN 引擎的有限 Component 种类。

## World 结构

```rust
struct World {
    entities: Vec<Entity>,
    components: HashMap<ComponentId, HashMap<Entity, Box<dyn Any>>>,
    resources: HashMap<ResourceId, Box<dyn Any>>,
}

// VN 引擎的全部 Component（< 10 种）
struct Sprite    { handle: AssetHandle, pos: Vec2, alpha: f32, scale: f32 }
struct Text      { content: String, speaker: String }
struct Audio     { handle: AssetHandle, volume: f32, playing: bool }
struct Transform { pos: Vec2, rotation: f32, scale: Vec2 }
struct Transition { kind: TransitionKind, progress: f32, duration: f32 }
```

## Plugin 系统

```rust
trait Plugin: Any + Send + Sync {
    fn build(&self, app: &mut App);
}

// 使用示例
App::new()
    .add_plugin(ScriptPlugin)
    .add_plugin(RenderPlugin { backend: WgpuBackend::new() })
    .add_plugin(AudioPlugin)
    .add_plugin(HexzResourcePlugin::with_password(env_key))
    .add_plugin(EditorSyncPlugin)       // WebGAL 协议
    .add_plugin(RollbackPlugin)         // 差分存档回退
    .run();
```

## Schedule 系统

```
Startup  → 一次初始化
Update   → 每帧：脚本执行 → 动画更新 → 音频更新 → 输入检测
Render   → 每帧：Extract → 命令队列 batch → wgpu submit → present
```

帧率不敏感的关键：Update 和 Render **完全解耦**。
Update 是事件驱动的 `step_until_interactive()`，Render 是固定的 vsync 循环读快照。

## Crate 划分

```
keine/
├── core/               # package keine-core
├── loader/             # package keine-loader；统一 asset/script/store adapter
│   └── adapter/asset.rs # fs、auto、hexz_k 标准 Hexz 资源包
├── keine-render/     # wgpu 渲染后端 + Displayable trait
├── keine-audio/      # rodio 音频
├── keine-rollback/   # 差分快照 + 回退
├── keine-editor/     # Tauri + Svelte 编辑器
│   ├── src-tauri/      # Rust: Tauri commands + EditorSync
│   └── src/            # Svelte: 编辑器 UI + 调试面板
└── keine-cli/        # CLI: build / pack / preview / check
```

## 与传统 VM 架构的对比

| 维度 | VM 方案 (nova) | ECS 方案 (keine) |
|------|---------------|-------------------|
| 状态管理 | 单一 VmState | Component 分散在 Entity 上 |
| 扩展性 | 改 OpCode enum | 加 Component + System |
| 渲染解耦 | 手动同步 | 双世界 Extract 自动 |
| 存档 | 序列化整个 VM | 序列化 ECS World（差分） |
| 并行 | 无（单线程 VM） | System 可并行调度 |
