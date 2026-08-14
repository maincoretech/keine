# 桌面视频后端与 Hakutaku 流式读取

> 状态：macOS 已接入 AVFoundation；Windows/Linux 当前保留 FFmpeg。移动端暂缓。

## 目标与边界

视频命令、阻塞规则、循环、淡入淡出、全屏/混合层级和 Screen blend 都属于
`keine-core` 与 Bevy scene 的公共语义。平台后端只负责三件事：

1. 打开统一逻辑路径对应的媒体源；
2. 以平台播放器的音频时钟推进并输出最新视频帧；
3. 报告结束或明确错误。

渲染实体、稳定 `Image` handle、材质、viewport 和清理流程集中在
`src/scene/video/shared.rs`，平台后端不得复制这些规则。

## 当前桌面矩阵

| 平台 | 后端 | 视频/音频时钟 | 当前帧上传 | 状态 |
| --- | --- | --- | --- | --- |
| macOS | AVPlayer + AVPlayerItemVideoOutput | AVPlayer 单时钟 | CVPixelBuffer → CVMetalTexture → 稳定 Bevy GPU Image | 已用 FS 与加密 Hakutaku fixture 验收解码及 GPU 导入 |
| Windows x64 | FFmpeg + rodio | rodio 音频时钟；无音轨时用逻辑时钟 | 软件 RGBA → 稳定 Bevy Image | 已接入并真实解码 |
| Linux | FFmpeg + rodio | rodio 音频时钟；无音轨时用逻辑时钟 | 软件 RGBA → 稳定 Bevy Image | 已接入并真实解码 |
| Android/iOS | 无承诺 | — | — | 延期 |

发行构建只启用目标平台所需的一个后端：macOS 使用 `video-native`，Windows/Linux 使用
`video-ffmpeg`。开发别名可同时启用两个 feature；macOS 仍只选择 AVFoundation，其他平台
只选择 FFmpeg，不会启动两套播放器。

Windows CI 使用 `.github/actions/setup-video/vcpkg.json` 的完整 vcpkg baseline 固定 FFmpeg
8.1.2，与 Rust `ffmpeg-next 8.1` binding 保持同一 release branch。升级任一侧时必须同步升级
另一侧并通过 Windows feature-specific unit、acceptance 与 release build，不能依赖 runner
预装 vcpkg registry 的最新 FFmpeg 主版本。Windows vcpkg、Linux development packages 与
Rust crate features 都只启用解码所需的 `avcodec`、`avformat`、`swscale`、`swresample`
（以及隐式 `avutil`）；不构建或安装未使用的 `avdevice`、`avfilter`。macOS 发行只使用
AVFoundation，不携带 FFmpeg SDK。

首选发行容器为 MP4/M4V，编码为 H.264 + AAC。MOV 可作为开发输入；WebM/MKV 只保证
FFmpeg 后端。透明视频不进入第一阶段承诺，现有 `VideoMode::Mixed` 是黑底素材的 Screen
混合，不等同于 alpha channel。

### 可插拔边界

视频按两层插件边界组织，而不是让平台对象穿过整个运行时：

- `keine-media` 是小型、无 Bevy 依赖的 codec crate，承接可独立测试和 fuzz 的有界原生
  编解码入口；图片 loader 只负责把其输出适配成 Bevy `Image`；
- 顶层 `VideoPlugin` 只按 target/feature 选择一个 backend `Plugin`。FFmpeg、
  AVFoundation/Metal 和无后端降级插件分别注册自己的资源、线程模型和 Update systems。

新后端应实现这一 backend plugin 形状，并复用 `video/shared.rs` 的 source、visual 和全局
内存预算契约。这里有意不用动态 trait object：FFmpeg playback 是可发送的 worker/resource，
AVFoundation playback 则必须是 macOS 主线程 `NonSend` 状态；构建期选择能保留这两个所有权
不变量，也不会为每帧调用增加虚分派。

## 当前数据路径

FS mount 保留平台快速路径，直接把真实文件路径交给播放器。Hakutaku 和其他虚拟 mount 则
复用 `ContentMount`/`ContentFile` 的长度、seek 与短读合同，不创建完整内存副本，也不写
明文临时文件：

- macOS 使用自定义 `keine-video://` URL 和
  [`AVAssetResourceLoaderDelegate`](https://developer.apple.com/documentation/avfoundation/avassetresourceloaderdelegate)，
  把 AVFoundation 的 content-info、byte-range 与取消请求映射到新的独立读取游标；
- Windows/Linux 使用 FFmpeg 原生 `AVIOContext` read/seek 回调，直接读取同一个随机访问源；
- 每个回调游标直接持有 `AssetCursor`，Hakutaku 的 memory-constrained block cache
  继续限定解压内存；播放器结束后随 session 一起释放。

测试 fixture 是 1 秒、320×240、H.264 Constrained Baseline + AAC、fast-start MP4。CI 会把它
现场加密进临时 Hakutaku 包再真实解码；不包含项目素材或发行密钥。

## 时钟与循环语义

FFmpeg 后端只负责解码。存在音轨且未静音时，rodio sink 的实际播放位置是视频主时钟，
暂停和恢复不会靠帧数推算；无音轨或明确静音时才使用引擎逻辑 elapsed。主音量变化直接
更新既有 sink，不重建音频流。循环以媒体时长累加时间线并 seek 回起点，不把单次循环的
时间戳倒退暴露给画面调度；音频流也在 EOF 后重新打开随机读取源，不使用 rodio 会缓存
整段解码 PCM 的通用循环器。

软件 decoder 与 Bevy 帧线程之间使用容量为 2 的 bounded channel，避免额外保留第三个
1080p RGBA buffer。队列满时 decoder 通过 Crossbeam `select_biased!` 同时等待发送容量和
session cancellation，并优先处理取消；因此暂停消费不会产生原先每 2ms 一次的轮询唤醒，
session 清理也不会被阻塞发送卡住。所有 `Ready`、frame、end 和 error event 都经过同一
可取消发送路径。

自动化覆盖加密 Hakutaku 随机读取、音视频时长误差、三次循环单调性、暂停/恢复、无音频长时
回退时钟，以及 macOS AVFoundation 的 FS/Hakutaku 首帧解码和 Core Video → Metal → wgpu
实际复制。Windows 当前只发布并验证 x64；ARM64 原生构建暂缓，在具备明确发行需求和
真实硬件验收条件后再恢复。

`.github/workflows/media-safety.yml` 另外在媒体代码变化和每周计划任务中执行两层防御：

- `cargo-fuzz` 将项目内真实 WebP 作为 seed，持续变异字节并调用生产 libwebp decoder；
  libFuzzer 默认启用 AddressSanitizer，单次 CI smoke 限 60 秒、输入限 4 MiB、RSS 限 2 GiB，
  这些是 CI 资源参数而不是运行时资源格式上限；
- nightly Rust ASan 运行 Linux FFmpeg 的 `keine-video-acceptance`，同一次执行覆盖
  filesystem 与加密 Hakutaku source。构建时重编译并 instrument Rust 标准库；系统 FFmpeg
  等预编译 native 库本身不会因此获得完整 instrumentation，所以这属于 FFI
  ownership/buffer 防御纵深，不替代 codec 上游安全更新或专门的 native-library sanitizer
  build。具体工具链用法遵循
  [Rust sanitizer 文档](https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html)。

macOS 的 native unit、filesystem/Hakutaku 首帧和 Metal 导入仍由常规 CI acceptance 强制
通过，但不把完整 Bevy binary 的 ASan 链接设为 gate：当前 nightly + macOS runner 的 `ld`
会在链接大量 instrumented Bevy archive 时以 `initializer pointer has no target` 失败，尚未
生成可执行文件。工具链升级后应重新评估；在此之前依靠收紧的 unsafe invariants、竞态测试
和真实 native acceptance，而不是用 `continue-on-error` 制造虚假的 sanitizer 绿灯。

fuzz crash 输入会作为 workflow artifact 上传；本地复现使用 nightly 与固定的
`cargo-fuzz 0.13.2`：

```text
cargo +nightly fuzz run webp_decode fuzz/corpus/webp_decode \
  projects/test-project/assets/backgrounds -- -max_len=4194304
```

## 可选后续优化

- Windows 可在真实播放与发行稳定后评估 `IMFMediaEngine` + `IMFByteStream`，以利用系统
  硬件解码；这不是消除明文临时文件的前置条件。
- 增加损坏头、无音轨、长 GOP 与尾部 `moov` fixture，并验证中途取消和进程退出。
- 发布检查可进一步拒绝非 fast-start 的大 MP4，减少远端或分块源的尾部探测。

编译进客户端的资源密钥仍按既定离线游戏模型处理；自定义 byte source 的目标是消除
持久明文副本；它不把内置密钥变成不可提取的 DRM。

## 帧传输与后续性能阶段

macOS 已不再锁定 `CVPixelBuffer`、分配 `Vec<u8>` 或经过 main-world 像素副本。
[`CVMetalTextureCacheCreateTextureFromImage`](https://developer.apple.com/documentation/corevideo/cvmetaltexturecachecreatetexturefromimage%28_%3A_%3A_%3A_%3A_%3A_%3A_%3A_%3A_%3A%29)
将解码帧映射为 Metal texture，render world 再用一次 GPU blit 写入
稳定的 Bevy texture。稳定 handle 保留了材质 bind group、Screen blend、sRGB 采样和
清理语义；源 pixel buffer 与 `CVMetalTexture` 会一直持有到对应 queue submission 完成。
这消除了 CPU 像素复制，但不是“直接采样解码纹理”：后者还可省一次 GPU 内部复制，代价
是每帧更换外部纹理绑定并处理更复杂的同步与设备丢失，当前不值得破坏项目一体性。

Windows/Linux 仍使用 FFmpeg 软件 RGBA 帧和 Bevy texture upload。未来只有在能提供真实
硬件、发行和设备丢失验收时，才分别评估 IMF/DXGI 与 VA-API/DMABUF；在那之前不以额外
跨平台封装或大依赖换取未经验证的“零拷贝”。
