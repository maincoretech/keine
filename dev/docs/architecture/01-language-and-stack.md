# keine 语言与技术栈 (v2)

## 决策

| 层 | 技术 | 理由 |
|----|------|------|
| 核心引擎 | Rust (keine-core, keine-loader) | 确定性执行、可序列化状态、零成本抽象 |
| 游戏引擎 | Bevy 0.19 | 成熟 ECS、跨平台渲染、Plugin 架构、bevy_ui |
| GPU 渲染 | wgpu (via Bevy) | 跨平台 Vulkan/Metal/DX12 |
| UI | bevy_ui | Node 布局、Interaction 系统、Bootstrap Icons 字体图标 |
| 音频 | 待定 (rodio / Bevy audio) | -- |
| 桌面壳 | 原生 winit (via Bevy) | 无 WebView 开销 |
| 资源打包 | Hakutaku (`.haku` + `.taku`) | AES-256-GCM、zstd、签名随机访问 |
| 存档序列化 | Postcard | 紧凑、Serde 原生、持续维护的版本化状态快照 |
| 脚本 | 可注册语言适配器（内置 WebGAL `.txt`） | 各语法统一编译为 `Action` IR |

## 不选

| 不选 | 原因 |
|------|------|
| 字节码 VM | VN 脚本执行频率极低（每次点击一次），VM 过度设计 |
| Tauri / WebView | 增加构建复杂度，原生 winit 更轻 |
| Lua 嵌入 | 当前创作需求由轻量语言适配器覆盖，无需嵌入完整 VM |
| 手写 wgpu 渲染 | Bevy 提供成熟的渲染抽象和 UI 系统 |

## 参考项目

| 项目 | 借鉴点 |
|------|--------|
| Bevy | Plugin trait、Schedule、双世界、AssetPlugin::Processed |
| WebGAL | 编辑器预览协议、脚本语义、UI 布局参考 |
| YU-RIS | 延迟命令队列 + batch 合并 |
| Ren'Py | RevertableObject、差分 rollback |
| hakutaku-core / pack | 加密快照、增量 segment 与随机读取 |

## 性能目标

- 二进制 < 20MB (Bevy + assets 嵌入)
- 存档 < 2KB (ECS snapshot)
- 60fps stable
- 冷启动 < 2s (含资产预处理缓存)
- 脚本热重载 < 50ms
