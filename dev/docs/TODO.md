# keine TODO

> 对齐 WebGAL 脚本标准。当前事实以
> [`webgal-compatibility/semantic-matrix.md`](webgal-compatibility/semantic-matrix.md) 为准；
> `10-webgal-k-gap-analysis.md` 仅保留为 0.2.0 历史快照。勾选表示端到端可用，而不只是能够解析。

---

## 当前优先级

1. **桌面视频后端** — macOS AVFoundation 已接入，继续 Windows Media Foundation 与 Hexz byte source；Linux 保留 FFmpeg
2. **跨平台视觉基线** — 在 Linux/Windows、1× DPI、超宽/高窗口建立独立 screenshot/golden

## WebGAL 4.6.2 兼容性审计与内部格式 (DONE)

- [x] 按官方 4.6.2 文档与命令注册表逐项审计 31 个命令：已实现 5、部分支持 23、不支持 3
- [x] 对不支持的 `showVars/applyStyle/callSteam` 产生可定位 warning，不退化为对白或伪成功
- [x] WebGAL continuation、quoted `-when`、嵌套数组 Choice、默认 transform target 与显式零时长回归
- [x] 不可变 `Program`：packed Action、label 预索引、`Arc` 共享；`step()` 借用 Action，存档不再携带脚本
- [x] 稀疏 32-byte `TransformPatch`，区分未出现字段与显式 0，避免连续演出重置状态
- [x] 存档绑定 Program fingerprint：不匹配时原子拒绝；匹配时夹紧 cursor、清理 frame/Backlog、回到最近有效 caller 或安全结束
- [x] profile/read/gallery 与单槽 save/rollback 分离；CLEAR ALL 同步清理磁盘、运行态和 writer cache
- [x] FS 递归扫描阻止 symlink cycle/根逃逸；Hexz 复用一次 archive index，避免目录级重复全包扫描
- [x] 修复 Choice 首帧误确认、启动 Enter 穿透与 mini avatar 遮挡正文
- [x] 独立报告目录 `dev/docs/webgal-compatibility/`：语义矩阵、缺口、视觉边界、内部格式与复现入口

## Phase 0 — Bevy 引擎基础 (DONE)

- [x] Bevy 0.19 三相机分层（场景 + 普通 UI + Dialog）
- [x] GPU 高斯模糊后处理（WGSL）
- [x] 背景 + 立绘渲染 + letterbox
- [x] 文本框 + 名字栏 + 控制栏（WebGAL 布局）
- [x] MavenPro + HanaMin 日文假名并集字体（format 12 cmap）与全局黑色文字背景阴影
- [x] Bootstrap Icons 图标 + hover 动画 + toggle 状态机
- [x] 打字机逐字显示 + 鼠标/键盘推进
- [x] 打字机首字符零延迟，并通过版本化设置淘汰 HiDPI 滑杆 bug 写入的异常低速值

## Phase 1 — 脚本引擎 & 核心命令 (DONE — 待用户验收)

- [x] say — 基础对话 + speaker
- [x] say — `-vocal/-volume/-notend/-concat/-next/-when`、插值与富文本可读降级
- [x] changeBg — 基础背景切换
- [x] changeBg — WebGAL 参数、none、transform/filter
- [x] changeFigure — 基础立绘入场/退场（left/center/right）
- [x] changeFigure — 自定义 id/free figure/zIndex/初始 transform/动画参数；blend 元数据保留
- [x] choose — 脚本解析、核心等待状态与分支跳转
- [x] choose — 桌面 Bevy 选项面板、局部模糊、鼠标/键盘选择与恢复推进
- [x] choose — 场景/callScene 目标与条件显示/启用
- [x] label / jumpLabel — 跳转
- [x] changeScene / callScene — WebGAL 解析、场景切换、嵌套调用与自然返回
- [x] end — 核心流程终止并清空场景栈
- [x] end — 重置舞台并返回标题 UI
- [x] setVar — WebGAL parser、表达式、数组与全局变量
- [x] setTransform — 立绘 offset/alpha/scale/rotation 基础渲染
- [x] setTransform — duration/easing/background/filter；接通 blur 渲染
- [x] 公共参数：`-when/-next/-notend` 与执行循环上限
- [x] 变量插值：content 与 args 中的 `{variable}`
- [x] parser source span、明确错误诊断、资源/子场景扫描
- [x] callScene/changeScene 一跳资源预取，避免首次进入子场景时同步等待

验收入口：`projects/test-project`。详细步骤见
[`../../projects/test-project/ACCEPTANCE.md`](../../projects/test-project/ACCEPTANCE.md)。实际富文本 span/ruby 排版与
multiply/screen/add 合成属于演出渲染，不以“已解析”冒充完成，分别留在 Phase 6/5。

## Phase 2 — 控制栏 (DONE)

- [x] Auto / Skip / Hide / Lock toggle；常用操作统一为 Ctrl 组合键，Esc 统一返回
- [x] Q·SAVE / Q·LOAD（Postcard 版本化存档、确认 Dialog、原版比例 hover 舞台快照）
- [x] Q·SAVE / Q·LOAD 紧凑 hover 浮窗、独立增强模糊与快速淡入淡出
- [x] Hide 自动隐藏动画（内容/按钮/单图标）
- [x] Lock 锁/开锁图标切换

## Phase 2.5 — 架构重构与质量基线 (DONE)

- [x] `main.rs` 仅保留入口，启动/相机配置拆到 `app.rs`
- [x] 移除 `Arc<RwLock<State>>`，使用 ECS 独占资源直接管理游戏状态
- [x] 三相机固定职责：场景、普通 UI、Dialog
- [x] 统一 `DesignViewport`，集中处理设计分辨率与 letterbox 坐标换算
- [x] 背景和立绘改为增量同步，避免每帧全量重建实体
- [x] 项目加载顺序稳定，并拒绝重复场景名
- [x] ScriptWatcher 持有 watcher 生命周期，脚本修改后真实重载
- [x] 三 crate 按稳定职责重组：core `model/runtime`、loader `adapter/loader`、Bevy `runtime/scene/storage/ui`
- [x] 根 package 直接产出 `keine` 引擎二进制，内部库目录收敛为 `crates/core` 与 `crates/loader`
- [x] 验收项目收敛为唯一、自包含的 `projects/test-project`，移除旧空壳、无效脚本、生成物和未使用配置/依赖
- [x] WebGAL 语法、语言类型与解析实现集中到 `crates/loader/src/adapter/`
- [x] 合并短生命周期模块：core transition math、loader watcher、Bevy plugin/runtime 注册
- [x] 文本框系统按职责拆分，Dialog 模糊、专用样式与键盘操作完成实际交互验证
- [x] 存档 API 返回 `Result`，使用临时文件 + rename 原子替换
- [x] Rust 2024、格式检查、严格 Clippy，并在 Phase 2.5 当时建立 51 项测试质量基线（当前无默认功能全量为 217 项）
- [x] UI 公共字体、文本原语、菜单顶栏与淡入/模糊生命周期集中化；CONFIG 控件改为数据表驱动
- [x] 全仓目录收敛：领域入口统一为同名 `.rs` 门面并清零 `mod.rs`；storage、UI support、
  scene effects 与 adapter 按执行机制放入同名目录，FS/Hexz 等紧密实现保持同文件；内嵌资源
  集中到 `src/assets/`，Phase 1–7 验收文档合一。结构不再以最低文件数为目标，而以稳定边界、
  易定位和避免巨型入口为准

## Phase 3 — 控制栏扩展 (DONE — 待用户验收)

- [x] Backlog 轻量快照（舞台、变量、scene stack、cursor）与 200 条容量上限
- [x] 已读历史持久化 + Skip All/Read 语义（默认 Read，`Shift+S` 切换模式）
- [x] WebGAL K 风格 Backlog UI、反向滚动、语音重播、回想跳转与进出动画
- [x] WebGAL K 风格 SAVE / LOAD：20 页 × 10 槽、空槽保护、覆盖/读取确认
- [x] 槽位 WebP 截图、角色/文本/时间预览、分页与右键删除确认
- [x] v9 专用二进制存档：元数据前置、状态载荷分离、metadata/state 双 CRC32、只读前缀槽位检查、Program fingerprint、多粒子发射器状态、完整相机与句尾退格状态、`SavedState` 恢复边界、固定 golden、原子替换与独立 WebP 预览
- [x] SYSTEM 设置面板（主音量、文字速度、Auto 延迟、Skip 模式与本地持久化）
- [x] TITLE 返回标题画面
- [x] 对齐 WebGAL K 的菜单模糊、分页/卡片 hover、0.2 秒页面过渡、Backlog 退出与标题按钮反馈
- [x] SAVE / LOAD / CONFIG 共用同尺寸壳层与 blur 生命周期，存档预览有界缓存，CONFIG 水印置于右下角模糊底层
- [x] 所有 UI/Dialog 锁定舞台 16:9 viewport；悬浮层统一淡出 textbox/namebar；存档分页使用紧凑大按钮与中点换页淡出
- [x] CONFIG 常驻隐藏以消除重复入场重建；槽位 Dialog 同帧切换渲染层，空存档使用淡色整块卡片
- [x] Backlog 退出同步衰减文字阴影并统一释放时序；资源与 UI 字体未加载完成时锁定按键、指针和滚轮
- [x] 悬浮层同步衰减 textbox 文字阴影；SAVE/LOAD 取消槽位交错入场，存档后留在当前页并即时刷新信息/预览
- [x] 全屏菜单统一增强至 48 强度高斯模糊，并加深 SAVE / LOAD / CONFIG / Backlog 暗色层
- [x] CONFIG Dialog 模态层级、K 风格滑杆/选项布局、TITLE 轻量 hover 与区域模糊像素边界修正
- [x] CONFIG 15%/85% 原版页面结构、紧凑顶栏、深色高对比模糊层与 TITLE 可见按压反馈
- [x] CONFIG 对齐 K 的完整三页控件密度（全屏、字号、透明度、五路音量与文本预览）并持久化
- [x] TITLE 按钮整块缩放、原版宽度收缩曲线与 CONTINUE 预览顶部模糊越界修正
- [x] UI 稳态性能收尾：隐藏 CONFIG 停止更新、滑杆松手落盘、Backlog 有界入场、存档预览异步解码，并为 textbox/TITLE/SAVE 动画增加变更检测
- [x] CONFIG 全控件与 blur 同步淡入淡出、SL→CONFIG 无白闪交接；全局文字使用四向低成本字形扩散阴影
- [x] GAL 生命周期调度：活动态按显示器自然刷新，稳态事件驱动休眠；Release 失焦暂停，
  dev 失焦持续轮询 Studio/脚本变化并实时同步 textbox；剧情与 UI 动画不绑定固定帧率
- [x] 稳态渲染收敛：背景、立绘、资源预取按各自渲染快照更新，打字机进度不再触发舞台重建；纹理只在载入/重载时准备并上传，稳定实体原地更新组件；隐藏/零强度 blur 跳过并复用区域缓冲
- [x] UI 绘制收敛：全局扩散文字阴影由 8 次采样压缩为 4 次双线性对角采样，关闭的 Extra 页面不进入更新链
- [x] 1080p 设计与图片内存管线：逻辑画布固定 1920×1080，原生 libwebp 在解码阶段直出目标尺寸，背景纹理不超过画布、立绘按设计高度；GPU 上传后释放 CPU 像素，资源离开预取窗口即卸载
- [x] SL 分页彻底移除黑幕并改为槽位内容自身淡出/淡入；CONFIG 预览设置不再穿透显示舞台 textbox；TITLE 预览与按钮缩进解耦并移除左边框
- [x] 集中式 UI 输入 scope（Loading/Dialog/Menu/Backlog/Title/Stage）阻止快捷键穿透；SL 子页与 SL↔CONFIG 使用连续 blur 和内容淡入淡出交接
- [x] TITLE 统一低透明黑色可交互按钮与低透明灰色禁用按钮；CONFIG 导航对齐内容垂直节奏；SL 分页采用方向滑动并收敛 UI 动画为帧率无关时间函数
- [x] SL 页码栏与滑动槽位网格解耦；SAVE/LOAD/CONFIG 复用持久全屏 blur；未开始游戏禁止存档，无快速存档时禁用带 blur 的 CONTINUE
- [x] SAVE / LOAD / CONFIG 固定顶栏下改为连续全宽滑轨：出入页并存、页码即时切换、动画结束后释放旧页，且所有位移按真实时间推进
- [x] 按 WebGAL_K Options CSS 细粒度对齐 CONFIG hover：页面文字 0.175/0.5/0.8，NormalButton 文字 0.376/0.667 与 0.188 浅白横向填充，并修正 0.2 秒左上入场方向
- [x] SL↔CONFIG 使用单一共享暗幕、缓存布局宽度与无越界 cubic 加减速曲线，消除双层黑幕闪帧；通用按钮补齐按下缩小与平滑复位反馈

## Phase 4 — 音频 (DONE)

- [x] bgm — 循环背景音乐、毫秒级 `-enter` 淡入/淡出、单曲 volume 与读档/回滚恢复
- [x] vocal — 本地语音播放、`-vocal` 简写、单句 volume 与 vocal 分总线
- [x] playEffect — 一次性音效、volume、带 id 循环/替换/停止与存档恢复
- [x] Replay — 控制栏当前语音重播与 Backlog 任意语音重播
- [x] 主音量/BGM/vocal/SE 分总线实时更新；淡入生命周期保持事件驱动调度活跃
- [x] 推荐 Ogg Opus 发行格式：BGM/vocal/SE/UI 共用加载入口和增量 Opus 解码器，同时兼容 WAV/MP3/Vorbis/FLAC
- [x] WebGAL K UI 提示音：hover/click/switch 无损 remux 为内嵌 Opus，单通道替换播放并服从 UI 音量总线

验收入口与逐步预期见 [`phases.md`](acceptance/phases.md)。

## Phase 5 — 演出 (DONE — 待用户验收)

- [x] setAnimation — 帧率无关预制动画（enter/exit/shake/方向入场/缩放/blur/film）
- [x] setTransition — 按 target 持久化并应用自定义进/退场规则
- [x] setFilter / setComplexAnimation / setTempAnimation — 统一演出时间线与 GPU filter
- [x] changeFigure 非 alpha blend（add/multiply/screen）Material2D 实际渲染
- [x] intro — 黑屏分页独白、自动推进与 `-hold` 点击推进
- [x] filmMode — 固定设计视口电影遮幅
- [x] wait — 真实时间延时和 `-next` 非阻塞语义
- [x] pixiPerform / pixiInit — 无 Web 运行时的有界 Bevy 演出层
- [x] 转场效果 — 不拉伸纹理的 wipe 与噪声 dissolve
- [x] 粒子特效 — rain/snow/sakura/dust 预制与同帧替换/清理；每个 emitter 合并为单动态网格

验收入口与逐步预期见 [`phases.md`](acceptance/phases.md)。

## Phase 6 — 文本增强 (DONE — 待用户验收)

- [x] -notend / -concat — 对话中插阻塞演出并保持 speaker/markup
- [x] 文本拓展语法 — glyph cluster style、颜色、字号、粗体与斜体
- [x] ruby / furigana — 正文上方独立排版、同步打字与自动换行
- [x] getUserInput — 设计视口模态输入、Unicode/删除/非空确认与变量回写

验收步骤见 [`phases.md`](acceptance/phases.md)。

## Phase 7 — 工程化核心 (DONE — 待用户验收)

- [x] setTextbox — 持久 hide/show 与 `:` 单句自动恢复
- [x] `;` 全行注释与 `//` 行内注释
- [x] `comment:` 显式 no-op，不再误解析成对话
- [x] unlockCg / unlockBgm — 解析、资源扫描与独立原子持久化
- [x] Title EXTRA — WebGAL K 风格 CG/BGM 鉴赏、分页预览、全屏查看与播放器
- [x] 本地资源清单、当前场景/子场景前看预取、标题页入口资源预热与加载状态
  （Bevy `AssetServer`）
- [x] `keine-loader` adapter 按 asset/script/store 分类，并由配置分别选择 FS、WebGAL 与存档 codec
- [x] `config.yaml` 有序多来源、同名 scene/资产确定性覆盖与多目录热重载
- [x] 统一 InputAction 层（鼠标、触控、键盘、手柄）
- [x] `hexz_k` 标准加密 `.hxz`、受限缓存、配置/脚本直读与 seekable Bevy `AssetReader`
- [x] macOS .app bundle 脚本
- [x] Linux / macOS / Windows fmt、Clippy、测试与 release CI
- [x] WebGAL_k 风格 CI 加密发布 — 通过 GitHub Actions Secret / 手动构建输入注入
  `KEINE_HEXZ_PASSWORD`，让 Hexz 打包与引擎编译使用同一密钥，并确保日志、缓存与
  artifact 不泄露密钥

Phase 7 已完成不依赖第三方专有 SDK 的工程主线。Live2D、Spine、Steam 和
Flowchart 仍保留为可选适配工作，不能用静态占位或实验依赖冒充完成。逐步验收见
[`phases.md`](acceptance/phases.md)。

### 延期适配（不属于 Phase 7 完成条件）

- GIF、Spine 与 Live2D（Live2D 已由用户决定暂缓）
- [x] macOS AVFoundation 单播放器音画时钟、BGRA 帧输出、共享 Bevy 渲染生命周期与后台
  Hexz 临时源准备；发行按内容启用 `video-native`
- [ ] Windows Media Foundation frame-server 后端与 Hexz `IMFByteStream`；完成前 Windows/Linux
  按项目资源启用 `video-ffmpeg`
- [ ] 移除 Hexz 视频的明文临时文件：macOS `AVAssetResourceLoader` 与 Windows byte stream
  共用稳定随机读取合同；具体 API、缓存、取消和 fixture 见
  [`09-native-video.md`](architecture/09-native-video.md)
- Android / iOS 视频解码暂缓：移动端不承诺 FFmpeg 交叉编译与分发，待桌面平台 byte
  source 与硬件解码生命周期稳定后再验收
- MainCore 固定 UI 的完整本地化（主题和运行时换肤明确不做）
- Steam 集成与 Flowchart 内容页
- SafeArea、横竖屏、响应式断点及 Android / iOS / Web 设备验收

## LetsGal Studio 1.9.1 adapter（DONE — 待用户验收）

- [x] 原生 `project.json`、章节、角色、场景与 `assets/.manifest.json` 多文件读取
- [x] 34 种已知内置 block 穷举编译；未知字段保留，未知 block 明确报错
- [x] Studio 原生资源目录直接挂载，hash 与逻辑路径均可解析
- [x] 内容/清单变化热重载；`.studio/state.json` 调试位置变化只做 fragment/block seek
- [x] 多来源 FS 资源热重载、覆盖层删除 fallback、manifest alias 刷新与静态资源存在性校验
- [x] keine 内建字体与外部工程解耦，不要求 Studio 工程复制引擎 UI 资源
- [x] 7 个系统 slot 映射到固定 MainCore UI，默认图库方法映射到 `Unlock`
- [x] 全屏 curtain 与 letterbox curtain 使用原生、帧率无关的演出状态
- [x] floatingText 的位置、颜色、字号、淡入/保持/淡出与阻塞语义
- [x] portraitStyleRule 与五种内置对话样式进入原生 core/UI 状态
- [x] animateSprite 时间轴 frames/loop/waitForComplete 使用帧率无关的原生关键帧状态
- [x] 1.8.0 `stageAnimation` 共享时钟时间轴：camera/character/sceneLayer 关键帧、四种
  easing、有限/无限循环、播放倍率、阻塞规则及 camera/particle/scene 事件进入 typed core
- [x] 1.8.0 新增相机后处理字段进入稀疏 boxed patch、运行状态插值和单通道 GPU 材质，
  不把大型参数表复制进每一条 Action
- [x] 1.9.0 `waitForInput`、时间轴 audio cue、立绘视口相对高度、表情级覆盖与角色皮肤选择
  全部降级为 adapter-neutral core 状态；跨章节 fragment 继续使用原生 call stack
- [x] 1.9.0 SDK 变更完成边界复核：keine 仍不执行扩展；Backspace 1.3.0 提升 SDK/宿主
  版本声明并保留官方兼容的统一 method 实现
- [x] 1.9.1 `chapterFolders` / `chapterTreeOrder` 作为章节树执行顺序，缺失时回退旧
  `chapterOrder`；默认壳对话框打字间隔、淡入时长、10 种出现效果及参数进入原生 config/UI
- [x] 视频和完整相机后处理进入 typed core、原生运行状态与 Bevy backend；内置 block
  不得经 `HostCommand` 降级，流式视频由项目特征按需启用
- [x] Godray 八字段进入稀疏 IR、状态插值和单通道 GPU 材质；动态 `callFragment`
  继续使用原生 scene call stack
- [x] 无扩展同步：`cargo studio-sync <project>` 直接读取 Studio 开放 JSON 与
  `.studio/state.json`，不安装 SDK bundle、不注入界面、不启动 localhost bridge
- [x] 当前 fragment/block 变化跨帧稳健同步；chapter/config/manifest/variables 保存后先原子
  重编译，再按最新选中剧情块从入口确定性重放
- [x] 调试位置 watcher 加 200 ms 去重轮询兜底，系统事件被合并时仍能恢复同步
- [x] 导入 slot/shared 变量声明默认值及角色属性默认值；调试重放不继承上一次预览的瞬态变量
- [x] Studio 为调试控制面：同步窗口不接受独立剧情推进，避免 keine 与编辑器选中剧情块产生双状态
- [x] 明确区分剧情调试位置与鼠标指针；个性化鼠标指针不兼容，统一使用系统默认指针
- [x] 同步会话禁止存档、设置、profile、已读和图鉴写回，保持 Studio 工程只读
- [x] Studio adapter 保持只读且 loader-only；普通 dev、library embed 与发行运行不依赖该 adapter
- [ ] （延期）视频和完整相机后处理的移动端逐像素/音画验收与平台解码后端分发

明确不支持 Studio 扩展或注入。`reference/12` 只保留为 Studio 格式与历史 API 调研资料，
不构成 keine 运行时依赖或后续实现清单。

实现和当前能力矩阵见
[`08-letsgal-studio.md`](architecture/08-letsgal-studio.md)；API 反向工程证据见
[`12-letsgal-studio-extension-api.md`](reference/12-letsgal-studio-extension-api.md)；逐步联调见
[`18-letsgal-studio-acceptance.md`](acceptance/18-letsgal-studio-acceptance.md)。

---

## 架构文档

| 文档 | 内容 |
|------|------|
| [01-language-and-stack.md](architecture/01-language-and-stack.md) | 语言与技术栈选型 |
| [02-ecs-architecture.md](architecture/02-ecs-architecture.md) | ECS 架构设计（历史参考） |
| [03-render-pipeline.md](architecture/03-render-pipeline.md) | 渲染管线 |
| [04-rollback-and-save.md](architecture/04-rollback-and-save.md) | 存档与回溯 |
| [05-bevy-architecture.md](architecture/05-bevy-architecture.md) | Bevy 架构设计（当前权威） |
| [06-hexz-packaging.md](architecture/06-hexz-packaging.md) | Hexz 标准打包、校验与挂载 |
| [07-content-loader.md](architecture/07-content-loader.md) | 内容来源、adapter、多根覆盖与 Hexz 加载契约 |
| [08-letsgal-studio.md](architecture/08-letsgal-studio.md) | LetsGal 原生工程、动作编译、资源与步进调试边界 |
| [09-native-video.md](architecture/09-native-video.md) | 桌面原生视频后端、FFmpeg 回退与 Hexz byte source 待办 |
| `crates/loader/src/lib.rs` | 可注册语言、诊断与资源引用合同 |
| [07-references.md](reference/07-references.md) | 业界引擎参考 |
| [09-webgal-script-reference.md](reference/09-webgal-script-reference.md) | WebGAL 脚本参考 |
| [10-webgal-k-gap-analysis.md](reference/10-webgal-k-gap-analysis.md) | 本地 WebGAL_K 4.6.1 与 keine 0.2.0 的历史缺口快照 |
| [11-engine-advantages.md](reference/11-engine-advantages.md) | keine 的差异化、优势支柱与量化验收标准 |
| [webgal-compatibility/README.md](webgal-compatibility/README.md) | WebGAL 4.6.2 当前语义矩阵、视觉证据、内部格式与综合示例 |
| [phases.md](acceptance/phases.md) | Phase 1–7 桌面、状态 UI、音频、演出、文本及工程能力验收步骤 |
