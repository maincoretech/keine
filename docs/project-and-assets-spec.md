# Kēne 原生项目与资源规范（v1 草案）

> 状态：草案。所有条目都以当前代码事实为准（`config.yaml` / `GameConfig` /
> `ResourceKind` / loader adapter / Hakutaku 打包），未实现的部分明确标注「预留」。
> 本规范是 LetsGal → Kēne 转换的目标形态，也是后续 demo 项目的落点。

## 0. 目的

1. 定义 Kēne 原生项目（Native Project）的目录布局与资源组织方式；
2. 为「LetsGal 资源完全转换为我们自己的资源」提供权威目标格式；
3. 为发布打包（`game.haku` + `data/`）与编译产物（`.keine/compiled/program.bin`）预留稳定路径；
4. 让脚本、配置、预取、图库与打包共用同一套资源语义，不依赖 LetsGal 专用字段。

## 1. 项目形态

Kēne 支持三种输入形态，本规范只定义后两种（原生形态），LetsGal 工程属于兼容输入：

| 形态 | 入口文件 | 说明 |
|---|---|---|
| LetsGal 工程 | `project.json` | 加载期由 letsgal editor adapter 在内存中转换，无需物理迁移 |
| Kēne 原生目录项目 | `config.yaml`（项目根） | 本规范定义的主要开发形态 |
| Kēne 打包项目 | `game.haku` 与同级 `data/` | 原生目录项目的发布形态 |

## 2. 项目目录规范（预留布局）

```text
<project>/
├── config.yaml              # 必需入口：项目根 = 含 config.yaml 的目录
├── assets/                  # 默认资产根（可在 config 中替换或叠加多层）
│   ├── background/          # 背景
│   ├── figure/              # 立绘
│   ├── vocal/               # 语音
│   ├── bgm/                 # 背景音乐
│   ├── se/                  # 音效
│   ├── particle/            # 粒子贴图
│   ├── video/               # 视频
│   ├── luts/                # 调色 LUT（WebP）
│   ├── font/                # 自定义字体（预留）
│   └── ui/                  # UI 图标 / 对话框皮肤（预留）
├── scripts/                 # 脚本场景（script: webgal 时放 .txt；结构化工程可留空）
├── .keine/                  # 引擎产物：program.bin 等（源项目永不打包；发布时在 staging 内重新生成并随包）
├── saves/                   # 开发运行用户数据；发行版改用平台用户数据目录（永不打包）
└── imported_assets/         # Bevy 生成缓存（永不打包，.gitignore 已排除）
```

### 2.1 目录是约定，不是硬约束

- 规范目录是默认回退路径（见第 4 节），也是转换工具的目标位置。
- 任何偏离规范目录的资产，必须通过 `config.yaml` 的 `assets` 别名表或脚本中的
  完整相对路径引用；否则引擎按默认回退路径解析会找不到。
- 目录可分层叠加：`adapter.asset` 声明多个 fs 来源，后者覆盖前者，
  覆盖粒度是逻辑相对路径（如 `background/day.webp`）。

### 2.2 保留名

| 名称 | 用途 | 约束 |
|---|---|---|
| `.keine/` | 引擎编译产物与内部缓存 | 源项目永不打包；发布脚本在 staging 内重新生成 `.keine/compiled/program.bin` 并随包发布（运行时契约） |
| `saves/` | 开发期存档、profile、阅读历史、图库 | 永不打包；发行版使用平台用户数据目录 |
| `imported_assets/`、`*.meta` | Bevy 资产处理器生成物 | 永不打包；不进 git |

## 3. 配置（config.yaml）

最小原生项目：

```yaml
title: "My Game"
project:
  id: my-game
  bundle_identifier: moe.example.my-game # 可省略；默认 moe.maincore.keine.my-game
  description: "A short visual novel."
adapter:
  asset:
    - path: "."
      format: fs
  script: webgal
  store: keine
assets:
  backgrounds:
    day: background/day.webp
  figures:
    aya: figure/aya_smile.webp
  voices:
    aya_01: vocal/aya_01.opus
  bgm:
    main: bgm/main.opus
  effects:
    click: se/click.opus
```

规则：

- `project.id` 是发行身份，不是显示名：只接受 1–64 字节的小写 ASCII 字母、数字和连字符，
  首字符必须是小写字母、末字符必须是字母或数字。开发运行可以暂时省略，`cargo assets
  --pack` 与 `cargo bundle` 必须提供；LetsGal 工程直接沿用 `project.json.id`。
- `project.bundle_identifier` 可选，必须是合法 reverse-DNS 标识；省略时从 `project.id`
  确定性派生。修改已经发行的 id 或 bundle identifier 会切换到另一套用户数据命名空间。
- 目录工程继续写 `<project>/saves`。Hakutaku 发行包保持只读，分别写入 macOS
  `~/Library/Application Support/<bundle-id>/saves`、Windows
  `%LOCALAPPDATA%/Kēne/<project-id>/saves`、Linux
  `$XDG_DATA_HOME/keine/<project-id>/saves`（默认 `$HOME/.local/share`）。
- 首次按新布局启动时，如果发行内容旁有旧版 `saves/` 且新目录尚未初始化，引擎会事务化复制
  一次；不会覆盖新目录，也不会删除旧副本。

- `adapter.asset` 声明顺序即层顺序，越靠后优先级越高；未声明等价于
  `[{ path: ".", format: "fs" }]`。
- `assets` 别名表（`backgrounds/figures/bgm/voices/effects/videos/luts`）
  是 **Kēne 原生项目的唯一权威别名来源**：逻辑名 → 相对路径。
- 脚本既可以直接写安全的、多段相对路径（`background/day.webp`），也可以写逻辑名
  （`day`，通过别名表或默认目录解析）。别名始终优先；显式路径会原样传给内容挂载层，
  不会再次添加类别目录。路径必须使用 `/`，且不能含空段、`.`、`..`、反斜杠或盘符。
- 未在别名表中的名称按第 4 节的默认回退目录解析。
- 目录工程始终解析源脚本；发布流水线在 staging 内生成
  `.keine/compiled/program.bin`，打包后的 `.haku` 缺少或损坏该产物时拒绝启动。

## 4. 资源类别规范

| 类别 | ResourceKind | 脚本入口 | 规范目录 | 兼容目录（adapter 接受） | 默认回退 |
|---|---|---|---|---|---|
| 背景 | Background | `ShowBg image` | `background/` | `backgrounds/`、`cg/` | `background/{name}` |
| 立绘 | Figure / MiniAvatar | `ShowSprite image` | `figure/` | `figures/`、`character/`、`characters/` | `figure/{name}` |
| 语音 | Voice | `Say vocal` / `Vocal` | `vocal/` | `voice/`、`voices/` | `vocal/{name}` |
| BGM | Bgm | `Bgm file` | `bgm/` | `bgm` | `bgm/{name}` |
| 音效 | Effect | `Effect file` | `se/` | `se`、`sound/`、`sounds/`、`effect/`、`effects/` | `se/{name}` |
| 粒子 | Particle | `ShowParticles texture` | `particle/` | 任意 | 路径原样使用 |
| 视频 | Video | `PlayVideo` | `video/` | `videos/` + 扩展名识别 | `video/{name}` |
| LUT | Lut | 后处理 preset / camera patch | `luts/` | `lut/`、`luts/` | `luts/{name}.webp` |
| 字体 | — | 预留 | `font/` | — | 内置 MavenPro-CJK |
| UI | — | 预留 | `ui/` | — | 内置 |

兼容与迁移细节：

- 显式音效路径（如 `se/click.opus`）无需登记别名；裸名称（如 `click.opus`）直接解析到
  `se/{name}`。其他位置必须使用显式路径或在 `assets.effects` 中登记别名。
- `cg/` 目录兼容背景：LetsGal 场景可用多个背景层，第一层走背景渲染器，其余层
  走通用 sprite 路径（同一资产同时登记为背景与立绘别名）。
- 运行时图片角色来自脚本编译产物中的 `ResourceKind`，不通过路径前缀猜测。因此
  `assets.figures.hero: art/hero.webp` 仍按立绘高度和真实宽高比加载，任意目录中的背景
  仍受背景解码上限约束。一个 WebP 同时用于多个角色时，启动阶段先合并其需求，再以
  所需目标中分辨率较大的一个只解码、上传一次；这避免 Bevy 同路径 loader settings 的
  “首次加载生效”语义造成不确定结果。
- 图库（`features.extra`）解锁来自实际播放过的场景背景，不需要单独的 `cg/`
  资源目录；`cg/` 仅是 LetsGal 兼容目录名。

## 5. 文件格式规范

| 类型 | 规范格式 | 兼容格式 | 说明 |
|---|---|---|---|
| 背景 / 立绘 / 粒子 | WebP | PNG、JPEG | WebP 是生产资源目标并走 libwebp 专用解码（支持解码期缩放）；PNG/JPEG 仅保留 Bevy 通用 loader 兼容能力 |
| LUT | WebP | PNG | LUT 是 `ResourceKind::Lut` 一等资源；有可见强度时参与静态校验与预取，所有 asset mount 内的 LUT 都受发布格式检查；默认回退路径为 `luts/{name}.webp` |
| 语音 / BGM / 音效 | Ogg Opus（`.opus`） | WAV、MP3、Vorbis（`.ogg/.oga/.spx`）、FLAC | 开发版保留兼容 decoder；发行引擎固定只带 `ui-sounds`/bundled Opus，不按项目扩展名扩张 feature set |
| 视频 | MP4（H.264） | WebM、MOV、MKV | FFmpeg 后端全格式；macOS native 后端走 AVFoundation（MP4/MOV） |
| 字体 | TTF / OTF | — | 目前内置字体随引擎打包，自定义字体路径为预留能力 |

设计空间固定为 1920×1080：

- 原生 YAML 与 project adapter 生成的 `GameConfig` 在进入 runtime 前使用同一个
  semantic validator；hot reload 也先验证新配置，失败时不替换当前配置。所有数值必须
  finite；percent/alpha 按实际范围校验；`sprite_height`、字体大小、时间与速度分别应用
  正值、上限或非负约束。允许负值的设计像素 offset 仅要求 finite；
- 背景建议源图 ≤ 4K 并尽量使用 WebP，避免首次切换大图卡顿。自定义 WebP loader
  接受最多 64 MiB 压缩输入、64 Mi pixels 源图和 16 Mi pixels 输出；背景角色的解码
  目标不超过 1920×1080，别名或自定义目录不会绕过该策略；
- 立绘为透明 WebP，站立高度按 `layout.sprite_height` 控制（原生项目默认；
  LetsGal 导入项目强制 1080 全高以保持 Studio 比例），运行时最高接受 4320；
- FFmpeg 视频帧单边不超过 4096、总像素不超过 4096×2304；AVFoundation 不使用
  这组 FFmpeg 常量；
- 必须整文件驻留以支持 seek/loop 的兼容音频最多 128 MiB；项目 Opus 继续流式读取；
- 构建期资源约束优先于运行时处理：过大图片在打包前统一缩放/转 WebP。
- `cargo assets --pack` 与 `cargo bundle` 只接受 WebP 项目图片（包括 LUT）和 `.opus`
  独立音频；兼容格式必须先在独立转换步骤生成目标文件，再用 `cargo assets --remap`
  迁移引用。打包器不会隐式转码，也不会把兼容 decoder 带入发行引擎。视频容器、字体和
  引擎内嵌图标不受这条媒体合同影响。

完整数值、适用 backend 与失败阶段见
[资源、发行包与持久化限制](resource-limits.md)。

## 6. 命名与路径约束

- 路径分隔符统一 `/`；Windows 反斜杠在入口处归一化，不进 IR。
- 禁止：绝对路径、`..` 越界、空段、以 `/` 开头的路径。
- 逻辑名（别名键）不含扩展名；文件路径含扩展名。
- 推荐：小写 ASCII（`a-z0-9_-`）；中文文件名允许（UTF-8），但跨平台打包时
  必须一致编码。
- Hakutaku 单条逻辑路径最多 65,535 UTF-8 bytes，完整 path pool 最多 32 MiB；当前
  没有单独的目录深度常量。
- `adapter.asset.path` 和每次文件系统读取都必须保留在 canonical project/mount root 内；
  `cargo bundle` 进一步拒绝所有 symlink 与 special file。
- 包体、compiled program 和媒体解码的其它硬上限见
  [资源、发行包与持久化限制](resource-limits.md)。

## 7. LetsGal → Kēne 转换规范

### 7.1 现状：加载期内存转换

`keine-loader` 的 letsgal editor adapter 已经能在加载时把 LetsGal 工程完整转换：

- 读取 `project.json`、`characters.json`、`chapters/*.json`、
  `assets/.manifest.json`、扩展壳的 `dialogue-box.json`；
- 按目录头分类资产：`background/backgrounds/cg` → 背景，
  `character(s)/figure(s)` → 立绘，`bgm` → BGM，`voice(s)/vocal` → 语音，
  `se/sound(s)/effect(s)` → 音效，`video(s)` 或视频扩展名 → 视频，
  `lut(s)` → LUT；
- 把 `assets/.manifest.json` 的逻辑名（hash）与路径同时登记进 config 别名表；
- 把对话行为、打字机、文字出现效果映射为 config styles；
- 章节 JSON 编译为统一 `Vec<Action>`（与 WebGAL 脚本同一 IR）。

结论：**运行 LetsGal 工程不需要先转换资源**。转换工具的价值是生成可脱离
LetsGal 长期维护的原生项目（config.yaml + 规范目录 + 脚本）。

### 7.2 目标：独立转换工具生成原生项目

形态：独立 CLI/脚本（非 LetsGal Studio 扩展；不注入、不依赖 Studio）。
转换只读源工程，写入新目录，绝不修改源工程。

输出目录：

```text
<output>/
├── config.yaml        # 生成：title/description/features/layout/styles/assets 表
├── assets/
│   ├── background/    # 来自 backgrounds/、cg/
│   ├── figure/        # 来自 characters/、figures/
│   ├── vocal/         # 来自 voices/、vocal/
│   ├── bgm/           # 来自 bgm/
│   ├── se/            # 来自 se/、sound(s)/、effect(s)/
│   ├── video/         # 来自 video(s)/ 或 *.mp4/*.webm/*.mov/*.mkv
│   └── luts/          # 来自 lut(s)/
└── scripts/           # 转换后的脚本（形态待 demo 确认；引擎 fs mount 固定挂载 scripts/）
```

转换步骤：

1. 枚举 `assets/.manifest.json` 全部条目，按第 7.1 节分类规则映射到规范目录；
2. 拷贝文件；可选转码：非 WebP 图片 → WebP、非 Opus 音频 → Opus（发布推荐，
   开发可跳过）；
3. 生成 `config.yaml`：`assets` 别名表由 manifest 条目翻译（逻辑名与路径双键），
   `layout`/`styles` 来自 Studio 默认壳配置，`features` 来自 `project.json` 的
   `keine` 段；
4. 转换章节为 `scripts/*.txt`（script: webgal）或结构化为编辑器工程（待定）；
5. 输出后必须通过 `cargo validate <output>` 才能算完成；
6. 校验项：所有脚本引用都能解析到文件、无重复 scene 名、资源类别归属正确。

### 7.3 边界

- 转换器不实现 LetsGal Studio 扩展、不走 TCP/localhost、不写回源工程；
- 转换后项目不再依赖 letsgal adapter 的字段模型（`chapterFolders`、
  `dialogue-box.json` 等），但允许保留 `.manifest.json` 作为过渡输入；
- 无法静态枚举的动态路径（如 `bg/{route}.webp`）保持动态引用，不强制物化。

## 8. 打包规范（Hakutaku）

- 打包输入 = 原生目录项目或 LetsGal 工程；输出 `config.yaml` 位于包根（LetsGal
  的适配器配置在打包时物化），`assets/` 暴露为 asset root，脚本在 `scripts/`
  （或编译产物）。
- 排除：`saves/`、`imported_assets/`、`*.meta`、`.DS_Store`、`.keine/` 之外
  的所有临时文件；`.keine/compiled/program.bin` 为预留编译产物（打包脚本写入，
  见 program.bin v2 设计）。
- 输出固定为签名 `game.haku` 快照和内容寻址 `data/*.taku` segment；按访问类型选择
  256 KiB streaming block、1 MiB bulk block 或 FastCDC，并用 zstd + AES-256-GCM。
- 运行时用 `ResourceBudget::memory_constrained()` 打开：16 MiB plaintext block cache，
  各 512 KiB map-page/prefetch cache、4 个 idle segment handles；这不是进程总内存预算。
- 单 snapshot 最多引用 128 segments、1,000,000 files 和 10,000,000 blocks；完整
  wire-format 与 packer 策略数值见
  [资源、发行包与持久化限制](resource-limits.md)。

## 9. 版本与演进

- 本规范版本 v1；目录、类别、格式的兼容集合冻结自当前 adapter 行为。
- 新增资源类别流程：`ResourceKind` 枚举 → 规范目录 → `config.assets` 表 →
  LetsGal 分类函数（`is_background` 等）→ 本规范表格，五处同步更新。
- 规范目录与兼容目录集合以本表为准；新兼容目录需先在 letsgal adapter 落地
  再回填本表。

## 10. 已确定的生产约束

1. 原生工程的 `scripts/` 使用 WebGAL `.txt`；LetsGal 结构化章节只由 project adapter 读取；
2. 字体与固定 UI 资源由引擎内置，当前不增加项目级主题或换肤目录；
3. 发行视频首选 MP4（H.264 + AAC），兼顾 AVFoundation 与 FFmpeg；
4. `cargo assets` 只负责打包和事务化引用重映射，不隐式决定 WebP 质量或 Opus 码率；转码参数属于创作工具输入；
5. 源工程始终以源码为权威；`cargo bundle` 在 staging 中重新生成
   `.keine/compiled/program.bin`，发行包只从该编译产物启动。
