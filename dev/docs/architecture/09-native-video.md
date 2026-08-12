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

首选发行容器为 MP4/M4V，编码为 H.264 + AAC。MOV 可作为开发输入；WebM/MKV 只保证
FFmpeg 后端。透明视频不进入第一阶段承诺，现有 `VideoMode::Mixed` 是黑底素材的 Screen
混合，不等同于 alpha channel。

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

自动化覆盖加密 Hakutaku 随机读取、音视频时长误差、三次循环单调性、暂停/恢复、无音频长时
回退时钟，以及 macOS AVFoundation 的 FS/Hakutaku 首帧解码和 Core Video → Metal → wgpu
实际复制。Windows 当前只发布并验证 x64；ARM64 原生构建暂缓，在具备明确发行需求和
真实硬件验收条件后再恢复。

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
