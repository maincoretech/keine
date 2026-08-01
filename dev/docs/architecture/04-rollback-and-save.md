# 存档与回退（当前实现）

## 边界

keine 将四类生命周期不同的数据分开处理：

| 数据域 | 当前表示 | 持久化位置 | 恢复规则 |
|---|---|---|---|
| 编译脚本 | `Arc<Program>` | 不进入存档 | 启动或热重载时由当前项目安装 |
| 剧情时间点 | `State` 中的执行、舞台、交互、音频和局部变量 | v9 槽位 `.sav` | 只能恢复到 fingerprint 相同的当前 `Program` |
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

fingerprint 是确定性的兼容身份，不是密码学签名，也不替代文件完整性校验。v9 存档分别以 CRC32 校验 metadata 与 state payload。

## v9 二进制存档格式

当前原生存档版本为严格的 **v9**。一个 `slot_N.sav` 由 28-byte 固定 header、Postcard metadata 和 Postcard state payload 顺序组成：

```text
offset  size  field
0       8     magic = "KEINE\0\0\0"
8       4     version = 9 (little-endian u32)
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

metadata 上限为 64 KiB，state payload 上限为 64 MiB；完整解码时 header 声明的两段长度必须与文件实际长度完全一致。metadata 与 state 各有一个 CRC32；槽位列表只读取并校验 header + metadata，真正 LOAD 时才读取、校验并反序列化 state payload。

`State` 的 Serde payload 会跳过：

- `Arc<Program>`；
- global variables；
- 已读历史；
- CG/BGM 解锁；
- 一次性 `effect_queue`。

因此脚本 Action 总数不会直接放大存档；长期玩家数据也不会被复制进每个槽位。

固定 golden 位于 [`crates/loader/tests/fixtures/store-v9.sav`](../../../crates/loader/tests/fixtures/store-v9.sav)，由 `save_v9_golden_is_stable` 防止无意改变字节格式。v9 在 v8 的 LetsGal 1.8.0 相机状态之上加入可恢复的句尾退格状态，因此动画中途存档会在读档后从同一字符和点击等待阶段继续；舞台时间轴本身仍是恢复时清理的瞬态演出，不写入存档。v9 不兼容旧二进制存档。

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

预览 WebP 不嵌入 `.sav`，也不参与 v9 CRC。删除槽位会同时删除 state 与 preview；只清除游戏槽会保留 settings/profile/read history/gallery，而 UI 的 CLEAR ALL 会删除整个 `saves/` 数据目录并同步清理内存 writer cache。

写入采用同目录临时文件、`write_all`、`sync_all` 和 `rename` 原子替换。进程在替换前中断时不会用半写入 payload 覆盖现有槽位。

## 版本与损坏处理

- version 不是 9：`inspect` 返回 `StoreStatus::Unsupported(version)`，`decode` 返回错误；当前没有旧版本迁移器；
- magic、长度、metadata CRC32 或 metadata schema 无法解析：`Corrupt` 或 decode error；
- state 截断或 state CRC32 不匹配：槽位前缀仍可展示，但实际 LOAD 返回 decode error；
- v9 内容有效但 Program fingerprint 不匹配：文件格式有效，剧情恢复被 `ProgramMismatch` 拒绝。

旧版本不能通过“尽量反序列化”静默加载。若未来需要迁移，应增加明确的版本 adapter、输入上限、迁移测试与新的固定 golden，并保持解码结果只能通过 `SavedState::restore_into` 进入运行态。
