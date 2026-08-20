# 代码审计跟踪（2026-08-20）

审查基准：`7c7b430f794e11d4d9890e43e468f7e7d655a697`。

本文把静态审查结论转换成可验收的维护清单。前三项已经结合运行路径复核并修复；
其余项仍须在实施前重新核对当前代码、真实项目数据和平台行为，不能仅凭本文直接修改。

## 威胁模型与范围

- 玩家通常只接触签名的 Hakutaku 发行包；开发态工程输入不等同于发行包攻击面。
- 源工程、Studio 工程和迁移工具仍应能有界地拒绝异常输入，避免编辑器或 CI OOM。
- Hakutaku、存档、媒体和 native FFI 属于独立边界，各自的限制不能替代另一入口的限制。
- 性能修复必须补可重复基准；媒体和视觉改动还需实际平台验收。

## 状态总览

| 状态 | 项目 | 优先级 |
|---|---|---|
| 已修 | transient SFX 批处理保留命令顺序 | P1 |
| 已修 | `ExecutionLimit` 不再作为普通 yield 静默返回 | P1 |
| 已修 | 直接资源路径不再重复添加目录；裸 Effect 进入迁移期 | P1 |
| 已修 | source-project 统一有界读取 | P1/P2 |
| 已修 | 兼容音频 encoded-input 上限 | P1/P2 |
| 暂缓 | PNG/JPEG 保持兼容 loader，发行资源目标仍为 WebP | P2 |
| 已修 | looped FFmpeg 音视频 no-progress guard | P2 |
| 已修 | LetsGal 源工程 confinement 与 native 内容层一致 | P2 |
| 已修 | backup/publisher commit 后 cleanup 只告警 | P2 |
| 待复核 | compiled payload 分配前结构预算与 fuzz | P2/P3 |
| 待复核 | overlay 目录枚举合并所有 mount | P3 |
| 待复核 | user input 严格截断到最大 Unicode scalar 数 | P3 |

## 已修项目

### 1. transient SFX 命令顺序

**问题**：一次 step 内的 `Play`、`Stop`、`StopOneShot` 被压平成集合状态，导致 stop
错误地吞掉其后的 play。

**最小复现**：`Stop → Play B` 应播放 B；`Play A → Stop → Play B` 应只播放 B。

**约束**：stop 仍须作用于已经存在的 player；Bevy deferred commands 产生的新 player
不能被同批 stop 误伤；具名与无名 one-shot 语义一致。

**验收**：覆盖 `Stop→Play`、`Play→Stop`、`PlayA→Stop→PlayB`、
`StopOneShot(id)→Play(id)` 四种顺序。

### 2. forward execution safety limit

**问题**：Core 返回 `ExecutionLimit` 后，普通 runtime 把它当作正常推进完成，玩家只会
看到脚本静默半停。

**最小复现**：连续执行 1025 个不 yield 的 action，后接 dialogue。

**约束**：不能自动在下一帧继续，否则无穷 jump 会变成持续 CPU 循环；错误路径不能
提前破坏状态，标题转场仍负责最终清理。

**验收**：达到上限时记录明确错误并 fail closed 到标题转场，dialogue 不出现，也不需要
玩家再点击一次。

### 3. 原生资源 resolver

**问题**：`background/day.webp` 等显式路径被再次添加类别目录；Effect 的裸名称历史上
回退到 `vocal/`，与规范目录 `se/` 不一致。

**最小复现**：背景 `background/day.webp` 不应解析为
`background/background/day.webp`；音效 `se/click.opus` 应无需别名。

**约束**：别名优先；只有安全的、多段、使用 `/` 的相对路径才原样通过；最终路径
containment 仍由内容挂载层执行。为避免破坏旧工程，裸 Effect 暂时继续解析为
`vocal/{name}`。

**验收**：所有资源类别的显式路径保持原样；单段逻辑名保留既有默认；不安全路径不被
识别为显式路径；`cargo validate` 和发布编译对裸 Effect 发出一次迁移警告。

## 已完成 hardening

### 4. source-project 有界读取

**状态**：已实现单文件 32 MiB 的有界 reader，native mount 与 LetsGal JSON 复用读取逻辑，
LetsGal 同时检查 canonical project root。工程文件数与总源码量不设硬上限，因为它们不等价于
峰值内存，且会误伤大型或高度拆分的合法工程。

**问题**：native script、LetsGal JSON 和迁移工具存在单文件无界读取入口。最初审查同时
建议限制文件数和总源码量；复核后确认这两项不是有效的峰值内存模型，因此不采用。

**实施约束**：先统计真实大工程的单文件、文件数和总字节分布，再确定默认值；native、
LetsGal 和 tooling 复用同一个 bounded reader，错误包含路径、实际值和上限。

**验收**：覆盖单文件超限、读取期间增长、文件数超限、总量超限和正常大型工程；拒绝
发生在 parser 分配完整 payload 之前。

### 5. 兼容音频输入预算

**状态**：已为必须驻留 encoded input 的兼容音频和非 mount Opus 增加 128 MiB 上限；项目
Opus 继续使用流式 ContentFile，不受该内存输入路径影响。

**问题**：MP3/WAV/Vorbis/FLAC 为支持 seek/loop 会把完整 encoded file 留在内存中，
与流式 Opus 的内存模型不一致。

**实施约束**：短期在 `read_to_end` 前后执行一致的 encoded-byte 上限；长期 streaming
不得破坏 ContentFile range-read、seek、loop 和取消语义。

**验收**：边界值可载入，超过一个字节立即失败，读取期间增长不能绕过限制，预取不产生
额外完整副本。

### 6. PNG/JPEG 预算

**状态**：暂缓。PNG/JPEG 只保留 Bevy 兼容 loader；Kēne 的生产资源方向仍是打包前转换为
WebP，因此不为兼容格式复制一套长期 codec 预算架构。现有文档继续明确这一边界。

**问题**：WebP 有压缩字节、源像素、输出像素三层限制，generic PNG/JPEG 路径没有同等
Kēne 级别的硬约束。

**实施约束**：在大额分配前检查 encoded bytes、width/height、pixel count 和输出长度；
正常发行资源的预算必须与打包期转换策略一致。

**验收**：超大维度、压缩炸弹、乘法溢出和正常边界图均有测试；文档同步准确数值。

### 7. looped FFmpeg 进度不变量

**状态**：已实现。每个循环必须产出 frame/sample；视频还要求有限、正的 duration。音频
decode/resample error 不再被当成“暂时没有 sample”吞掉。

**问题**：可成功 open/seek、但一个循环周期没有产出 frame/sample 的媒体可能高速反复
seek，且不受 bounded output queue 的背压。

**实施约束**：每个 loop cycle 至少产出一个有效 frame/sample；合法的短媒体和 decoder
flush 尾帧不能被误判；错误必须终止 worker 并可观测。

**验收**：零输出、零 duration、尾帧延迟、正常 loop 和取消同时发生均有覆盖。

### 8. LetsGal confinement

**状态**：已随 source reader 完成。所有实际读取的 LetsGal JSON 都检查 canonical target
仍在 canonical project root 内，并应用同一文件数/字节预算。

**问题**：LetsGal adapter 的直接 filesystem JSON 读取尚未确认复用了 native
`ContentMount` 的 canonical containment。

**实施约束**：统一 confined reader；每次打开都检查最终真实路径，避免 symlink escape；
publisher 的 symlink 拒绝不能代替开发态边界。

**验收**：绝对路径、`..`、文件/目录 symlink escape、正常根内 symlink 策略均有明确测试。

### 9. commit 后 cleanup 语义

**状态**：已完成。正式 rename 与必要目录同步仍可返回错误；其后的旧副本删除/同步失败只
记录 warning，不再覆盖已经成功的 import/publish 结果。

**问题**：backup import 或 publisher 在正式 rename 已成功后，旧 backup 删除失败仍可能
返回整体失败，造成“显示失败但实际已切换”。

**实施约束**：明确不可逆 commit point；commit 前失败返回错误，commit 后清理失败只返回
可观测 warning，并保留后续可重试清理的信息。

**验收**：对 rename 前失败、正式 rename 失败、commit 后 cleanup 失败分别注入故障并检查
最终目录和返回状态。

## 待复核 hardening、一致性与 UX

### 10. compiled payload 分配前预算

**问题**：scene/action/string 的语义上限主要在 postcard 已反序列化后检查。

**实施约束**：保持 schema/version/CRC/fingerprint 兼容；先通过 fuzz 证明实际风险，再选择
depth/count-aware decoder 或更小的 envelope 分段，避免为低风险边界引入复杂格式。

**验收**：恶意 length prefix、深层 action、超长 string、超量 scene/action 在预算内失败，
并持续运行 fuzz/sanitizer smoke。

### 11. overlay directory union

`read_directory()` 应与高优先级覆盖的直接 `read()` 形成一致视图：合并所有 mount 的直接
children，再按 layer priority 去重。实施前先确认 runtime 是否已有 folder enumeration 调用。

### 12. user input 最大长度

追加一次包含多个 Unicode scalar 的 `Key::Character` 时，只取剩余容量；不得按 byte 截断，
也不能先制造超长状态再依赖提交校验拒绝。

## 需要产品决定的语义

`muted` 或无音轨视频是否应 duck BGM/vocal 不是安全问题。若视频始终代表独占 cinematic，
维持现状；若 muted 表示“只显示画面”，duck 条件应基于实际 active 的 unmuted video audio。
在产品语义确定前不修改。
