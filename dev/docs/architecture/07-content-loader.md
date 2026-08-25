# Content loader 与多来源协议

## 边界

`keine-loader` 是所有外部内容进入引擎的唯一入口，且不依赖 Bevy：

```text
config.yaml
    ├── adapter.asset[] ──> asset/{fs, auto...} ──> logical roots
    ├── packaged .haku ──> project/hakutaku ──> logical roots
    ├── adapter.script ───> script/{webgal...}          ──> State actions
    └── adapter.store ────> store/{keine...}          ──> SavedState / StoreStatus
```

`adapter` 先按稳定能力分类，再在类别下加入具体格式：

```text
adapter.rs          registry 与稳定能力入口
adapter/
├── asset.rs        fs、auto 目录挂载与 Hakutaku packaged-project opener
├── editor.rs       完整编辑器工程的稳定 trait 与 registry
├── editor/
│   ├── letsgal.rs  LetsGal adapter 门面
│   └── letsgal/    多文件编译与开放 JSON 模型
├── script.rs       脚本语言 registry
├── script/
│   └── webgal.rs   WebGAL parser 与统一 Action 导出
└── store.rs        存档状态格式编码、检查与解析
```

顶层只表示稳定能力类别。asset/script/store 各有独立 registry 和 trait 边界；
editor 负责一次读取 project、chapter、character、scene 与 manifest 的完整编辑器工程。
具体格式仍由所属类别注册，例如 `editor/letsgal`、script 类别的 `webgal.rs` 和 store 类别的
keine codec。类别拥有独立 trait 或 registry 时使用同名门面文件；紧密耦合且没有独立
生命周期的实现（例如 asset 的 fs/auto 分派与 Hakutaku project opener）保留在一个文件中，不为视觉上的对称制造目录。

### 职责不可越界

| 层 | 负责 | 明确不负责 |
|---|---|---|
| `adapter/asset.rs` 的 Hakutaku project adapter | 检测完整 `.haku` 工程、解析包内 config、生成通用 seekable content mount | Bevy `AssetReader`、渲染资源、运行状态 |
| `adapter/editor/letsgal` | 检测工程、只读解析 JSON/manifest/Studio 当前选中剧情块、补齐来源默认值、生成统一 config/mount/Action | 文件监控、写回工程、Bevy、窗口、进程、TCP、Studio DOM |
| loader | adapter registry、来源合并、`notify` 监控、临时编译完整 Program | 运行游戏状态、重放剧情、渲染 UI |
| core/runtime | 消费统一 Action、原子替换 Program、保留持久状态并重建瞬态演出 | 理解 LetsGal JSON 字段或 UUID 模型 |

Rust crate 依赖方向强制这一边界：`keine-loader -> keine-core`，loader 不依赖根引擎、
Bevy、Winit 或 Electron。LetsGal adapter 的回归测试还会对 fixture 做调用前后逐文件快照，
保证 detect/open/load/config/debug-position 全流程不修改源工程。

根引擎只调用 `ProjectAdapter`、`StructuredSceneLoader` 和统一 `ContentProject`；它不导入任何
LetsGal 类型。`dev --sync` 只是给通用 runtime 加一个只读同步会话标记，不安装扩展、不启动
localhost 服务，也不覆盖 Studio 原版 Player。因此移除或替换某个 editor adapter 不会改变
VM、渲染器、UI 或原生 keine 工程的启动路径。
library embedder 还可从 `LoaderRegistry::empty()` 开始只注册自己需要的 adapter；`Default`
只是最终 keine 二进制采用的内置组合，并不是 runtime 的类型依赖。

源码工具可运行 `cargo configure`，用方向键和空格配置默认 registry 中的具体实现与已有的
运行能力，回车保存、Esc 取消。该命令显式启用非默认 `configure` feature；Crossterm TUI
不会编入普通或发行引擎。配置保存在用户配置目录的 `engine.conf`，只作用于启用了该 feature
的开发 CLI 启动路径；项目内的 `config.yaml` 仍负责从已启用集合中选择格式，`run_with_loader` /
`build_app_with_loader` 等嵌入接口也不会读取这份全局配置。缺少的新 adapter 默认启用，
asset/script/store 三类至少各保留一个，避免保存出无法启动的组合。旧 `cargo adapters` 与
`adapters.conf` 继续作为兼容入口，下一次保存时写入新的通用格式。

配置界面按能力而不是实现技术分区：Content 下列出 project/asset/script adapter，Persistence
下列出 store codec，Media 下提供视频 `automatic`/`disabled` 单选。`automatic` 显示当前二进制
按 target/feature 实际解析到的 AVFoundation、FFmpeg 或 unavailable；关闭后仍安装稳定的拒绝
实现，使包含视频指令的工程得到明确诊断，而不是留下半初始化播放器。只有一个实现、无法真正
切换的 Presentation、Provider 或 Host 能力不会显示伪选项。

## 配置

```yaml
adapter:
  asset:
    - path: "."
      format: fs
    - path: "content/shared"
      format: fs
  script: webgal
  store: keine
```

- 声明顺序即层顺序；越靠后优先级越高。
- 资产使用逻辑相对路径覆盖，scene 使用不带扩展名的 scene name 覆盖。
- 未声明 `adapter.asset` 时等价于 `[{ path: ".", format: "fs" }]`。
- 未声明 `adapter` 时默认使用 `script: webgal` 与 `store: keine`。
- 所有相对路径都以 `config.yaml` 所在目录为基准，并在加载前规范化。

## 内置选项

| 类别/名称 | 输入 | 输出 |
|---|---|---|
| asset / `fs` | 开发时无需打包的本地目录 | 项目根或纯资产根 |
| asset / `auto` | 本地目录或容器 | 根据路径委派 asset adapter |
| project / `hakutaku` | `.haku` 快照与同级 `data/*.taku` | `hakutaku_core::Package` 验签、解密与随机读取 |
| script / `webgal` | `.txt` scene | `ParseReport<Action>` |
| store / `keine` | v10 原生 `.sav` bytes 或当前 `State` | 编码后的 bytes；解码后的 `SavedState`；可独立检查的 `StoreStatus`/metadata |

`LoaderRegistry` 按类别解析名称。asset source 可以有多个且保持后声明覆盖；script 和 store
各选一个明确格式，未知名称在启动阶段直接报错，而不是静默回退。

### Store 恢复边界

`StoreAdapter` 只负责格式层：

- `encode(&State)` 生成版本化存档 bytes；
- `inspect(reader)` 校验格式并返回 `Ready(metadata)`、`Corrupt` 或 `Unsupported(version)`；带元数据前缀的格式可在 state payload 前停止读取；
- `decode(bytes)` 返回 `SavedState`，不会返回可直接替换运行态的 `State`。

`SavedState` 只允许通过 `snapshot()` 做只读预览投影，或通过 `restore_into(&mut current)` 合入当前项目。恢复时 core 会核对存档与当前 `Program` 的 fingerprint，重新附着当前 `Arc<Program>`，保留 profile/read-history/gallery 等槽外数据，并拒绝不同脚本布局的存档。UI 的 metadata 过滤只是提前反馈，不能替代 core 检查。

当前 keine store 使用 v10、Postcard metadata/state 与双 CRC32；槽位列表只读取 header + metadata，Bevy storage 层另行维护独立 WebP preview sidecar。完整字节布局、版本策略与 Backlog 恢复合同见 [04-rollback-and-save.md](04-rollback-and-save.md)。

## 运行时规则

- `keine-loader` 从统一 `ContentMount` 合并脚本并生成最终 scene/resource manifest。
- 打包项目的 compiled loader 在启动时把已解码 scene payload 一次性移交给
  `Program`；交付后 loader 只保留内容 mount，不再长期保留第二棵 Action tree。
  可热重载的目录/编辑器 loader 仍通过可重复的 `load()` 路径重建，不受这一所有权
  优化影响。
- `OverlayAssetReader` 只做 Bevy 接口桥接，按相反顺序查找资产，只消费通用
  `ContentMount/ContentFile`；metadata 查询先用 containment 选出实际提供资产的最高优先级
  layer，再只从同一 layer 读取 `.meta`，不会为选择 layer 额外打开资产文件。它不知道底层
  容器格式，也不会复制或落盘 archive entry。
- 资源预取继续使用统一逻辑路径，因此来源数量不会增加业务层分支。含 `{...}` 的
  runtime interpolation 模板不会被静态检查、图片角色登记、迁移或 speculative
  prefetch 当作字面文件；变量解析后的活动资源仍由正常场景同步路径加载。
- FS 脚本/结构化工程在非默认 `hot-reload` feature 下由 `notify` 递归监控；`cargo dev`
  显式启用它，普通构建与发行引擎不解析、编译或链接文件监听后端。变更后先完整解析到临时
  Program，再一次替换，并从当前 scene 开头重建瞬态演出/交互状态。变量和图库等持久数据保留。
- FS 资源根由 `OverlayAssetWatcher` 监控；逻辑路径事件交给 Bevy AssetServer，原 handle 原位
  重载。多来源中高优先级文件被删除时会立即重新读取低优先级 fallback；图片重新执行尺寸
  限制与 CPU 像素释放。Hakutaku 是只读发行来源，不创建 watcher。
- 启用 `hot-reload` 时，macOS/Windows/Linux 使用同一 `notify::RecommendedWatcher` 生命周期
  与相同逻辑路径，差异仅在 notify 选择的系统后端；Windows 的反斜杠不会进入 IR 或资源键。
- Hakutaku 使用受限 block cache 和 seekable `AssetCursor`；配置、脚本、图片、字体与音频共享
  一个快照索引。打开快照时同时生成排序的 parent → direct children 表，列目录不再扫描
  全部文件。

## 约束

- adapter 必须返回只读、稳定、规范化的逻辑 mount。
- editor adapter 只描述 `watch_roots/accepts_change` 这类格式规则；实际 watcher 生命周期由
  loader 持有，实际 reload/state rebuild 由 runtime 执行。
- 不把 Bevy handle、ECS 类型或 UI 状态放入 `keine-loader`。
- store adapter 不得把 decoded payload 暴露成可直接运行的 `State`；执行态恢复必须经过 `SavedState::restore_into`。
- 不允许业务代码直接拼接某个 source 的物理路径。
- 新容器格式必须先实现路径安全、完整性校验和流式读取，再进入 registry。
