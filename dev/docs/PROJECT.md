# keine project map

keine 是 Bevy 0.19 构建的专用视觉小说引擎，兼容 WebGAL 脚本。模块按依赖方向组织，
不是按平台或 UI 页面拆成大量 crate。

## Workspace

```text
keine-core   <- keine-loader <- keine
纯状态与执行      内容与语言适配        最终引擎、ECS、渲染、UI、存储
```

```text
keine/
├── Cargo.toml                     根 package、workspace 与公共依赖
├── build.rs                       Windows PE 图标资源嵌入
├── assets/icons/                  跨平台图标母版及 PNG、ICO、ICNS 发行资源
├── src/
│   ├── lib.rs                     可复用引擎 library 入口
│   ├── main.rs                    最小桌面 binary 入口
│   ├── runtime.rs + runtime/      Plugin 入口及 bootstrap、平台、AssetReader、帧推进
│   ├── scene.rs + scene/          ScenePlugin、资源/音频/背景/立绘
│   │   └── effects/               特殊 blend/filter 材质与有界粒子层
│   ├── storage.rs + storage/      StoragePlugin 门面及设置、存档、历史、鉴赏实现
│   ├── render.rs + render/        RenderPlugin 门面及 Blur pipeline 实现
│   ├── assets/                    内嵌字体、UI 音效与 WGSL shader
│   ├── ui.rs + ui/                GameUiPlugin 门面与固定 MainCore UI 实现
│   └── ui/
│       ├── support.rs + support/  通用 UI 门面及字体、输入域、加载、音效等机制
│       ├── stage.rs + stage/      舞台 UI 门面及 Textbox、控制栏与选项
│       ├── overlays.rs + overlays/ 覆盖层门面及 Backlog、Dialog
│       └── screens.rs + screens/  页面门面及 Title、Save/Load、Config、菜单路由
├── crates/
│   ├── core/                      package: keine-core
│   │   └── src/
│   │       ├── lib.rs             公共 model/runtime 门面与转场数学
│   │       ├── config.rs          游戏配置数据
│   │       ├── model.rs + model/  model 门面及 Action、State、公共值与渲染参数
│   │       └── runtime.rs + runtime/ runtime 门面及表达式、确定性 step 执行器
│   └── loader/                    package: keine-loader
│       └── src/
│           ├── adapter.rs + adapter/ 可配置格式 registry 及四类 adapter 实现
│           │   ├── asset.rs       fs、auto 与 Hakutaku packaged-project 适配
│           │   ├── editor.rs + editor/ 完整编辑器工程门面与 LetsGal 实现
│           │   ├── script.rs + script/ 脚本门面与 WebGAL 统一 IR 导出
│           │   └── store.rs       存档状态格式接口与 keine codec
│           ├── loader.rs + loader/ 多来源挂载、场景发现和开发热重载
│           └── lib.rs             通用语言注册表、span、资源与诊断合同
├── projects/
│   └── test-project/              唯一端到端测试项目
├── .github/workflows/             桌面平台 fmt、Clippy、测试和 release CI
├── dev/scripts/                   发布/打包辅助脚本
└── dev/docs/
    ├── architecture/              当前与历史架构设计
    ├── reference/                 WebGAL 对照、参考与产品优势
    ├── acceptance/                Phase 1–7 总清单与 LetsGal 独立验收
    ├── PROJECT.md                 项目边界和目录规则
    └── TODO.md                    唯一进度入口
```

## Plugin ownership

- `RuntimePlugin`: 游戏输入、脚本推进、窗口布局。
- `keine-loader`: 按配置挂载内容来源、合并脚本、扫描资源并维护热重载；不依赖 Bevy。
- `ScenePlugin`: 资源预取、背景/立绘同步、vocal 与轻量演出层。
- `StoragePlugin`: 已读历史与鉴赏解锁持久化；存档 API 位于同模块。
- `GameUiPlugin`: Textbox、控制栏、Choice、Dialog、Backlog、Save/Load、Title。
- `BlurPlugin`: RenderApp pipeline、区域规划和 WGSL 后处理。

顶层 `GamePlugin` 只声明 `Input -> Sync -> Layout -> Ui` 顺序并组合上述插件。功能模块应
自行注册资源和系统，不再把新系统逐项加入中央文件。

## Generated data

以下目录是运行时生成物，必须保持在 `.gitignore` 中：

- `target/`
- `projects/*/saves/`

`**/imported_assets/` 是旧版开发期 Asset Processor 缓存，继续忽略以便安全清理，
但当前引擎直接读取本地 `assets/`，不再生成或依赖它。项目源文件只包含
`config.yaml`、`scripts/` 和 `assets/`。

## Design constraints

- 设计空间固定为 1920×1080，背景纹理解码上限与设计分辨率一致，由 viewport 和 letterbox 适配窗口。
- 三相机职责固定：Scene、普通 UI、Dialog/Modal。
- `keine-core` 不依赖 Bevy。
- 内容来源按配置顺序分层，后声明来源覆盖前面的同路径资产和同名 scene。
- adapter 顶层只按 `asset`、`editor`、`script`、`store` 能力类别组织。
- 需要跨多个 JSON 建立引用关系的格式放入 `editor/<format>/`（如
  `editor/letsgal/`）；它只负责检测、统一 IR 编译、资源挂载和调试游标。
- 特定编辑器的安装包放在该 adapter 的宿主子模块；根 runtime 只保留格式无关的本地桥接
  协议，普通启动不得启用桥接或导入具体 adapter 类型。
- Hakutaku 属于 packaged-project adapter，格式与打包协议由独立 Hakutaku workspace 负责。
- 完整资源包也由 asset adapter 作为通用 `ProjectAdapter` 打开；bootstrap 不按扩展名分支。
- Bevy 仅通过只读 `ContentMount/ContentFile` overlay reader 消费统一逻辑路径；包格式和语言
  适配不得进入 runtime 或渲染层。
- 专用 MainCore UI 不引入主题系统。
- 桌面优先，但 library 入口必须可供后续 Web/Android/iOS launcher 复用。

## Directory discipline

- 代码首先按稳定职责域分组；同一目录出现三个以上紧密相关模块时，应建立领域子目录。
- 不为单个文件制造目录，也不按每个页面、平台或微小类型拆 crate；目录必须表达依赖边界。
- 新模块优先放入现有领域，并通过同名领域入口 `.rs` 暴露最小 API；门面只保留注册、稳定
  导出与少量领域协调，具体生命周期或执行机制放入同名目录。不再新增只有声明的 `mod.rs`，
  也不把互不相干的实现压进一个巨型入口文件。
- 每个阶段结束时检查零引用源码、资源、测试夹具和依赖；确认无运行时或测试用途后删除。
- 构建产物、存档和导入缓存只能存在于忽略目录，不得成为项目结构的一部分。

## Application icon

`assets/icons/keine.png` 是唯一高清透明母版。运行时嵌入 256 px PNG，供 Windows 与
Linux/X11 窗口使用；macOS 裸二进制开发运行通过 AppKit 设置同一 PNG 的 Dock 图标；
Windows 构建由 `build.rs` 将 ICO 嵌入可执行文件；macOS app bundle 由
`bundle-macos.sh` 在 `cargo bundle` 的加密发行产物外安装 ICNS。`keine package`
只携带 256 px PNG，供 Linux
`.desktop`/hicolor 使用；Android adaptive icon 与 iOS asset catalog 的后续平台 launcher
应从仓库内母版生成自己的尺寸集合。

Wayland 不允许普通客户端自行设置窗口图标，因此 Linux 正式安装器仍应将母版安装到系统
图标主题并在 `.desktop` 中声明；这属于 launcher/安装包职责，不进入引擎生命周期。
