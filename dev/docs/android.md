# Android Engine

当前范围是独立游戏运行版的实验性 ARM64 构建：Android 8 / API 26 起、横屏、Vulkan / OpenGL ES。
**安卓暂不支持视频**；`video-native` / `video-ffmpeg` 在安卓目标上明确拒绝编译。
Editor、publisher、桌面文件选择器不进入安卓运行依赖。Core、Loader、剧本、存档格式与桌面共用。

## 构建与 CI

日常推荐使用 GitHub Actions：打开独立的 **CI Android** workflow，点击 Run workflow；
结束后下载 `keine-android-arm64-engine` artifact 中的 APK。
GitHub 的 [Ubuntu runner](https://github.com/actions/runner-images) 已有 Android SDK；
workflow 安装固定 NDK/Gradle/Rust 工具，无需开发机常驻整套环境。
这是云端构建 VM，不是手机 GPU 验收环境。当前不维护额外 Docker 镜像。

需要本地构建时：

安装 Rust `aarch64-linux-android` 目标、cargo-ndk 4.1.2、JDK 17、Gradle 9.8.1 / AGP 9.4.1、CMake、Python 3；
Android SDK 安装 `platforms;android-35`、`build-tools;36.0.0`、NDK `28.2.13676358`。
设置 `ANDROID_HOME` 为 SDK 根，`ANDROID_NDK_HOME` 为该 NDK 目录：

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
dev/scripts/build-android.sh
```

脚本将现有 `keine` library 编译成 release 优化的 `libkeine.so`，不改变桌面 library 类型；
最终链接显式设置 16 KB max/common page size，保证 LOAD 和 RELRO 边界不依赖偶然布局。
Gradle 使用 NativeActivity 封装为 **debug 签名的测试 APK**，没有 Java/Kotlin 应用依赖。
唯一 Java 子类 `EngineActivity` 将 API 33+ 已完成的系统返回回调和旧版返回送到 Rust，
唤醒现有事件循环；UI 取消/关闭逻辑只有一个 owner。它还通过系统文档选择器把用户
选中文件的描述符交给公共备份逻辑，不引入 AndroidX 或 Kotlin。
NativeActivity 销毁等待 Rust 线程保存/清理完成后结束独立引擎进程，避免退出后重开时
复用 winit/Bevy 的进程级单例；普通暂停/恢复不走销毁流程。
AGP 的模块级 `enableKotlin = false` 关闭无需使用的 Kotlin 编译与自动标准库依赖；
CI 拒绝 APK 中意外进入的 Kotlin 标准库资源，并要求 `debugRuntimeClasspath` 为空；
自有返回桥接和 AGP 生成的 R/API DEX 保留，见 [官方配置](https://developer.android.com/build/migrate-to-built-in-kotlin)。
APK 为 `dev/android/app/build/outputs/apk/debug/app-debug.apk`，原生库为
`target/android/jniLibs/arm64-v8a/libkeine.so`。正式游戏签名和发布尚未接入。

独立 **CI Android** workflow 在 push/PR 或手动触发时检查依赖隔离、Android Clippy 与 Java lint，构建原生库与 APK，验证 NativeActivity
启动符号、ELF 16 KB 对齐、APK 对齐及签名，确认 Vulkan/GLES feature closure，
复核 APK 的 GLES/Vulkan 硬件声明，上传 `keine-android-arm64-engine` artifact。
不读取 publisher identity，也不打包 tday；原生构建缓存与桌面分开。
Android 构建队列独立，不等待桌面/视频 workflow 结束。

工具和平台入口依据 [Bevy 0.19.1 Android 示例](https://github.com/bevyengine/bevy/blob/v0.19.1/examples/README.md#android)、
[AGP 9.4 构建要求](https://developer.android.com/build/releases/agp-9-4-0-release-notes)、
[Android 16 KB 对齐要求](https://developer.android.com/guide/practices/page-sizes)。

## 渲染与后端测试

默认先初始化 Vulkan；surface、adapter 或 device 初始化返回错误时释放该次资源，
再尝试独立的 GLES instance/surface。APK 要求 OpenGL ES 3.0，Vulkan 为可选硬件特性；
具体驱动仍须满足现有 Bevy/wgpu 的 shader features/limits。驱动崩溃或已经运行后的
shader/device 错误不在启动回退范围内。实现与上游边界见
[Bevy patch](../vendor/bevy_render/KEINE-PATCH.md)；硬件声明依据
[Android uses-feature](https://developer.android.com/guide/topics/manifest/uses-feature-element)。

实验测试包支持私有 `files/render-backend`，同一 APK 可分别测试 GLES/Vulkan：

```sh
printf gl | adb shell -T run-as moe.maincore.keine tee files/render-backend
adb shell am force-stop moe.maincore.keine
adb shell am start -n moe.maincore.keine/.EngineActivity
adb logcat -d | grep -E 'GPU.*(Vulkan|Gl)|AdapterInfo|Android Vulkan initialization|wgpu|panicked'
```

将 `gl` 改为 `vulkan` 可强制 Vulkan；删除 `files/render-backend`（或写 `auto`）恢复自动选择。
单独指定后端时不会偷偷改用另一后端。测试顺序为 auto、gl、vulkan，并分别核对
GPU 日志中的 `Vulkan` / `Gl`、画面、触摸、音频和后台恢复。
只有 Gl 运行成功还不能证明故障时自动回退成功，须同时观察到 Vulkan 初始化失败与 Gl adapter。

## 资源与数据

APK 仅包含引擎和许可声明；构建从锁定 crate 的实际源码附上内嵌 WebP/Opus 的许可与源码获取地址。
现有 `NOTICE` 仍是概览，正式发行前需按 `release.md` 补齐其余适用的第三方声明。
初次启动没有游戏资源时显示提示页。
实验入口从 Activity 的私有 files 目录下 `game/` 读取普通工程，沿用 Loader 的只读挂载及路径约束。
每个工程必须提供稳定 `project.id`，存档/设置使用私有 `files/userdata/<project.id>/`。
不会依赖 `HOME`、`XDG_DATA_HOME`，也不在游戏资源目录写入存档。
设置页提供导出/导入存档与选项，格式与桌面相同。通过系统创建/打开文档选取文件，
只访问用户选中的文档，不申请存储权限。取消不修改数据；无效或中断的导入保留已有存档，
沿用公共备份的大小/文件名限制和目录替换恢复。文档可能是不可 seek 的管道，按有界流读取。
Java `detachFd` 转移唯一描述符所有权给 Rust `File`，由 Rust 在完成或出错时关闭。
依据 [Storage Access Framework](https://developer.android.com/training/data-storage/shared/documents-files)
及 [ParcelFileDescriptor.detachFd](https://developer.android.com/reference/android/os/ParcelFileDescriptor#detachFd())。
安卓不记忆桌面窗口位置。
安卓始终使用全屏，设置页不显示桌面全屏开关。

入口使用 `singleTask`，从桌面图标返回时复用已有 Activity，避免第二个 NativeActivity
重建进程内的 winit/Bevy 单例。普通切后台/返回及文档选择器往返保留当前界面和剧情，
仅重建渲染 surface；主动退出或系统杀死进程后的冷启动仍使用现有存档/主菜单。
启动模式依据 [Android Activity launchMode](https://developer.android.com/guide/topics/manifest/activity-element#lmode)。

## 触摸与返回

触摸由共享 owner 在按下时捕获：按钮/滑块、滚动容器优先，移出控件不转为导航。
设置页 tab 仅点击切换，不识别左右划；Save/Load 也不提供左右划导航。剧情空白区上划打开 backlog；
backlog 正文拖动只滚动列表。普通按钮轻点松手后确认点击，拖动不触发起点按钮；
滑块按捕获位置实时更新。命中检查包含裁剪与层叠遮挡，遵守 Bevy 的 FocusPolicy：
Block 遮挡，Pass 容器空白处穿透；顶部透明整页容器不拦截下面的设置控件。
普通 Node 不作为滚动容器。按钮保持在原控件内且未拖动时，按住较久也可在松手时点击；
手势超时仅用于剧情导航。
剧情轻点松手后才推进，上划不同时推进。
系统返回沿用 Esc 的当前界面取消/关闭路径，标题页仍先弹出退出确认；不在划动过程中执行。
屏幕边缘 24 逻辑像素保留给系统；画布外、按钮/滑块/列表、多指、取消、失焦、窗口或路由变化、
动画中及选择/视频覆盖期间不识别导航。划动至少 56 逻辑像素、主轴为副轴 1.8 倍，
在 800 ms 内松手且轨迹不能明显折返；移动超过 10 像素不再视为轻点。
手势超时使用真实时间，不受剧情休眠暂停影响。恢复运行时清除休眠前的渲染时间戳并
重置真实时钟基点，避免新菜单动画吃进休眠时间；桌面和安卓保持同一套动画时长。
不提供剧情左右划跳读、列表下划关闭等容易误触的操作。

## 部署测试工程

开发测试可先安装 APK，再通过 debug 包的 `run-as` 部署现有 fixture（不修改 fixture）：

```sh
adb install -r dev/android/app/build/outputs/apk/debug/app-debug.apk
adb shell run-as moe.maincore.keine mkdir -p files/game
tar -C tests/fixtures/native-smoke -cf - . | \
    adb shell -T run-as moe.maincore.keine tar -xf - -C files/game
adb shell am force-stop moe.maincore.keine
adb shell am start -n moe.maincore.keine/.EngineActivity
```

这不是正式游戏发行流程；Hakutaku 游戏打包、资源安装/更新与正式 APK 签名后续单独接入。

## 验收边界

交叉编译和 APK 校验不能代替手机运行。真机仍需验证启动/恢复、菜单触摸、滚动/滑块、
输入法、音频与 GPU 效果；当前只承诺构建入口，不声称已经通过这些验收。
