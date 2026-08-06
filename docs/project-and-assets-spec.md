# Kēne 原生项目与资源规范（v1 草案）

> 状态：草案。所有条目都以当前代码事实为准（`config.yaml` / `GameConfig` /
> `ResourceKind` / loader adapter / hexz 打包），未实现的部分明确标注「预留」。
> 本规范是 LetsGal → Kēne 转换的目标形态，也是后续 demo 项目的落点。

## 0. 目的

1. 定义 Kēne 原生项目（Native Project）的目录布局与资源组织方式；
2. 为「LetsGal 资源完全转换为我们自己的资源」提供权威目标格式；
3. 为发布打包（`game.hxz`）与编译产物（`.keine/compiled/program.bin`）预留稳定路径；
4. 让脚本、配置、预取、图库与打包共用同一套资源语义，不依赖 LetsGal 专用字段。

## 1. 项目形态

Kēne 支持三种输入形态，本规范只定义后两种（原生形态），LetsGal 工程属于兼容输入：

| 形态 | 入口文件 | 说明 |
|---|---|---|
| LetsGal 工程 | `project.json` | 加载期由 letsgal editor adapter 在内存中转换，无需物理迁移 |
| Kēne 原生目录项目 | `config.yaml`（项目根） | 本规范定义的主要开发形态 |
| Kēne 打包项目 | `game.hxz`（包根含 `config.yaml`） | 原生目录项目的发布形态 |

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
│   ├── luts/                # 调色 LUT（PNG）
│   ├── font/                # 自定义字体（预留）
│   └── ui/                  # UI 图标 / 对话框皮肤（预留）
├── scenes/                  # 脚本场景（script: webgal 时放 .txt；结构化工程可留空）
├── .keine/                  # 引擎产物：compiled program.bin、缓存等（永不打包）
├── saves/                   # 运行时用户数据：slot_*.sav、profile.bin 等（永不打包）
└── imported_assets/         # Bevy 生成缓存（永不打包，.gitignore 已排除）
```

### 2.1 目录是约定，不是硬约束

- 规范目录是默认回退路径（见第 4 节），也是转换工具的目标位置。
- 任何偏离规范目录的资产，必须通过 `config.yaml` 的 `assets` 别名表或脚本中的
  完整相对路径引用；否则引擎按默认回退路径解析会找不到。
- 目录可分层叠加：`adapter.asset` 声明多个来源（fs / hexz），后者覆盖前者，
  覆盖粒度是逻辑相对路径（如 `background/day.webp`）。

### 2.2 保留名

| 名称 | 用途 | 约束 |
|---|---|---|
| `.keine/` | 引擎编译产物与内部缓存 | 永不打包；开发模式生成 |
| `saves/` | 存档、profile、阅读历史、图库 | 永不打包；打包脚本显式排除 |
| `imported_assets/`、`*.meta` | Bevy 资产处理器生成物 | 永不打包；不进 git |
| `.hexz/` | hexz pack 的 staging 目录 | 打包器自动跳过 |
| `memory`（项目根） | hexz 辅助流 | 保留给打包工具，业务层不使用 |

## 3. 配置（config.yaml）

最小原生项目：

```yaml
title: "My Game"
project:
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

- `adapter.asset` 声明顺序即层顺序，越靠后优先级越高；未声明等价于
  `[{ path: ".", format: "fs" }]`。
- `assets` 别名表（`backgrounds/figures/bgm/voices/effects/videos/luts`）
  是 **Kēne 原生项目的唯一权威别名来源**：逻辑名 → 相对路径。
- 脚本既可以直接写相对路径（`background/day.webp`），也可以写逻辑名
  （`day`，通过别名表解析）。
- 未在别名表中的名称按第 4 节的默认回退目录解析。

## 4. 资源类别规范

| 类别 | ResourceKind | 脚本入口 | 规范目录 | 兼容目录（adapter 接受） | 默认回退 |
|---|---|---|---|---|---|
| 背景 | Background | `ShowBg image` | `background/` | `backgrounds/`、`cg/` | `background/{name}` |
| 立绘 | Figure / MiniAvatar | `ShowSprite image` | `figure/` | `figures/`、`character/`、`characters/` | `figure/{name}` |
| 语音 | Voice | `Say vocal` / `Vocal` | `vocal/` | `voice/`、`voices/` | `vocal/{name}` |
| BGM | Bgm | `Bgm file` | `bgm/` | `bgm` | `bgm/{name}` |
| 音效 | Effect | `Effect file` | `se/` | `se`、`sound/`、`sounds/`、`effect/`、`effects/` | `vocal/{name}`（现状） |
| 粒子 | Particle | `ShowParticles texture` | `particle/` | 任意 | 路径原样使用 |
| 视频 | Video | `PlayVideo` | `video/` | `videos/` + 扩展名识别 | `video/{name}` |
| LUT | — | 后处理 preset | `luts/` | `lut/`、`luts/` | `luts/{name}.png` |
| 字体 | — | 预留 | `font/` | — | 内置 MavenPro-CJK |
| UI | — | 预留 | `ui/` | — | 内置 |

已知现状细节（规范不改变，只记录）：

- 音效的默认回退目前是 `vocal/{name}`，与规范目录 `se/` 不一致；使用 `se/` 的
  原生项目必须在 `assets.effects` 中登记别名。
- `cg/` 目录兼容背景：LetsGal 场景可用多个背景层，第一层走背景渲染器，其余层
  走通用 sprite 路径（同一资产同时登记为背景与立绘别名）。
- 图库（`features.extra`）解锁来自实际播放过的场景背景，不需要单独的 `cg/`
  资源目录；`cg/` 仅是 LetsGal 兼容目录名。

## 5. 文件格式规范

| 类型 | 规范格式 | 兼容格式 | 说明 |
|---|---|---|---|
| 背景 / 立绘 / 粒子 | WebP | PNG、JPEG | WebP 走 libwebp 专用解码（支持解码期缩放）；PNG/JPEG 走 Bevy 通用 loader（png/jpeg feature 已启用） |
| LUT | PNG | WebP | 默认回退路径为 `luts/{name}.png` |
| 语音 / BGM / 音效 | Ogg Opus（`.opus`） | WAV、MP3、Vorbis（`.ogg/.oga/.spx`）、FLAC | 默认 `bundled-opus` 静态 libopus；发布脚本按项目实际扩展名自动裁剪 audio features |
| 视频 | MP4（H.264） | WebM、MOV、MKV | FFmpeg 后端全格式；macOS native 后端走 AVFoundation（MP4/MOV） |
| 字体 | TTF / OTF | — | 目前内置字体随引擎打包，自定义字体路径为预留能力 |

设计空间固定为 1920×1080：

- 背景建议源图 ≤ 4K 并尽量使用 WebP，避免首次切换大图卡顿；
- 立绘为透明 WebP，站立高度按 `layout.sprite_height` 控制（原生项目默认；
  LetsGal 导入项目强制 1080 全高以保持 Studio 比例）；
- 构建期资源约束优先于运行时处理：过大图片在打包前统一缩放/转 WebP。

## 6. 命名与路径约束

- 路径分隔符统一 `/`；Windows 反斜杠在入口处归一化，不进 IR。
- 禁止：绝对路径、`..` 越界、空段、以 `/` 开头的路径。
- 逻辑名（别名键）不含扩展名；文件路径含扩展名。
- 推荐：小写 ASCII（`a-z0-9_-`）；中文文件名允许（UTF-8），但跨平台打包时
  必须一致编码。
- 单文件大小与目录深度按打包器上限约束（program.bin 等编译产物另有独立上限）。

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
└── scenes/            # 转换后的脚本（形态待 demo 确认）
```

转换步骤：

1. 枚举 `assets/.manifest.json` 全部条目，按第 7.1 节分类规则映射到规范目录；
2. 拷贝文件；可选转码：非 WebP 图片 → WebP、非 Opus 音频 → Opus（发布推荐，
   开发可跳过）；
3. 生成 `config.yaml`：`assets` 别名表由 manifest 条目翻译（逻辑名与路径双键），
   `layout`/`styles` 来自 Studio 默认壳配置，`features` 来自 `project.json` 的
   `keine` 段；
4. 转换章节为 `scenes/*.txt`（script: webgal）或结构化为编辑器工程（待定）；
5. 输出后必须通过 `cargo validate <output>` 才能算完成；
6. 校验项：所有脚本引用都能解析到文件、无重复 scene 名、资源类别归属正确。

### 7.3 边界

- 转换器不实现 LetsGal Studio 扩展、不走 TCP/localhost、不写回源工程；
- 转换后项目不再依赖 letsgal adapter 的字段模型（`chapterFolders`、
  `dialogue-box.json` 等），但允许保留 `.manifest.json` 作为过渡输入；
- 无法静态枚举的动态路径（如 `bg/{route}.webp`）保持动态引用，不强制物化。

## 8. 打包规范（game.hxz）

- 打包输入 = 原生目录项目；输出 `config.yaml` 位于包根，`assets/` 暴露为
  asset root，脚本在 `scenes/`（或编译产物）。
- 排除：`saves/`、`imported_assets/`、`*.meta`、`.DS_Store`、`.keine/` 之外
  的所有临时文件；`.keine/compiled/program.bin` 为预留编译产物（打包脚本写入，
  见 program.bin v2 设计）。
- 容器参数固定：64 KiB block、zstd、AES-256-GCM 加密（`HEXZ_PASSWORD`）。
- 运行时用 `memory_constrained()`（约 16 MiB 解压块缓存）打开，包句柄全周期保持。

## 9. 版本与演进

- 本规范版本 v1；目录、类别、格式的兼容集合冻结自当前 adapter 行为。
- 新增资源类别流程：`ResourceKind` 枚举 → 规范目录 → `config.assets` 表 →
  LetsGal 分类函数（`is_background` 等）→ 本规范表格，五处同步更新。
- 规范目录与兼容目录集合以本表为准；新兼容目录需先在 letsgal adapter 落地
  再回填本表。

## 10. 待定项（demo 项目引入后确认）

1. `scenes/` 的脚本形态：WebGAL `.txt` 还是结构化 JSON（LetsGal 章节内联）；
2. `font/` 与 `ui/` 的实际需求（自定义字体目前仅内置）；
3. 视频编解码默认组合（H.264 vs VP9，依赖目标平台视频后端）；
4. 转换工具的转码选项默认值（WebP 质量、Opus 码率）；
5. `.keine/compiled/program.bin` 与源脚本并存时的优先级细节。
