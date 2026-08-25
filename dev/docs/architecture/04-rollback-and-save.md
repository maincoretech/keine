# 存档与回退（当前实现）

## 边界

keine 将四类生命周期不同的数据分开处理：

| 数据域 | 当前表示 | 持久化位置 | 恢复规则 |
|---|---|---|---|
| 编译脚本 | `Arc<Program>` | 不进入存档 | 启动或热重载时由当前项目安装 |
| 剧情时间点 | `State` 中的执行、舞台、交互、音频和局部变量 | v10 槽位 `.sav` | 只能恢复到 fingerprint 相同的当前 `Program` |
| 长效玩家数据 | global variables、已读历史、CG/BGM 解锁、设置 | `profile.bin`、`read_history.bin`、`gallery.bin`、`settings.bin` | 不被单槽读档或 Backlog 回想覆盖 |
| 一次性运行事件 | `effect_queue` 等 | 不持久化 | 由呈现层消费，恢复时清空 |

权威剧情状态位于 [`crates/core/src/model/state.rs`](../../../crates/core/src/model/state.rs)。存档 codec 位于 [`crates/loader/src/adapter/store.rs`](../../../crates/loader/src/adapter/store.rs)，文件系统槽位生命周期位于 [`src/storage.rs`](../../../src/storage.rs)。

## Backlog 与回想

每次记录已显示对白时，core 生成一条 `BacklogEntry`。列表上限由 `DEFAULT_BACKLOG_CAPACITY` 固定为 **200 条**；超过上限时从最旧记录开始删除。

每条记录包含展示用 speaker/text/vocal，以及一个轻量 `RollbackSnapshot`：

- 当前 scene、cursor 与 `callScene` 栈；
- 背景、立绘、transform/filter、textbox、film mode、粒子和 transition rules；
- 当前对白、BGM、循环音效与局部变量；
- 创建快照时的 `program_fingerprint`。

快照不包含编译后的 `Program`、global variables、已读历史或鉴赏解锁。回想因此不会复制整个脚本，也不会把玩家长期进度退回到旧值。

`restore_backlog` 在恢复前再次核对 fingerprint 和 scene：

1. fingerprint 与当前 `Program` 不同则拒绝；
2. scene 不存在或为空则拒绝；
3. cursor 夹紧到当前 scene 边界；
4. scene stack 中失效 frame 被删除，其余 cursor 被夹紧；
5. 动画、等待、菜单、一次性音效等瞬时状态被清理后，再由权威状态重建呈现。

安装新 `Program` 时，旧 fingerprint 的 Backlog 记录会失效并被清除，不能跨脚本布局执行旧回想点。

## Program fingerprint

`Program` 为 scene 建立稳定顺序后，对 scene 名和 typed Action payload 的 Postcard 表示计算 64-bit FNV-1a fingerprint。它用于识别“这个执行位置属于哪一份编译脚本”：

- scene 输入顺序变化不会改变 fingerprint；
- scene 名、Action 内容或布局变化会改变 fingerprint；
- `State`、槽位 metadata 和每个 `RollbackSnapshot` 都携带该值；
- `Program::insert_scene` 与 `State::install_program` 会同步重新计算或安装该值。

fingerprint 是确定性的兼容身份，不是密码学签名，也不替代文件完整性校验。v10 存档分别以 CRC32 校验 metadata 与 state payload。

## v10 二进制存档格式

当前原生存档版本为严格的 **v10**。一个 `slot_N.sav` 由 28-byte 固定 header、Postcard metadata 和 Postcard state payload 顺序组成：

```text
offset  size  field
0       8     magic = "KEINE\0\0\0"
8       4     version = 10 (little-endian u32)
12      4     metadata_len (little-endian u32)
16      4     state_len (little-endian u32)
20      4     CRC32(metadata payload)
24      4     CRC32(state payload)
28      ...   Postcard SerializedMetadata
...     ...   Postcard State
```

metadata 包含：

- `saved_at_unix`；
- `program_fingerprint`；
- scene 与 cursor；
- 当前 speaker 与纯文本对白预览。

metadata 上限为 64 KiB，state payload 上限为 64 MiB，连同 28-byte header 后完整文件最多
67,174,428 bytes。LOAD 在完整分配前先用该总上限执行 bounded read，再由 decoder 要求 header
声明的两段长度与文件实际长度完全一致。metadata 与 state 各有一个 CRC32；槽位列表只读取并
校验 header + metadata，真正 LOAD 时才读取、校验并反序列化 state payload。

`State` 的 Serde payload 会跳过：

- `Arc<Program>`；
- global variables；
- 已读历史；
- CG/BGM 解锁；
- 一次性 `effect_queue`。

因此脚本 Action 总数不会直接放大存档；长期玩家数据也不会被复制进每个槽位。

固定 golden 位于 [`crates/loader/tests/fixtures/store-v10.sav`](../../../crates/loader/tests/fixtures/store-v10.sav)，由 `save_v10_golden_is_stable` 防止无意改变字节格式。v10 保存可恢复的句尾退格状态，因此动画中途存档会在读档后从同一字符和点击等待阶段继续；舞台时间轴本身仍是恢复时清理的瞬态演出，不写入存档。v10 只接受自身的二进制布局。

v10 进一步持久化脚本游标之后仍会影响后续行为的逻辑表现状态：等待推进、系统消息、
幕布、浮动文字、立绘规则、对白/段落样式及 reveal override、sprite sequence。FFmpeg
decoder、共享 stage timeline、camera/keyframe animation 等 native 时间轴仍不进入 payload。

## 可恢复 checkpoint 合同

`State::persistence_safety()` 是所有保存入口共享的边界：

- 普通 dialogue/typewriter、可序列化 transform、幕布/浮动文字、样式和 sprite sequence
  可 exact resume；
- blocking 与 non-blocking video、shared stage timeline、camera shake/effect/transform、
  background/sprite keyframe/position animation 在 active 期间不是 exact-save-safe；
- 已消费的一次性 host/audio event 不恢复，避免 LOAD 后重复触发。

手动 SAVE 与 Q·SAVE 遇到第二类状态时不写文件，并向用户说明需要等待当前演出结束。
返回标题和 graceful exit 使用 `ContinuationCheckpoint`：runtime 只在脚本恢复边界把最后一个
exact state 克隆到 RAM，危险演出期间保存该内存快照；本次会话若尚无 checkpoint，则保留已有
quick save，不以不完整状态覆盖它。

checkpoint 捕获本身没有文件创建、周期备份或 `fsync`。只有用户实际保存、返回标题或正常退出
才执行一次既有原子写入，因此该可靠性合同不会以持续写放大或额外硬盘磨损为代价。

## SavedState 恢复边界

`StoreAdapter::decode` 不返回可直接运行的 `State`，而是返回 [`SavedState`](../../../crates/loader/src/adapter/store.rs)：

```text
slot reader
  -> inspect: validate header / version / metadata length + CRC32
slot bytes
  -> decode: validate full length / metadata CRC32 / state CRC32
  -> decode SavedState
  -> snapshot()                 # 只读 metadata/preview 投影
  -> restore_into(current)      # 唯一执行态恢复入口
```

`SavedState::restore_into` 调用 `State::restore_saved`。恢复合同如下：

1. 存档 fingerprint 与当前 `Program` 不同，返回 `ProgramMismatch`，当前 State 保持不变；
2. 匹配时重新附着当前项目已经安装的 `Arc<Program>`；
3. 保留当前 global profile、已读历史和鉴赏解锁；
4. 对槽内 scene、cursor、scene stack 与 Backlog 做防御性协调；
5. 没有任何有效执行位置时，清空位置并安全进入 ended 状态。

Save/Load UI 可以先用 metadata fingerprint 隐藏或禁用不兼容槽位，但 core 的 `restore_into` 检查仍是最终安全边界，不能依赖 UI 过滤代替。

## 槽位文件与原子写入

每个槽位的状态和预览图分开保存：

```text
saves/
  slot_0.sav       # quick save
  slot_0.webp      # 独立舞台预览 sidecar
  slot_N.sav
  slot_N.webp
```

这里的 `saves/` 相对于独立的 persistence root：可编辑目录工程使用项目根，Hakutaku
发行版使用由稳定 `project.id` 决定的平台用户数据目录。内容根始终只读；旧发行版遗留在
内容旁的 sidecar 只会在新目录不存在时复制一次，永不反向覆盖或自动删除。

预览 WebP 不嵌入 `.sav`，也不参与 v10 CRC。保存时由独立相机直接渲染到不超过
480x270、保持当前窗口宽高比的目标，随后在有界后台队列中以质量 80 的有损 WebP
编码；不回读全窗口纹理，也不在渲染线程缩放或编码。存档页通过 `IoTaskPool` 按当前页
异步解码，强 `Handle<Image>` 缓存仅保留十个可见槽位。

每次 state 替换都会先推进预览 generation 并删除旧 sidecar，再原子提交新的权威 state；
删除槽位、清空数据或导入备份也先推进 generation。截图与后台编码任务携带其 slot
generation，最终原子提交前再核对；过期任务会被丢弃，不能在 delete/CLEAR ALL/import
后重新写回 sidecar。进程在任意提交点退出，最多留下“旧 state + 无 preview”或“新
state + 无 preview”，不会留下“新 state + 旧 preview”。队列满、编码失败或 state
写入失败同样只表现为暂时没有预览。返回标题或正常退出只写 continuation、不生成截图时，
也在 state 提交前删除 slot 0 的旧 preview。generation 只协调内存状态与最终提交，不增加
周期性存档或磁盘同步。

删除槽位会同时删除 state 与 preview；只清除游戏槽会保留 settings/profile/read
history/gallery，而 UI 的 CLEAR ALL 会删除整个 `saves/` 数据目录并同步清理内存
writer cache。

写入采用同目录临时文件、`write_all`、`sync_all` 和 `rename` 原子替换。进程在替换前中断时不会用半写入 payload 覆盖现有槽位。同步只发生在上述真实写档操作，不在 checkpoint 捕获或每帧执行。

## 版本与损坏处理

- version 不是 10：`inspect` 返回 `StoreStatus::Unsupported(version)`，`decode` 返回错误；当前没有旧版本迁移器；
- magic、长度、metadata CRC32 或 metadata schema 无法解析：`Corrupt` 或 decode error；
- state 截断或 state CRC32 不匹配：槽位前缀仍可展示，但实际 LOAD 返回 decode error；
- v10 内容有效但 Program fingerprint 不匹配：文件格式有效，剧情恢复被 `ProgramMismatch` 拒绝。

旧版本不能通过“尽量反序列化”静默加载。若未来需要迁移，应增加明确的版本 adapter、输入上限、迁移测试与新的固定 golden，并保持解码结果只能通过 `SavedState::restore_into` 进入运行态。
