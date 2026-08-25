# Kēne 资源、发行包与持久化限制

本文记录当前实现会主动拒绝或截断的资源边界。数值来自 Kēne 当前源码和
`Cargo.toml` 固定的 Hakutaku revision `24c39ea`；升级依赖或调整常量时，必须同步更新本文。

这些限制分为三类：

- **硬上限**：输入超过后直接返回错误或拒绝打开；
- **运行时预算**：限制可重建缓存，不代表进程总内存；
- **打包策略**：控制分块与 segment 滚动，不是 wire format 的最大包体积。

## 1. 项目路径与目录资源

| 边界 | 当前行为 | 生效阶段 |
|---|---|---|
| `adapter.asset.path` | 必须解析为项目根目录内的现有目录；绝对路径和 `..` 逃逸均拒绝 | 项目加载 |
| `config.yaml` | 解析前最多读取 256 KiB；超过时拒绝项目 | 项目加载 |
| 单个原生脚本或 LetsGal JSON | 32 MiB | 源码加载、校验与打包编译 |
| 文件系统 mount 内的读取 | 每次访问都解析真实路径，并确认仍位于该 mount 根目录内；指向外部的文件或目录 symlink 拒绝 | 运行时读取 |
| Hakutaku 打包输入 | 只接受普通目录和普通文件；任何 symlink 或 special file 均拒绝 | `cargo bundle` |
| 包内逻辑路径 | 非空 UTF-8 相对路径，使用 `/`；禁止前后 `/`、反斜杠、NUL、空段、`.` 和 `..` | Hakutaku 打包与读取 |
| 单条包内路径 | 最多 `65,535` UTF-8 bytes | Hakutaku 打包 |

文件系统 mount 的逐次 canonical containment 可以阻止静态工程中的 symlink escape。
它不是 capability/openat 模型；如果本机另一个进程能在检查与打开之间并发替换路径，仍属于
更严格的本地 TOCTOU 威胁边界。

源码 reader 只限制单个输入，避免一个异常文件触发无界分配；它不限制工程的文件数量或总
源码量。读取会先检查文件报告的长度，再最多读取“单文件上限 + 1”字节并复核实际长度，
因此读取期间增长不能绕过限制。LetsGal 的 project、chapter、character、manifest、Studio state
和 shell JSON 同样检查项目根 containment。该边界只用于开发态加载、校验和打包编译，不进入
游戏逐帧路径。

## 2. 图片与视频

### WebP

Kēne 的自定义 WebP loader 在分配输出前应用以下硬上限：

| 项目 | 上限 |
|---|---:|
| 压缩文件输入 | 64 MiB |
| 源图像素数 | 64 Mi pixels（`67,108,864`） |
| 解码/缩放后像素数 | 16 Mi pixels（`16,777,216`） |
| `layout.sprite_height` | 设计高度的 4 倍，即 `4,320` 逻辑像素 |

项目配置在进入 runtime 前拒绝零尺寸、非有限、非正或超过 4320 的
`sprite_height`。WebP loader 内部仍保留回退/截断作为防御，确保未来非配置调用者也
不能绕过分配上限。尺寸和 RGBA buffer 大小使用 checked arithmetic，并在写入前进行
fallible allocation。

这些限制只覆盖自定义 WebP 路径。PNG/JPEG 仍由 Bevy 通用 loader 处理，目前没有同一套
Kēne 压缩字节/像素硬上限。

### 视频

FFmpeg 与 macOS AVFoundation backend 对每个解码帧应用同一尺寸边界；FFmpeg 另有显式
decoder queue：

| 项目 | 上限 |
|---|---:|
| 单边宽或高 | 4,096 pixels |
| 总像素数 | `4,096 × 2,304 = 9,437,184` pixels |
| decoder → frame thread 队列 | 2 个 RGBA 帧 |
| 全局 video surface-equivalent 预算 | 256 MiB |

行宽、stride、源数据范围和 RGBA 总字节数都在复制前检查。后续帧的像素格式、宽或高
发生变化时，scaler 会按当前帧重建。
两帧队列满后 decoder 会阻塞等待容量，但同时监听 session cancellation；取消不会等定时
轮询，也不会因消费者停止取帧而把 decoder 永久卡在发送操作中。

每个活跃视频按 4 份 RGBA surface 预留全局预算，覆盖当前 GPU surface、decoder/scaler
surface 和至多两个 queued frames；同分辨率帧复用 reservation，动态分辨率变化会重新核算。
超出预算的 session 终止而不会继续分配。该预算覆盖 Kēne 可见的 surface，不包含 FFmpeg 或
AVFoundation 内部无法由调用方精确计量的 codec working set。

Looped FFmpeg 音视频的每个循环周期必须至少产出一个 frame/sample；视频周期还必须具有
有限且为正的 timeline duration。无输出或无时间进度的媒体会终止 decoder，而不会反复 seek
形成空转。

### 音频

项目挂载中的 Ogg Opus 使用可重开的 `ContentFile` 流式解码，不整文件驻留。为提供统一的
seek/loop，WAV、MP3、Vorbis 和 FLAC 兼容格式会共享一份 encoded input，其单文件硬上限为
128 MiB；非项目挂载来源中必须退化为内存输入的 Opus 也应用同一上限。读取按 64 KiB 分块，
在扩容前检查上限并使用 fallible allocation。

## 3. 编译脚本产物

`.keine/compiled/program.bin` 的 reader 和 writer 共享以下硬上限：

| 项目 | 上限 |
|---|---:|
| metadata | 1 MiB |
| encoded payload | 512 MiB |
| scenes | 1,000,000 |
| actions | 100,000,000 |
| scene name、资源路径或 sub-scene reference | 每项 16 MiB |

metadata/payload ceiling 在反序列化前生效；scene/resource/sub-scene 的逐项限制属于解码后的
semantic validation。Action 内对白、Choice 文本和表达式等字符串由 512 MiB payload 总上限
约束，不声称具有相同的逐字段 16 MiB 上限。这些是编译产物 envelope 的防御边界，不是建议
的工程规模。`source_manifest_hash` 只记录构建 provenance，运行时完整性由 envelope CRC、
Program fingerprint 与发行包认证负责。源脚本热重载会在独立 worker 完成全量解析和 Program
构建，再在帧边界安装结果；大项目仍会占用后台 CPU，但不再把完整解析时间直接压进单帧预算。

## 4. Hakutaku 发行包

### Wire format 硬上限

当前 `hakutaku-core` revision `ce8fe3c` 对一个已签名 snapshot 执行：

| 项目 | 硬上限 |
|---|---:|
| 解码后 catalog | 64 MiB |
| 加密 catalog | 65 MiB + 16 bytes |
| 单个解码 metadata page | 1 MiB |
| 单个 plaintext content block | 1 MiB |
| referenced segments | 128 |
| indexed files | 1,000,000 |
| content blocks | 10,000,000 |
| metadata pages | 100,000 |
| canonical path pool | 32 MiB |
| 单条 canonical path | 65,535 UTF-8 bytes |

Header、count、offset 和 length 会在分配前验证并使用 checked arithmetic。Hakutaku
没有另设一个简单的“总包体积”或“单资源文件字节数”常量；实际可接受规模同时受到上述
catalog、block、segment 和路径池上限约束。不要把下面的 segment target 当成格式硬上限。

### Kēne 运行时预算

Kēne 使用 `ResourceBudget::memory_constrained()` 打开资源包：

| 可重建状态 | 预算 |
|---|---:|
| 解密 block-map page cache | 512 KiB |
| plaintext content-block cache | 16 MiB |
| prefetch plaintext cache | 512 KiB |
| idle segment handles | 4 |
| Normal block probation entries | 64 |

这约束的是 Hakutaku 可丢弃并重建的 cache。常驻 catalog、Kēne 的文件集合/direct-children
目录索引、当前/前一个 streaming block、独占 transient buffer、媒体 decoder buffer、
GPU texture 和 Kēne 自身状态都不包含在这些数值中，
因此不能把 `17 MiB` 误写成进程或资源系统总内存上限。

Kēne 在这层之上使用关键路径优先的时间线预取：当前画面所需资源不限制并发并进入
loading gate；活动中的逐帧立绘按原顺序优先；普通时间线预测只保留前 8 个不同资源，
且同时最多启动 1 个投机加载。20 个 action 的扫描窗口只是寻找候选的范围，不代表会
同时加载 20 个资源。视频继续由独立的流式播放与全局 surface budget 管理，不进入这套
通用 AssetServer 预取。

### 当前打包策略

`cargo bundle` 使用 Hakutaku 默认策略：

| 分类 | block / chunk 策略 | 新 segment 滚动目标 |
|---|---|---:|
| Hot（文件不超过 32 KiB） | 单文件固定块 | 64 MiB |
| Normal（不超过 64 MiB 的非媒体） | FastCDC 32/128/512 KiB | 256 MiB |
| Normal bulk（更大的非媒体） | 固定 1 MiB block | 256 MiB |
| Transient | 由访问分类决定 | 128 MiB |
| Streaming（音频/视频） | 固定 256 KiB block | 512 MiB |

required 与 deferred 资源不会写入同一个 segment。上述值是滚段目标，单个最后写入的 block
可能令 segment 略超目标；它们也不会覆盖 wire format 的 128-segment 等硬上限。

## 5. 存档与辅助持久化

所有文件先通过 bounded read，再交给 postcard 或 save envelope decoder。运行时写入
settings、gallery、profile、read history 和单槽 save 时也执行同一 envelope 上限，因而
不会生成下一次启动时被自身拒绝的持久化文件：

| 文件/结构 | 字节上限 | 数量上限 |
|---|---:|---:|
| 单个 save metadata | 64 KiB | — |
| 单个 save state | 64 MiB | — |
| 完整 save envelope | 67,174,428 bytes（28-byte header + 上述两段） | — |
| settings | 64 KiB | — |
| gallery | 16 MiB | 65,536 个 CG+BGM 条目 |
| profile | 16 MiB | 65,536 个 global variables |
| read history | 64 MiB | 1,000,000 条记录 |
| backup envelope | 128 MiB | 4,096 个文件 |
| backup 内单文件 | 72 MiB | — |
| 单项目 `config.yaml` | 256 KiB | — |
| 全局 `engine.conf` | 256 KiB | — |

Backup V2 import 直接从已受限的 envelope 借用文件名和 payload，不再为每个文件复制一份完整
数据。Export 仍会同时持有源文件集合和序列化 buffer，所以 128 MiB 是当前整体编码方案的
短期安全界限；若要进一步降低峰值，应升级为 streaming container，而不是重新放大该常量。

`StoreAdapter::maximum_encoded_size()` 是单槽存档读写的共同上限。LOAD 会在为完整文件分配
内存前先检查 filesystem metadata 报告的文件长度，并以 `maximum + 1` 的 bounded read 防止并发增长；
SAVE 也会拒绝 codec 写出超过同一上限的 payload。槽位列表使用格式自己的前缀检查，但
adapter 的默认完整检查路径同样遵守该上限。

Backup import 和 publisher 的正式目录 rename（以及要求的目录同步）是事务 commit point。
Commit 之后删除旧副本失败只记录明确 warning，并留待下次操作重试清理；它不会把已经成功
安装的存档或发行目录报告成整体失败。Commit 之前或正式 rename/同步失败仍返回错误。

项目配置和全局 `engine.conf` 均先经过 bounded read，再解析；引擎配置上限为 256 KiB。

## 6. 确定性执行保护

| 路径 | 上限 | 超限结果 |
|---|---:|---|
| 单次 core forward execution | 1,024 actions | 返回 `ExecutionLimit`，防止无 yield 脚本死转 |
| editor/backlog seek replay | 65,536 steps | 停止 seek 并记录警告 |
| editor blocking presentation replay | 1,024 steps | 停止 replay，防止残留 blocking 状态形成无界循环 |

这些是一次执行/回放的确定性保护，不代表整个项目只能包含相同数量的 actions。

## 7. 失败发生在哪一层

- `cargo validate`：配置、mount、脚本与引用层错误；
- `cargo assets --pack` / `cargo bundle`：非 WebP 项目图片、非 Opus 独立音频、symlink、
  special file、非 canonical path 和 Hakutaku 格式上限；
- 打包游戏启动：签名、catalog、segment 与 compiled program envelope 上限；
- 资源实际加载：WebP、FFmpeg 帧、symlink containment 等按需限制；
- 存档/导入：持久化文件在反序列化前执行字节与数量限制。

更改任何数值时，应同时更新对应回归测试和本文；资源格式常量还需要核对固定的 Hakutaku
revision，而不是在 Kēne 文档中维护另一份可能漂移的数值。

## 8. 实现锚点

- 路径与 mount：[loader source](../crates/loader/src/loader/source.rs)、
  [runtime asset reader](../src/runtime/asset_reader.rs)；
- 图片与视频：[bounded native codecs](../crates/media/src/lib.rs)、
  [image integration](../src/scene/images.rs)、[video](../src/scene/video.rs)；
- 源码输入：[source input](../crates/loader/src/source_input.rs)、
  [LetsGal adapter](../crates/loader/src/adapter/editor/letsgal.rs)；
- 内存音频：[audio integration](../src/runtime/audio.rs)；
- compiled program：[compiled](../crates/loader/src/compiled.rs)；
- 存储：[storage](../src/storage.rs)、[backup](../src/storage/backup.rs)；
- 确定性执行：[core step](../crates/core/src/runtime/step.rs)、
  [editor seek](../src/runtime/tick.rs)；
- Hakutaku 固定 revision：[Cargo.toml](../Cargo.toml)。
