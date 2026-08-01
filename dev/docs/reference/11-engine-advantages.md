# keine 如何建立引擎优势

> 本文是面向产品方向的能力总结与路线说明，不是二进制协议或逐字段 API 规范。文中的
> “当前事实”以本仓库代码和测试为准；存档/回滚的精确合同见
> [04-rollback-and-save.md](../architecture/04-rollback-and-save.md)，loader 边界见
> [07-content-loader.md](../architecture/07-content-loader.md)。这里的“优势”必须能够由代码、
> 测试或指标证明，不能只是一组愿景。

## 定位

keine 不应该只是“用 Rust 重写 WebGAL”，也不应该把目标设成逐文件复制其
React/Redux/Pixi 实现。更准确的定位是：

> **兼容 WebGAL 创作生态的原生视觉小说运行时：以确定性状态机为核心，以 Bevy/wgpu
> 提供高质量舞台，以版本化快照支持存档和回滚，以 Rust 工具链完成检查、预览、打包和发布。**

WebGAL 兼容性负责降低迁移和创作成本；Rust、Bevy 与独立核心负责建立运行时差异。
只有两者同时成立，keine 才有独立价值。

## 优势公式

```
引擎优势 = 创作兼容性 × 运行时质量 × 状态可信度 × 开发效率 × 交付能力
```

这是乘法而不是加法：脚本不能运行、存档不可信、开发流程太慢或无法发布，都会让其他
技术亮点失去实际价值。因此实现顺序应优先闭合主链，再扩充特效数量。

## 六个优势支柱

### 1. 确定性、可序列化的核心

脚本先由 adapter 编译为 typed `Action`，再集中构建不可变 `Program`：场景动作压成
`Box<[Action]>`，label 在构建期索引，运行态通过 `Arc<Program>` 共享脚本。
`keine-core` 的 `step` 只在对话、选择或阻塞演出等边界 yield；核心不依赖 Bevy，
运行结果由当前 `Program`、`State` 和输入序列决定。

这套结构带来的长期价值是：

- 同一输入序列可以得到同一状态，便于单元测试、回放和故障复现；
- 存档只保存剧情时间点中的权威 State，`Program` 与 Bevy World 都不重复序列化；
- Backlog、回滚、自动测试和编辑器预览可以共享同一快照边界；
- 渲染、UI 或音频后端升级时，不需要改写脚本语义。

当前 `Program` 会对稳定排序后的 scene 名和 typed Action payload 计算 fingerprint；
State、槽位 metadata 和 RollbackSnapshot 都携带这份身份。读档和回想因此不会把属于
另一份脚本布局的数字 cursor 静默注入当前项目。未来仍需要正式的 schema migration，
但版本边界和拒绝错误恢复的安全基线已经成立。

### 2. 原生 GPU 舞台，而不是 WebView 容器

Bevy/wgpu 直接使用 Metal、Vulkan 或 DX12。当前三相机结构已经解决一个很有代表性的
合成问题：场景区域模糊、普通 UI、Dialog 背景模糊和清晰 Dialog 分属明确的渲染层，
并且在正确的 pass 顺序中合成。

这条路线的价值不只是“能模糊”：

- 舞台、UI 和后处理共享同一个 GPU 管线；
- `DesignViewport` 统一 1920×1080 设计空间和 letterbox，减少各系统自行换算；
- 背景与立绘通过稳定 ID 增量同步，避免每帧重建 ECS 实体；
- 后续 transition、filter、粒子和视频可以进入统一的 Render Graph；
- 原生窗口避免把游戏运行时绑定到浏览器 DOM、CSS 和 WebView 生命周期。

但“更快、更小”目前仍是假设而非结论。必须用帧时间、显存、启动时间、包体和 draw call
实测证明，不能直接从 Rust/原生推导。

### 3. 清晰、可替换的三层边界

```
WebGAL .txt / language adapters
        ↓
keine-loader   内容来源、解析、诊断、资源扫描、热重载
        ↓ typed Action / Program
keine-core     不可变 Program、权威 State、控制流、快照、回滚
        ↓ State
keine          输入、ECS 投影、UI、音频、GPU 合成
```

这种边界让兼容层和运行时不互相污染。不同语言适配器把源命令映射为 keine 的领域
动作，而不是迫使引擎复制某种语言的内部 store；新增语法不需要修改核心状态机。

系统调度采用显式的 `Input -> Sync -> Layout -> Ui` 顺序；热重载、项目加载和存档 IO
也有独立所有者。相比把状态散落到 UI 组件，职责更容易测试和替换。

### 4. 兼容已有生态，但提供更严格的创作反馈

WebGAL 兼容是 keine 最快获得真实剧本、资源布局和用户心智的路径。优势不应停留在
“能读 `.txt`”，而应继续发展为：

- 行为兼容：`-when/-next/-notend`、场景调用和演出阻塞语义一致；
- 迁移友好：现有项目可以逐步运行，不要求一次性改写；
- 严格诊断：未知命令、错误参数、重复场景、缺失资源必须带 source span 报错；
- 预检能力：CLI 在发布前扫描场景图、资源引用和不可达分支；
- 适配器策略：WebGAL 负责现有项目兼容，其他语言可独立接入同一 `Action` IR。

“静默忽略未支持命令”会直接破坏这一优势。兼容度应由 fixture corpus 和端到端项目测试
衡量，而不是由 parser 中出现了多少关键词衡量。

### 5. 把存档、Backlog 和回滚视为同一个状态问题

传统实现很容易分别维护“当前画面”“存档数据”“历史文本”，最终产生无法恢复的隐式
状态。keine 当前以一次交互提交后的权威快照统一它们：

- Backlog 最多保留 200 条展示元数据和轻量 `RollbackSnapshot`，不复制 `Program`；
- 回想在恢复场景、变量、舞台、音频和 cursor 前，再次核对 Program fingerprint 并修正
  scene stack；安装新 Program 时会清除不再兼容的记录；
- 原生 `slot_N.sav` 当前为严格 v9：固定 header、Postcard metadata/state、长度上限和
  metadata/state 双 CRC32；槽位列表只读取元数据前缀，不会同步扫描全部剧情状态；
- `StoreAdapter::decode` 返回不可直接执行的 `SavedState`，唯一运行态入口
  `restore_into(&mut State)` 会重新附着当前 `Arc<Program>`，并拒绝 fingerprint 不匹配；
- 已读记录、用户设置、全局变量和鉴赏解锁独立于单个存档；
- 槽位预览是独立的 `slot_N.webp` sidecar，不嵌入 `.sav`，状态完整性与图片生命周期各自
  清晰；删除槽位会同时删除两者；
- `.sav` 采用同目录临时文件、`sync_all` 和 rename 原子替换；
- 加载后由权威 State 重建 ECS、UI 和音频，不依赖残留实体。

这套边界已经使存档可信度成为可测试的当前能力，而不是愿景。尚未完成的是跨旧版本的
schema migration：目前 v1–v4 会被明确拒绝；未来若需要迁移，必须增加独立版本 adapter、
输入上限、迁移测试和对应 golden，不能削弱 `SavedState` 与 fingerprint 的恢复边界。

### 6. 从开发预览一直覆盖到安全交付

目标工作流应是一条连续链：

```
keine new → dev 热重载 → check 静态检查 → build → pack .hxz → 发布
```

当前已经有脚本热重载、稳定场景加载和重复场景检测。后续把资源预取、编辑器协议、
CLI 检查、`.hxz` 压缩/加密随机访问和平台打包接入同一项目模型，可以同时服务个人作者
和正式发行项目。

真正的优势是“同一份项目在创作期严格、运行期稳定、发布期可控”，不是单独拥有一个
加密格式或一个编辑器窗口。

## 与参考引擎的差异化

| 参考对象 | 应继承 | keine 应增加的价值 |
|---|---|---|
| WebGAL | 易写脚本、项目生态、成熟 VN UI 语义 | 原生运行时、严格诊断、确定性状态、版本化存档 |
| Ren'Py | rollback 思路、成熟叙事能力 | Rust 类型边界、Bevy GPU 管线、WebGAL 迁移路径 |
| YU-RIS | 延迟执行、批处理意识 | 可测试 Action/State 模型与跨平台现代渲染 |
| Suika2/NScripter | 轻量、直接、容易发布 | 更强 GPU 演出、热重载和现代工具链 |
| 通用 Bevy 游戏 | ECS、Schedule、Render Graph | 面向 VN 的脚本语义、存档回滚、创作工具和资源约定 |

这不是宣称 keine 已经全面优于这些引擎，而是说明每个参考对象提供哪块经过验证的
设计，以及 keine 必须在哪些交叉点形成自己的组合优势。

## 当前已经成立的证据

- `core/loader/bevy` 三层已经拆分，核心与 parser 可脱离 Bevy 测试；
- typed Action 被构建为带 label 索引和稳定 fingerprint 的共享 `Program`，State clone、
  rollback 和 save 不再随脚本 Action 总数线性复制；
- State 由 ECS resource 直接拥有，没有全局锁和跨系统共享锁；
- 三相机 Dialog 模糊已经完成实际交互验证；
- 背景和立绘使用稳定 ID 增量同步；
- 1920×1080 设计空间和 letterbox 换算已集中管理；
- 脚本 watcher 生命周期正确，修改后会真实重载；
- 场景加载顺序稳定，重复 scene stem 会报错；
- v9 多槽存档拥有固定 golden、metadata/state 长度校验、双 CRC32、前缀检查和 Program fingerprint；
- codec 只暴露 `SavedState`，读档必须经过 `restore_into`；预览使用独立 WebP sidecar；
- 快速存档采用临时文件、`sync_all` 和 rename，IO 错误显式返回；
- Rustfmt、严格 Clippy 和现有测试构成了最低质量门槛。

这些证明 keine 的技术方向可行，但还不足以证明完整 WebGAL 项目可迁移或生产项目
可交付。

## 里程碑状态：历史闭环与后续路线

M1/M2 是早期用于闭合主链的历史路线，保留在这里用于解释架构选择，不再表示“尚未开始”。
持续兼容缺口以语义矩阵和验收文档为准；M3/M4 才是这里继续向前的产品路线。

### M1（历史）：可玩的 WebGAL 核心

- 主线已经具备桌面 Choice、跨场景目标、条件选项与 `end` 标题流程；
- typed args、source span、诊断、`-when/-next/-notend`、表达式和变量插值已进入统一
  Action/Program 路径；

这一里程碑的主链已经闭合；剩余命令和参数差异继续由兼容矩阵逐项追踪，而不是重新打开
一套平行的 M1 实现。

### M2（历史）：可信赖的 VN 产品能力

- Backlog、已读记录、带 fingerprint 的 RollbackSnapshot 已落地；
- v9 多槽存档、独立 WebP 预览、`SavedState` 恢复边界和槽外 profile/gallery/settings
  已落地；
- BGM、vocal、SE/replay、音量设置，以及 Options/Title/Save/Load 主流程已形成当前基线；
- 未完成项是未来 schema migration、更多压力/故障注入测试和目标平台实机验收，不是重写
  现有存档或回滚模型。

历史完成标准仍作为回归约束：保存/加载/回想不能丢失场景栈、变量、舞台或音频状态，
Skip 必须区分已读，长期玩家数据不能被单槽读档倒退。

### M3：体现原生渲染价值

- 统一 perform 生命周期和阻塞语义；
- animation、transition、filter、wait；
- 富文本、视频、粒子，以及按需求接入 Live2D/Spine；
- 视觉回归测试和 GPU 性能基线。

完成标准：常见 WebGAL 演出可兼容，keine 原生效果在不同分辨率和平台上结果稳定。

### M4：完整创作与交付链

- `new/dev/check/build/pack` CLI；
- 资源图、预取、加载进度和缺失资源检查；
- `.hxz` 与 macOS/Windows/Linux 打包；
- 编辑器预览协议、CI 和可复现 release。

完成标准：新项目能从模板创建，经自动检查后生成无需开发环境的发布包。

## 量化优势，而不是假设优势

| 维度 | 必须建立的指标 |
|---|---|
| 兼容性 | WebGAL fixture 通过率、已支持命令/参数矩阵、真实项目走通率 |
| 确定性 | 相同脚本与输入的状态 hash 一致；存档加载后 hash/画面一致 |
| 正确性 | parser 负例、scene stack、回滚、迁移和损坏存档测试 |
| 渲染 | 1080p/1440p 帧时间、draw call、显存、不同 GPU 后端视觉回归 |
| 启动与体积 | 冷启动时间、可执行文件与完整发行包大小；区分代码和资产 |
| 开发体验 | 热重载延迟、全项目检查耗时、错误是否定位到文件/行/参数 |
| 稳定性 | 长时间 Auto/Skip、频繁切场景、反复存读档和窗口缩放压力测试 |

早期文档中的 `<20MB`、`<2KB save`、`<2s cold start`、`<50ms hot reload`
都应保留为待验证目标，而不是已经实现的事实。尤其 Bevy 二进制和带预览图的存档需要
按 release 构建实测后再确定合理预算。

## 必须坚持的原则

1. **行为兼容，不复制内部实现。** 对齐 WebGAL 的项目和脚本语义，不复制其前端架构。
2. **权威状态只有一份。** core State 是事实来源，ECS、UI、音频都是可重建投影。
3. **端到端完成才勾选。** parser、core、frontend 和交互缺一不可。
4. **不静默降级。** 未支持命令和非法参数必须报出位置与原因。
5. **先闭合主链，再堆特效。** Choice、场景、状态、存档、音频优先于高级 shader。
6. **每项优势都要有基线。** 没有 benchmark、fixture 或回归测试，就只能称为目标。
7. **保持 Rust 层可独立测试。** 不让 Bevy 类型渗入脚本语义和持久化格式。
8. **全平台 UI 从设计时成立。** 桌面、移动端和 Web 共用行为语义；UI 不依赖 hover，
   不把桌面布局简单等比缩小到手机屏幕。

## 全平台 UI 约束

keine 的目标平台包括桌面端、移动端和 Web。Bevy 的跨平台能力只是基础，只有输入、
布局、资源和发布流程都经过目标设备验证，才能宣称对应平台受支持。

- **统一输入动作。** 点击、触摸、键盘和手柄映射到 Select/Confirm/Cancel/Advance 等
  领域动作；游戏逻辑不直接判断某个鼠标按键。
- **触控优先。** 所有核心操作必须可直接触摸；hover 只能增强反馈，不能承载功能；
  交互区域使用不小于约 44–48 逻辑像素的目标尺寸并留出间距。
- **响应式而非等比缩放。** 舞台可以保持设计分辨率和 letterbox，但 Choice、Backlog、
  Save/Load、Options 等 UI 要根据可用宽高切换布局、字号、滚动和按钮排列。
- **安全区。** iOS 刘海、圆角、Home Indicator 和 Android cutout 必须进入统一的
  SafeArea/Viewport 计算，关键文字与按钮不能只依赖屏幕物理边缘定位。
- **方向与窗口变化。** 支持窗口缩放、横竖屏变化和超宽/窄屏；布局重算不能改变核心
  State，也不能打断当前选择、输入或演出。
- **文本与输入法。** 中日韩字体、动态字号、软键盘、IME 组合输入和屏幕键盘遮挡要在
  `getUserInput`、存档命名等界面统一处理。
- **性能分级。** blur、粒子、视频和高分辨率纹理要允许按设备能力降低质量，降级不能
  改变脚本时序或交互结果。
- **平台验证。** CI 至少覆盖各 target 的编译；里程碑发布还要在桌面、手机和浏览器上
  运行同一套输入、布局、存读档和恢复测试。

## 最终判断

keine 最有机会形成的优势，是把 WebGAL 的低门槛创作方式、Ren'Py 的状态回滚思想、
Bevy 的原生 GPU/ECS 能力和 Rust 的可靠工具链组合成一个统一产品。它的护城河不会是
某个按钮或 shader，而是：**剧本容易迁移，运行结果可预测，画面可扩展，存档可信，
错误可定位，项目最终能稳定发布。**
