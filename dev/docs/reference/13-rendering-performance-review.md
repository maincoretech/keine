# Kēne 渲染性能审查结论（13 检查点）

> 依据：`KEINE_RENDERING_PERFORMANCE_REVIEW_PRACTICAL.md` 问题集，对当前
> `main`（2eab45a）逐项核查。结论遵循"先确认热路径、有证据才改"的原则；
> 标"暂缓"的项需要 profile/截图数据后才能进入实现。

## 结论总览

| # | 检查点 | 结论 | 证据 |
|---|---|---|---|
| 1 | blur 是否重复执行 | **不改** | 两条路径语义独立 |
| 2 | 视频帧复制与上传 | **macOS 已修；其他平台保留回退** | macOS 无 CPU 像素副本，FFmpeg 仍软件上传 |
| 3 | StageMaterial 重效果组合 | **暂缓** | 仅按 blend 4 变体 specialize |
| 4 | 立绘同步重复查找 | **不改** | 有快速返回，对象少 |
| 5 | 逐字符富文本 | **不改** | glyph 惰性 reveal，台词短 |
| 6 | 区域 Gaussian blur 范围 | **暂缓** | 已复用 + scissor；半分辨率需截图 |
| 7 | 静态/动态效果生命周期 | **暂缓** | 静态 film 可能阻止休眠，需先确认 |
| 8 | 粒子上传 | **不改** | 60 Hz + 单 mesh + 清理 |
| 9 | 大图加载 | **不改** | WebP 解码期缩放；构建期约束已入规范 |
| 10 | 预取窗口 | **不改** | 有界 lookahead，视频单独处理 |
| 11 | UI 相机与隐藏页 | **不改** | 相机休眠已有测试 |
| 12 | Material 更新方式 | **不改** | 静态不重写，动画按需 |
| 13 | 运行时诊断插件 | **小改（可选）** | FrameTime 无消费者 |

## 1. blur 是否重复执行 —— 不改

对象级 authored blur 只进入 `StageMaterial`：`src/scene/sprites.rs` 的
`filter.blur += transform.blur`，随后进入材质 uniform 的 shader 模糊。
区域 Gaussian blur（`src/render/blur.rs` 的 `BlurCamera`/`SceneBlurCamera`/
`UiBlurCamera`）只被 UI 面消费（textbox、dialog、backlog、settings、choice），
矩形来自 UI 组件而不是场景 sprite。两条路径视觉语义不同，同一 authored 值
不会被应用两次。当前行为正确，不修改。

## 2. 视频帧复制与上传 —— macOS 已修；其他平台保留回退

AVFoundation 后端已移除 `copy_bgra_frame()`：解码得到的 `CVPixelBuffer` 经
`CVMetalTextureCache` 映射为 Metal texture，再在 render world 内 GPU 复制到稳定的
Bevy texture。main world 不再分配或复制整帧 BGRA；queue completion 回调保证外部纹理
生命周期覆盖 GPU 使用期。为保持材质绑定稳定，当前仍有一次 GPU 内部 blit。

FFmpeg 路径继续复用 scaler frame，并把软件 RGBA 上传到相同 `Image`。Windows/Linux 的
真正零 CPU-copy 分别依赖 DXGI 和 DMABUF 外部纹理互操作，必须由真实平台基准与设备丢失
测试驱动，不能只在 macOS 上写一套无法验收的条件编译代码。

## 3. StageMaterial 重效果组合 —— 暂缓

`StageMaterialKey` 只按 `BlendMode` 生成 4 个 pipeline 变体；blur、色差、
bloom、LUT、film 等全部走同一 fragment 的 uniform 动态分支。普通无效果对象
确实承担了同一 shader 成本，但同屏对象少、转场效果短暂。拆分粗粒度 variant
需要先确认 GPU 开销与可回退性，暂缓。

## 4. 立绘同步重复查找 —— 不改

`sync_sprites()` 在状态未变、config/尺寸/窗口未变时直接返回；变化时才遍历
现有 `SpriteNode` 并按 id 查找。普通场景几名角色，O(n²) 最坏场景不构成热点。
保持现状。

## 5. 逐字符富文本 —— 不改

`DialogueGlyph` 为每个字符创建实体，但类型机已做惰性处理：只有当前在屏的
glyph 保持渲染循环活跃，未来字符保持隐藏；ruby 叠加在 base glyph 行内。
台词典型短，不构成瓶颈。

## 6. 区域 Gaussian blur 范围 —— 暂缓

已有 `BlurRect[16]`、scissor、intermediate 纹理复用和无区域时跳过；blur 矩形
来自 UI 面，数量通常 1–2 个。半分辨率 blur 实验需截图质量与 GPU 帧时间对比，
按数据决定，暂缓。

## 7. 静态/动态效果生命周期 —— 暂缓

`core_is_animating()`（`src/runtime/platform.rs`）对 `bg_films` 非空或任意
sprite 带 `films` 一律视为动画中；`godray` 已按 speed 判断。若某 film 是纯静态
（无时间依赖），会阻止引擎进入 Idle 低功耗。这是偏功耗的正确性点，但修改有
冻结真实动态效果的风险；先确认 film 是否可真静态并列出所有依赖 `globals.time`
的 shader 分支，再决定 `is_time_varying()` 统一判断。

## 8. 粒子上传 —— 不改

粒子已按固定 60 Hz 模拟、每 emitter 单 mesh、stale mesh 清理、opacity 渐变。
与问题集"通常已经足够"的判断一致。

## 9. 大图加载 —— 不改

WebP 走专用 loader（`src/scene/images.rs`），解码期直接缩到目标尺寸；
PNG/JPEG 走 Bevy 通用 loader（无缩放下采样）。构建期约束优先：资源规范已规定
背景 ≤ 4K、推荐 WebP。可选后续：`cargo validate` 对超大资源出警告（暂缓）。

## 10. 预取窗口 —— 不改

`LOOKAHEAD_ACTIONS = 20` 有界 lookahead，`callScene` 只预热开头窗口，视频绕过
普通资产缓存单独处理，符合"简单、有界"的建议。

## 11. UI 相机与隐藏页 —— 不改

三层相机（SceneBlur / UiBlur / Dialog）已有 `sync_dialog_camera_activity` 休眠
与生命周期测试；隐藏页按需更新。问题集建议不合并相机，维持现状。

## 12. Material 更新 —— 不改

`sync_sprites()` 有快速返回；静态场景不重写 material，动画帧按需通过
`animation_uniform` 更新。没有发现"每帧重写全部宽 uniform"的路径。

## 13. 运行时诊断插件 —— 小改（可选，并入基准 commit）

`FrameTimeDiagnosticsPlugin` 在 `bootstrap.rs` 无条件安装且**无任何消费者**
（帧时间由 benchmark capture 自行采样）。`EntityCountDiagnosticsPlugin` 仅被
benchmark capture（`ui/support/performance.rs`）消费。可把两个插件改为仅在
benchmark/dev 模式安装（或移除 FrameTime），开销小、低风险；等 profile 数据
确认收益后并入性能 commit。
