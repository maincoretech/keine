# 架构与数据合同

```text
keine/
├── crates/
│   ├── core/        Bevy-free：typed Action / Program / State、表达式、确定性执行
│   ├── loader/      Bevy-free：asset / editor / script / store adapters、内容与诊断
│   ├── authoring/   Bevy-free：Editor–Engine 二进制协议
│   ├── editor/      GPUI 工作台、源文档、文件与资源、Preview 客户端
│   └── media/       有界原生 WebP 解码
├── src/
│   ├── runtime/     启动、宿主、输入、脚本驱动、Preview 生命周期
│   ├── scene/       图片、立绘、音视频、特效
│   ├── render/      render-world 管线
│   ├── ui/          固定 MainCore UI、舞台、覆盖层
│   ├── storage/     存档、设置、profile、gallery、history、backup
│   ├── migration/   publisher-only 工程转换与资源重映射
│   └── assets/      内嵌字体、音效、shader、图标与品牌图片
├── tests/           集成回归、bench、fuzz、视频验收与 fixtures
├── dev/             文档与构建/fixture 脚本
└── projects/        忽略的本机作者工程
```

依赖方向为 `core ← loader ← engine`。Runtime 只消费 typed `Program` / `State`，
不解析 Editor JSON、工程兼容字段或包格式细节。外部编辑器逻辑留在 editor adapter。
已有 Rust 模块/API 名保持兼容，文件按目录职责组织；不新增通用 backend、主题或插件框架。

## 内容与资源

- 挂载为只读、有序的 `ContentMount` / `ContentFile`；后挂载覆盖先挂载。
- 路径必须受挂载根目录约束；绝对路径、`..` 逃逸、符号链接逃逸和发行 special files 被拒绝。
- 文件打开不能只依赖先前的 canonical 路径检查：Unix 逐级按目录句柄打开并拒绝新出现的符号链接；Windows 在读取前核对已打开句柄的实际路径。目录内合法链接仍可使用。
- 工程所有资源引用在 Loader 中解析；运行时不会依赖当前工作目录寻找工程资源。
- 当前画面资源优先加载；普通预取只保留前 8 个不同资源，最多 1 个投机加载。
- 非关键预取另有 128 MiB 保留预算，按图片解码数据/纹理 payload 估算和音频内存字节计费，流式 Opus 不计整文件；按 urgent/预测顺序淘汰，当前显示与作者显式 blocking 资源不淘汰。尺寸未确认时暂停追加加载，超限项在当前计划中延后。
- 视频独立流式播放；挂载中的 Opus 通过可重开的内容流解码。
- PNG/JPEG、WAV/MP3/Vorbis/FLAC 是开发兼容输入；正式打包拒绝非 WebP/Opus 独立媒体。

## 渲染与生命周期

内建动画预设的纯 transform 计算归 `crates/core/src/runtime/animation.rs`；
Action 入场初态与 Engine 每帧插值共用同一规则，计时、完成移除和渲染仍由各自生命周期负责。

1920×1080 是唯一设计空间。viewport/letterbox 转换只有一个 owner。
scene、normal UI、dialog camera 职责固定，特效不得改变合成顺序。
Editor 通过显式开始/停止控制独立原生 Engine；Block 与执行位置双向同步。
失去窗口焦点不会代替停止指令。

## 持久化

Save v11 只恢复到 fingerprint 相同的 Program；不支持的二进制布局直接拒绝。
profile、read history、gallery、settings 不随 slot rollback 回滚。
发行数据写入稳定 `project.id` 对应的平台 user-data 目录；不写到只读 bundle 旁。
Preview 使用子进程临时数据根，子进程退出后清理。

读写共用大小上限；读取先检查长度并使用 bounded read，写入先编码验证再原子替换。
Editor 文档/恢复草稿及 Loader 作者清单使用限量读取；Hakutaku 配置与目录配置共用 256 KiB 上限，编译文件先验证固定头中的 metadata/payload 长度，再分配、读取正文。
Save 与独立持久化数据均拒绝 postcard 尾部多余字节；设置中的非有限数回到默认值。
Backup 按实际读取字节限制总量；拒绝路径/设备名、重复及大小写冲突，并在未提交目录中独占创建文件，避免临时名与文件系统别名覆盖。
解码按 [postcard 剩余字节接口](https://docs.rs/postcard/1.1.3/postcard/fn.take_from_bytes.html) 检查完整消费；跨平台文件名约束依据 [Windows 命名规则](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)。
Backup import 同步待提交目录后切换正式目录；publisher 在安装失败时恢复旧目录，
提交后的旧副本清理失败报告 warning。源文档保存另检查外部修改；保存/刷新完成后的草稿清理失败只报告 warning。
Backup 有启动恢复；publisher 两次 rename 之间被强制终止时，旧发布可能仍在 `.名称.backup-PID`，需手动恢复。现有测试不保证真实断电下的全平台持久性。

## 编译与发行格式

```text
release/
├── keine[.exe]        hardened runtime
├── game.haku          完整、签名、加密的 Hakutaku v1 快照
├── LICENSE / NOTICE   引擎许可与署名
├── FONT-LICENSES.txt  内嵌字体许可
└── data/*.taku        不可变密文内容段
```

编译 Program envelope v1 / IR schema v7 保存 typed 内容；发行资源由 allowlist 重建。
包中不包含 `.shou`、WebGAL 脚本、LetsGal 作者工程或 publisher identity。
Hakutaku 的字节布局以锁定依赖自身 `FORMAT.md` 为准；Kēne 不维护第二套 wire spec。
每个文件直接引用物理块；快照更新复用旧块并原子切换。随机读取不先解密整包到磁盘。
签名和 AEAD 保证身份与完整性；离线内嵌密钥不承诺抵抗能调试客户端的攻击者。

## 必要限制

数值对应现有实现，改变上限须同时修改读写 owner 和边界回归。

| 内容 | 上限 |
|---|---:|
| 工程 `config.yaml` / Editor 单文档 | 256 KiB / 1 MiB |
| Loader 单源文件 | 32 MiB |
| WebP 压缩输入 / 源像素 / 输出像素 | 64 MiB / 67,108,864 / 16,777,216 |
| 视频 RGBA 队列 / 全局 surface-equivalent 预算 | 2 帧 / 256 MiB |
| 兼容音频 encoded input | 128 MiB |
| 编译 metadata / payload | 1 MiB / 512 MiB |
| 编译 scenes / actions | 1,000,000 / 100,000,000 |
| Save metadata / state | 64 KiB / 64 MiB |
| settings / gallery / profile / read history | 64 KiB / 16 MiB / 16 MiB / 64 MiB |
| Backup envelope / 文件数 / 单文件 | 128 MiB / 4,096 / 72 MiB |
| core 单次执行 / seek / blocking replay | 1,024 / 65,536 / 1,024 步 |

Hakutaku 另有 catalog、page、block、数量和路径限制，由锁定依赖分配前验证。
运行时使用 `memory_constrained()`：block-map cache 512 KiB、plaintext cache 16 MiB、
prefetch cache 512 KiB、4 个 idle handles。缓存预算不等于整个进程的内存上限。
128 MiB 预取预算也不包含当前画面、codec/GPU 开销、其他 owner 的强引用或淘汰前的一项在途解码；释放 cache handle 不代表资源立即从整个进程消失。
视频 surface 预算也不包含后端 codec working set。

实现入口：[`core`](../../crates/core/src/lib.rs)、[`loader`](../../crates/loader/src/lib.rs)、
[`compiled`](../../crates/loader/src/compiled.rs)、[`media`](../../crates/media/src/lib.rs)、
[`storage`](../../src/storage.rs)、[`publisher`](../../src/publisher.rs)。
