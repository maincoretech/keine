# WebGAL 兼容性审计

本目录记录 keine 对 WebGAL 脚本语义的实际兼容范围、可复现证据、视觉验收状态和高性能内部格式。结论以代码和测试为准，不以“解析器接受了关键词”或“已有验收清单”冒充端到端支持。

## 审计基线

- 审计日期：2026-07-15
- WebGAL 稳定版：[`4.6.2`](https://github.com/OpenWebGAL/WebGAL/releases/tag/4.6.2)（2026-07-04 发布）
- WebGAL 引擎提交：[`e7f0abeb855b5b442460743bdaa9778ca751b43f`](https://github.com/OpenWebGAL/WebGAL/tree/e7f0abeb855b5b442460743bdaa9778ca751b43f)
- WebGAL 文档提交：[`121bd6a64f4095b41c5caa9c2bbafa8f0a8d83b9`](https://github.com/OpenWebGAL/WebGAL_Doc/tree/121bd6a64f4095b41c5caa9c2bbafa8f0a8d83b9)
- 官方脚本参考：[docs.openwebgal.com/script-reference](https://docs.openwebgal.com/script-reference/)
- 官方命令注册表：[sceneParser.ts @ 4.6.2](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/parser/sceneParser.ts)
- 官方执行模型：[SCRIPT_AUTHORING.md @ 4.6.2](https://github.com/OpenWebGAL/WebGAL/blob/e7f0abeb855b5b442460743bdaa9778ca751b43f/packages/webgal/src/Core/gameScripts/SCRIPT_AUTHORING.md)

基线共有 **31 个文档化、已注册的可执行命令**。`comment` 是分号注释语法，`(global)` 是公共参数页，二者不计入 31 个命令。引擎源码还注册了未进入当前文档目录的 `setFilter`，但 WebGAL 4.6.2 的实现是空操作，因此也不计入这 31 项。

## 结论

| 状态 | 数量 | 含义 |
|---|---:|---|
| 已实现 | 5 | 命令本地的文档化主语义已有自动测试覆盖；若需要呈现层，也必须有可核验的呈现证据 |
| 部分支持 | 22 | 有可用子集，但仍有参数、状态、时序、资源类型、自动测试或视觉证据不完整 |
| 不支持 | 4 | 没有等价 Action/runtime；loader 会给出明确 warning，而不会误当普通对话 |
| 合计 | 31 | 与 WebGAL 4.6.2 文档化命令注册表逐项对齐 |

这不是“完整 WebGAL 兼容”的声明。尤其是公共 `-continue`、高级 filter、自定义多段动画、Live2D/Spine、视频、运行时 UI 换肤和 Steam 桥接仍未等价实现。`setTransform` 已改为稀疏 `TransformPatch`，不会再重置未出现字段；它仍因 easing/filter/writeDefault/keep 子集而保持“部分支持”。四个不支持命令保留 warning 防误降级测试。完整逐项证据见 [semantic-matrix.md](semantic-matrix.md)，明确缺口见 [unsupported.md](unsupported.md)。

## 证据口径

每项能力分四层审计：

1. **P — Parser**：语法、转义、参数、资源引用和诊断是否正确。
2. **C — Core**：Action、状态迁移、阻塞/继续、场景栈和持久状态是否正确。
3. **R — Render/UI/Audio**：Bevy 呈现、输入或音频链路是否消费了状态。
4. **V — Visual**：固定环境下的截图/golden 或有记录的人工视觉验收。

`P+C+R` 不等于 `V`。当前仓库有语义、布局数学和渲染辅助逻辑的自动测试，但**没有截图快照、像素差分或其他自动视觉回归基线**。当前也不保留人工截图作为基线；页面、动画关键帧、尺寸与平台仍保持待验，详见 [visual-audit.md](visual-audit.md)。现有 `dev/docs/acceptance/` 文件只是人工验收步骤，不是已经通过的证据。

## 文档导航

- [semantic-matrix.md](semantic-matrix.md)：31 个命令的官方合同、当前实现、测试证据与状态。
- [visual-audit.md](visual-audit.md)：自动/人工视觉矩阵、现有证据边界和 golden 方案。
- [unsupported.md](unsupported.md)：明确不支持项、部分支持项的高风险差距和完成标准。
- [internal-format.md](internal-format.md)：当前高性能 `Program`/`State` 格式、复杂度与后续演进约束。

## 当前内部格式摘要

脚本在加载后被编译为不可变 `Program`：每个场景使用 `Box<[Action]>` 紧凑保存，并在构建时生成 label 索引与稳定 fingerprint；运行时 `State` 通过 `Arc<Program>` 共享脚本。这样 clone/rollback/save 不再按脚本大小复制全部 Action，label 跳转也不再每次线性扫描。`Program` 被排除在 serde 存档外；热重载整体替换 `Arc`，协调仍有效的位置，并让旧 fingerprint 的 Backlog checkpoint 失效。

`step()` 每次调用只共享一个 `Arc<Program>` 并借用当前 Action，避免热循环深拷贝脚本 payload。`setTransform` 使用 presence-mask `TransformPatch`，在不分配内存的情况下只覆盖脚本实际写出的字段。v9 存档 codec 解码为不可直接运行的 `SavedState`，只有 `restore_into()` 通过 Program fingerprint 检查后才能合入当前状态；不匹配时原子拒绝。`global_vars`、已读历史和鉴赏解锁也已经从单槽 save/Backlog rollback 中分离；长期变量单独写入版本化 `saves/profile.bin`，成功读档不会回滚当前 profile。

这项优化已经消除了“脚本越长，状态快照越重”的主要结构性成本；它不代表所有状态都已最优。Backlog 快照仍会复制 sprites、变量和场景栈，高级演出也仍需更精细的 typed payload 与增量快照。详细约束和建议见 [internal-format.md](internal-format.md)。

## 复现入口

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

测试结果以当前命令输出为准，不维护容易过期的独立数字快照。人工视觉执行时应另行保存平台、GPU、窗口尺寸、步骤、截图/录屏和实际结果。
