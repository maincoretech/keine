# Hakutaku 打包与挂载

> 状态：Kēne 直接固定依赖 `maincoretech/hakutaku`，发行路径只有 Hakutaku v1。

## 命令边界

`cargo assets --pack <project>` 只编译项目内容并生成 Hakutaku 资源包：

```text
target/package/
├── game.haku
└── data/
    └── <content-id>.taku
```

它不生成运行时 key shares、不调用 Cargo 构建引擎，也不复制可执行文件、图标或平台动态库。
`cargo assets --remap` 只负责事务化迁移项目内的资源引用。`cargo bundle <project>` 才复用
资源打包阶段、构建与身份匹配的 hardened 引擎，并组装完整可运行发行目录；benchmark 也只属于
bundle。

两个发布入口共用 canonical media gate：asset mount 内的项目图片（背景、立绘、粒子和
LUT）必须是 WebP，独立语音/BGM/音效必须是 Ogg Opus `.opus`。PNG/JPEG、WAV、MP3、
Vorbis 和 FLAC 只属于开发/导入兼容路径；打包器发现它们会直接报错，不隐式转码，也不
据此扩大 shipping engine 的 Cargo features。发行引擎固定启用 `ui-sounds`（bundled Opus），
再按项目是否包含视频选择当前平台 video backend。

独立资源包仍绑定 publisher identity；它只能由嵌入同一 identity 运行时材料的发行引擎打开，
并不是可由普通开发版引擎通用加载的无密钥归档。

## 发布布局

`cargo bundle <project>` 生成：

```text
release/
├── keine[.exe]
├── game.haku
└── data/
    └── <content-id>.taku
```

`game.haku` 是签名快照与加密目录，`data/*.taku` 是不可变、内容寻址的加密 segment。
发行物不包含启动脚本：Windows 将 FFmpeg DLL 放在程序目录，Linux 视频发行版则在引擎的
ELF `DT_RPATH` 中固定 `$ORIGIN/lib`，因此可以从任意工作目录直接运行引擎。
更新时 Kēne 先将上一版快照和 segment 以硬链接（失败时复制）放进临时发布目录，Hakutaku
只写新增块，并在新快照提交后清理未引用 segment。最终目录仍通过同目录 rename 事务发布。

Hakutaku 内容目录不是持久化目录。发行项目必须声明稳定、路径安全的 `project.id`；运行时
在 macOS Application Support、Windows LocalAppData 或 Linux XDG data home 中建立独立
命名空间，`.app/Contents/Resources` 和普通 release 目录保持只读。首次升级会保守复制旧
sidecar `saves/`，但已有新目录永远优先，旧副本不会自动删除。

## 发布身份

首次本地打包会创建 `.keine/publisher.hakutaku-key`。它同时决定 Hakutaku archive ID、AES
根密钥和 Ed25519 发布身份；archive ID 与配置中用于存档命名空间的 `project.id` 是两个不同
合同。私钥必须备份且不得随游戏发布。也可用 `KEINE_HAKUTAKU_IDENTITY` 指向外部
身份文件；CI 使用 base64 的 `HAKUTAKU_IDENTITY_BASE64` secret 还原同一身份。

GitHub 手动构建明确分成两种身份模式：

- `temporary`（默认）：每个平台 runner 在其受限临时目录创建独立 identity，完成后删除；适合
  fork、`test-project`、验收和 benchmark，不承诺跨平台或跨构建更新兼容；
- `stable`：从仓库 Secret `HAKUTAKU_IDENTITY_BASE64` 恢复同一 identity；只用于正式发行和
  后续更新。缺少 Secret 时在 checkout、工具链和媒体依赖安装之前失败。

正式项目的 identity 在可信开发机首次执行 `cargo assets --pack <project>` 时自动生成。可用
`openssl base64 -A -in <project>/.keine/publisher.hakutaku-key` 转成单行文本并添加到
**Settings → Secrets and variables → Actions → Repository secrets**。Windows PowerShell 等价命令为
`[Convert]::ToBase64String([IO.File]::ReadAllBytes('<path>'))`。这些输出都是完整私钥，不能进入
Git、workflow 输入、日志或 Actions artifact。fork 不继承上游 Secret，必须使用 temporary 或
配置自己的 stable identity。

打包器为每次引擎构建产生随机 XOR key shares。loader build script 分别嵌入两份 share 和
发布公钥，运行时才重组 AES-256 根密钥。这样不会让完整密钥以连续常量落入二进制，但离线
客户端中的密钥仍可能被逆向提取；这是分发保护，不是不可破解的 DRM。

## 运行时读取

loader 打开快照时一次建立文件集合和排序的 parent → direct children 目录索引；后续
`read_directory` 只克隆目标目录的直接子项，不随包内总文件数线性扫描。`ContentFile`
直接包装 Hakutaku `AssetCursor`，
图片、音频、视频和 Bevy `AssetReader` 共用同一个随机读取实现：

- 先验证签名目录，再按需验证并解密 page/block；
- Streaming/Transient 块保留独占明文 buffer，不进入全局缓存；
- Hot 块直接进入有界 CLOCK cache，Normal 块二次访问后才晋升；
- 视频游标保留当前和前一个 Streaming 解密块，不再由 Kēne 叠加第二层 read-ahead；
- FS 来源仍把真实路径交给原生视频后端，Hakutaku 来源通过同一个 `Read + Seek` byte source。

完整格式、安全边界和更新模型见 [10-hakutaku-format.md](10-hakutaku-format.md)。
当前 Kēne/Hakutaku 的格式硬上限、打包滚段策略、运行时 cache budget 和媒体解码边界统一记录在
[资源、发行包与持久化限制](../../../docs/resource-limits.md)；其中 cache budget 只约束可重建状态，
不能作为进程总内存或最大包体积解读。
