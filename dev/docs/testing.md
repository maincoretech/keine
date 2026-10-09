# 开发与验收

## 测试用例组织

- 同一规则的参数变化优先使用带案例名的表驱动测试；保留每个输入、期望值和边界断言，合并测试入口不等于删除覆盖。
- 独立故障保持独立回归，特别是 parser panic、递归预算、存档恢复、文件事务、IPC 与平台行为。Parser、Core 求值、Editor 投影保护不同入口，不能因输入相似而去重。
- 不以测试数量设目标，也不只为减少数量合并不相关的合同。新增回归先检查同规则案例表，避免复制初始化代码。
- CI 的默认、无默认功能 + publisher、平台音视频专项覆盖不同 feature 组合；合并前必须证明配置与路径等价，不能只看测试名称重叠。

当前整理将配置默认值/资源路径、archive 路径规范化、Editor 工程身份、立绘基线、Unicode 输入上限与 APK 图标错误的 18 个测试入口收敛为 8 个；原有案例与断言保留，Rust 案例包含输入/路径等失败上下文，Python 使用 `subTest` 独立报告三个错误案例。
实际验证：`cargo test --offline --workspace --features publisher,video-native,hot-reload`
1043 passed / 21 ignored / 0 failed（IPC/Trash 在沙箱外）；Python packaging
11 passed，benchmark collector 7 passed；fmt、workspace check、all-targets Clippy
（同一 feature 组合）及 native-smoke validate 通过。测试整理不改变产品代码、feature 组合或发布流程，不作为性能改善或跨平台运行态验收证据。

## Android 完整性能采集

Android benchmark 复用桌面场景清单与 runtime 测量器，通过 ADB 逐项冷进程运行、
回传原始结果。73 个支持的必测 render 场景；视频/桌面窗口尺寸 5 项明确标为不支持，
另有 7 次启动、opening/相机拆分及真实 APK 挂载 I/O。压力场景明确不含视频。
运行命令、温度/后端/尺寸证据与非独占 CPU 归因边界见 [Android guide](android.md#完整-android-benchmark-与-adb-回传)。
不以移动 GPU 必须满帧为门槛；在可接受功耗/温度、分辨率与视觉质量下比较真实负载。

本地实测：Motorola edge 70 max / Android 16 / Adreno 829 / Vulkan，物理显示
1440×3168；以下完整命令完成 124 次样本、73/73 个支持场景，0 failed，5 项明确不支持：

```sh
python3 target/keine-android-arm64-local-benchmark/benchmark-android.py \
  --apk target/keine-android-arm64-local-benchmark/game.apk --serial DEVICE_SERIAL \
  --output target/moto-edge-70-max-benchmark-auto-full
```

报告与逐次日志、原始帧、设备/温度信息均自动回传。
当前窗口接口未提供刷新预算，报告保留 unknown；pass 时间与 CPU/RSS 仍可分析，
不能把 VSync 限帧下的 interval 排名或空的超预算排名解释为没有热点。

实际验证：`cargo test --offline --workspace --features publisher,video-native,hot-reload,startup-metrics`
1045 passed / 21 ignored / 0 failed（IPC/Trash 在沙箱外）；相同功能的 workspace
check/all-targets Clippy、无默认功能 check/Clippy、Android ui-sounds/startup-metrics Clippy、
fmt、native-smoke 与派生 fixture validate 通过。Python collector 9 passed、packaging
11 passed；Gradle assembleRelease/lintRelease、APK 图标/16 KB/签名/NOTICE/计划校验、
匹配 host 符号库的 `.text` 校验及 workflow actionlint 通过。此次真机只测试 Vulkan；
GL、演出画面/触摸与音频听感不由性能采样代替验收，远端新 workflow 尚未运行。

## 桌面包辅助文件与 Linux ABI

`2423505` 将 Linux 发行 runner 从 `ubuntu-latest` 改为 `ubuntu-26.04`，用户的旧系统
随后无法加载程序/附带库所需的 GLIBC 2.42/2.43 和 `GLIBC_ABI_DT_X86_64_PLT`。
发行游戏/benchmark/Editor 与桌面 Linux CI 改用 `ubuntu-24.04`，明确 GLIBC 2.39
上限；发布前扫描每个 ELF 的 version needs，超过上限或要求未知/private ABI 即拒绝。
libmvec、libresolv、libnss、libanl、libutil 与 libc/loader 一样由宿主提供。

Windows DLL、benchmark PDB/dSYM、Python 采集器和启动标记统一放进 `lib/`。
Windows shipping Engine 内嵌私有 assembly 依赖，打包的 `lib/lib.manifest` 列出 SDK
DLL；用受限 PATH 和不相关 cwd 启动包内 Engine 验证，避免构建机 SDK 掩盖漏包。
Editor 的匹配 Preview Engine 同用此布局，临时 Build 导出沿用已有 lib 树复制。
新布局的 marker、默认游戏路径、移动后采集与 ELF/DLL 边界纳入既有回归。

实测验证：workspace check/all-targets Clippy（publisher,video-native,hot-reload,startup-metrics）、
fmt、native-smoke validate、workflow actionlint 通过；完整 workspace 测试
1045 passed / 21 ignored / 0 failed，IPC/Trash 在 macOS 沙箱外重跑；Python packaging
12 passed、benchmark collector 9 passed。用 NDK Clang/LLD 生成真实 ELF version-needs
边界样本，GLIBC 2.39 正确接受，2.43 正确拒绝；这仅验证门禁，不代替发行程序启动。

此批修复以 0.14.3 发行。Windows/Linux 新包须按对应提交的远端 CI 和实机运行分别验收；
旧 Ubuntu 26.04 基线的下载不能用于验证修复，源码检查也不能代替新包实机验收。

持续维护门禁：每日审计、桌面 CI 和所有发布入口共用 locked/all-features cargo-deny；
手动 tday/Editor 也须按源提交重新审计。Dependabot 的 Cargo 仅提出分组安全修复，
Actions 每月一个普通更新分组 PR；关闭自动 rebase/合并，更新仍由正常 CI 审查。
Linux 的最终 Engine/benchmark/Editor ZIP 与正式游戏目录，在 Ubuntu 24.04 runner
检查全部 ELF 和 executable 动态库，并加载 Engine 与游戏；普通发布直接检查最终包，
隔离 SDK 的容器复核移到手动 **Verify Linux Download**，复用现有 ZIP。
游戏验证传入 `game.haku` 文件；无游戏快照的 Editor 包验证 native-smoke 开发工程目录。
容器验证不声称窗口、GPU 或音频验收；完整真机 benchmark 仍单独运行。

CI 耗时审查的原始基线：Release Engine #61 Linux，`cargo build --release --locked
--no-default-features --features publisher --target-dir target/runner` 828s，
`target/runner/release/keine bundle ... --benchmark` 935s，最终容器检查 31s。
该运行用的是旧 CI 的 `8c5d04e`，cache miss 且验证失败后没有保存 cache；不是修复版
手动 tday #60（`7c81f94`，四平台及发布已成功）。实查缓存总量约 9.45 GiB，单个
Linux CI cache 4.44 GiB。调整缓存副本/调试信息、publisher 构建和过时发布拦截，
详见 [发布 guide](release.md#ci-成本与失败重试)。这些是结构性修复；新配置的实际
编译耗时、cache 大小和命中率需远端运行后比较，不声称已实测提速。
Windows Editor 的旧自动构建另在打包后出现 WinError 14001；将真实 DLL 私有 assembly
绑定检查前移到 SDK 安装后，并将两端身份明确为当前发行目标 amd64。本机未执行
Windows activation context；仍需 Windows runner 验证，不将静态检查记为该错误已修复。

## 表达式诊断与深度预算

在 `1dc025b` 上复现 `scene x { let a = ) == 1 }`：分组分类栈弹空后
`last_mut().unwrap()` panic。现在多余、错配及未闭合括号返回准确行/列诊断，
插值诊断也映射到实际出错 token；Editor 投影可报告错误，修正源码后恢复正常 blocks。

`MAX_EXPRESSION_DEPTH = 64` 由 core model 统一定义；原生 parser 限制递归嵌套，
并在构造时迭代检查树深度，覆盖无括号的长二元运算、索引和 `.length` 链。
typed 求值逐层检查实际访问的子表达式（短路规则保留），超限返回
`ExpressionTooDeep`；兼容字符串求值仅补相同递归保护，不扩展语义。
回归覆盖 64/65 层与 4096 次嵌套、一元运算、列表、索引、二元链、插值及 Editor 修正恢复。

```sh
cargo test --offline -p keine-loader expression_ -- --nocapture
cargo test --offline -p keine-core
cargo test --offline -p keine-editor malformed_expression_projection_recovers_after_source_correction
cargo test --offline --workspace --features publisher,video-native,hot-reload
cargo check --offline --workspace --features publisher,video-native,hot-reload
cargo clippy --offline --workspace --all-targets --features publisher,video-native,hot-reload -- -D warnings
cargo check --offline -p keine --no-default-features
cargo build --offline -p keine --features publisher,video-native,hot-reload
target/debug/keine validate tests/fixtures/native-smoke
target/debug/keine validate projects/tday
target/debug/keine --version
```

验证：专项 Loader 3 passed、Core 123 passed、Editor 1 passed；工作区
1051 passed / 21 ignored / 0 failed（包含沙箱外 IPC/Trash）；check、Clippy
与无默认功能 check 通过；当前 Engine debug 构建通过，native-smoke 与 tday 均零警告，
`--version` 输出 `Kēne 0.14.2`。依赖版本、IR schema 与 Save v11 未改变。
原生 Editor 交互、Windows/Linux/Android 运行态及 release 二进制尚未复验；
以上表达式预算不作为嵌套 statement 或编译包反序列化的完整防护证明。

## 发行声明合并与 Android 沉浸模式

- 桌面游戏包、Editor 安装包、Build 临时导出和 Android APK 共用单个发行 `NOTICE`。
  完整保留引擎许可、原署名、字体文本、已收集的 native 版权/专利/源码地址和游戏版权；
  工程根目录的 `LICENSE` 作为 `GAME-LICENSE` 章节，不再额外复制 `TDAY-LICENSE`。
  Editor 导出沿用已安装 Engine 的 native SDK 声明；项目声明通过受根目录约束的 mount 有界读取。
  Android staging 清除之前生成的零散文件，游戏包之后重建引擎测试包不会残留游戏版权。
- Activity 补上系统导航栏隐藏，创建、恢复及获得焦点时应用；API 30+ 使用原生
  WindowInsetsController，API 26–29 使用 immersive sticky，无新增 AndroidX/Kotlin 依赖。
  系统边缘滑动仍可临时呼出导航栏；不拦截 Home/返回手势。

验证：workspace publisher/video-native/hot-reload tests 为 1046 passed / 21 ignored / 0 failed
（IPC/Trash 在沙箱外运行）；workspace all-targets Clippy、check、fmt 和 diff 检查通过。
Python 包装回归 13 passed，覆盖原文/换行完整保留、缺失或截断 native 文本拒绝、缓存清理、
游戏→引擎声明切换、Linux/Windows SDK 版权合并。native-smoke 0 warnings。
Android 离线 debug/release 组装与 lintDebug 通过，两份引擎 APK 的 assets 只有 `NOTICE`；
使用既有 tday 密文资源重组的 release APK 通过完整内容/图标/入口/16 KB 校验，
声明根目录只有 `NOTICE`，包含完整 shiftz 游戏版权。macOS Editor app 组装、签名及内置声明检查通过。
APK 包装验证复用了已有原生库，未重新编译当前分支的 ARM64 原生库或覆盖安装手机；
Pixel 的手势条隐藏、临时呼出、输入法及系统文件选择器/前后台恢复仍待新版实机验收。
Windows/Linux 发行目录通过包装回归检查，未在对应系统重新组装或原生运行；远端 CI 未触发。

## 演出中存档与音频输出恢复

- 普通新槽位、覆盖存档和快速存档共用即时检查点选择：现场可完整恢复时保存现场，
  否则保存最近一个可恢复 action 边界。创建瞬态演出前捕获边界，连续 forward batch
  中的变量修改保留，读档重新执行演出入口；不推进现场、不等待演出结束。
- 同 Program 检查点才能回退；缺少/外部 Program 检查点不覆盖已有槽位。
  回退卡片文本取检查点的当前/上一句，删除旧缩略图且不截取较晚现场；Save v11 不变。
- Bevy 音频同版 patch 保留逻辑 mixer、Player 和解码器，只重建设备 stream。
  回归覆盖无设备静音、不推进源位置、断流恢复、暂停与音量保留、不同物理格式、
  Windows 无错误的默认 endpoint 切换、CoreAudio 健康路由不重建、退避上限。
  淡化时钟按健康 stream 提交时间计算，输出缺失不会消耗淡入/淡出或持续要求重绘。

验证命令：

```sh
cargo test --workspace --features publisher,video-native,hot-reload
cargo clippy --workspace --all-targets --features publisher,video-native,hot-reload -- -D warnings
cargo clippy -p keine --no-default-features --all-targets -- -D warnings
cargo ndk -t arm64-v8a -P 26 clippy --locked -p keine --lib --no-default-features --features ui-sounds -- -D warnings
cargo deny --all-features check
target/debug/keine validate tests/fixtures/native-smoke
target/debug/keine validate projects/tday
```

验证结果：workspace 1043 passed / 21 ignored / 0 failed（IPC 在沙箱外运行）；
fmt、workspace all-targets Clippy、Engine debug 构建、Android ui-sounds Clippy、
all-features cargo-deny 通过。native-smoke 与 tday validate 均为 0 warning。
无默认音频的 all-targets Clippy 覆盖关闭 Rodio 直接依赖的测试边界。

当前手机截图及用户确认定位到旧版“当前演出结束后才能保存”提示。
自动回归不能代替新版手机实际存档/读档、蓝牙/有线/USB 切换听感、Windows/Linux
原生默认设备切换、Android 前后台路由和恢复时间验收。未覆盖安装当前手机 APK。


## 应用图标派生

release 图标校验回归：本地 `assembleRelease` 复现源码路径被替换成 `res/BW.xml`、`res/TO.png`，
旧脚本失败；校验改为沿 manifest ID → compiled resource table → adaptive XML → foreground → bitmap。
修复后同一 release APK 和 debug APK 均通过 `verify-icons.py --apk`。
`python3 -m unittest discover -s tests/packaging -p 'test_*.py'`：8 passed，覆盖原路径/缩短路径、
资源名不同、悬空 manifest 引用、缺失 bitmap 和错误密度尺寸；Android workflow actionlint 通过。
CI Android 增加同一原生库的 release 包资源校验；不关闭 AAPT2 优化，也不跳过图标检查。
同一次 `clean assembleDebug assembleRelease lintDebug` 使用派生资源通过，release APK 的 16 KB ZIP 对齐通过；
fmt、workspace check/all-targets Clippy、publisher/video-native/hot-reload workspace tests（1016 passed / 21 ignored）
和 native-smoke validate（0 warning）复验通过。此处不代表新远端 tday release 已发布或手机画面验收。

`cargo test --workspace --features publisher,video-native,hot-reload`：1016 passed / 21 ignored / 0 failed
（IPC/Trash 在沙箱外运行）。同组合 fmt/check/all-targets Clippy、Engine/Editor debug 构建、
无默认功能 + ui-sounds 的 Android Clippy，以及 all-features cargo-deny 均通过；native-smoke validate
为 1 scene / 1 action / 1 source / 0 warning。
PNG/WebP 回归覆盖派生一致性、透明隐藏 RGB 不产生彩边、尺寸/输入大小限制、路径越界/缺失、
原工程不改写、发行配置移除图标源引用及临时导出保留图标。包装 Python 测试 4 passed。

`python3 dev/scripts/build-icons.py target/icons-final-review` 与 `verify-icons.py --icon-dir target/icons-final-review`
通过；macOS `iconutil -c iconset` 可读取生成的全部 ICNS 条目。
现有 JDK/SDK/Gradle 离线运行 `assembleDebug lintDebug -PengineIconDir=.../target/icons-derived-review/android`
通过；`verify-icons.py --apk .../app-debug.apk` 检查实际 adaptive 资源和 manifest，zipalign 16 KB 检查通过。
workflow actionlint 通过（本地规则补充已有 ubuntu-26.04 runner 名称）。
这些是格式、构建与包内容证据；Windows EXE/桌面、Linux 应用菜单、macOS Dock 及 Android launcher
的实际显示尚未重新目测；远端新 CI 未触发，APK 未覆盖安装到手机。已有 Android 缩略图/滑块修改保持不变，tday 未改写。

## UI 模块与资源页

Editor 的面板状态与视图归各功能模块，工作台保留分派、Dock 和共享输入；文件/场景名
Enter 提交移到输入事件，回归确认渲染不提交、Enter 只提交一次，且不改原剧本。
Engine 的 Settings、Save/Load、Dialog 分离 state/view/actions/motion/sync，保留系统入口与顺序；
槽位截图仍在 UI，后台编码和落盘归 storage，沿用有界队列与代际保护。

存档截图相机复用 `DesignViewport` 的逻辑画布尺寸，排除手机宽屏/桌面窗口的 letterbox；
回归覆盖 3168×1440、HiDPI、宽屏及高窗口的取景范围和 480×270 目标。
滑块数值气泡同时读取捕获的拖动状态，回归覆盖实时更新、仅当前滑块显示、松手隐藏和鼠标悬停。
`cargo test --workspace`：947 passed / 21 ignored / 0 failed；fmt、workspace check/all-targets Clippy、
Android Clippy 与 release APK 校验通过。修复包已同签名覆盖安装，Vulkan 启动日志正常；
用户已确认手机滑块气泡持续显示、实时更新和松手隐藏正常；新存档缩略图仍待实测，
旧缩略图须重新存档生成。

Assets 增加可移除筛选标签、结果/选择计数、导入及多选操作、空结果提示和状态跳转。
回归确认清除筛选保留视图偏好与跨筛选选择。macOS 原生新版已检查统计卡、搜索无结果及恢复、
Missing 筛选、多选/清除选择，以及约 190 逻辑像素窄栏中计数完整、操作按钮整组换行。
导入/删除沿用已有事务回归，本次未在真实工程执行；Engine 菜单及 Windows/Linux 原生视觉未复测。

`cargo test --workspace --features publisher,video-native,hot-reload`：993 passed / 21 ignored / 0 failed；
IPC 沙箱权限失败后完整放行重跑。fmt、上述组合的 workspace check/all-targets Clippy、
无默认功能 + video-native check、Engine/Editor debug 构建通过；native-smoke validate 为
1 scene / 1 action / 1 source / 0 warning。tday、依赖清单和 lockfile 未改动；
验证偏好已逐字节恢复，临时 App 已清理。模块拆分不作为性能提升的证据。

## Editor 动效

展开/收起 UX：资源统计卡、资源筛选分组、文件树和搜索分组共用 160 ms 可反向过渡；
统计卡/筛选复用 GPUI 的测量展开，文件树/搜索用实际动画行高的前缀位置做可见区裁剪。
Block 右键菜单补淡入/淡出及关闭任务代际保护，旧菜单动画和 Build 旋转遵循减少动态效果。
检查范围与即时反馈规则见 [Editor 交互](editor.md#交互)。
GPUI 回归覆盖资源统计卡带动实际列表 viewport、快速反向无跳变、窄栏换行重新测量、
减少动态效果直接到达端点、Block 菜单关闭后快速重开不被旧任务清除；几何边界覆盖
部分行高、零高行、空列表及滚动底部。Computer Use 未发现可操作的运行中 Editor；
fmt、workspace check（publisher/video-native/hot-reload）、all-targets Clippy、
workspace tests 992 passed / 21 ignored（IPC 在沙箱外）及 Editor debug 构建通过。
原生动效目测仍待重启新版，三平台的验收不由这些布局测试代替。

Block 插入弹窗：180 ms 轻微上移淡入、90 ms 关闭淡出；右上角 X 与 Esc/Tab/选择条目共用关闭流程，
沿用 Editor 动画和减少动态效果设置。Favorites 置顶并默认打开，All 放底部；预置常用收藏，
收藏按自身顺序显示。自定义列表改为拖动柄排序，复用 Block 预览卡、200 ms 位移插值和边缘滚动，
占位让位后只在松手保存偏好。GPUI 鼠标回归覆盖跨分类多位置移动、拖动中不写入、
外部取消、分类切换使拖动失效、点击不插入剧本及偏好落盘；位移回归覆盖中途改目标连续性。
持久化回归覆盖旧收藏保留、旧空收藏补入默认及新版主动清空。fmt/check、workspace all-targets Clippy、988 项工作区测试
（21 ignored，publisher/video-native/hot-reload，IPC 在沙箱外）与 Editor debug 构建通过。
Computer Use 未发现可操作的运行中 Editor；本次未做原生拖动/动画目测，Windows/Linux 交互仍待实机验收。

设置页数据管理简化为“还原设置 / 清除存档”，保留分别确认，移除整项目数据清除入口及其专用路径。
简体、繁体、日语、英语的标题、按钮和确认文案已同步；存档清除沿用保留设置的现有回归。
workspace tests（publisher/video-native/hot-reload）984 passed / 21 ignored，fmt、check 和
workspace all-targets Clippy 与 Engine debug 构建通过；IPC 在沙箱外运行。本次界面尚未原生目测。

Editor 关闭确认：统一 macOS NSAlert 的 1000 起始返回值与 GPUI 的零起始按钮索引，
修复 Close Without Saving 被当作 Cancel；共享流程中恢复草稿写入失败不再取消不保存关闭。
真实 GPUI 窗口回归覆盖 Cancel 保留窗口/修改、Save and Close 保存并移除窗口、
不保存关闭不改源文件，以及恢复目录不可写时仍关闭；后一边界在修复前复现失败、修复后通过。
workspace tests（publisher/video-native/hot-reload）985 passed / 21 ignored，fmt、
workspace all-targets Clippy 与 Editor debug 构建通过；IPC 在沙箱外运行。
macOS 新 Editor 原生已验证 Cancel 后保留草稿，再选择不保存关闭后进程退出；
Windows/Linux 原生弹窗尚待新构建复测。临时 app 与验证产生的 Editor 偏好/恢复记录已清理或恢复。

对白排版：默认字号及 tday 显式字号 45→43，字形行高 1.2→1.1 倍，flex 行间距
10.5→2.5 设计像素；保留原文本框尺寸和按钮位置。macOS 新 Engine 用截图中的三行
实际文本检查，第三行完整且不覆盖底部按钮。workspace tests（publisher/video-native/hot-reload）
983 passed / 21 ignored；fmt、all-targets Clippy、Engine debug 构建与 native-smoke validate 通过。
临时验证剧本已逐字节恢复，临时 app/存档已清理；Windows/Linux 排版未实机复测。

## 窗口与菜单输入

Android 触摸由一个共享 owner 捕获，控件和列表不转为导航；设置 tab 仅点击切换，左右划识别已移除；剧情上划 backlog、
backlog 正文拖动滚动及系统返回复用现有屏幕操作。回归覆盖轻点松手只推进一次、上划不推进、
多指/取消/失焦/路由与剧本变化/边缘拒绝、折返与斜划、控件/列表不导航，以及 HiDPI 滑块
拖出轨道仍夹取自身数值、拖动中不写盘、松手与弹窗打断才持久化。
控件松手才点击，拖动不触发起点按钮；命中回归覆盖父级裁剪和层叠遮挡，滚动 owner
仅识别实际 scroll overflow。手势超时使用单调时钟，不随游戏时钟暂停或唤醒重置改变。
`cargo test --workspace --features publisher,video-native,hot-reload`：1008 passed / 21 ignored / 0 failed；
系统回收站/IPC 在 macOS 沙箱外重跑。fmt、workspace check/all-targets Clippy、Android
`cargo ndk -t arm64-v8a -P 26 clippy --locked -p keine --lib --target-dir target/android --no-default-features --features ui-sounds -- -D warnings` 通过。

唤醒动画回归用 `cargo test --lib waking_animation_does_not_consume_sleep_from_the_render_clock -- --nocapture`：
模拟两条休眠前渲染时间戳，修复前新动画第二帧吃进 250 ms；恢复时清空队列并重置真实时钟后，
60/120 Hz 的步进分别为 16.667/8.333 ms，两者 100 ms 累积进度一致。原始输出在
`target/android/animation-wake-before.log` 和 `target/android/animation-wake-after.log`。
保持菜单原时长；手机反馈确认动画、滑块、返回、剧情上划与退出重开正常；左右划切 tab
未通过，按用户要求完全删除。最终测试 APK 的 JNI 返回入口、
ELF LOAD/RELRO 与 APK 16 KB 对齐、签名、空 Java runtime classpath 均通过；已安装到
Motorola 测试机，`target/android/gestures-phone-final.log` 确认 Native Smoke ready / Adreno 829 Vulkan。
该 fixture 仅一句对白，不能作为长 backlog 列表滚动的真机验收。

设置页避免重复写 `Node.display` / `UiTransform`，保持原动画。原始前后命令相同：
`cargo test --lib settled_settings_panels_do_not_dirty_layout -- --nocapture`；60 次静止帧更新后，
每帧被标记布局变化的面板从 4 个降为 0 个。
原始输出在 `target/android/tab-layout-before.log` 和 `target/android/touch-regression.log`；
移除左右划后的回归在 `target/android/no-horizontal-tests.log`；覆盖设置/Save/Load 横划无导航，
以及菜单空白处不触发下层控件。
这证明重复变更已消除，不代表手机切 tab 的帧率；移除左右划后的测试包仍需单独确认。

Android 后台恢复：旧包日志显示 Home 后点击桌面图标以 LAUNCH_MULTIPLE 新建第二个
EngineActivity，随后进程被结束。入口改为 singleTask，复用活动中的 Activity；本次目标为
保留进程内当前界面/剧情，不新增冷启动自动恢复。安卓导入/导出使用 SAF 文档选择器，
JNI 转交唯一文件描述符，公共存储逻辑支持无 seek 的有界流。回归覆盖桌面/流格式互通、
读取失败时恢复旧事务、截断/无效备份保留旧数据、导出 flush 失败。最新完整测试在
`target/android/backup-tests.log`，check/Clippy 和 all-features cargo-deny 均通过；
增加的直接 jni 依赖已有于锁文件闭包中，没有新增 crate 或 Java runtime 依赖。
最终 APK 构建、Java lint（0 errors / 5 既有 warnings）、16 KB/签名/JNI/singleTask 检查通过，
Java runtime classpath 为空；已安装，21 个既有用户数据文件逐个 SHA256 一致。
打包检查记录在 `target/android/backup-package.log`，真机运行记录在
`target/android/backup-phone-runtime.log`。用户确认后台往返和移除横划正常；系统日志也确认
桌面图标两次复用活动中的进程（LAUNCH_SINGLE_TASK）。清除最近任务后系统 remove task
结束进程属于冷启动边界。设置按钮漏点阻挡了文件选择器测试，导出/导入/取消仍未验收。
漏点专项重现了透明 MenuHeader 整页容器误捕获，以及普通按钮被 800 ms 手势超时拒绝；
触摸现在遵守 FocusPolicy::Pass/Block，按钮松手点击保留拖动/裁剪/遮挡检查但不受手势超时限制。
专项前后原始记录为 `target/android/control-pass-before.log` / `control-pass-after.log`，
按住超过两秒的旧版失败记录为 `target/android/control-hold-before.log`；修复后的同一系统回归通过。
修复后完整 workspace 回归仍为 1008 passed / 21 ignored / 0 failed，记录在
`target/android/control-pass-tests.log`；workspace check/Clippy、Android Clippy、fmt/actionlint 通过。
新 APK 的 JNI/16 KB/签名/singleTask 与 Java lint 检查通过，记录在
`target/android/control-pass-package.log` / `control-pass-java-lint.log`。本机 ADB 在沙箱内
无法创建 USB interface plug-in；沙箱外重启后恢复通信，已安装新包，21 个用户文件校验一致
（`target/android/continue-phone-{before,after}.sha256`）。漏点修复与备份文件选择器的
最终真机验收待用户确认，日志在 `target/android/control-pass-phone-runtime.log`。

非 Stage 右键使用现有 Esc 返回路径；Stage 仍只切换文本框，存档删除改为悬停后 Delete 并确认。
窗口正常关闭时单独保存大小、位置与最大化状态；首次打开适配显示器，恢复时处理 DPI 改变、
显示器移除及越界位置，不把全屏/最小化尺寸写成普通窗口大小。benchmark/隐藏启动不受此策略影响。
Wayland 的窗口位置由合成器决定，Windows/Linux 原生恢复与首次显示尚待实机复测。

Load 重建卡片时立即套用当前透明度，修复缓存跳过更新造成的不透明首帧；内容与背景使用同帧淡入进度。
回归覆盖重建首帧、各菜单返回、窗口边界/DPI/显示器移除、持久化与只读会话。
`cargo test --workspace --features publisher,video-native,hot-reload` 为 983 passed / 21 ignored
（IPC/Trash 在沙箱外运行）；最终全屏保护调整后窗口专项 6 passed。
fmt、workspace all-targets Clippy、上述功能组合 check、无默认功能 + video-native check、
Engine debug 构建及 native-smoke validate（0 warning）通过。
macOS 原生已检查设置/Load 右键返回、标题退出确认及取消、Stage 右键不推进对白；
1404×790 窗口移动、关闭重开与全屏往返后保存的边界逐字节一致。
Load 重开已观察正常显示，首帧修复由回归验证；不将静态截图记作逐帧闪屏验收。
本次临时 app 和 native-smoke 生成的存档已清理，tday 未改写。

## 发布渠道验证

缓存 key 单独将 feature 分隔逗号编码为连字符，Cargo feature 参数保留原值；Release 与
Project Release 的 key/restore-key 已按三平台、三个 feature 组合验证。Linux Editor 打包
保留已复制库到原 SDK 文件的来源映射，新增回归覆盖第二个 executable 的 RPATH 复用及
未解析依赖拒绝。上次 CI 成功，Release 三平台缓存失败；Editor macOS/Windows 成功、
Linux 来源查找失败，修复后的远端构建和附件发布尚待确认。

`Release` 分开发布 benchmark 自动包与 tday 手动试玩；`Editor Release` 发布独立
Editor + 同版本 Preview。actionlint 1.7.12 对本次两个 workflow 检查通过，shell 语法与
Python 编译检查通过。用本轮已构建的 macOS debug 二进制执行 `package-authoring.py`，
产出单个 App；ZIP 解压后两 executable 的执行位、`codesign --verify --deep --strict`
和内置 Engine 的 native-smoke validate 均通过（1 scene / 1 action / 0 warning）。
输出目录冲突被拒绝，未覆盖已有包；无游戏包/私钥文件。此次没有另建持久化安装 App。
这些证据确认打包布局，不等于 release profile、Windows/Linux 动态库打包或 GUI 验收通过；
远端三平台构建和下载更新须按实际 Actions 结果核对。此前 engine/editor 回归结果见下文。

## 剩余事项与分工

本轮审计、预取预算、密钥文件改名、跨平台废纸篓与进程采样已实现。
Windows/Linux 新代码已交叉编译；三平台 CI 的运行结果须按本次推送的提交核对。
下面列的是实际剩余动作；已通过的本机输入、拖放、音频循环和全屏目测无需重新整轮验收。

迁移资源分类修复：workspace（publisher，含默认音频）835 项通过、8 项性能测试忽略；
fmt/check/clippy、native-smoke 与 tday 校验通过。回归覆盖背景层分类、同路径别名去重、
sprite 引用背景及错误音频类型拒绝、Editor 增量引用计数和资源选项。tday 的 45 张误分类背景
已移动并更新清单，保持文件字节、ID 和剧本不变；新 Editor 的目录、预览和资源下拉已目测。
完整剧情验收仍按下表安排。

迁移文件名与 Explorer 排序：publisher workspace 856 passed / 8 ignored，fmt/check/Clippy、Editor 构建、native-smoke 与 tday validate 通过。回归覆盖源文件名/子目录保留、中文路径、大小写重名、路径隔离，以及入口优先、场景自然排序、多场景文件和增量更新。现有 tday 的 84 个资源恢复原名，168 个原素材/转换成品 SHA256 和 13 个剧本均未变，ID 与引用保留；新版 Editor 已目测原名缩略图、`bedroom.opus` 及全部 13 个剧本的排列。tday 保留 1 条未引用背景警告。

章节接续与右键菜单：publisher workspace 857 passed / 8 ignored，fmt/check/Clippy、Editor 构建及 native-smoke 校验通过；追加菜单焦点修正后 Editor Clippy 和 208 项测试通过（7 项性能测试忽略，IPC/Trash 放行重跑）。线性章节隐式结束回归覆盖下一章、空末章、禁用章、预处理与辅助 fragment 返回；迁移后的实际执行覆盖主线→支线→返回→下一章→结束。当前 tday 的 13 个剧本仅追加章末 goto/结束，删除新增内容可逐字节恢复原正文；validate 为 13 scene / 2659 action，保留原有 1 条未引用背景警告。macOS 新版实测微小滚动不关闭菜单且不滚动下方卡片，点击外部和 Esc 正常关闭；Preview 从 start.shou 章末进入 1.shou，下一章画面、对白及 Editor 文件定位正常。完整剧情仍由用户后续验收。

tday 图片复用清理：逐文件 SHA256 与解码后 RGBA 核对，50 张登记图片中 15 份完全重复；合并 19 处引用后为 35 张图片、69 个资源，保留场景参数和当前正文。重复原素材/成品移到 `projects/.backups/tday-image-merge-20261004`，保留文件哈希未变。validate 为 13 scene / 2659 action / 0 warning；Editor 已显示 69 files / 136.6 MB。登记媒体由 177,273,165 降至 143,211,885 bytes；使用隔离测试密钥执行 `target/debug/keine pack projects/tday --output target/authoring/tday-image-merge-{before,after}`，两次成功，实际 `.haku`/`.taku` 合计由 143,371,230 降至 143,369,230 bytes，仅减少 2,000 bytes：现有 Hakutaku 已按内容去重，此次没有修改引擎。

Build 临时试玩导出：`cargo check --workspace --features publisher`、
`cargo clippy --workspace --all-targets --features publisher -- -D warnings`、
`cargo test --workspace --features publisher`（862 passed / 8 ignored；IPC 放行沙箱重跑）、
Editor 与含 publisher/video-native 的 Engine debug 构建通过；native-smoke 与 tday validate
分别为 1 scene / 1 action、13 scene / 2659 action，均无警告。
macOS 新 Editor 从 Build 选择目录导出 tday，显示 Export ready；输出 13 个剧本、69 个媒体及
4 个 YAML，86 个项目文件均与源文件 SHA256 一致，未复制原素材或私有文件。
导出的 Game.app 独立启动，标题菜单、Start 后的排练室背景与对白已目测。
目录冲突、越界路径、失败清理和开发版默认工程查找有回归；Windows/Linux 实机导出、
音视频与完整剧情未验收。临时试玩不使用发行密钥，正式发行仍按 release.md 的 bundle 流程。

背景转换 Q80：`cargo test --workspace --features publisher` 为 862 passed / 8 ignored，
Clippy、fmt、Editor 构建以及 native-smoke/tday validate 通过。导入回归覆盖 Q80 有损编码、
尺寸/透明度/ICC/EXIF 保留、只读原文件和立绘/粒子无损编码；已有合规 WebP 仍直接复制。
tday 的 31 张背景用 `cwebp -q 80 -m 4 -alpha_q 100 -metadata icc` 从匹配的原 PNG 转换，
登记背景由 70,275,364 降至 4,444,576 bytes，全部媒体为 77,381,097 bytes。
124 个其他项目文件哈希未变，原 WebP 备份在 `projects/.backups/tday-background-q80`。
新版 macOS Editor 已显示 69 files / 73.8 MB（界面使用二进制单位），夕阳背景预览正常。
检查时误触重置布局产生重复栏目，已修复重置遗漏外围 dock 的问题，并恢复三列布局。
旧试玩导出仍保留旧资源，需重新导出；本轮没有新 release 包或完整剧情验收。

引擎快捷键：publisher workspace 868 passed / 8 ignored，无默认功能 + video-native
的 Engine lib 282 passed / 1 ignored；fmt、workspace Clippy、含 publisher/video-native/hot-reload
的 workspace check 与 Engine debug 构建通过。Ctrl 按住自动播放、松开停止，保留正常打字
速度与自动播放间隔；A/Ctrl+A 快捷键取消，画面自动播放按钮保留。回归覆盖持续按键不重启
自动播放计时、组合键抑制、输入框/弹窗/加载隔离、失焦后不恢复自动播放、弹窗中的停止，
右键只在舞台切换文本框且不推进对白，以及 Esc 和全屏状态切换。
macOS 新 Preview 已目测右键隐藏/恢复文本框、A 不激活自动播放；此前已目测 H 文本框开关、
B 历史、Esc 设置与关闭、F5/F9 确认框及取消、F11 全屏往返。Ctrl 持续按住与输入框边界由回归测试验证，
未执行物理键盘持续按键及 Windows/Linux 原生快捷键验收；键位见 readme.zh.md。

### 常用创作能力收尾

原生粒子稀疏控制、blink/talk 帧序列、角色清单编辑入口，以及迁移的条件/选项、
候选素材登记、静态头像、BGM loop、富文本和未知时间轴诊断已实现。
本轮 `cargo test --workspace --features publisher` 为 879 passed / 8 ignored；
Unix socket 测试先被沙箱阻断，放行后完整重跑通过。
`cargo check --workspace --features publisher,video-native,hot-reload`、workspace Clippy 和 fmt 通过；
`cargo test -p keine --no-default-features --features video-native --lib` 为 284 passed / 1 ignored，
最终粒子颜色 alpha 修正后 10 项粒子回归通过；native-smoke validate 零警告。
回归包含分支内变量改变不执行 else、有序赋值失败不部分提交、空选项继续、
迁移→原生解析→执行、动态帧边界、blink 休息保持低频计时、粒子数量上限/零速有限值、
GPU 颜色透明度与缓冲更新、Save v11 瞬态回放标记及回滚保留参数。

Editor debug 构建与本地 app 组装通过。Computer Use 两次读取新版 app 超时，
本轮未完成原生界面及动画目测；此限制不算 GUI 验收通过。
未做新的 CPU/帧时采样、完整剧情或 Windows/Linux 原生运行验收；不主张性能提速或完整上游兼容。
`projects/tday` 未改动。新能力使用方式及明确的导入边界见 language.md / compatibility.md。

Editor 补全与角色维护已补齐：复用 Loader 命令/参数表，原生候选支持命令、上下文参数、
资源原文件名/ID、角色、章节、变量及合法值；悬停提供参数提示。Inspector 原子切换动态序列模式，
角色面板支持增改删、对白引用、头像/表情帧关联、批量图片登记和显式剧本插入。
0.13.0 的 `cargo test --workspace --features publisher` 为 889 passed / 8 ignored，IPC/Trash 测试在沙箱外执行；
`cargo check --workspace --features publisher,video-native,hot-reload`、
`cargo clippy --workspace --all-targets --features publisher -- -D warnings` 与 fmt 通过。
Editor debug 构建和 app 组装通过；`keine validate tests/fixtures/native-smoke` 为 1 scene / 1 action / 0 warnings。
macOS 原生实测自动候选、Enter 接受、Esc 关闭及 Alt+/ 手动菜单；Ctrl+Space 在本机未触发，
可能受到输入法快捷键拦截，保留 Alt+/ 备用入口。实测角色新增、改名、删除确认，以及空根清单 `{}`
的首次新增；在既有临时工程中选图登记表情/头像并在记忆的剧本位置插入 Show，原文保持完整。
验收输入已丢弃，fixture 和 `projects/tday` 未改动。中文已提交文字输入正常；本轮未重做物理 IME
组词验收，组合态保护由 GPUI 事件路径和回归覆盖。blink/talk 的新界面入口有解析/参数联动回归，
动态画面尚未目测；场景构图、时间轴和变量调试按用户要求暂缓。

DL1 与依赖许可门禁：cargo-deny 0.20.2 的 `--locked --all-features check advisories licenses bans sources`
通过，保留上游重复版本/路径依赖警告；DL1 条款与官方原文一致，仅替换版权主体。
fmt/check/Clippy、publisher workspace 889 passed / 8 ignored、无默认功能 + video-native
284 passed / 1 ignored 通过；IPC/Trash 测试在沙箱外执行。最新 publisher 组装回归、workspace
bins 构建与 native-smoke 零警告通过。macOS Editor/Engine 开发 app 的三份许可文件逐字节匹配，
ad hoc codesign 验证通过；未执行正式签名/公证、远程 CI 或跨平台发行验收。
合成字体只改变 name/head 表，其余 13 个表的 SHA256 未变；没有主张新的 GUI 目测验收。
第三方正式发行许可清单和字体来源验证边界见 [发布](release.md#许可证与发行署名)。tday 未改动。

### 项目所有者（用户）

| 何时 | 你要做什么 | 完成标准 |
|---|---|---|
| 后续剧情验收 | 在 Editor 打开 `projects/tday`，从入口播放完整剧情，检查分支、画面、字幕、BGM/SE/语音及自动/手动继续。问题反馈带章节、场景或 Block 编号、操作步骤和预期结果 | 确认剧情与视听效果符合原工程意图；缺失的 3 个 SE 按已批准方案跳过并标注 |
| 有多屏设备后 | 提供可测试的显示器与缩放配置，协助跨屏移动 Editor/Preview、切换全屏并检查输入与坐标 | 开发者完成跨屏复验并记录配置与结果；此项继续延后 |
| 正式发行前 | 决定发行平台与时机；备份游戏 `publisher.key`；发行 macOS 版时准备 Developer ID Application 证书与 notarytool Keychain profile，提供名称供打包使用 | 发行凭据就绪；具体准备步骤见 [发布](release.md#发行前的用户准备) |
| 准备收尾时 | 审阅当前改动并明确授权提交/推送 | 开发者完成提交、推送及 CI 核对；不需要你手动修改引擎代码 |

### 后续开发者 / 维护者

| 工作 | 具体动作 | 完成标准 |
|---|---|---|
| 集成收尾 | 审查当前 diff，保留无关修改，按下方门禁复验；用户授权后提交推送，检查 Linux/macOS/Windows CI，修复真实失败 | 记录提交 SHA 与 CI 结果；不能把本机交叉编译当作远程 CI 通过 |
| 下一阶段：Extra 页面设计（未开始） | 按下方设计范围完成 CG/BGM 鉴赏页方案，确认后实现与验收 | 布局、交互、多语言和实际设备验收分别记录；当前存档与音频恢复的实机验收仍需完成 |
| Windows/Linux 与显示验收 | 在对应系统原生运行 Editor/Engine；验证入篓→撤销→重做、同名冲突保护、CPU/内存采样与进程重启；检查保存重开、IME、Preview、音视频。用可用设备补测 1× DPI、Preview 极端比例；多屏设备到位后再测跨屏 | 各系统/显示配置分别记录通过、失败与未测；CI 单元测试不能替代 GUI/音视频实机验收 |
| 独立性能阶段（仍延后） | 复用现有 bench、`cargo perf` 和 portable benchmark，测 Editor 滚动/大文档/搜索、启动与持续 CPU/RSS、Preview CPU/GPU 帧时、粒子全屏/瞬时峰值、媒体流与 Hakutaku I/O；先定位实际热点，再修改 | 同主机、同输入、同 release 配置保留前后原始命令/日志，报告 p99、最大帧时及超过刷新预算的帧数。结合硬件、分辨率和可承受开销评价视觉质量与性能，窗口切换单独标记；平均 FPS 或 p99 单项不能证明零掉帧 |
| 正式发行 | 在发行副本规范化存量 tday 媒体为 WebP/Opus 并保持 ID/引用，原工程只读；复用稳定 project.id 与 identity，验证并完整打包。macOS 在用户凭据就绪后执行正式签名/公证；测试解压、独立安装启动、Engine discovery、媒体播放、用户数据位置与更新 | 发行包满足生产格式，签名/公证检查通过，包中没有私有 identity；实际安装运行通过后才记录为发行验收完成 |

### 下一阶段：Extra 页面设计

状态：计划，尚未开始。先交付可审阅的布局与交互方案，再实施；入口为 Engine 的 Extra 鉴赏页，
现有实现位于 `src/ui/screens/extra.rs`，已有 CG 分页/全图切换、BGM 播放/暂停/停止/切曲/拖动进度和页面动效。

- **页面布局**：优先设计 CG / 音乐分区切换，替代固定各占一半的布局；明确标题、返回、内容和播放器的层级。
  复用设计空间与 viewport 转换，覆盖桌面窄窗口、宽屏及手机横屏；文字、分页与触控按钮保持可读、可操作。
- **CG 鉴赏**：缩略图网格、名称与分页统一；全图保持原比例，明确上一张/下一张/关闭。
  设计空列表、加载中、资源缺失状态；现有数据只记录已解锁项目，不凭空显示未解锁总数或隐藏资源。
- **音乐鉴赏**：曲目列表支持长名称与滚动，播放器稳定显示曲名、播放状态、时间和进度；
  明确进入、切曲、停止与退出时鉴赏音频和舞台 BGM 的衔接，沿用现有音频输出恢复机制。
- **交互与文案**：统一打开/关闭与切换动效；Esc、右键、Android 返回先关闭全图，再返回上级；
  防止点击穿透。空状态、错误、按钮提示同步所有现有语言，图标配合简短文字。
- **实现边界**：按实际职责整理 Extra 的状态、视图、交互与动效，复用已有 UI 支持；
  保持 `gallery.unlock` 和独立持久化语义，读档不撤销鉴赏解锁。

验收：覆盖零项、单项、多页 CG、大量曲目、长名称和缺失资源；验证 CG 切换、播放/暂停/拖动、
退出后的音频状态及重启后的解锁保留。必要边界回归通过后，在 macOS、Windows、Linux 和 Android
分别记录原生布局、返回路径与视听结果；构建通过不算画面或听感验收。

维护者只按当前架构与已选政策收尾：Save 仍严格匹配 fingerprint，预取保持现有数量
限制及 128 MiB 非关键预算，合法包版本回退允许，密钥仅显式轮换。
不把跨版本存档迁移、自动轮换或额外框架列为欠缺功能。
状态与证据更新本指南；密钥/发行步骤更新 `release.md`，不另建重复计划或阶段报告。

## 每次集成

从当前 checkout 的分支、worktree list、status、diff 开始。保留未提交修改。

```sh
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate tests/fixtures/native-smoke
```

Feature 变更再检查相应 feature；发布者/媒体路径使用：

```sh
cargo check --features hot-reload,video-native,video-ffmpeg,publisher
```

Linux 单独运行 `cargo test --locked -p keine-editor --lib` 时，Editor 的测试依赖必须
自行启用 Bevy `x11`，不能依赖 workspace 构建时引擎的 feature 合并。Linux 依赖树已确认
包含 `winit/x11`；本机 fmt/check/clippy、publisher workspace 835 项测试通过（8 项忽略）。
完整 Linux 交叉检查受本机缺少 `x86_64-linux-gnu-gcc` 限制，远程 CI 仍需重跑确认。

### Windows renderer

Windows HD 4600 / `Gl` 旧包已通过远程原生窗口复现：设置/存档页背景清晰且底色缺失，
启动日志没有 shader 错误。三层相机默认 4× MSAA，已发现后续空透明 pass 的 resolve
可能覆盖单采样纹理上的模糊和 UI；现统一为 `Msaa::Off`，不更改模糊参数与合成顺序。
回归覆盖 runtime 与三个 benchmark 相机组合；fmt、workspace check/Clippy、workspace
866 passed / 8 ignored 通过（IPC 在沙箱外重跑）。macOS Engine debug 构建及 native-smoke
validate 通过（1 scene / 1 action / 0 warning）；新版 Windows 是否恢复及 macOS 未显现
异常的具体原因尚待实机比对，MSAA 覆盖尚不能作为此次故障的已验证根因。

Windows shipping / benchmark 编译 DX12、Vulkan 和 OpenGL；原生 GL 复用锁定的
wgpu 29.0.4 WGL 实现，不附带 ANGLE 或额外渲染 DLL。`bevy_render/gles` 与 Linux
共用同版本 feature 合并，CI 检查 Windows 三项 backend 均存在。
启动沿用 Bevy 的 `Backends::all()` 与 HighPerformance 默认值：wgpu 在所有可用
backend 中按 device type 排序，硬件 GPU 优先于 CPU 软件 adapter，窗口兼容性
仍由 wgpu 检查。没有自建 adapter 选择器、重启或降级特效框架；显式环境变量
`WGPU_BACKEND`、`WGPU_ADAPTER_NAME`、`WGPU_FORCE_FALLBACK_ADAPTER` 仍按上游规则生效。
实现依据锁定源码 `bevy_render/src/settings.rs`、`wgpu-core/src/instance.rs` 和
`wgpu-hal/src/gles/{mod,wgl,adapter}.rs`；Bevy 0.19.1 文档中的 Windows ANGLE
说明未反映此锁定 wgpu 的 WGL 实现。

HD 4600 的 Intel Windows 驱动不支持 Vulkan，较新驱动撤销 DX12；OpenGL 是这台
机器的硬件兼容路径（[Intel API 表](https://www.intel.com/content/www/us/en/support/articles/000005524/graphics.html)、
[DX12 撤销说明](https://www.intel.com/content/www/us/en/support/articles/000057520/graphics.html)）。
已有舞台、遮罩和模糊 shader 使用 uniform/纹理采样；粒子
额外要求 vertex storage buffer（GL 4.3/相应扩展及实际 limits），不能把 backend
编译通过当作全部 shader 已被该驱动接受。不改变 1920×1080、镜头合成、粒子或特效语义。

实体机先默认运行，再在 PowerShell 中做 GL 对照：

```powershell
Remove-Item Env:WGPU_BACKEND, Env:WGPU_ADAPTER_NAME, Env:WGPU_FORCE_FALLBACK_ADAPTER -ErrorAction SilentlyContinue
.\keine.exe
Copy-Item .\keine-benchmark-report.txt .\benchmark-auto.txt
$env:WGPU_BACKEND = 'gl'
.\keine.exe
Copy-Item .\keine-benchmark-report.txt .\benchmark-gl.txt
Remove-Item Env:WGPU_BACKEND
```

核对两份报告的 `GPU` / `GPUINFO` 为实际 Intel adapter（GL 对照应为 `Gl`），
不是 `Microsoft Basic Render Driver · Cpu`；CPU adapter 的计时只能算软件渲染。
GL GPU timestamp 未提供时记为 unavailable，不补零。基本包缺少 authored timeline
时，粒子/特效/压力项仍为 skipped，完整验收必须提供这些时间轴。
用户原始 Windows 报告 `26100601.txt`（b772dc0）为软件 Dx12：opening continuous
606.19 ms / 1.6 FPS、scene-only 386.76 ms / 2.6 FPS，不能作为硬件 GPU 基线。
新版 HD 4600 的 adapter、画面、粒子与性能验收 **NOT TESTED**；测试包需在新提交的
CI / Release 完成后取得，旧包仅设置 `WGPU_BACKEND=gl` 不会增加未编译的 backend。

本机验证：`cargo check --locked -p wgpu-hal --target x86_64-pc-windows-gnu` 和
`cargo check --locked -p bevy_render --target x86_64-pc-windows-gnu` 通过，包含 WGL；
Windows 的 dx12/vulkan/gles、Linux 的 vulkan/gles feature closure 均通过。
macOS shipping check、publisher/video-native/hot-reload workspace check、publisher
workspace Clippy、fmt/actionlint/cargo-deny、native-smoke 零警告和采集器 7 项通过；publisher workspace tests
899 passed / 8 ignored（IPC/Trash 在沙箱外执行）。完整 Windows Engine target check
在 native C 依赖因缺少 `x86_64-w64-mingw32-gcc` 受阻，仍需 Windows CI；没有新性能
采样，不宣称比软件 Dx12 的旧报告提升了多少。

### Linux 窗口、renderer 与 portable benchmark

kids（openSUSE Leap 16 / Mesa 24.3.3 / HD 4400）的旧 tday 包在 Wayland/GL
和 XWayland/GL 均由用户复现 `Fifo, Options: []` 崩溃。SSH 只读检查确认
Wayland EGL 支持硬件 GL 4.6，但 Vulkan loader 没有可用 ICD；不能要求切换
Vulkan作为现成解决方案。Bevy 0.19.1 的 instance 初始化固定 `display: None`，
wgpu 29 的 Mesa EGL 因此进入 surfaceless 分支，生成不能呈现的 surface。

现对官方 `bevy_render` 0.19.1 做局部 vendor patch，游戏和启动错误页均在
RenderPlugin 前传入 winit owned display；GPU recovery 复用同一连接。
保留原 public 初始化 API、无窗口调用和后端选择规则；未增加 unsafe 或运行时依赖。
源文件/上游许可证与差异范围见 `dev/vendor/bevy_render/KEINE-PATCH.md`。
新增 descriptor 的连接转发/生命周期及无窗口回归，并接入三平台 CI；新版
Linux 实机启动和效果验收待构建后完成，当前不记为通过。

本轮 fmt、workspace check/Clippy、actionlint、all-features cargo-deny 通过。
workspace 单元/集成测试 889 passed / 8 ignored（含 renderer 23 项），另有上游
doctest 18 passed / 13 ignored；IPC 在沙箱外执行。Linux/Windows renderer 的
gles 交叉 check 与 Linux raw_vulkan_init check 通过；macOS Engine debug 构建、
native-smoke validate（1 scene / 1 action / 0 warning）与 640×360 短窗口运行
通过，实际为 Metal / Fifo / 120 个渲染样本。此项只确认初始化与正常退出，
不作为性能改善或 Linux 画面验收证据。

Linux shipping / benchmark 同时编译 native Wayland 与 X11（含 XWayland），以及
wgpu Vulkan 与 native OpenGL/GLES compatibility backend；Bevy 保持 0.19.1。
Linux target 的同版本 `bevy_render/gles` 合并到既有 wgpu，未使用 `webgl2`。
正常启动沿用 Bevy/wgpu 自动选择；下面只用于测试或故障排查：

```sh
WGPU_BACKEND=vulkan ./keine
WGPU_BACKEND=gl ./keine
# Wayland 桌面上测试 X11/XWayland，须有可用的 DISPLAY。
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET WGPU_BACKEND=vulkan ./keine
# 便宜的 feature closure 检查，不编译或启动 GUI；CI 检查相同的四项。
cargo tree --locked --target x86_64-unknown-linux-gnu -p keine --no-default-features --features ui-sounds -e normal,build,features -i winit
cargo tree --locked --target x86_64-unknown-linux-gnu -p keine --no-default-features --features ui-sounds -e normal,build,features -i wgpu
```

依赖树必须包含 `winit/x11`、`winit/wayland`、`wgpu/vulkan` 与 `wgpu/gles`。
Linux CI/Release 共用 setup-video action 安装 `libwayland-dev`。
GL 驱动需提供 EGL 和满足 wgpu/Bevy features/limits 的 OpenGL/GLES；编译进 backend
不代表所有老 GPU 均能运行。选择规则沿用
[Bevy WgpuSettings](https://docs.rs/bevy_render/0.19.1/bevy_render/settings/struct.WgpuSettings.html)
及锁定版本的 `settings.rs`，不增加重启或软件渲染兜底。

无参数启动 portable benchmark 时，package root 是 `current_exe()` 的父目录；
`game.haku`、marker `keine-benchmark.conf`、`keine-benchmark-report.txt` 均在该目录，
Hakutaku 按 snapshot 位置寻找 sibling `data/`，与启动 cwd 无关。Linux executable
名为 `keine`；搬移时保留整个包，含可能存在的 `lib/`。Python collector 按自身所在目录
的 marker 自动发现包、`keine` 与 `game.haku`，不访问 Git 或作者工程；显式 `--output`
等相对参数仍相对调用者 cwd；无参数启动引擎的 suite 报告写在包旁。

报告保留实际 GPU/adapter、Vulkan/Gl backend、OS 和 architecture。Linux 新增
`WINDOWSYS`，来自 primary window 的实际 raw handle；Wayland 表示 native Wayland，
X11 表示应用使用 X11，无法仅靠 handle 区分原生 X server 与 XWayland。
缺少 handle 会明确输出 unavailable；`XDG_SESSION_TYPE` 不作为 native Wayland 证据。
Python metadata 同样保留 GPU 与 WINDOWSYS 日志。

本轮 macOS 验证：default / `ui-sounds` locked check、publisher/hot-reload/video-native
workspace check、publisher Clippy、fmt、collector 7 项与 native-smoke 零警告通过；
default / 无默认功能 publisher workspace tests 的 IPC 沙箱失败已放行后完整重跑。
Linux 四项 feature closure 与 all-features cargo-deny 通过，但两条 Linux target check
均在 `alsa-sys` 因缺少交叉 sysroot/pkg-config 配置受阻；Linux 编译及远程 CI 尚未通过验收。

实体机验收 **NOT TESTED**：openSUSE Leap 16 / KDE Plasma Wayland /
Intel HD Graphics 4400 / Mesa，至少分别跑 A native Wayland + Vulkan、
B native Wayland + `WGPU_BACKEND=gl`、C X11/XWayland + Vulkan；从无关 cwd 启动
解压后搬移的包并检查报告、资源与 collector。记录实际 adapter/backend/WINDOWSYS
及失败的驱动原因，不把 llvmpipe 或 hosted runner 编译通过当作实体 GPU 验收。

### CI 的工作范围与缓存

每次推送保留依赖政策、Linux fmt/Clippy/workspace 与 publisher 测试、benchmark
采集器回归，三平台检查规范发行 features；macOS/Windows 合并视频、进程指标和
系统 Trash 单元测试，仍实际链接并运行视频验收。Linux 的 workspace 测试已包含
Editor 平台测试，不再另跑同一组。完整优化编译/打包由 Release / Project Release
执行，常规 CI 不重复编译整个 release profile；发行优化参数保持不变。
macOS 另检查无音频 `video-native` 测试的编译边界，避免默认音频掩盖意外的 rodio 引用。
main 的成功 CI 会触发 Release 更新 `benchmark-latest`；发布必须分别确认构建、
附件上传、滚动 tag 和 Release 提交匹配，不能把 CI 成功当作发布完成。

Windows job 总上限为 180 分钟（不是闲置计时），Linux/macOS 为 60 分钟。
main 的运行中 CI 不被后续推送取消；同组只保留最新待运行提交，其他分支/PR
继续取消过时任务。Rust 缓存在普通测试失败后也保存，取消/硬超时不保证保存。
常规 CI 不注入发行 identity；发行 workflow 仍独立清理含密钥的 Engine/Loader 产物。

Windows FFmpeg SDK 仅编译动态 Release 库：ffmpeg-sys-next 使用 vcpkg-rs 的
`installed/<triplet>/lib`，不使用 `debug/lib`。SDK 缓存按 manifest、triplet 与提交建键，
允许恢复同配方的前次缓存；vcpkg 自行校验 ABI，编译器变化引发重建后可保存新键。
改为 Release-only 的首次运行须重新编译；SDK 安装成功即保存，不等待 Rust 测试。

调整前 CI #238（`722dc7f`）Windows 冷缓存在 90 分钟超时：FFmpeg 的 Release / Debug
分别约 10m47s / 10m15s，视频单测 31m31s、视频验收 17m32s、独立 Editor 指标
测试 15m46s，之后才开始完整 release 编译。原命令及日志摘录保存在
`target/performance/ci-audit/`；调整后的 Windows 时长待新 CI，不能以本机暖缓存
结果推算跨平台加速。

本机新命令验证：fmt/check/Clippy、默认 workspace 845 passed / 8 ignored、合并平台
单测 20 passed、AVFoundation 文件/Hakutaku 视频解码、规范发行与无音频 native 测试 check、
benchmark collector 6 项、native-smoke 零警告通过；Windows SDK 与新总耗时待远程 CI。
工作流 YAML 与 actionlint 检查通过；发行模板的 `job.workflow_*` 字段由 GitHub 官方
文档确认支持，actionlint 1.7.12 对这三个字段的误报仅在本次本地检查中精确排除。
提交 `e9cec0d` 的远程 CI #239 与 Media safety 已全部通过，包括 Windows 平台测试、
视频验收和规范发行 feature check；GUI / 硬件 benchmark 验收仍独立。

## CI 命名

Workflow 使用空格分隔的 `类别 范围`：`CI Desktop`、`CI Android`、`CI Media Safety`，
以及 `Release Engine`、`Release Editor`、`Release Project`。
Job 使用 `阶段 平台 架构`（如 `Check Linux x64`、`Build macOS arm64`），
专项检查使用 `Check 对象`，发布辅助 job 使用 `Prepare Source` / `Publish 产物`。
步骤以 Check out、Install、Restore、Check、Build、Package、Upload、Publish 等动词开头。
文件名、job ID、产物名称保持稳定；发布监听 `CI Desktop`，concurrency group
固定为原队列标识，改名不会绕开仍在运行的旧任务。

## Android Engine

实验性 ARM64 Engine 使用 `--no-default-features --features ui-sounds`，安卓明确不支持视频。
默认先初始化 Vulkan，surface/adapter/device 返回错误时释放该次资源并重试 GLES；
单独指定 gl/vulkan 时保留指定后端。游戏与缺失资源提示页使用同一平台配置。
独立 `CI Android` workflow 构建 release 原生库及 debug 签名 APK，检查桌面/Editor/FFmpeg 依赖隔离、
Vulkan/GLES feature closure、NativeActivity 启动符号、ELF LOAD/RELRO 与 APK 的 16 KB
对齐、APK 签名与硬件声明，再上传 artifact；可单独手动触发，不等待桌面 CI 队列。
构建入口、资源部署及限制见 [Android guide](android.md)。

本机 NDK 28.2.13676358 交叉 Clippy、原生库链接、APK 打包及上述校验通过；无新增 Rust
依赖，Cargo.lock 未变。视频 feature 在安卓上明确拒绝编译，存储回归覆盖稳定项目 ID、
私有目录及路径逃逸拒绝；renderer 回归验证不存在 adapter 时返回可恢复错误。
GLES 交叉 Clippy、raw_vulkan_init 交叉 check、双后端 APK、LOAD/RELRO 显式 16 KB
链接与 APK 原生段一致性校验通过。workspace check/all-targets Clippy、989 项本机工作区测试（含并行 Editor 修改，21 ignored，
publisher/video-native/hot-reload，IPC/Trash 在沙箱外）、native-smoke 零警告、全特性
cargo-deny 和 CI actionlint 通过。远程 Android CI 按对应提交的 Actions 结果核对；手机画面、触摸、音频、
后台恢复与性能均未验收；APK 校验只证明构建产物，不代表运行态支持已经完成。

`Release Engine` 的 `package=tday` 增加 Android ARM64 release APK，与三平台 ZIP 一起通过后才发布。
本机完整打包已通过（13 scenes / 2619 actions / 0 warnings），APK 约 130 MiB；
`verify-android-release.py` 检查独立应用 ID、非 debuggable、内置加密资源不压缩、许可声明、
启动/JNI 入口和 ELF LOAD/RELRO 16 KB 对齐，zipalign/apksigner 均通过。
有界 APK 读取回归拒绝相邻条目与整数溢出，Loader 回归验证平台源保留验签、目录索引和流式读取。
完整 workspace 回归 1010 passed / 21 ignored / 0 failed，记录在
`target/android/tday-full-tests.log`；workspace fmt/check/all-targets Clippy、Android
`hardened,ui-sounds` Clippy、全特性 cargo-deny、actionlint 和 native-smoke validate 通过。
已安装到 Motorola 测试机，`target/android/tday-phone-runtime.log` 确认 tday ready /
Adreno 829 Vulkan；云端发布结果与手机画面/触摸验收继续单独核对。
CI 默认功能组合的 Clippy 发现 APK 边界测试误用 publisher 的可选 tempfile；测试改为标准库
打开后 unlink 的匿名文件，不新增依赖。默认 workspace Clippy 和 tests 重跑通过，
`target/android/tday-default-clippy-after.log` / `tday-default-tests.log` 保存原始结果，
publisher/video-native/hot-reload 组合的同一 APK 边界回归也通过。

## 测试布局

```text
tests/
├── authoring/process.rs       真实 Engine IPC / 生命周期 / 协议 / GPU Preview
├── coverage.rs               冻结兼容输入与 Stage 属性/事件覆盖
├── letsgal/sample.rs          本机官方示例资源与内容完整性
├── fixtures/
│   ├── native-smoke/          最小 tracked 原生工程
│   ├── native-benchmark/      三平台同负载原生性能工程
│   ├── letsgal-timeline/      舞台/镜头/退格/音频回归与 benchmark 场景
│   ├── webgal-showcase/       只作 parser / IR 冻结回归
│   └── video/                生成的合法与损坏视频
├── bench/                    core、loader、runtime、save、backup、video
├── fuzz/                     独立 nightly workspace，生产 WebP decoder
└── video/acceptance.rs        文件系统与加密 Hakutaku 视频验收
```

各 crate 的私有 unit tests 贴近 owner；Editor 的真实单实例进程回归位于
`crates/editor/tests/instance.rs`。不再保留只测试上游 Dock prototype 的 spike。
保留格式边界、路径隔离、事务/恢复、协议、执行和资源回归；不为了界面目测新增测试工程。

输入边界回归覆盖 Editor 文档/恢复草稿限量读取、三类原生清单超限拒绝、
真实签名 Hakutaku 配置的 256 KiB 边界和编译 Program roundtrip；编译头超限/长度错配
在读取正文前拒绝，读取中增长或截断也必须失败。Unix 路径回归在检查后确定性替换
文件/父目录，验证外部链接拒绝、已打开句柄稳定、目录内链接与搜索权限保留、FIFO 不阻塞。
这些不代替 Windows/Linux 上对应系统调用的运行态验收，也不证明整个进程的内存上限。

持久化回归覆盖实际字节总量、备份危险/冲突文件名、包内 `.tmp` 名碰撞、
有效 CRC 下 metadata/state 尾部垃圾拒绝、独立域严格解码、非有限设置恢复默认值，
以及源文档已保存后的草稿清理失败。恶意备份拒绝时原存档保持不变；合法 v11/V2 格式不变。
状态恢复沿用 fingerprint、游标/调用栈校正及独立域保留回归；WebP 补测损坏与截断输入。
本轮 workspace 780 项通过、8 项显式忽略；publisher lib 320 项、video-native 无默认音频 8 项、文档 18 项通过。
fmt/check/clippy、可选 feature 构建和 native-smoke validate 通过。IPC 沙箱权限失败后在放宽沙箱的同一 checkout 重跑通过。

官方 LetsGal 示例验收和 loader benchmark 通过 `KEINE_LETSGAL_PROJECT` 指定外部原工程；
不依赖本地 demo。`projects/tday` 是唯一忽略的开发 demo，CI 打包默认使用 tracked native-benchmark；native-smoke 保留作最小回归。

显式 Cargo `path` 注册嵌套 tests/benches，参见 [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)。

```sh
cargo bench -p keine-core
cargo bench -p keine-loader
cargo bench -p keine
cargo +nightly fuzz run --fuzz-dir tests/fuzz webp tests/fuzz/corpus/webp -- -max_total_time=60
cargo run --no-default-features --features publisher,video-native --bin keine-video-acceptance -- tests/fixtures/video/playback.mp4
cargo run --no-default-features --features publisher,video-native --bin keine-video-acceptance -- --expect-error tests/fixtures/video/damaged-header.mp4
bash dev/scripts/video-fixtures.sh
```

Linux 使用 `video-ffmpeg`；CI 另跑 FFmpeg ASan。fuzz 的目录参数由
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/options.rs) 提供。
视频 fixtures 只有生成的 pattern/tone：1 秒 H.264+AAC、无音轨、4 秒长 GOP、尾部 moov、截断头。
合法文件必须读到 EOF 并可 rewind，损坏头在 FS/加密包中均必须拒绝。

## 性能

### 三平台原生完整套件

自动 Release 的 Windows、macOS、Linux 包统一使用 `tests/fixtures/native-benchmark`，
不再用只有一句对白的 smoke。同一份 EYS v2、WebP、Opus、1080p H.264 素材
经过正常 publisher → Hakutaku → 解码/播放 → MainCore UI/场景渲染路径；macOS 视频
用 AVFoundation，Windows/Linux 用 FFmpeg。素材自生成，不包含 tday 私有内容。
71 个作者场景加 7 个运行模式/显示对照，共 78 个必测负载：

| 范围 | 场景 |
| --- | --- |
| 对照与日常 | 同画面无特效基线、中文/Latin 打字与自动翻页、双立绘移动/缩放、背景切换 |
| Shader | 32 个独立特效族、4 个经典镜头分组、8 个属性/事件组合 |
| 粒子 | 雪/雨/花瓣、80/256/768 密度、自定义纹理、大尺寸填充、多层 emitter |
| 媒体 | Opus 循环/淡入淡出/语音/SE、1080p 视频全屏/混合/循环/音轨 |
| UI/立绘 | 浮动文字、幕布、滤镜、环境光开关、16 项选择、输入、设置/存档/读档/80 条历史/鉴赏面板 |
| 压力与显示 | 粒子+视频+镜头+媒体、720p/1080p 粒子、实际屏幕全屏；对白/粒子/音频/视频正常 runtime 模式 |

另测 7 次隔离启动、开场正常休眠/唤醒、连续绘制、3 个 camera 分解及真实资源+
204.2 MiB 确定性 payload 的暖缓存 I/O。阶段预热 3 秒、采样 5 秒；基线、粒子、
主要组合热点跑 3 次，保留每次原始帧及中位数。总计 121 次 render 采样，加 7 次
启动和 1 次 I/O，共 129 次子进程采样；仅 render 预热/采样需要 16 分 8 秒，
进程启动、GPU 初始化、首帧与退出另计，不能把采样窗口当作整套预计时长。
关闭其他重负载，保持显示模式固定；窗口切换等排除记录不可删除。

桌面父进程以墙钟限制整套 30 分钟，启动/render 单次最多 60 秒，I/O 最多 120 秒；
单次超时终止并回收子进程，剩余总预算允许时继续后续负载。总预算耗尽停止启动
剩余采样，明确标记 `INCOMPLETE` 并返回失败，不减少场景或把超时当作通过。
控制覆盖子进程初始化、采样和退出，避免首帧未出现或退出卡住时无限等待。
`PROGRESS` 显示当前采样序号和累计时长；`CHILD` 记录每次完整墙钟耗时，便于
区分固定的 8 秒预热/采样与初始化/退出开销。每次采样之间追加并刷新报告，强制中断
后仍有已完成结果；运行中的 raw 行可与摘要交错，正常结束再原子写入最终布局。
慢机器触及预算可能得到不完整报告，这不表示低帧率本身不合格。

时长保护验证：`cargo test --offline --workspace --features
publisher,video-native,hot-reload,startup-metrics` 1047 passed / 21 ignored / 0 failed
（IPC/Trash 在沙箱外）；相同功能的 workspace check/all-targets Clippy、无默认功能
check、fmt 通过。新增两个回归验证单项/全套超时、部分输出与报告保留、预算耗尽后
不启动下一项，以及超过管道缓冲区的输出不会阻塞。尚未在 Windows/Linux 上实跑
这版完整套件；以上测试不证明用户那次一小时运行具体卡在哪项，也不代表帧性能改善。

`COVERAGE` 必须是 78 completed / 0 missing / 0 failed；单项失败继续后续场景，
保留部分报告并返回失败。正常播放必须有前台样本；全程后台暂停不能算作已测。
启动/开场/I/O 的失败也会使套件返回失败，原始报告仍保留。
自选工程同样必须提供目标，不能用缺测结果冒充完整套件。
三平台 CI 都启用进程采样 feature 与本平台媒体后端，检查全部目标唯一、引用完整、
可重放、透明素材有效及原生打包映射；Linux 也执行 FFmpeg 播放合同。CI 构建/媒体合同
测试不代替实机 GPU 性能验收。存读档事务、回退和 IME 的正确性仍由对应回归测试验证，
面板场景只测真实 UI 负载；Editor 大文档/拖动基准仍在独立 Editor 测试中。

报告首先定位优化候选：`HOTSPOT` 按同画面基线的帧时差排序并给复测命令；
结合各负载的 `PROCESS` CPU、`MEMORY` RSS、`UPDATE` 脚本/场景/布局/UI 调度耗时、
`RENDER` CPU/GPU span 与 `SLOW` 源码位置区分方向，再采调用栈确认函数。
UPDATE 是墙钟窗口，包含等待及并行重叠，不能加总作独占 CPU；Fifo 帧时会受垂直同步
限制，帧时相同也需比较 CPU 和阶段耗时。时间戳不可用明确标出，不能写零。
Windows 自动调用栈尚未实现，可用 PDB 与外部 profiler；这项缺口不冒充已覆盖。

```sh
cargo validate tests/fixtures/native-benchmark
cargo test -p keine --lib portable_benchmark_
# 包内执行；无需源工程/Python，完整套件生成 keine-benchmark-report.txt。
./keine
# 定位候选后，同硬件/尺寸/后端复测，再单独采栈，避免 sampler 扰动基线。
python3 profile-runtime.py --timeline bench_particle_layers_768 --window 1920x1080 --mode continuous --seconds 30 --stacks off --output particle-baseline
python3 profile-runtime.py --timeline bench_particle_layers_768 --window 1920x1080 --mode continuous --seconds 30 --output particle-stacks
```

当前本地证据：macOS / Apple M5 Pro / Metal / 120 Hz，优化且保留符号的包完成
74/78 个负载、7 次启动和暖缓存 I/O；4 个 `runtime` 对话/粒子/音频/视频负载全程
失焦，按合同判失败，整套结果为 INCOMPLETE，不能称为全覆盖验收。
Windows/Linux 新套件尚待实机运行，修改后的远端 CI 尚未执行。
报告在 `target/performance/native-0131-verified-benchmark/keine-benchmark-report.txt`，
69,882 条含排除记录的帧已转换为同级 `native-0131-report-json/`。
基线 CPU 三次中位数 14.81%，组合压力 21.53%（一核为 100%）；两者帧时均约
8.33 ms，因此优先进一步分解 CPU/渲染提交，而不能据 FPS 判断没有优化空间。
GPU span 本机不可用，未填零。另采的 scene+UI 调用栈成功且输入哈希未变，
主线程多为事件等待，采样覆盖启动且会扰动结果，暂未确认单个函数热点。

实际命令（bundle 的一次性测试 identity 通过 `KEINE_HAKUTAKU_IDENTITY` 指定，
测试后删除；不使用开发/发行密钥）：

```sh
CARGO_NET_OFFLINE=true target/debug/keine bundle tests/fixtures/native-benchmark --output target/performance/native-0131-verified --benchmark
target/performance/native-0131-verified-benchmark/keine
python3 dev/scripts/profile-runtime.py --report target/performance/native-0131-verified-benchmark/keine-benchmark-report.txt --output target/performance/native-0131-report-json
python3 dev/scripts/profile-runtime.py target/performance/native-0131-verified-benchmark --binary target/performance/native-0131-verified-benchmark/keine --mode continuous --camera scene-ui --seconds 10 --output target/performance/native-0131-scene-ui-stacks
```

代码门槛已通过：workspace 909 测试（8 个既有 ignored）、collector 7 测试、
fmt/clippy、原生项目与 smoke validate、原生及 FFmpeg feature check、Actionlint。
IPC 测试先遇到本地沙箱 PermissionDenied，提权重跑完整 workspace 后通过。

### 实际运行热点采样

`tests/bench` 保留调用真实模块的基准；真实热点使用同一个 `perf` 入口与系统调用栈。
原 `runtime-hotspots` 只测了复制的算法和空操作，无法反映现有引擎，已由本采集流程替换。
默认 `--mode runtime` 保留正常休眠/失焦策略，采集只在预热结束和测量结束增加唤醒；
`--mode continuous` 才持续绘制，并允许 camera 分解和循环选中的时间轴。
`--scene ID --cursor N` 可定位原生章节的实际 action，cursor 从 0 开始；不存在或无法
重放的目标报错退出，不能悄悄改测开场。不会修改项目或保存用户状态。

```sh
# macOS：与 release 相同优化，保留符号供系统 profiler 解析。
# Linux 视频改用 video-ffmpeg；需安装对应开发库。
cargo build --profile profiling --no-default-features --features bundled-opus,ui-sounds,startup-metrics,video-native --bin keine
python3 dev/scripts/profile-runtime.py projects/tday --output target/performance/baseline --seconds 30 --stacks off
python3 dev/scripts/profile-runtime.py projects/tday --output target/performance/stacks --seconds 30
# 动态场景/全屏粒子用实际章节和 action；先由 Editor 核对目标。
python3 dev/scripts/profile-runtime.py projects/tday --scene scene_0002 --cursor 10 --mode continuous --output target/performance/scene --seconds 30
python3 -m unittest discover -s tests/bench/runtime -p 'test_*.py'
```

输出目录必须不存在，旧结果不会覆盖。`metadata.json` 保留命令、主机、构建工作区状态、
二进制和源码清单哈希、退出状态及引擎汇总；`frames.json` 保留每帧 scene、next cursor、
源码行、窗口物理尺寸、刷新预算、活动/焦点状态与排除原因；`slow-locations.json` 按超预算
时间定位剧本位置，它是相关性，不能替代调用栈或宣称该 block 本身耗时。
next cursor 是 Core 下一条指令的位置，源码行对应刚执行的 action；不是 Editor 视觉序号。
活动状态来自本帧更新后的 lifecycle；是否经历休眠以上一帧安排的活动状态为准。
源码增删改、非零退出、引擎 ERROR 或缺少逐帧输出均使采集失败；结果不能冒充有效基线。
哈希清单覆盖项目目录内的剧本/配置；外部 mount、素材和用户设置须保持相同，不能宣称已冻结全部输入。
运行窗口需保持前台；若全程失焦，保留排除记录及进程 CPU，但没有活跃帧时结论。

系统调用栈在 macOS 用 `sample` 的 10 ms 间隔与 `-mayDie`，Linux 用 `perf record`
的 99 Hz DWARF 栈；未安装/权限失败明确记录，Windows 自动调用栈采集尚未实现。
采样涵盖进程启动，需区分初始化、活跃函数和系统等待；macOS `sample` 是所有线程的栈
命中次数，不能当作函数的精确 CPU 百分比。安装了支持 Rust 的 `c++filt`/`llvm-cxxfilt`
时另输出 `stacks-readable.txt`，原始符号和源码位置保留；缺少工具明确记录。
Linux 在本机用 `perf report -i <输出目录>/perf.data` 阅读调用栈。
系统采样会扰动被测进程，性能门槛以
`--stacks off` 同条件基线为准，另一次栈采样用于定位。

独立 benchmark 包完整复用上述采集 owner：增加单独的正常 runtime 休眠/唤醒样本，
连续负载和 camera 分解另测；报告 RAWFRAME 附录保留全部 14 项逐帧字段及 workload/run。
被排除的间隔同样保留。benchmark 单独附加源码文件名/行列映射，与 Program 指纹和 action
数量校验匹配；有界读取上限 4 MiB，拒绝尾部字节、绝对路径与不匹配表。它不包含剧本正文，
不改变正式 Program envelope/schema。没有映射的普通发行包不冒用 Loader 的合成 line 1。
包内附同一 `profile-runtime.py`，省略 project/binary 时自动使用旁边的 Engine 和 Hakutaku
包，不要求 Git 或作者工程；采集前后验证 game.haku 与所有 .taku 内容哈希。

```sh
# 在解压后的 benchmark 目录中；output 必须尚不存在。
python3 profile-runtime.py --output baseline --seconds 30 --mode continuous --stacks off
python3 profile-runtime.py --output stacks --seconds 30 --mode continuous
# 将自运行 suite 的完整逐帧报告转为 JSON，不重复跑分。
python3 profile-runtime.py --report keine-benchmark-report.txt --output report-json
```

benchmark 用 profiling 优化/符号构建，不启用阻断系统采样的 hardened；普通发行不变。
自运行 suite 无需 Python，调用栈作为单独受扰动采样；Windows 自动栈采样和不支持的 GPU
阶段计时仍明确缺测。完整渲染 coverage 依赖所选项目已有时间轴，缺少时不得声称全覆盖。
正常 runtime 样本使用可见窗口，持续负载可使用隐藏窗口；保持各自模式，不能混合比较。
采样的 reactive wait 在同一休眠阶段保持稳定，避免递减倒计时不断触发 Bevy 重排程；
临近边界时有界缩短等待，避免 winit 的提前唤醒重复等待整个测量区间。

Bevy 组件统一为 0.19.1；Core/Loader/authoring 仍不依赖 Bevy。版本修复覆盖文字测量/性能、2D 保留绘制项
闪烁、UI 裁剪/颜色/相机、窗口 resolution 与缩放、mesh 重新分配和 MP4 音频解码。
依据 [官方差异](https://github.com/bevyengine/bevy/compare/v0.19.0...v0.19.1)。
Windows DX12 锁文件约束：`wgpu-hal 29.0.4` 与 `gpu-allocator 0.28.0` 必须
共同使用 `windows 0.62.2` 的 D3D12 类型。后者的宽版本范围可能在依赖更新时选中
`0.57`/`0.58`；即使图中其他依赖合法使用旧版本，也不能让这两个 crate 的接口分裂。
本次修复只对齐 Cargo.lock 中 gpu-allocator 的一条依赖，不新增 shim、依赖或关闭 DX12。
`cargo check --locked --target x86_64-pc-windows-gnu -p wgpu-hal` 已通过，实际启用 dx12/vulkan；
它验证后端 Rust 编译，不代替 Windows MSVC 完整 CI、链接与 benchmark 实机验收。
修复后 publisher workspace check/Clippy、898 项测试（8 ignored）、fmt 与四项 cargo-deny 审计通过。

0.19.1 未修复 macOS reactive 空闲循环；`dev/vendor/bevy_winit` 使用官方同版完整源码，
只保留 `about_to_wait` 的待办/定时判断，包含新版 DPI/窗口修复。来源和单一改动见该目录
KEINE-PATCH.md；上游许可随源码保留。无需求的 3D/光追等 feature 不自动扩大引擎范围。

刷新预算优先取 `--hz`，否则取所在 Monitor；未知刷新率明确缺测，不假定 60 Hz。
严格超过预算的次数、最长连续次数与估算错过刷新槽分别报告；提交间隔不是屏幕实际呈现，
VSync 的微小抖动也可能超预算，不能把这些次数直接说成实际掉帧。跨越预热/结束边界的
活跃间隔单独标记 `sample-boundary`，保留原始记录，不混入有效区间统计。正常模式排除有意休眠、
失焦及窗口尺寸/刷新率改变的跨界间隔。CPU 是测量区间全进程累计时间之差，1 个核心为 100%；
CPU 实际计数跨度见 PROCESS 的 wall seconds，唤醒偏晚时不能宣称覆盖整个名义采样区间。
RSS 是进程生存期峰值，不能称作采样区间内峰值。逐帧记录最多 240,000 条，超限报告 omitted。
CPU/GPU 渲染阶段分别排名，不相加嵌套 span；GPU 排名要求实际设备同时支持 encoder/pass
时间戳写入。不满足时明确缺测，省略不足以证明渲染阶段成本的 encoder 零值。
本机 Metal 不满足该条件；shader 热点需要另用 Instruments Metal GPU trace。
阶段排名只覆盖已经记录 diagnostic span 的渲染工作，不能当作整个 Renderer 的成本清单。
没有有效活跃间隔时，帧时/FPS 明确不可用；旧 portable 数值协议仍保留 frames=0 哨兵。

设计/ABI 依据：[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)、
[getrusage](https://man7.org/linux/man-pages/man2/getrusage.2.html)、
[GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)，
以及锁定 `bevy_render 0.19.1/src/diagnostic/mod.rs` 的 Supported platforms。

采集工具验证使用 Apple M5 Pro / 24 GiB / macOS 27.0.1、1920×1080、120 Hz、
上述 profiling 构建，缓存未控制。原始命令：

```sh
python3 dev/scripts/profile-runtime.py tests/fixtures/native-smoke --output target/performance/verified-runtime-cpu --seconds 3 --stacks off
python3 dev/scripts/profile-runtime.py projects/tday --scene start --cursor 16 --mode continuous --output target/performance/verified-stacks-cpu --seconds 3
```

静止/失焦采集保留 1 条排除记录，CPU 为 0.006 秒 / 3.000 秒，FPS 不可用；
修复前同一命令的 `verified-runtime/` 漏记 CPU 起始计数，报告不可用。
tday 采集保留 360 条，359 条有效、1 条 sample-boundary，调用栈已解析 Rust 函数和源码位置，
CPU 区间为 3.000 秒；两个项目源码清单均未变化。该验证证明采集路径可用，
不是引擎提速或“不掉帧”验收。日志/哈希见各输出目录；901 项 workspace 测试、
3 项 Python 边界测试、clippy 与 `video-native` 无默认 feature 检查通过。
Windows/Linux 交叉检查分别缺少 `x86_64-w64-mingw32-gcc` / `x86_64-linux-gnu-gcc`，
未完成目标平台编译及实机采样；不标为通过。

#### Bevy 0.19.1 与独立包验证

同一 M5 Pro / 24 GiB / 120 Hz，profiling 优化构建，缓存未控制。三次 tday 命令均为：

```sh
python3 dev/scripts/profile-runtime.py projects/tday --scene start --cursor 16 --mode continuous --output target/performance/bevy0190-before --seconds 3 --stacks off
# 更新后 output 分别为 bevy0191-after 和 bevy0191-after-repeat。
```

旧 0.19.0 记录平均 8.33 ms / p99 8.89 ms / CPU 26.76%；试开 multi_threaded 的
0.19.1 两次为平均 8.36 ms / p99 8.79、8.77 ms / CPU 50.53%、54.06%，平均 FPS
都约 120。试验期间有后台编译，这组记录不用于选择线程配置；下面补做同配置对照。
未开多线程的独立 profiling Engine 只读复测 tday，记录平均 8.38 ms / p99 8.82 ms /
max 24.88 ms / CPU 24.25%，源码哈希不变。此包没有 video-native feature，与前两组不同，
不能宣称同配置提速；原始记录见 `target/performance/bevy0191-single-final/`。

多线程复核：两版均为当前代码、Bevy 0.19.1、profiling、
`--no-default-features --features bundled-opus,ui-sounds,startup-metrics`；实验版额外使用
`bevy/multi_threaded`。关闭该 feature 指 Bevy 的并行调度开关，不表示整个 Engine
没有渲染线程或其他工作线程。两版使用各自的内嵌 key，但只读加载明文项目，未使用 key。
采样期间没有后台编译，不采集调用栈；三轮交替运行，持续画面每轮 8 秒、静止 5 秒，
各预热 3 秒，缓存未控制。复现构建与采样命令：

```sh
cargo build --locked --profile profiling --no-default-features --features bundled-opus,ui-sounds,startup-metrics --bin keine
# 将结果复制到 target/performance/threading-ab/binaries/single 后，再构建实验版：
cargo build --locked --profile profiling --no-default-features --features bundled-opus,ui-sounds,startup-metrics,bevy/multi_threaded --bin keine
# 将结果复制到 target/performance/threading-ab/binaries/multi。
python3 dev/scripts/profile-runtime.py projects/tday --scene start --cursor 16 --mode continuous --binary target/performance/threading-ab/binaries/single --output target/performance/threading-ab/opening-r1-single --seconds 8 --stacks off
python3 dev/scripts/profile-runtime.py tests/fixtures/letsgal-timeline --timeline 'benchmark stress composition' --mode continuous --binary target/performance/threading-ab/binaries/single --output target/performance/threading-ab/stress-r1-single --seconds 8 --stacks off
python3 dev/scripts/profile-runtime.py tests/fixtures/native-smoke --mode runtime --binary target/performance/threading-ab/binaries/single --output target/performance/threading-ab/idle-r1-single --seconds 5 --stacks off
# 每种负载再测试 multi；三轮顺序为 single/multi、multi/single、single/multi。
```

| 负载 | feature 关闭 CPU，三轮 | feature 开启 CPU，三轮 | 帧间隔 p99 关闭 / 开启，中位数 |
| --- | --- | --- | --- |
| tday start:16 | 13.32 / 15.12 / 13.67% | 48.65 / 44.74 / 52.78% | 9.30 / 10.22 ms |
| 256 粒子与镜头特效 | 25.20 / 14.80 / 14.66% | 60.27 / 47.77 / 45.45% | 9.34 / 9.72 ms |
| native-smoke 静止 | 0.62 / 0.40 / 0.28% | 0.35 / 0.40 / 0.36% | 休眠，无有效 FPS |

CPU 按一个核心 100% 计算。持续画面多数平均约 120 FPS；特效开启版第二轮有一次
1007.22 ms 停顿，该记录处于失焦状态，continuous 模式按约定保留，未确定原因，
不能归因为多线程。静止关闭版第一轮的 CPU 计数区间为 4.183 秒，其余接近 5 秒；
比例使用各自实际区间，不将其当作完整 5 秒。所有原始记录、命令、二进制 SHA256、
输入前后哈希和比较 JSON 均在 `target/performance/threading-ab/`；33 个输入文件未改变，
其中外层清单覆盖 LetsGal fixture 的 JSON 和素材，弥补采集工具原清单只包含本地剧本格式的范围。
当前机器/负载下，开启 feature 未显示稳定帧时收益而持续 CPU 成本较高，默认继续关闭；
Bevy 版本保持 0.19.1。这不是 Windows/Linux 或更大 ECS 负载的结论，也没有证明根因。

采集自身的定时回归另用 debug 优化构建隔离验证：

```sh
target/debug/keine perf tests/fixtures/native-smoke --seconds 5 --mode runtime --raw
```

两次 CPU 为 0.018 / 5.000 秒（0.37%）与 0.016 / 4.999 秒（0.32%）；各保留
2 条休眠记录，无有效活跃 FPS。验证的是采样器不再制造忙循环/重复等待，不能当作
动态画面提速或跨平台性能验收。原始日志保存在 `target/performance/bevy0191-gates/`。

独立包使用临时 identity 构建；自运行、报告转换与包内调用栈采集均从工程目录外运行：

```sh
# identity 仅用临时目录，结束后清理；不写 native-smoke 或 tday。
(
    umask 077
    task_identity_dir="$(mktemp -d /private/tmp/keine-benchmark-identity.XXXXXX)"
    trap 'rm -rf "$task_identity_dir"' EXIT
    KEINE_HAKUTAKU_IDENTITY="$task_identity_dir/test.key" target/debug/keine bundle tests/fixtures/native-smoke --output target/performance/portable-0191-complete --benchmark
)
cd /private/tmp
/Users/shiftz/dev/keine/target/performance/portable-0191-complete-benchmark/keine
python3 /Users/shiftz/dev/keine/target/performance/portable-0191-complete-benchmark/profile-runtime.py --report /Users/shiftz/dev/keine/target/performance/portable-0191-complete-benchmark/keine-benchmark-report.txt --output /Users/shiftz/dev/keine/target/performance/portable-0191-complete-json
python3 /Users/shiftz/dev/keine/target/performance/portable-0191-complete-benchmark/profile-runtime.py --output /Users/shiftz/dev/keine/target/performance/portable-0191-complete-stacks --seconds 3 --mode continuous
```

本机没有 GPU 阶段时间戳数据，报告明确 unavailable；native-smoke 没有作者特效时间轴，
相应项明确 skipped。Windows/Linux 的 CI 构建定义已接入同一采集链路，尚未执行远端
构建和目标平台实机采样。构建/接口回归、系统采样和视觉验收分别记录。
最终独立包 suite 正常退出，JSON 保留 2,402 条记录、5 个 workload，源码均正确定位
到 `main.shou:2`。其 runtime 区间为 0.018 CPU 秒 / 5.000 秒（0.37%），两条休眠记录
不参与 FPS；包内独立采样脚本还能输出 native Rust 调用栈，采集前后包内容哈希一致。
903 项 workspace 回归通过 / 8 ignored，6 项 Python 边界回归通过；fmt、Clippy、
无默认 feature 的 video-native/publisher 检查和 cargo-deny 的四项审计通过。
CI 缓存清理脚本已检查 native/Windows 参数语法，Cargo dry-run 覆盖 Engine/Loader
产物及密钥份额 OUT_DIR；未实际删除本机编译缓存，远端执行尚未验证。

### Engine 静止 CPU

本机 macOS ARM64、`publisher,video-native` debug 优化构建，tday 的夕阳背景和
“虽然已经过去很久了”对白停稳，BGM 继续播放，同一窗口尺寸、无操作。
每 5 秒读取 `ps` 的累计 CPU 时间，四次读数跨度约 15 秒：旧版增加 7.36 CPU 秒
（49.02%），新版增加 0.23 CPU 秒（1.53%）。这是整个 Engine 进程的时间增量，
包含 Opus 解码和系统音频线程；不使用瞬时 `%CPU` 的 0.0 读数替代它。
移除临时诊断后的最终构建复测增加 0.24 CPU 秒 / 15.02 秒（1.60%），见 `final.cpu.json`。

锁定的 Bevy 0.19 窗口循环用 `Instant::checked_add(wait)` 安排唤醒，
`Duration::MAX` 溢出时保留旧的动画 deadline/control flow，导致静态画面继续跑整帧。
Runtime 的静止/后台等待改用最多 60 秒的有限 deadline；输入与 IPC 仍可立即唤醒，
控制栏淡出和光标闪烁仍用更近的 deadline，动画/粒子/视频/自动播放保持原有节奏。
新版 10 秒调用栈中，主线程 8,130/8,139 个样本等待系统事件，持续非等待工作主要为
Opus；此次结果不能外推为动态画面提速或其他平台的实测结果。

原始数据及旧二进制在 `target/authoring/performance/idle/`；采样命令：

```sh
cargo build --features publisher,video-native
# Editor 启动 Preview，定位到上述对白，等画面停稳；前后使用各自 Engine PID。
python3 target/authoring/performance/idle/measure.py <PID> <before-or-after>.cpu.json
sample <PID> 10 1 -file <before-or-after>.sample.txt
```

fmt/check/Clippy、publisher workspace 869 passed / 8 ignored（IPC 放行沙箱重跑）、
无默认音频的 video-native 生命周期 21 项回归及最终 Engine 构建通过。
原生 Preview 已检查休眠后的空格推进、Textbox 隐藏/恢复及菜单唤醒。
Windows/Linux 静止 CPU 尚未实测。

性能修改必须提供同主机、同输入、同 release 配置的前后命令与原始结果。
`cargo perf --startup` 测隔离启动；`cargo bundle <project> --benchmark` 生成独立 portable 发行基准。
bench 源码保留可重复的热点测量，日志放忽略的 `target/`，不持续新增日期报告。

### Editor 滚动热点

本轮针对 tday 的 Assets 掉帧启动 Editor 性能检查；粒子 CPU 已定位，GPU 专项仍待测。
macOS arm64 / GPUI 0.3.5：滚动采样定位到未变化 Text 概览的 Scene replay，
逐笔画插入 bounds tree 占主要活跃主线程样本。Text/Blocks 概览现使用同一绘制图层，
保留笔画顺序、视口/选择标识与导航。浏览器使用后台生成的 384×256 内存缩略图，
64 项 LRU（像素数据上限 24 MiB/面板）；大小/mtime 参与缓存键，淘汰和关闭释放图片。
路径约束和解码在 worker 中执行，原图与 Asset Preview 不变，不新增磁盘缓存。

实际 GPUI Scene replay 的 release A/B：5 组各 30 次，取组均值的中位数。

| 笔画数 | 原方式 ms | 单图层 ms |
|---:|---:|---:|
| 1,200 | 0.311 | 0.029 |
| 4,800 | 2.918 | 0.081 |
| 19,200 | 67.795 | 0.339 |

```sh
cargo bench -p keine-editor --bench browser
sample <editor-pid> 20 2 -file target/authoring/performance/scroll/paired-before.txt
sample <editor-pid> 20 2 -file target/authoring/performance/scroll/paired-after.txt
```

原始命令、release 输出和调用栈保存在 `target/authoring/performance/scroll/`；
bench 源码为 `tests/bench/editor/browser.rs`，直接调用锁定 GPUI 的 Scene replay。
原生应用另以相同 dev profile、tday、3024×1772 / 2× 窗口与 24 次交替上下 3 页滚动对照：
physical footprint 1.1 GiB → 388.9 MiB，观测 peak 1.1 GiB → 465.5 MiB。
这些是该次样本的进程 footprint，并非 RSS 或跨机器保证；release A/B 是绘制路径微基准，
不能当作整窗 FPS/GPU 帧时。首见图片仍需解码，原图预览仍占用原图内存。
搜索、透明立绘、音频列表、原图预览及两个概览导航已做原生检查；
缩略图尺寸/通道/透明度、缓存容量/更新键回归通过。

Blocks 继续按源码版本缓存文本/索引/顺序/结束关联、几何与概览；普通滚动仅定位并挂载可见行，折叠、实测行高或源码变化使几何失效。当前行/执行行也复用索引。拖放和折叠动画保留原有冻结布局。Text 配色、原生换行与笔画构建移到后台，改宽复用配色；新任务取消旧任务并拒绝过期结果，不添加输入延迟。

资源拖放的整窗采样（`sample <editor-pid> 45 2 -file target/authoring/performance/drag/after-confirmation.txt`）
显示忙时热点在 GPUI 根视图 prepaint、Taffy 布局、元素 ID/样式复制与分配；字段缓存微基准不能代表 CPU 占用，试验改动已撤回。
锁定 GPUI 的鼠标拖动路径调用 `Window::refresh`，绕过视图缓存；Blocks 拖放期间也禁用稳定几何路径。
后续优化须以相同窗口/剧本/拖动操作的进程 CPU、帧耗时和松手后空闲占用对照，保留原始日志，不能据字段微基准宣称流畅。

同主机 release 微基准（`tests/bench/editor/{blocks,text}.rs`）：Block 为 11 组 × 100 次，Text 为 11 次，取中位数；测试串行运行。

| 路径 / 规模 | 原计算 ms | 新计算 ms |
|---|---:|---:|
| Block 静态准备 / 1,000 | 0.062621 | 0.000132 |
| Block 静态准备 / 5,000 | 0.283800 | 0.000152 |
| Text 概览 / 1,000 行 | 0.589 | 0.348 |
| Text 概览 / 5,000 行 | 3.239 | 1.881 |

Block 测量包括源码复制、行索引、顺序、行位置与概览，对照实际缓存命中和可见行定位；不包含控件/绘制。Text 在 GPUI 测试平台比较原 DisplayMap 构建与新 worker 构建；5,000 行改宽复用配色为 1.905 ms。它们是计算微基准，不代表原生输入延迟或整窗 FPS。

```sh
cargo test -p keine-editor --release --lib benchmark:: -- --ignored --nocapture --test-threads=1
sample <editor-pid> 20 2 -file target/authoring/performance/layout/blocks-before.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/blocks-after-final.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/text-before-ascii.txt
sample <editor-pid> 20 2 -file target/authoring/performance/layout/text-after-final.txt
```

原始命令、输出和 dev 调用栈位于 `target/authoring/performance/layout/`。tday、3024×1772 / 2×、相同三列布局下，24 次交替上下 3 页滚动：原采样中全文行索引重建累计 291 栈样本，最终采样未出现该调用；这仅是本次采样热点消除证据。24 次 ASCII 插入/删除的最终采样中，概览构建在 default-qos worker，而非主线程。
原生检查覆盖概览导航、改宽换行、折叠/展开、普通及嵌套重排/撤销、多行文本增高/撤销；tday 测试输入已恢复。回归覆盖行高增加/移除、折叠、版本/冻结源码，以及中文、emoji、Tab、CRLF、空行和极窄宽度与原 DisplayMap 的映射一致性、取消/过期发布。workspace 785 通过、5 个显式忽略的性能测试；fmt/check/clippy 通过。

中低项：Assets 缓存筛选/排序、目录、选择顺序与行布局，空搜索不转换所有字段；脚本索引只复制保留下来的贡献，旧任务在文件之间取消；缩略图低优先级且最多同时两项，静态 WebP 用现有 media 解码器直接缩小输出。未变文件的公共 Vec 索引仍需要复制，未新增持久化缓存或另一套索引模型。

同主机 release、11 次中位数（筛选每组 100 次），原始输出位于 `target/authoring/performance/resources/`：

| 计算路径 | 原方式 ms | 新方式 ms |
|---|---:|---:|
| 5,000 句对白 / 5,000 资源，更新 1/10 个文件 | 2.227 | 1.556 |
| 同规模，更新全部 10 个文件 | 9.034 | 8.414 |
| 5,000 资源，空搜索筛选（缓存未命中） | 0.500 | 0.039 |
| 1920×1080 背景，派生有损 WebP 缩略图 | 34.026 | 11.750 |

PNG 为 17.976 → 18.041 ms，无损 WebP 为 26.964 → 26.507 ms，单张解码没有明显改善；并发上限用于限制竞争。浏览缓存命中微基准约 0.000006 ms，只测结果复用；所有结果均不等于整窗帧时或首屏延迟。生产解码库复用 `keine-media`，没有增加外部 codec 依赖。缩放后的输出缓冲有界，但 codec 工作内存另计。

```sh
cargo test -p keine-editor --release --lib index::benchmark:: -- --ignored --nocapture --test-threads=1
KEINE_BENCH_IMAGE=/absolute/path/to/background.png cargo test -p keine-editor --release --lib benchmark:: -- --ignored --nocapture --test-threads=1
```

第二条使用同一背景派生无损/有损 WebP，交替执行原解码与缩放解码；未指定图片时生成固定图案。计时不包含编码、任务排队或 GPU 绘制。回归涵盖索引错误恢复/未变文件/取消、浏览结果失效/时间筛选/Unmapped、解码并发上限、透明度与 EXIF 旋转。

本轮 workspace 791 通过、8 个显式忽略的性能测试；fmt/check/clippy、关闭音频的视频构建及 audio-opus/audio-seekable 检查、native-smoke validate 通过。原生 tday 检查音频卡片紧凑行距/上下留白、同一 Asset Preview 的播放→暂停→继续、重播和列表按钮同步；暂停沿用当前 Player，继续不重建解码器。协议升至 v9，旧 Editor/Engine 必须成对更新。系统输出的听感未代替用户验收。

### 运行时预取预算

原数量限制继续保留；非关键图片纹理 payload 与内存音频另受 128 MiB 保留预算约束，当前显示/显式 blocking 不淘汰，流式 Opus 不按文件长度计费。尺寸未知时暂停追加，超限项延后到新计划，避免同计划重复解码。

固定计费输入对照原来的 8 项数量保留与新增预算：8×4 MiB 为 32→32 MiB；8×64 MiB 为 512→128 MiB。命令/原始输出在 `target/authoring/performance/prefetch/policy.log`：

```sh
cargo test -p keine --lib scene::assets::tests::speculative_budget_preserves_priority_and_never_evicts_current_assets -- --nocapture
```

这是缓存计费合同回归，不是进程 RSS、GPU 实占或 FPS 测量；128 MiB 是初始政策，机器性能测量仍在独立阶段。回归覆盖优先级、当前资源保护、未知尺寸/延后、render extraction 后计费保留，以及流式/内存音频区分。
当前 workspace 783 项通过、8 项显式忽略；publisher 324 项、无音频 video-native 8 项通过；fmt/check/clippy、可选功能构建、native-smoke 与发布 workflow YAML 解析通过。publisher.key 迁移验证身份、公钥与根密钥保持不变。

### 独立性能阶段（其余项目待测）

剩余范围为 Editor 完整帧时/大文档/搜索、Engine 启动与持续 CPU/RSS、实际 Preview FPS/GPU 帧时、粒子 GPU/全屏与瞬时峰值、音视频流与 Hakutaku I/O；使用现有 bench 与 portable benchmark 入口，不建立另一套采集协议。

启动时固定同主机、同 release、同输入及分辨率；先记录基线，再定位与修改热点，最后保存前后原始命令/结果并复验功能。Windows 自动 benchmark 的构建/打包入口保留；平台跑分和性能结论留到此阶段。

功能阶段继续运行以下接口/正确性回归，不依赖帧率、耗时或机器性能阈值：

| 接口 | 当前保留的测试合同 |
|---|---|
| Editor → Engine | 协议版本、长度边界、启动握手、崩溃重开、不同工程隔离 |
| Performance 采样 | 实际采样间隔、CPU >100%、计数回退、缺测、PID 重用、peak 重置、有界历史 |
| 源码 → Parser → Block | 全部 70 个插入入口、EYS 旧入口拒绝、分组字段与精确写回、换图省略保留；保存格式化的 token/语义不变、幂等、共享文档写入与撤销、外部冲突保护；保存前后 Text 选区、焦点与屏幕纵坐标保持，重复保存视口不动；自动聚焦的角色别名、旁白与角色继承 |
| 资源与文件 | 改名/类型迁移、映射/文件共同撤销、冲突与路径隔离、导入失败回滚 |
| Editor 交互模型 | 拖放取消/过期版本/嵌套几何、概览两端和独立平移、失去文档容器后的开页落点 |
| Audio / Video / Media | 循环 rewind/Opus pre-skip、BGM sink 交接、视频 EOF/rewind/损坏输入、队列与解码预算 |
| 发行与 benchmark | 正常包与 benchmark 分离、挂载覆盖、确定性 payload、保留目录识别；不执行跑分 |

对应 owner：`tests/authoring/process.rs`、`crates/authoring`、`crates/editor`、`src/runtime/audio.rs`、`src/runtime/package/benchmark.rs`、`src/scene/video`、`crates/media`。现有 `tests/bench` 与 CI 正确性检查保持可编译，不因暂缓删除测试或接口。

### 雨雪视觉与成本

性能目标是合理开销下的良好表现，按相同硬件、分辨率和负载比较。
HD 4600 在 5120×2880 的压力结果不能按 1080p 的帧率目标判错：像素量是后者约 7.1 倍。
参考本机 LetsGal Studio 的 `ParticleObject` / `ParticleObjectsEffect` 与雨雪预设，
保留平滑风变化、个体摆动和远近层次的思路；[官方粒子说明](https://docs.avg-engine.com/manual/writing/blocks/particle/)介绍预设与自定义参数。

现有 emitter 批处理、每粒子 64 字节、60 Hz 模拟及每 emitter 256 上限不变。
雨丝缩短并按落速/风向倾斜；雪片以轻微纵横飘动、旋转及边缘朝向变化增加层次。
省略 wind 时有缓慢阵风；显式 wind 固定风速。默认落速保持稳定，显式重力/阻力继续生效。
原纹理优先，程序生成纹理只用于没有指定纹理的粒子。

对照命令与原始日志保留在 `target/performance/weather-behavior/`。
使用同一 M5 Pro / Metal、profiling 优化构建、1280×720 可见窗口与 continuous 模式；
编译和原生画面检查结束后交替运行旧/新各三轮，预热 3 秒、采样 8 秒。
CPU 为采样期间 `(user+system)/wall`，一个核心为 100%；p99 是帧间隔，包含呈现节奏。
三轮中位数如下；CPU 跨轮波动明显（雨：旧 19.07–20.61%、新 10.06–20.53%；
雪：旧 9.65–19.97%、新 9.73–18.51%），因此不据此宣称提速。

| 用例 | 数量 | CPU % 旧 → 新 | FPS 旧 → 新 | p99 ms 旧 → 新 |
|---|---:|---:|---:|---:|
| 雨 | 256 | 19.81 → 18.92 | 60.0 → 60.0 | 18.23 → 18.03 |
| 雪 | 256 | 14.39 → 10.94 | 60.0 → 60.0 | 17.61 → 17.54 |

旧雨第二轮有未归因的 102.87 ms 长帧；保留完整日志，不声称零掉帧。

```sh
cargo build --locked --profile profiling --no-default-features --features bundled-opus,ui-sounds,startup-metrics,video-native --bin keine
# before/after/keine 分别保留修改前后构建；run.py 顺序运行以下雨/雪命令各三轮。
target/performance/weather-behavior/before/keine perf tests/fixtures/native-benchmark --timeline bench_particle_rain_256 --seconds 8 --mode continuous --window 1280x720 --raw
target/performance/weather-behavior/after/keine perf tests/fixtures/native-benchmark --timeline bench_particle_rain_256 --seconds 8 --mode continuous --window 1280x720 --raw
target/performance/weather-behavior/before/keine perf tests/fixtures/native-benchmark --timeline bench_particle_snow_256 --seconds 8 --mode continuous --window 1280x720 --raw
target/performance/weather-behavior/after/keine perf tests/fixtures/native-benchmark --timeline bench_particle_snow_256 --seconds 8 --mode continuous --window 1280x720 --raw
```

fmt/check/Clippy、workspace 909 passed / 21 ignored（含 vendor 测试，IPC 放行重跑）、
无默认音频的 video-native 粒子回归 12 passed，以及 native-smoke validate 通过。
Computer Use 直接检查了原生雨雪画面。Windows/Linux 核显上的成本和实际项目的视觉效果仍需对应硬件验收。

粒子优化实测：Apple M5 Pro / Metal，release LTO，仅额外保留采样符号。
tday 临时追加独立 benchmark fragment，复用同一背景，结束后逐字节恢复脚本。
可见窗口 1920×1080 / scale 1 / Fifo，`scene` profile 隔离 UI/对白绘制。
本组 CPU 对照使用旧 `perf` 的 60 Hz 唤醒模式，不作为高刷新率/全屏验收证据。
旧/新二进制交替运行三轮；每轮预热 3 秒、采帧 12 秒，下表为中位数。

| 用例 | 粒子数 | CPU % 前 → 后 | FPS 前 / 后 | p99 帧间隔 ms 前 → 后 |
|---|---:|---:|---:|---:|
| 无粒子对照 | 0 | 12.69 → 12.09 | 60.0 / 60.0 | 17.89 → 18.04 |
| 大雪 | 192 | 15.81 → 12.49 | 60.0 / 60.0 | 17.97 → 17.79 |
| 16 层大雪（压力用例） | 3,072 | 22.12 → 21.86 | 60.0 / 60.0 | 18.41 → 17.92 |

CPU 为 `time` 的 `(user+sys)/real`，含启动和预热；帧间隔排除预热，并非 GPU 执行时长。
18 次短测均为 720 帧；新版最大帧间隔 28.11 ms。
已移除 `perf` 独有的固定 60 Hz 唤醒计时，采用与正常播放一致的更新方式。
macOS 活跃帧按 `OnMonitor` 的实际刷新率安排 deadline，合成 redraw 不再额外触发更新；
输入在下一个显示周期消费，静态/后台休眠规则保持不变。未知刷新率及其他平台保持 Continuous + Fifo。
旧 60 秒测试片段结束早于采帧结束，其结果只供诊断，不能作为帧边界验收；
全屏检查延长临时片段持有时间，覆盖预热和完整采帧。
普通雪景 CPU 下降，压力场景 CPU 变化很小，不能声称全局提速或 FPS 提升。
“不因优化新增掉帧”为验收硬边界，粒子数量、质量及运动插值保持不变。

原始 `sample` 7 秒 / 1 ms：旧版压力场景网格分配/上传路径 252/627 个非等待叶样本，
其中顶点打包/拷贝 112 个样本，不能相加；粒子运动仅 26 个样本。
新版对应 Mesh 分配/打包路径为 0，粒子更新 18/437 个非等待叶样本；
仍有 ShaderBuffer 上传与 Metal staging 成本，采样分布不能替代前后帧耗时测量。
锁定 Bevy 0.19 源码确认：旧 Mesh 修改会释放并重建顶点/索引分配；
现用固定 ID/角点网格和 ShaderBuffer，同尺寸更新通过 `RenderQueue::write_buffer` 复用目标 GPU 缓冲。
四顶点展开放到顶点着色器；每粒子动态数据由 216 降至 64 字节（减少约 70%），
固定 UV/索引只在创建时上传，移除无效动态 AABB 重算，密度变更/清除同步释放缓冲。

```sh
cargo rustc --release --bin keine -- -C strip=none -C debuginfo=1
/usr/bin/time -lp target/particle-profile/before/keine perf projects/tday --seconds 12 --timeline profile_snow --camera scene
/usr/bin/time -lp target/release/keine perf projects/tday --seconds 12 --timeline profile_snow --camera scene
sample <engine-pid> 7 1 -file target/particle-profile/after-snow16.sample.txt
```

原始临时源码、旧二进制、逐轮命令/日志/调用栈、分析与构建记录在 `target/particle-profile/`；
交替结果在 `paired/`，长时间帧边界检查在 `frame-gate/`。这些 timeline 已从 demo 恢复移除。
隐藏窗口仅作准备路径对照，不作为绘制/FPS 证据；采样器运行不计入帧边界验收。
fmt/check/clippy 通过；workspace 752 passed / 8 ignored（IPC 在沙箱外重跑），
粒子默认功能和 `--no-default-features --features video-native` 各 10 passed。
原生 Metal 窗口确认雪/雨绘制；用户目测确认全屏无卡顿并要求停止窗口验收。
120 Hz 全屏压力场景常态 p99 从 17.14 降至 10.28 ms；完整日志包含窗口切换/操作，
用户确认验收期间操作过窗口，不能把这些运行当成零长帧或零丢帧的客观证明。
其他平台和有效 GPU pass 时间尚未测量。
Metal 的 Bevy RenderDiagnostics 输出 0 不能作为 GPU 时间；
透明混合、场景/文本框模糊及原先瞬时峰值不在本轮性能结论内。

最近已记录的 0.11.0 → 0.11.1 macOS arm64 / Rust 1.97.1、warm-cache 中位数：

| 操作 | 输入 | 前 ms | 后 ms |
|---|---|---:|---:|
| Block projection | 5,000 commands | 156.142 | 3.759 |
| Native parser | 5,000 commands | 87.422 | 2.476 |
| 完整索引 | 500 files / 25,000 lines | 40.521 | 35.754 |
| WorkspaceSession | 165 LetsGal entries | 1.032 | 0.702 |

原始 probe/paired logs：`target/authoring/editor-audit-20260929/0111/`。
普通 probe 为 11 samples；增量单文档索引 31 samples，中位数 2.341 ms。
小输入完整索引有变慢；这些数字不是 UI FPS、GPU、磁盘或长期内存结果，不声称全局改善。

Performance 进程采样的 release probe（macOS arm64，11×5,000 次）复用实际
`preview/performance/process.rs`：关闭采样中位数 0.0002 µs，启用系统查询 0.3869 µs/次。
Mach timebase 换算的 CPU 增量 0.299014 s，`getrusage` 对照 0.299035 s（ratio 0.9999）。
这只测本机系统查询，不代表完整 View/渲染开销；原始命令与结果位于
`target/authoring/performance/probe.rs`、`process-probe.txt`。
Focused tests 覆盖实际 elapsed / >100% / counter reset、有界过期、PID 重用时 peak 重置、
unavailable 缺测和 Mach 换算边界；不新增验收工程。

## 当前验收边界

当前唯一 demo 为 `projects/tday`：按当前已修改剧本拆为 13 个章节文件与小型入口 `main.shou`，共 14 scene / 2644 action。拆分逐段字节可重组为原 main，Loader 前后完整动作内容摘要相同；源文件和资源/角色/对象清单未变。
原工程 4403 个文件的路径与 SHA256 均未改变。副本跳过缺失的 equipment_power_down.wav、
window_slam.wav、convenience_store_chime.wav，位置标注在脚本开头；旧本地 demo 已移入废纸篓。
分文件迁移回归覆盖 LetsGal 同章多 fragment、独立入口、跨文件 call/goto、嵌套目录、文件名冲突、初始变量及跨文件聚焦规则；11 项通过。publisher workspace 845 项通过 / 8 项忽略，新增 LetsGal 分组用例另行通过；fmt、publisher Clippy、native-smoke 与当前 tday 校验通过，Editor 中入口及独立章节已打开检查。

保存位置修复：格式化后恢复映射选区和滚动位置，Blocks 保留映射后的测量行高与编辑框。屏幕坐标回归通过；publisher workspace 847 项通过 / 8 项忽略（IPC 在沙箱外重跑），fmt、Clippy 与 Editor 构建通过。macOS 新 Editor 已打开，用户确认 Ctrl+S 保存位置验收成功。
完整 `letsgal-timeline` fixture 的 migrate 因既有 `SelectSpriteImage` 不支持而拒绝发布目标；本轮未扩展该能力。
震动振幅/频率补间、混合镜头原子动作、带 ID 单次/循环音效及淡入淡出、退格和点击等待的
解析/执行/迁移 roundtrip 回归通过。IR schema v6，Editor–Engine 协议 v9，save 仍为 v11。
fmt、workspace check/clippy/tests（publisher）、官方 LetsGal 外部示例验收、native-smoke 与 tday
校验通过；无默认 feature + video-native 的 tests check 通过。tday 唯一警告为标题背景未被脚本引用。
macOS 新 Editor/Preview 已启动 tday，日期字幕、排练室背景与开场对白可见。
按用户要求，完整剧情、画面和实际音效验收交由用户后续完成；素材仍为开发格式，未做发行规范化。

Editor 资源导入已实现即时规范化，源文件只读、成品留在目标资源目录；PNG 透明像素、
JPEG 方向、动态图片拒绝、冲突/失败清理和文件/映射共同撤销回归通过。
本机显式运行 FFmpeg 验收：WAV/FLAC/MP3/Vorbis → Opus 的解码样本数不变、
合规 Opus 字节不变、损坏音频失败不留输出，以及 MOV → H.264 MP4 实际解码均通过。
这两项外部工具测试默认 ignored，安装 FFmpeg 后运行：
`cargo test -p keine-editor --lib workspace::file_ops::tests:: -- --include-ignored`。
fmt、workspace check/clippy（publisher）、workspace tests（781 passed / 3 ignored）、
Editor debug 构建与 native-smoke/tday 校验通过；Trash/IPC 测试在沙箱放行后通过。
本轮新导入的界面操作与主观媒体验收留给用户；存量 tday 素材仍保留迁移时的开发格式。
后续资源状态提示已通过 workspace check/clippy/tests 与 Editor 构建；macOS 新 Editor
打开 tday 的 Assets 已目测 `Resource · PNG` 信息行，保留完整缩略图及既有 ID/引用数。

资源统计/转换：publisher workspace 854 passed / 8 ignored，fmt、Clippy、构建和 native-smoke 通过；FFmpeg 音频转换测试另行显式通过。回归覆盖文件头格式识别、别名体积去重、精确缺失引用范围、转换冲突/失败与原文件保护，以及发行副本排除未登记的兼容源素材。
macOS Editor 实测图标统计卡、Convert all 和完成文件数进度条（收起卡片仍可见）：tday 50 张 WebP / 34 条 Opus 全部完成，登记资源从 757.1 MB 降至 169.1 MB；85 个原文件 SHA256 与 13 个脚本未变，清单仅改变资源路径和引号。转换后 tday validate 通过，保留 1 条未引用背景警告。源码红色波浪线与 Explorer 错误数已有模型回归，完整交互验收仍需用户确认。

```text
0.13.0 / EYS v2.0
├── macOS：原生 Engine、Text/Block/Inspector 写回、保存重开、故障恢复已有运行态证据
├── 最新工作台：下拉框、Text 概览、全文搜索、Asset Preview 关闭/重开已目测
├── 每句文本结束开关：灰/蓝状态、增删关联指令、Inspector 同步及 Cmd+Z 恢复已目测
├── Asset 生命周期：macOS 改名、Unmapped/Remap、系统 Trash、Type 迁移、撤销/重做与保存重开已目测
├── Performance：macOS 启停/失败恢复、关闭后持续采样、重开历史、重启 peak 重置已目测
├── UI 容器简化：多标签圆角、切换、面板缩放/滚动与跨分组拖放已用新 release 目测
├── 中文 IME：用户已通过本轮无固定延迟输入验收
├── Audio：macOS 两秒 Opus 循环与两次 BGM 交接已由用户实听通过
├── 单屏：2× 默认/最小/宽矮 Editor 窗口、Preview 最大化与全屏黑色留白已目测
├── 待显示验收：1× 与 Preview 极端比例；多显示器按用户要求延后
├── 独立性能阶段（暂缓）：实际 Preview FPS/GPU 帧时、持续 CPU/RSS、粒子与其他热点
├── Windows x64 / Linux：构建 CI 与运行态验收分别看待；运行态暂缓
└── 发行：正式签名/notarization 脚本已接入；实际公证待证书与凭据
```

当前代码已通过 fmt、workspace check、clippy、Editor debug 构建与 native-smoke validate；默认 workspace tests 729 passed / 1 ignored，no-default-features + publisher workspace tests 763 passed / 1 ignored。无默认 feature 与 bundled-opus 的音频边界各 2 项及 video-native 视频 8 项回归通过。audio-opus 单独配置的 check 通过，测试链接因本机缺少系统 libopus 未完成；bundled-opus 覆盖相同的无 seekable 路径。组合 hot-reload/video-native/video-ffmpeg/publisher 的 check 通过。
测试覆盖源码/文件事务、macOS 废纸篓恢复、对白尾部参数解析/执行/迁移；这些不代替完整运行态验收。

功能约定核对：EYS 统一入口/旧写法拒绝、Position/Layout 分组、稀疏换图与 scale、逐立绘 light、对白尾部选项和行内 wait；工作台下拉框、Text/Block 概览、全文搜索、Preview 关闭、拖放、序号/执行标识、结束开关、输入检测；资源网格/音频列表、筛选/Tags、物理改名/类型移动、Unmapped/Remap/Trash；迁移裸 ID/objects 映射/particles 清单和原子镜头动作均有当前实现与功能回归。原生视觉证据与用户通过项以上表为准。
本轮补齐：sprite.update 插入模板省略位置/缩放，Loader 与 Block 投影不再把对白后的 return/break 吞作语音 ID。插入模板回归扩展为全部 70 个入口。
这两项新修复已换上新 Editor/Engine 原生复验：插入模板只生成目标与图片参数；对白后的 Return/Break 各自显示为 Block，实际执行能返回调用处、退出循环，换图后位置与缩放保持。原有未保存草稿保留，项目文件未改写。
无解码器配置的 Preview 曾因 AudioSource 未注册而 SIGABRT；现在由音频配置入口补齐类型注册，并保留已有音频资源。回归覆盖普通/图库播放及已有 registry 不被重建；无音频构建原生 Preview 已验证缺少 Opus 时不崩溃、对白仍能继续。Preview 失败长提示改为固定尺寸图标，详情留在 tooltip/Output；最小窗口已检查图标不遮挡 Text/Blocks 标签。
宽矮 Editor 的 Text/Blocks、概览、资源框和 Inspector 滚动已目测。按用户确认，极窄窗口通过调整分隔线或关闭一列 View 使用，不要求三列同时保留的极限布局适配；撤销专为此添加的类型标识收缩/裁剪，不再列为本轮待修复项。
音频复验复用临时环境工程，播放无首尾静音的两秒 Opus 纯音；用户确认循环无停顿/爆音、第一首→第二首→第一首的 1 秒淡入淡出均正常。停止指令后脚本继续到下一句。此结果只覆盖本机该输入，不推定所有设备/素材已通过。临时新增脚本/音频已移除；环境工程 53 个文件和 letsgal-native 95 个文件的路径与 SHA256 均恢复基线。
Windows/Linux 系统废纸篓与进程指标已实现，CI 对三平台增加实际进程查询与系统入篓/撤销测试；本机交叉编译只证明平台代码可编译，不代替 Windows/Linux 原生运行态验收。正式签名/notarization 已有可选脚本入口，实际执行仍需用户证书和 Keychain profile。跨平台/显示适配、完整 tday 剧情及主观媒体保持独立验收，不混入性能阶段。
平台补齐验证：fmt/check、publisher clippy、默认 workspace 787 passed / 8 ignored、
无默认功能 + publisher workspace 821 passed / 8 ignored；macOS 系统 Trash 与 IPC
在沙箱放行后通过。CPU/RSS 查询与单位/非法计数回归、Linux 恢复记录/冲突/不安全
Trash 拒绝回归通过；新增平台 owner 与测试在 Windows GNU、Linux GNU 目标交叉编译。
平台编译隔离依赖 GPUI，实际 Editor 原生测试接入三平台 CI；该 CI 本轮未远程执行。
macOS 新 Editor debug 构建与开发包签名通过，ZIP 解压后的两个 app 严格验签通过；
正式 Developer ID 公证、Windows/Linux GUI 与显示验收未执行。

EYS v2.0 的配置版本为 2；默认值、显式旧版/未知版拒绝及迁移输出版本已通过回归。0.12.0 的 Editor/Engine 构建与版本输出、fmt/check/clippy、workspace tests、可选 feature 和 native-smoke/letsgal-native 校验通过。
统一入口的解析、执行、迁移及旧入口拒绝已通过回归；IR schema 为 v6。
Position/Layout 分组语法覆盖解析、旧平铺字段拒绝、嵌套补全、Inspector 精确范围/批量写回及迁移 Action 对等回归。
当前 publisher workspace tests、clippy、含 hot-reload/video-native/video-ffmpeg/publisher 的 check 与 debug 构建通过。
macOS 原生界面已检查 Position X、viewport Height 修改保存保留其他参数，以及淡色 viewport 补全用右方向键接受。
Block 拖放采用固定尺寸/布局快照、中央落点判断、200 ms 位移预览和一次写回。macOS 新构建已目测普通行重排、Option 携带 Goto、整个 Choice 移动、跨分支拒绝、面板外取消及一次撤销；连续操作后嵌套高度/间距未增长，试改已撤销。用户已通过本轮 Block 拖放验收。几何、取消、文档身份/版本、结构与重复行映射回归通过。
sprite.update 的省略保留、显式重设、缺席目标、动画目标与编译包 roundtrip 回归通过；迁移保留旧输入的重设行为，统一缩放输出 scale。无默认 feature 的 video-native 视频测试已有 8 项通过证据。本地沙箱阻断 IPC socket 时放行后重跑，通过才算完成。
现有 letsgal-native（10 scene / 1008 action）重新校验通过，仅一条未使用背景资源警告；环境光工程（65 scene / 1137 action）已有分组写法校验证据；native-smoke 无警告。
环境光的线性取样、明暗/色偏分离、透明像素、GPU 提取后缓存及重载边界已通过回归。
macOS debug 原生预览用只读复制的 tday 教室、海边夕阳、暴雨卧室目测；
用户通过夕阳和夜景，采用验收强度 1.0 为默认。临时工程位于
`/private/tmp/keine-environment-tday`，已只读复制全部 46 张背景并逐文件校验内容一致，
按章节提供目录及前后切换，先展示画面再打开导航；工程校验无警告。
用户反馈大部分画面正常并结束本轮验收；这不等于逐张无条件通过。
逐立绘 light 开关覆盖严格布尔解析、独立对象状态、恢复开启及存储回归；
原生 Inspector 的 Light 灰/蓝状态切换与保存写回已检查；旧作者字段 environment_light 被拒绝。
同背景双立绘的开启/关闭对照已检查。
no-default-features + video-native 测试也通过。
临时工程未加入仓库，原背景未修改。
这些是本机画面证据，不代表物理光照、全素材效果或 Windows/Linux 运行态验收。

遮幕/遮罩以计时完成解除等待，不能因透明度提前舍入到目标值而停止更新。2.2 秒 / 60 FPS 的脚本续行与已到目标透明度的等待解除回归通过（默认 feature 和 no-default-features + video-native）；fmt、workspace check/clippy/tests 通过。macOS 新 Engine 在 letsgal-native 的铁路雪景已检查开幕后进入对白，以及自动播放后续对白、收幕并返回标题；未重跑迁移。

引擎窗口底色显式设为黑色。macOS 新 Engine 全屏雪景已目测上下留白为黑色，舞台和对白正常；fmt、workspace check/clippy/tests 与 debug 构建通过。

macOS 新 Editor 在现有 letsgal-native 验收 Keep files → Unmapped → Remap、映射/未映射资源的系统 Trash、每种操作的撤销/重做；解绑和 Background → Figure 物理迁移均保存并关闭重开验证。PNG → Voice 被格式检查拒绝；改变类型后不兼容引用进入 Problems，撤销后消失。文件内容始终保持同一 SHA256；验收后 demo 的全部 95 个文件路径和内容恢复至基线。
关闭所有源码页后，资源撤销重新开页曾落入左侧窄栏；现在按存活文档组、Output 组、新文档组依次选择。落点只读取已登记的面板 ID 与布局，避免资源回调内重复借用正在更新的面板。macOS 新构建已验证无源码页时解绑开页，以及保存/关闭后撤销、再次关闭后重做，均落在中央且文件可恢复。容器消失及重新插入回归、fmt、workspace check/clippy/tests（727 passed、1 ignored）与 Editor debug 构建通过；IPC 测试放行沙箱后重跑。

视觉验收直接操作当前构建的原生应用；不得为采证新增 screenshot hook、环境变量协议或自动化。
输入检测由确认文字的 Change 事件触发，取消补全 120 ms、语法检查 180 ms 与派生索引 60 ms 固定等待；后台任务保留取消和过期结果保护，语法诊断写回前检查组合状态。fmt、workspace check/clippy/tests（727 passed、1 ignored）和 Editor debug 构建通过；macOS 新构建已目测淡色补全、右方向键接受、错误提示及撤销恢复。用户按本轮验证步骤反馈通过，中文 IME 组合/候选及确认后检测记为本机验收通过；不扩展为所有输入法或其他平台通过。
构建、测试、包签名和 UI/音视频验收必须分别报告。历史一次通过不替代当前代码复验。

Editor 输入与窄窗回归：`cargo test --workspace --features publisher,video-native` 为 910 passed / 8 ignored（IPC/Trash 在沙箱外运行）；fmt、workspace check（另含 hot-reload）、all-targets Clippy 和 Editor debug 构建通过。GPUI 回归覆盖 1222 px 右侧文档 Dock 在 1200/640/480/900 px 窗口中的控件边界，以及连续退格保持输入实体/焦点/选择、不逐字刷新索引、Enter 提交和输入中保存。新原生窗口的并排布局与退格手感尚待重启后目测；这些正确性回归不代表 CPU/帧时性能采样。

镜头首次晃动修复：已有晃动之间仍按选定振幅/频率补间；无前序晃动时直接采用指定值，避免公交场景的 600 秒持续时长变成从零渐强。`cargo test --workspace --features publisher,video-native` 为 912 passed / 8 ignored（沙箱外运行），fmt、workspace check（含 hot-reload）、all-targets Clippy 和 Engine debug 构建通过。回归覆盖首次启动一秒内的非零位移、持续时长终点、外层等待边界、既有震动补间、迁移→原生解析→启动，以及 Preview 跳到公交对白后继续晃动；无默认功能 + video-native 的公交 Preview 回归另行通过。native-smoke 与当前 tday validate 均为零警告。tday 文件未改写，实际公交场景仍待新 Preview 目测。

音效统一与自然尾部淡出：`se(asset, id: ..., loop: true, volume: ..., fade: ...)` 为唯一循环写法，旧 `se.loop` 已移除；Parser、迁移、Editor 补全与 Inspector 同步。单次 `fade_out` 按解码样本在结尾渐隐，不截断音频；循环换资源等新 sink 实际就绪后交叉淡化，`se.stop(*, fade: ...)` 清理单次和循环音效。回归覆盖立体声一致增益、短音频、seek、加载/暂停时序、全局停止顺序、同 ID 重新播放与迁移 round-trip。IR schema 为 v7，旧编译包须重新编译；Save v11 布局与 authoring 协议 v9 不变。BGM 无效 fade/loop/volume 报错，不再静默采用默认值。

`cargo test --workspace --features publisher,video-native` 为 924 passed / 8 ignored（IPC/Trash 在沙箱外运行）；fmt、workspace check（含 hot-reload）、all-targets Clippy 通过。无默认音频的 video-native check、仅 bundled-opus 与仅 audio-wav 的尾部淡出专项测试通过；Engine/Editor debug 构建及 native-smoke、native-benchmark、tday validate 均通过、零警告。tday 按磁盘内容局部修复：替换 32 条旧音效语句，按用户确认将 4 处开水阀改单次播放，补回缺失播放 ID、删除无对应播放的停止，浴室雾气/水波淡入为 450ms，7750ms 镜头补间等待完成后再重置，8 处单次音效显式添加尾部淡出。13 章连接、立绘目标和音效停止目标检查通过，979 条对白与修改前逐字一致，结束时没有遗留循环音效；资源和其他演出参数保留。当前修改前剧本及哈希记录保存在 `projects/.backups/tday-script-audio`。原生全剧情视听与 Windows/Linux 运行尚未验收；代码回归不代表这些验收通过。

LetsGal v2 角色位置修复：画布中心百分比转换为 Kēne 的水平中心偏移与底部基线，基准高度按表情→角色→全局优先级计算；旧无版本基线 fixture 保持原行为。登场/更新、局部距离方案覆盖、表情高度覆盖、原生渲染中心和缩放边界回归通过。当前 tday 的 6 个章节共 45 个位置字段已修正，逐字比较确认其余内容不变，修改前副本在 `/tmp/keine-portrait-position-backup`；不重新迁移或替换资源。`cargo test --workspace --features publisher,video-native` 为 917 passed / 8 ignored（IPC/Trash 在沙箱外运行）；fmt、check（含 hot-reload）、all-targets Clippy、无默认功能 + video-native check 和 Engine/Editor debug 构建通过。native-smoke、native-benchmark、tday validate 均为零警告。新版 Editor 已启动；Computer Use 无法定位未打包的 `target/debug/editor`，未记为画面验收通过。

Backlog 旧记录透明度修复：逐条错开入场仅用于最近 14 条，较早记录跟随面板淡入，避免动画停止后仍透明但能滚动/回跳。100 条实际记录生成的对白、姓名、回跳和语音图标在动画结束后全部可见，关闭时一致淡出；原有滚动方向和弹性边界回归保持通过。workspace（publisher/video-native）918 passed / 8 ignored；fmt、check（含 hot-reload）与 all-targets Clippy 通过。记录内容及回跳边界未改动；当前未打包 Preview 无法由 Computer Use 定位，实际画面验收待重启新版 Preview。
