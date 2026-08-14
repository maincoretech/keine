# Hakutaku 打包与挂载

> 状态：Kēne 直接固定依赖 `maincoretech/hakutaku`。不保留 Hexz 回退或兼容层。

## 发布布局

`cargo bundle <project>` 生成：

```text
release/
├── keine[.exe]
├── game.haku
├── data/
│   └── <content-id>.taku
└── run.sh | run.bat
```

`game.haku` 是签名快照与加密目录，`data/*.taku` 是不可变、内容寻址的加密 segment。
更新时 Kēne 先将上一版快照和 segment 以硬链接（失败时复制）放进临时发布目录，Hakutaku
只写新增块，并在新快照提交后清理未引用 segment。最终目录仍通过同目录 rename 事务发布。

## 发布身份

首次本地打包会创建 `.keine/publisher.hakutaku-key`。它同时决定项目 ID、AES 根密钥和
Ed25519 发布身份，必须备份且不得随游戏发布。也可用 `KEINE_HAKUTAKU_IDENTITY` 指向外部
身份文件；CI 使用 base64 的 `HAKUTAKU_IDENTITY_BASE64` secret 还原同一身份。

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
