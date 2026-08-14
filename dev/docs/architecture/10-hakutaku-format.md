# Hakutaku v1 游戏资源格式

> 状态：v1 wire format 已冻结并由 Kēne loader/packer 使用；Kēne 当前固定 Hakutaku
> revision `24c39ea`。v1 不读取 Hexz、不保留 `.hxz` 回退，也不建立兼容层。规范字段与
> 字节偏移以 Hakutaku workspace 内的 `FORMAT.md` 为准，本文件解释架构取舍；当前部署所用
> 的硬上限和运行时预算见
> [资源、发行包与持久化限制](../../../docs/resource-limits.md)。

## 决策摘要

Hakutaku 是面向离线游戏运行时的资源格式，不是通用压缩包、虚拟文件系统或在线 CAS。
它使用一个完整快照和若干不可变数据段：

```text
release/
├── keine[.exe]
├── game.haku
└── data/
    ├── <segment-id>.taku
    └── <segment-id>.taku
```

这是桌面发行和调试时的规范逻辑布局，不是移动端宿主路径契约；移动端可以由系统 asset pack 或
应用私有目录提供同一个 `game.haku` 与 SegmentId 集合。

- `game.haku` 是当前完整逻辑文件树，经过压缩、加密并由发行者签名；
- `.taku` 只保存不可变的密文块，不保存路径和可变目录；
- 每个文件直接引用物理块，不递归解析 parent，也不在运行时搜索 CAS；
- 新版本复用旧块引用，只顺序写入包含新块的新 segment，再原子替换 `game.haku`；
- 运行时从任意文件偏移直接得到 segment、物理偏移和解码参数；
- zstd 只压缩独立块，随机寻址由 Hakutaku 自己的索引完成，不使用
  `zstd-seekable` 的第二套 frame table。

## KISS 原则

Hakutaku 只解决三件事：把逻辑路径映射到块、认证并解码块、原子激活一个新快照。其余能力均
留在边界之外：

- 桌面和移动端使用同一种 wire format、同一个 parser、同一套签名与 AEAD 路径；
- Steam、Google Play、App Store 或自托管下载只是 transport，不进入 archive format；
- core 只认识 `Snapshot`、`SegmentSource`、`Asset` 和少量资源预算，不认识 CDN、asset pack、
  JNI、Swift、数据库或更新 UI；
- 平台差异通过“如何按 SegmentId 打开一个随机可读文件”消化，不复制 reader；
- v1 不加入可选算法协商、插件、通用 overlay、数据库 catalog 或脚本化 pack graph。

任何新增抽象都必须同时消除至少一个真实的平台分叉或运行时重复路径。只为可能的未来扩展而
增加一层间接，不属于 KISS。

## 目标与非目标

目标：

1. Windows、macOS、Android 与 iOS 固态存储上的低延迟随机读和高吞吐顺序读；
2. 视频、音频后端可以直接 `read_at` / `Read + Seek`，不产生明文临时文件；
3. 小改动只新增变化块，不重写大 archive，不让加密破坏平台增量更新；
4. 密钥内嵌的离线威胁模型下，提供合理的内容隐藏、强完整性与发行身份验证；
5. 运行时依赖、内存分配、锁竞争和文件句柄数量可预测；
6. 格式解析面对损坏或恶意输入时有严格长度上限且不 panic。
7. 移动端在低内存、进入后台和资源包位置变化时可释放状态并安全恢复。

非目标：

- 不阻止能够调试客户端的攻击者最终取得明文或嵌入密钥；
- 不实现通用文件权限、软链接、时间戳、扩展属性或任意 codec 插件；
- 不实现网络协议、CDN、S3、在线 updater 或跨游戏全局 CAS；
- 不读取 Hexz 0.8，也不迁移已有 `.hxz`；所有 fixture 和测试包直接重建；
- 不以 DirectStorage、Metal I/O 或某一种 SSD 的参数作为格式前提。

## 为什么是快照加不可变 segment

单体 archive 即使只改一个资源，也可能移动后续数据和 TOC offset；平台差分会看到大量变化，
客户端还可能为了一个小补丁复制整个 archive。parent 链则把同一个逻辑文件的读取变成递归查找，
链深、损坏处理和回收都会进入运行时热路径。

Hakutaku 的快照始终包含当前完整文件树，但数据块位于不可变 segment：

```text
Snapshot N
  file A -> [segment 1 / block 3, segment 4 / block 1]
  file B -> [segment 2 / block 8]

Snapshot N+1
  file A -> [segment 1 / block 3, segment 5 / block 0]
  file C -> [segment 5 / block 1]
```

运行时只打开 N+1 的完整 catalog。删除文件只是删除 catalog 记录；没有 overlay、tombstone、
parent depth 或全局 hash lookup。完全没有引用的旧 segment 不进入新发行目录；部分仍被引用的
segment 保留。`rebase` 显式重写当前完整数据并清理碎片，但绝不在普通增量构建中自动发生。

SteamPipe 建议修改局部化、避免重排、限制 pack 大小，并为新内容增加新 pack。不可变 segment
直接满足这些约束：<https://partner.steamgames.com/doc/sdk/uploading>。

## 物理布局

### `game.haku`

快照文件由固定 public header、常驻 catalog 和懒加载 map pages 组成：

```text
offset  size       content
0       4096       SnapshotHeaderV1，含固定 64-byte signature slot
4096    variable   zstd(catalog) 的 AES-256-GCM ciphertext || tag
...     variable   独立加密的 BlockMapPage ciphertext || tag
...     variable   仅 packer 读取的 ReusePage ciphertext || tag
```

header 至少包含：

- magic、严格的 format major/minor；
- 稳定 project id 与单调 release sequence；
- catalog ciphertext 长度、解压后长度及硬上限；
- catalog salt、nonce 与 page count；
- signing key id，仅用于诊断，公钥仍来自发行引擎；
- 对签名覆盖范围和 KDF context 的固定版本标识。

catalog 内含每个 map/reuse page 的 offset、stored/plain length、nonce ordinal 和完整 BLAKE3 digest。
signature slot 置零后，签名输入固定为：

```text
"Hakutaku snapshot signature v1" ||
BLAKE3(SnapshotHeaderV1-with-zero-signature || catalog ciphertext || catalog tag)
```

运行时只需读取 header 与 catalog 即可验签，不必在启动时扫描全部冷页。随后先验签、再解密
catalog；加载某个 map page 时先核对 catalog 中已签名的 digest，再执行 page AEAD 和解压。这个
单向承诺链既保持发行者完整性，也避免把整个 block map 常驻内存。未知 major 直接拒绝，v1 不做
“尽量读取”。

### `.taku`

segment 由一个 4 KiB header 和连续 payload region 组成：

```text
offset  size       content
0       4096       SegmentHeaderV1
4096    variable   block ciphertext || tag，按打包顺序连续排列
```

segment header 保存 magic、segment uid、KDF salt、nonce prefix、block count、payload length 和
header version。segment 本身不保存路径或 block TOC；所有物理位置来自已经验签的 catalog 与
BlockMapPage，因此数据区没有重复索引，也不会出现两个索引不一致。

catalog 的 `SegmentRecord` 保存 segment id、uid、文件长度、payload 长度、block count 和一个
二值 availability class：`required` 或 `deferred`。打开 segment 时必须将 header 与这份已签名
记录逐字段比较，不能把未签名 header 当成事实来源。segment 文件名只由 catalog 中的 id
构造，不接受 manifest 或项目资源提供任意宿主路径。

`required` segment 必须在快照激活前可解析；经网络新下载的文件还必须通过完整 hash。应用
bundle 或系统 asset pack 中的文件不在每次启动重复扫描，而是在实际读取时由 block AEAD 与
signed `cipher_digest_128` 认证。
`deferred` 允许尚未下载，但第一次读取只返回结构化的 `SegmentUnavailable(id)`，由宿主请求系统
asset pack 或自托管下载。格式不保存 Play/Apple pack 名称；部署工具另外产生
`SegmentId -> delivery pack` 映射，变更分包策略不需要重打 Hakutaku 数据或改变安全语义。

`SegmentId = BLAKE3(完整 segment 文件)`，文件名使用该 ID。普通启动不扫描整个大文件计算 hash；
缺失、长度和 header 在打开时验证，每个实际读取的 block 由 signed `cipher_digest_128` 与 AEAD
tag 延迟验证。`hakutaku verify` 和打包事务必须执行完整 segment hash。

数据区只在起点做 4 KiB 对齐。不能给每个小 block 填充到 4 KiB，否则大量脚本和小图片会产生
显著空间浪费。流式大资源由 packer 产生固定 256 KiB block；普通小块紧密排列。v1 reader 按块
执行 positioned read，不把设备扇区大小或尚未验证收益的 extent planner 写进格式。

## Catalog、冷页与 working set

所有结构都是手写、有界、little-endian 的 v1 wire format，不使用 serde、bincode 或 postcard。
所有 offset、count、length 在分配前检查，整数运算使用 checked arithmetic。

Hexz 值得保留的不是名为 `memory/auxiliary` 的第二数据流，而是更基础的 working-set 思路：常用
目录信息小而常驻，体积随 block 数增长的索引分页，plaintext cache 有硬预算，预读不能挤掉真正
热的数据。Hakutaku 用一个格式表达这件事，不复制 Hexz 的双 stream、两套 page directory 或
通用 VM/archive 语义。

### 常驻 catalog

启动时只解密并保留：

1. segment table；
2. path byte pool；
3. on-disk open-addressed path index；
4. compact file table；
5. BlockMapPage 与 ReusePage directory/page digests。

path index 由 packer 构建，slot 只保存 64-bit keyed path hash 和 file index；命中后仍比较完整
路径字节，避免碰撞错误。catalog 保持为一块 `Arc<[u8]>`，记录通过有界 offset 借用其中字节，
不为每个路径创建 `String`、不重建 `HashMap`、不生成目录树。编辑器需要的分类、树形统计和
缩略图 metadata 不进入发行 catalog。

v1 对 catalog 存储/解码字节数、单个 page/block、文件数、block 数和 referenced segment 数设置
编译期硬上限；未验签的 catalog stored length 最多为 65 MiB + 16 bytes，单个解码 block/page 最多
1 MiB。Kēne 的初始
referenced segment 上限为 128。增量构建接近上限时必须显式 `rebase`，不能让多年补丁把启动
成本和文件句柄数变成无界状态。128 是运行时安全界限，不代表 packer 应等到最后一个 segment
才提醒维护。

### 懒加载 BlockMapPage

`BlockRef` 按全局 block ordinal 连续打入固定容量 map page，初始解压后上限约 16 KiB。文件不强制
对齐 page 边界，避免大量小文件产生碎片；`FileRecord` 保存首个全局 block ordinal 与 block 数量，
reader 用除法直接得到 page ordinal 和页内 record，不遍历全局 block table。固定分块文件还保存
实际 block 长度，可由逻辑 offset 直接定位；FastCDC 文件只在该文件的短 block range 内二分查找。

```text
FileRecord
  path_offset
  path_len
  layout_kind
  access_class
  logical_len
  first_block
  block_count
  fixed_block_len

BlockRef
  logical_offset
  segment_ordinal
  segment_block_ordinal
  physical_offset
  stored_len
  plain_len
  codec
  reserved_zero
  cipher_digest_128

ReuseRecord (packer only)
  chunk_id
  block_locator
```

v1 将 `BlockRef` wire record 固定为 48 bytes：segment ordinal 用 `u16`（上限 128），block ordinal、
stored/plain length 用 `u32`，只有 logical/physical offset 保留 `u64`；所有字段从 byte slice 显式
读取，不把 Rust struct padding 写盘。16 KiB decoded page 可覆盖约 341 个 block ref。即使 20 GiB
资源按 256 KiB 分块，完整 map 约 3.8 MiB，而 runtime 通常只常驻少量 16 KiB page。

固定大小 block 的文件通过除法定位；FastCDC 文件在该文件的有序 `BlockRef.logical_offset` 上二分，
不建立运行时 prefix-sum 或额外 seek 索引。若真实 workload 证明超大 FastCDC 文件需要 checkpoint，
可由后续格式版本增加；v1 不为尚未出现的热点增加 wire state。

`cipher_digest_128 = BLAKE3(ciphertext || GCM tag)[0..16]` 位于签名承诺的 map page。runtime 在
cache miss 时先扫描通常更小的 stored bytes 核对它，再原地 AEAD 和解压，避免为发行者完整性对
plain output 增加第二次大内存遍历。128-bit targeted second-preimage 强度与 v1 的内容保护目标匹配；
它不是可由已泄漏 content key 重算后绕过的 keyed tag。

`chunk_id = BLAKE3(plaintext)` 只存在于独立 ReusePage。packer 构建增量包时按需读取全部 reuse
pages 并建立临时 `chunk_id -> BlockRef` map；runtime 验证 catalog 中的 bounds 后完全跳过这些页，
不付出 I/O、解密、解压或常驻内存成本。ReusePage 保留在发行快照中，确保任何合法旧发行都能
直接作为下一次增量构建输入，不再维护另一份 publisher sidecar。

map page cache 与 plaintext block cache 分开计量；page miss 不持有 cache lock 做 I/O、AEAD、
解压或解析。page value 是不可变 `Arc<Vec<u8>>`，相同 file/segment 的并发读取共享一份页面，
解码后的 `Vec` 进入 cache 时不再搬运到第二份字节分配。

packer 按 `required/deferred` 与 `access_class` 分离 segment writer，并按规范化路径的确定顺序
写入。当前分类只依据 deferred prefix、文件大小和已知媒体扩展名；基于真实运行 trace 的场景
邻近排序尚未实现，不能把它当作现有性能保证。

## 分块与压缩

Hakutaku v1 只定义两个 codec：

- `RAW`；
- 独立 zstd block。

不引入 LZ4、Brotli、seekable-zstd frame、通用字典或 codec 插件。zstd 使用
`default-features = false`；打包器对每块试压缩，只有超过固定收益阈值才保存压缩结果。MP4、
WebP、PNG、JPEG、Opus 等通常自然回退 RAW，避免热路径无意义解压。

分块是打包策略，不是硬编码为一种大小：

| class | 初始策略 | 目的 |
|---|---|---|
| tiny | 小文件单独成块 | 不因 slab 重排扩大补丁 |
| mutable | FastCDC，初始候选 32/128/512 KiB | 脚本和可变二进制跨版本复用 |
| streaming | 固定 256 KiB，文件内连续 | 视频/音频 seek 与顺序预读 |
| bulk | 固定 1 MiB | 很少 seek 的大型冷数据 |

这些是 benchmark 初始候选，不是未经测量永久冻结的常量。format 保存实际 block 边界，未来
packer 调参不要求 reader 或磁盘版本变化。segment 默认目标大小为 512 MiB，允许发行配置在
256 MiB 到 1 GiB 内选择；这同样只影响打包，不进入兼容契约。

## 加密、签名与 nonce

开发者提供两个彼此独立的身份：

- 32-byte 高熵 content root key，编译进发行引擎；
- Ed25519 signing key，仅存在于发行构建环境，公钥编译进引擎。

Hakutaku 不接受人类密码，因此不携带 PBKDF2、Argon2、HMAC 或 SHA-2 KDF 链。BLAKE3
`derive_key` 使用固定、全局唯一的 context，从 root key、project id、segment uid 和随机 salt
派生 catalog/page/segment 子密钥。BLAKE3 官方实现明确提供 KDF 模式：
<https://github.com/BLAKE3-team/BLAKE3>。

每个 segment 生成随机 salt 和 64-bit nonce prefix；block nonce 为 prefix 加 32-bit block
ordinal。一个 segment 不得达到 `u32::MAX` blocks。key 和 nonce 都由 format 层保证唯一；随机
访问使用 `ring::aead::LessSafeKey` 的 in-place API，但不把 nonce 管理交给调用者。

v1 固定使用 AES-256-GCM，header、catalog 和 block record 均没有 cipher id；reader 不包含算法
协商、平台分支或回退实现。Hakutaku 以独立 block 作为认证和随机读取单元，每个单元使用 96-bit
nonce 与 128-bit tag，只有 tag 验证成功后才能把 plaintext 交给解码器。`ring` 的 prepared key
跟随有界 snapshot/segment handle 复用，不在每次小范围读取时重复执行 key schedule：
<https://docs.rs/ring/latest/ring/aead/>。

block AAD 至少绑定 project id、segment uid、block ordinal、codec、stored length 和 plain length。
攻击者不能交换 block、修改解码参数或把一个项目的数据复制到另一个项目。

AEAD 只证明“持有 content key 的一方生成了这个 block”；离线客户端中的 content key 最终可被
提取，所以发行者完整性必须另外形成完整承诺链：

```text
embedded Ed25519 public key
  -> signed catalog
  -> BLAKE3 digest of BlockMapPage
  -> signed cipher_digest_128 in BlockRef
  -> BLAKE3(ciphertext || GCM tag)[0..16]
```

读取顺序固定为 page digest、page AEAD、cipher digest、block AEAD、解压。任何 content key 持有者
都能阅读资源，却不能在没有 signing private key 的情况下制造官方引擎接受的新 ciphertext。
完整 segment hash 主要服务下载完成校验、文件命名和快速整段验证，不替代这个懒验证链。

root key 的混淆只负责避免明文常量出现在 strings，不计入密码学安全。runtime 按需在
`Zeroizing<[u8; 32]>` 中短暂重建 root material、派生 catalog/segment key 后立即清除原始字节；
活跃 AEAD key 的生命周期跟随有界 handle/page context，不建立包含所有 segment key 的全局表。
这减少意外 crash dump 和释放后 heap residue，但不能阻止攻击者在密钥正在使用时读取进程内存。

旧快照和旧 segment 一起被替换时仍可能形成离线 rollback；没有受信平台单调计数器就无法由
资源格式独立阻止。商店版本控制和可执行文件签名负责发行版本，Hakutaku 不伪装解决该问题。

## 反调试与反 dump

把反调试当成低成本的 release hardening 是合理的；把它当成 Hakutaku 的保密或完整性边界不合理。
攻击者控制设备时可以 patch 掉检测、替换引擎、注入代码，或在解码器/GPU 上传边界取得 plaintext。
OWASP 同样明确指出，客户端反调试只能增加逆向成本，无法对完全受攻击者控制的设备提供绝对
效果：<https://mas.owasp.org/MASTG/0x05j-Testing-Resiliency-Against-Reverse-Engineering/>。

因此 Hakutaku core 不包含 debugger/root/jailbreak 检测。Kēne packaged runner 的 `hardened`
feature 只采用一次性、无常驻线程的低风险措施：

- 所有平台：release LTO、strip symbols、`panic=abort`，不记录 key、nonce、plaintext 或解密错误
  的敏感细节；development/CI 始终可调试；
- Unix desktop：`RLIMIT_CORE=0`；Linux desktop 可再设置 `PR_SET_DUMPABLE=0`，失败只记非敏感
  诊断而不影响启动；
- macOS desktop：可保留一次 `PT_DENY_ATTACH`，但它只防最直接的 attach；
- Windows desktop：可保留一次 `IsDebuggerPresent` 检查；Microsoft 将它定义为当前进程的
  user-mode debugger 检测，不应宣传成防内存读取：
  <https://learn.microsoft.com/windows/win32/api/debugapi/nf-debugapi-isdebuggerpresent>；
- Android：release manifest 强制 `android:debuggable=false`，这是官方明确建议：
  <https://developer.android.com/privacy-and-security/risks/android-debuggable>；
- iOS：不使用未确认的 private/non-SDK anti-debug API，只依赖 release signing/entitlements、短
  plaintext 生命周期和格式自身完整性。App Store 要求只能使用 public API：
  <https://developer.apple.com/app-store/review/guidelines/>。

v1 明确不加入周期 debugger polling、`TracerPid`/Frida/root 猫鼠检测、timing trap、自修改代码、
加密堆、guard-page 迷宫或 platform-specific obfuscated VM。这些机制增加耗电、误报、崩溃与维护
成本，仍可被 patch，违反 KISS。

“反 dump”采用缩小暴露面而不是声称阻止 dump：没有明文临时文件；解密到调用者或复用 scratch；
`transient/streaming` 不进入共享 plaintext cache；密钥中间值安全清零；内存压力时 `trim()`；正常
core dump 尽量关闭。默认不 `mlock` 大块内存，因为它会增加常驻集并恶化移动端内存压力；也不对
普通 heap page 使用 `MADV_DONTDUMP`，其页粒度和平台分叉与收益不匹配。活跃剧情对象、解码帧和
GPU 资源仍可被有能力的攻击者取得，这是离线客户端不可消除的事实。

## SSD 与操作系统 I/O

### 默认使用 buffered positioned I/O

默认 backend 保持操作系统文件缓存，并使用无共享 cursor 的 positioned read：

- Unix/macOS：`FileExt::read_exact_at` / `pread`；
- Windows：`FileExt::seek_read`，异步 backend 可使用 overlapped `ReadFile`；
- segment 只在首次读取时打开；活跃 handle 由 `Arc` 共享，空闲 handle 受预算约束并可被逐出；
- 不用 `Mutex<File>` 加 seek，多个资源可并发读取同一 segment。

Rust 的 Unix `FileExt` 明确保证 offset 与当前 cursor 独立：
<https://doc.rust-lang.org/std/os/unix/fs/trait.FileExt.html>。

v1 默认不使用 `FILE_FLAG_NO_BUFFERING`、`F_NOCACHE` 或 direct I/O。Windows 官方要求 unbuffered
I/O 的 offset、length 和 buffer address 满足物理扇区对齐；小范围 seek、尾块、可变压缩长度和
缓存命中都会因此复杂化：<https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering>。
系统页缓存还能让重复密文读取、启动重开和多个资源共享物理页，不应在没有实测前绕开。

Hakutaku 也不把 NAND page、erase block、NVMe queue depth、APFS allocation block 或 NTFS cluster
大小写入格式。这些值不能由跨平台应用可靠地当成稳定设备能力，而且逐设备优化会破坏发行包的
确定性。格式负责连续 extent、少量大顺序写和不原地覆写；reader 的 batch、prefetch 与并发度
由目标机器基准和运行时反馈决定。

### 当前 read path 与 planner 边界

v1 Core 将一次逻辑读取映射到独立 block，逐块执行 positioned read、BLAKE3、AES-GCM 与可选
zstd。连续读取复用密文和解压 buffer；Streaming cursor 保留当前与前一个 block。显式
`Asset::prefetch_range` 逐块认证并写入独立的有界 prefetch cache，可由宿主任务池提前执行。

相邻 physical extent 合并和并行 block pipeline 仍是后续候选，不是当前实现。只有实际冷读 trace
证明系统调用或队列深度成为瓶颈时才增加，且不能扩大随机单块请求或引入新的 async runtime。

### 同步核心、可插拔异步调度

核心 API 始终提供同步 `read_at`，满足 FFmpeg AVIO 等回调；Kēne 使用现有任务系统调度异步加载，
Hakutaku 不依赖 Tokio。额外 backend 可以实现同一 extent API：

- macOS `DISPATCH_IO_RANDOM` 支持同一随机访问 channel 并发读取：
  <https://developer.apple.com/documentation/dispatch/dispatch_io_random>；
- Windows overlapped I/O 可以批量挂起请求；
- Windows DirectStorage 和 Metal 3 I/O 保留为后续 renderer backend，不进入 core 依赖。

DirectStorage 与 Metal I/O 都强调队列化和批量请求；Hakutaku 的连续 extent、稳定 offset 和独立
数据区允许以后接入。当前 AES-GCM 与 zstd 仍需 CPU 处理，不能假装平台 I/O API 会自动解码
Hakutaku 数据。Apple 的 Metal I/O 面向文件到 GPU resource：
<https://developer.apple.com/documentation/metal/resource-loading>。

### 缓存与预读

系统页缓存保存密文；Hakutaku cache 只保存已经认证并解码的 plaintext block。两者职责不能
混合。

- `hot`：第一次按需读取即可进入共享 CLOCK cache，但不永久 pin；
- `normal`：先进入只保存 key 的小型 probation ring，第二次命中才占 plaintext cache；
- `streaming`：不自动进入共享 cache，cursor 内保留当前与前一个 block；
- `transient`：读取后立即交给调用者，适合只解析一次的 program/config 和一次性大资源；
- 每个 `Package` 独占 cache，entry key 是 `(segment ordinal, block ordinal)`；
- cache value 是 `Arc<Vec<u8>>`，命中后不复制完整 block，cache admission 也不搬运明文；
- 总字节预算是硬限制，不能只限制 entry 数量。

cache 使用标准库实现针对 block 的固定策略，不引入通用 `lru`。每个 `AssetCursor` 有独立逻辑
位置；不可变 `Asset` 提供线程安全 `read_at`，因此视频 seek 不争用 cursor lock。显式预取进入
单独的 CLOCK cache，由 `prefetch_cache_bytes` 限制，不能驱逐 Hot/Normal plaintext cache。

`access_class` 是性能 hint，不是另一个数据 stream，也不导致 eager load。当前 reference packer
将不超过 32 KiB 的文件标为 `hot`，已知音视频扩展名标为 `streaming`，其余标为 `normal`；
`transient` 已存在于格式和 runtime，但尚无自动选择规则。启动依赖图或真实 trace 驱动的分类属于
后续 profile-guided packing，不能提前假设已经存在。

zstd 使用可复用的 `zstd::bulk::Decompressor` context，并通过 `decompress_to_buffer` 写入复用
buffer；该 API 正是为独立多块解压和减少重复内存成本设计：
<https://docs.rs/zstd/latest/zstd/bulk/struct.Decompressor.html>。

### 安全检查的性能边界

安全工作只发生在对应 cache miss，不进入每帧或 cache-hit 热路径：

- 启动：读取 4 KiB header 与 catalog，执行一次 BLAKE3 和一次 Ed25519 verify；不扫描 segment、
  BlockMapPage 或 ReusePage；
- map page miss：读取一个小 page（解码后目标约 16 KiB），执行一次 page BLAKE3、AEAD 和可选
  zstd；命中只取 `Arc`；
- block miss：对 `stored_len` 字节做一次 BLAKE3-128 核对，再原地 AES-GCM，必要时直接解压进
  caller/cache buffer；不再 hash 较大的 plaintext；
- block hit：不重复 hash、AEAD 或 zstd，只共享 `Arc` 并复制调用者实际请求的范围；
- streaming：安全检查按顺序 block 摊销；当前逐块读取，不为了合并而复制整个 batch；
- hardened runner：所有检查只在 bootstrap 执行一次，没有轮询线程、timer 或 frame system。

page cache、block cache、prefetch cache、空闲 handle 和 Normal probation 由 `ResourceBudget`
显式限制；cursor scratch 则由格式的最大 block 长度约束。任何进一步的安全层若需要对每帧轮询、
重复扫描已验证 plaintext、额外保留一份密文/明文，或让 streaming 多一次整块复制，默认拒绝。

### 写路径与 SSD write amplification

packer 采用有界并行生产、单一顺序 writer：

- FastCDC、压缩和加密可由固定数量的 scoped worker 处理；
- 结果按确定顺序交给一个大 `BufWriter` 连续写 segment；
- 不对每块 `fsync`，每个完成 segment 只执行一次 flush/sync/atomic rename；
- 新版本永不原地修改旧 segment；
- rebase 和 GC 是显式维护操作，避免每次补丁重写大量仍有效数据；
- 临时文件与目标在同一文件系统，发布前重新打开并完整验证。

这同时减少随机写、文件系统元数据操作、SSD 写放大和平台更新时的大文件复制。

## 移动端

移动端不是“弱安全模式”，也不拥有第二套包格式。Android/iOS reader 与桌面端执行完全相同的
快照验签、catalog/page AEAD、signed cipher digest 和逐块 AEAD；关闭 cache、减少并发或使用
系统 asset pack 都不能绕过其中任何一步。下载完成的 segment 还必须在进入可用 source 前验证
完整 `SegmentId`，不能仅相信 HTTPS、商店校验和或文件名。

content root key 和 verifying key 仍编译进客户端。root/jailbreak、调试和进程内明文提取继续属于
既定离线威胁模型；移动端 sandbox、Android 文件系统加密和商店签名是纵深防御，不替代 Hakutaku
自身完整性。core 暴露已签名 `release sequence`，宿主可以拒绝低于本机 high-water mark 的快照；
清除应用数据或完全控制设备后无法提供不可回滚保证，格式不会声称能够做到。

### 一个极小的存储边界

```text
SegmentSource::open(segment_id) -> PositionedFile | SegmentUnavailable
```

`PositionedFile` 只需要 `len` 和 `read_exact_at`。core 不关心它来自：

- 桌面的 `data/<id>.taku`；
- Android Play Asset Delivery 的当前 pack location；
- Apple Background Assets 返回的 file descriptor；
- 应用私有目录中自行下载的不可变 segment；
- 只读应用 bundle 与可写更新目录组成的宿主 resolver。

Android 官方明确要求每次启动重新查询 asset pack location，因为应用更新或清除数据会使旧位置
失效：<https://developer.android.com/guide/playcore/asset-delivery/integrate-java>。Apple Background
Assets 能直接返回适合程序读取的 file descriptor，并在应用版本之外更新资源：
<https://developer.apple.com/documentation/backgroundassets/downloading-apple-hosted-asset-packs>。
两者都只实现 `SegmentSource`；Hakutaku core 不直接依赖 Play Core、Foundation、JNI 或 Objective-C。

放入 APK/AAB 的 `.haku`/`.taku` 必须配置为 `noCompress`。密文基本不可再次压缩，而 Android
`AssetManager::openFd` 只接受未压缩 asset：
<https://developer.android.com/reference/android/content/res/AssetManager#openFd(java.lang.String)>。
这避免额外解包和明文/密文临时副本，同时保留 positioned read。

### 更新事务保持不变

自托管移动更新与桌面 bundle 使用同一事务：

1. 下载 `game.haku.part`，验签、解密并执行全部边界检查，但不激活；
2. 根据新 catalog 下载缺失的 `required` segment 到 `.part`，transport 可自行断点续传；
3. 每个 segment 完成后验证长度、header 和完整 `SegmentId`；block AEAD 保持按读取验证；
4. 再次确认新快照引用的所有 `required` segment 已可用；
5. 同文件系统原子替换活动 `game.haku`，最后再异步清理孤立 segment。

任何暂停、杀进程、断电或磁盘不足都只留下 `.part` 或未引用的完整 segment；旧快照在第 5 步前
始终有效。Android 可由 `AtomicFile` adapter 完成安全替换：
<https://developer.android.com/reference/android/util/AtomicFile.html>。Apple 平台使用 app container
内的 atomic safe-save/rename；应用 bundle 保持只读。Hakutaku 不实现下载器、后台任务、重试、
联网策略或更新 UI，也不引入 SQLite 记录链式 patch。

商店托管更新可以把同一组 `.taku` 任意组合为 install-time、essential、prefetch 或 on-demand pack；
自托管更新则直接按 SegmentId 下载。两种方式最终都提供相同不可变文件，平台分发策略不会改变
签名、加密或块引用。

### 内存、功耗与生命周期

移动适配只调整一个显式 `ResourceBudget`：map-page、plaintext、prefetch cache、空闲 segment
handle 与 Normal probation。core 不根据 User-Agent 或设备型号猜策略；Kēne 的平台层给出默认
预算，真实设备基准再调整。`Package::trim()` 清空 package 级 page/plaintext/prefetch cache、
probation 和空闲 handle；活跃 `AssetCursor` 的当前/前一 block 属于 cursor 工作集，生命周期结束
时释放，后续读取仍可按需重开 segment。

不 mmap 解密后的可写大区，不把整段视频解密进 RAM，也不因 page cache 看起来占用内存而复制
一份私有缓存。Android 官方说明 clean file-backed page 可以被系统回收，而被修改或匿名的页面会
增加常驻压力：<https://developer.android.com/topic/performance/memory-management>。因此移动端仍
采用 buffered positioned read，小 cache、有限预读和流式块，不新增另一条 I/O 热路径。

## 公共 API 形状

```text
PositionedFile: Send + Sync
  len()
  read_exact_at(offset, destination)

SegmentSource: Send + Sync
  open(segment_id) -> PositionedFile | SegmentUnavailable

Package::open(snapshot_file, segment_source, root_key, verifying_key, resource_budget)
Package::asset(path) -> Asset
Package::trim()

Asset: Clone + Send + Sync
  len()
  read_at(offset, destination)
  cursor() -> AssetCursor

AssetCursor: Read + Seek + Send
Package: Clone + Send + Sync
```

`Package`、`Asset` clone 只复制 `Arc`。`AssetCursor` 的 cursor 状态不共享；AVFoundation 使用
`Asset::read_at`，FFmpeg AVIO 使用 `AssetCursor`。core 不暴露 segment、codec 或 cache entry，
上层 loader 只看到稳定的文件语义。`PositionedFile` 和 `SegmentSource` 是平台边界，不是新的磁盘
层级；桌面默认实现仍只是标准库 `FileExt` 加确定路径。

## 依赖预算

运行时只允许四个直接依赖：

```toml
ring = { version = "0.17", default-features = false, features = ["std"] }
blake3 = "1"
zstd = { version = "0.13", default-features = false }
zeroize = { version = "1", default-features = false }
```

- `ring` 同时提供 AES-256-GCM、Ed25519 和系统随机数，避免 RustCrypto 算法组合产生多套 trait
  与传递依赖；这里使用现代 0.17，不恢复旧 `ring 0.16`/Windows ARM64 链路；
- `blake3` 统一内容 ID、segment ID、fingerprint 和子密钥派生；
- `zstd` 只提供独立块 codec，不启用 legacy、dictionary builder 或 seekable layer；
- `zeroize` 没有传递依赖、FFI 或过程宏，只清理由 Hakutaku 自己持有的短期 key buffer。自己用
  unsafe volatile write 重造同一能力反而不符合 KISS；它不承诺阻止 live process dump：
  <https://docs.rs/zeroize/latest/zeroize/>。

`hakutaku-pack` 只额外依赖无默认 feature 的 `fastcdc`。并行打包使用 `std::thread::scope` 和
有界 channel，不引入 rayon/crossbeam。CLI 可以单独使用 `lexopt` 与 `walkdir`，但 Kēne 直接
调用 pack library，不把 CLI 依赖带进引擎。

明确禁止进入 runtime core：serde、bincode、postcard、anyhow、thiserror derive、tokio、tracing、
clap、rayon、crossbeam、lru、memmap2、S3/HTTP/TLS、多个压缩 codec。

依赖数不是高于安全和正确性的目标。v1 的 AES-256-GCM wire contract 不因平台改变；若未来替换
底层 crypto provider，新实现也必须读取相同字节并通过同一 test vector，不增加运行时算法协商。

## 发布事务

`cargo bundle` 的资源阶段：

1. 锁定目标发行目录并验证当前 `game.haku`；
2. staging 内容按规范路径排序并计算 source fingerprint；
3. fingerprint 未变则不生成新 segment；
4. 从旧快照的 ReusePage 构建临时 `chunk_id -> BlockRef` index；
5. 顺序写入只包含新块的 `.taku.part`；
6. 完成、sync 并计算完整 hash 后按 SegmentId 原子改名；
7. 生成包含完整当前文件树的新 `game.haku.part`；
8. 重新打开、验签、解密，并逐文件与 staging 对照；
9. sync 后原子替换 `game.haku`；
10. 新快照激活后才删除不再引用的旧 segment。

构建中断时旧快照仍只引用旧的完整 segment 集；孤立的新 segment 可在下次 bundle 安全识别和
复用或清理。

## 验收与基准

格式实现完成前必须固定真实 workload 定义，至少覆盖：

- 冷启动打开、验签、catalog 解密和 path lookup；
- 4 KiB 随机 read、64/256 KiB block miss、1 MiB 顺序吞吐；
- RAW 与 zstd block 的 decrypt/decompress/copy 分项成本；
- signed cipher digest 的单独吞吐与其在 RAW/zstd cache-miss 路径中的增量成本；
- 10 万文件、100 万 block 下的 catalog RSS、map-page cache RSS 与启动读取字节数；
- 同 segment 多线程 positioned read；
- 视频连续播放、随机 seek 和 read-ahead 命中；
- cache disabled、memory constrained、high throughput 三种预算；
- full -> incremental 的复用率、新增字节和 SteamPipe Preview；
- 256 MiB、512 MiB、1 GiB segment 策略；
- macOS Apple Silicon/Intel 与 Windows x86_64 的真实 SSD；
- Android arm64 的真实 UFS 设备与 iOS arm64 真机，覆盖冷启动、视频 seek、低内存和温升；
- 移动端 `Package::trim()`、进入后台/恢复、asset pack location 变化和 deferred segment 下载；
- hardened 与普通 release 的启动耗时、常驻线程数和 idle CPU 差异；
- 打包时顺序写吞吐、峰值内存和 crash recovery。

性能验收不使用帧率节流替代运行时成本。结果写入
`dev/docs/performance-baseline.md`，块大小、read coalescing 阈值、cache admission 和 segment
target 必须由基准决定，不能因为某个 SSD 型号的宣传参数直接定稿。

正确性测试至少包含：

- parser fuzz 与所有长度/offset/count 上限；
- header、ciphertext、tag、signature、segment、block、catalog 和 page 定点篡改；
- 使用正确 content root key 修改 block 并重算 GCM tag，仍必须因 signed cipher digest 被拒绝；
- 交换/截断 BlockMapPage、ReusePage 或替换其 directory digest 必须在对应信任边界失败；
- missing/truncated/swapped block 与错误 key；
- path traversal、重复路径、非规范路径和 hash collision fallback；
- 增加、修改、删除、重命名资源后的完整逻辑树；
- 无变化构建不产生新文件；
- bundle 每个事务点强制终止后旧版本仍可启动；
- 移动更新每个下载/校验/激活步骤被系统杀死后，旧快照仍可读取且不会接受半文件；
- 相同 fixture 在 desktop directory、Android asset descriptor 和 Apple asset descriptor adapter 下
  得到完全一致的文件内容与错误分类。

## 实现状态与迁移顺序

1. **已完成：**在独立 Cargo workspace 冻结 v1 wire spec、严格 parser 和格式边界；
2. **已完成：**只读 core、AEAD/signature mutation tests、`Asset::read_at` 与流式 cursor；
3. **已完成：**full/incremental packer、基于旧 snapshot 的 block reuse、CLI 与独立 GUI；
4. **待迁移：**用 Kēne 真实 staging 和原生视频后端建立前后基准；
5. **待迁移：**一次性将 Kēne loader/package 切到 Hakutaku，重新生成 fixture；
6. **已迁移：**删除 `hexz_k`、Hexz patches、`.hxz` 回退和旧文档，不保留双实现；
7. **移动端阶段：**实现 Android/iOS 的薄 `SegmentSource` adapter、资源预算和生命周期测试，不复制
   core reader；
8. **最终验收：**通过完整 workspace gate 和桌面/移动 GUI、视频验收后发布独立仓库。
