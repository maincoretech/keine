# 桌面视频后端与 Hexz 流式读取

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
| macOS | AVPlayer + AVPlayerItemVideoOutput | AVPlayer 单时钟 | CVPixelBuffer BGRA → 稳定 Bevy Image | 已用 FS 与加密 Hexz fixture 验收 |
| Windows x64/ARM64 | FFmpeg + rodio | rodio 音频时钟；无音轨时用逻辑时钟 | 软件 RGBA → 稳定 Bevy Image | 已接入；x64 真实解码、ARM64 交叉编译 |
| Linux | FFmpeg + rodio | rodio 音频时钟；无音轨时用逻辑时钟 | 软件 RGBA → 稳定 Bevy Image | 已接入并真实解码 |
| Android/iOS | 无承诺 | — | — | 延期 |

发行构建只启用目标平台所需的一个后端：macOS 使用 `video-native`，Windows/Linux 使用
`video-ffmpeg`。开发别名可同时启用两个 feature；macOS 仍只选择 AVFoundation，其他平台
只选择 FFmpeg，不会启动两套播放器。

首选发行容器为 MP4/M4V，编码为 H.264 + AAC。MOV 可作为开发输入；WebM/MKV 只保证
FFmpeg 后端。透明视频不进入第一阶段承诺，现有 `VideoMode::Mixed` 是黑底素材的 Screen
混合，不等同于 alpha channel。

## 当前数据路径

FS mount 保留平台快速路径，直接把真实文件路径交给播放器。Hexz 和其他虚拟 mount 则
复用 `ContentMount`/`ContentFile` 的长度、seek 与短读合同，不创建完整内存副本，也不写
明文临时文件：

- macOS 使用自定义 `keine-resource://` URL 和
  [`AVAssetResourceLoaderDelegate`](https://developer.apple.com/documentation/avfoundation/avassetresourceloaderdelegate)，
  把 AVFoundation 的 content-info、byte-range 与取消请求映射到新的独立读取游标；
- Windows/Linux 使用 FFmpeg 原生 `AVIOContext` read/seek 回调，直接读取同一个随机访问源；
- 每个回调游标都持有 O(1) clone 的 `ResourceFile`，Hexz 的 memory-constrained block cache
  继续限定解压内存；播放器结束后随 session 一起释放。

测试 fixture 是 1 秒、320×240、H.264 Constrained Baseline + AAC、fast-start MP4。CI 会把它
现场加密进 Hexz 再真实解码；不包含项目素材或发行密钥。

## 时钟与循环语义

FFmpeg 后端只负责解码。存在音轨且未静音时，rodio sink 的实际播放位置是视频主时钟，
暂停和恢复不会靠帧数推算；无音轨或明确静音时才使用引擎逻辑 elapsed。主音量变化直接
更新既有 sink，不重建音频流。循环以媒体时长累加时间线并 seek 回起点，不把单次循环的
时间戳倒退暴露给画面调度；音频流也在 EOF 后重新打开随机读取源，不使用 rodio 会缓存
整段解码 PCM 的通用循环器。

自动化覆盖加密 Hexz 随机读取、音视频时长误差、三次循环单调性、暂停/恢复、无音频长时
回退时钟，以及 macOS AVFoundation 的 FS/Hexz 首帧解码。Windows ARM64 目前由交叉编译
保证 API 与依赖闭合；真实 ARM64 硬件播放仍属于发布验收。

## 可选后续优化

- Windows 可在真实播放与发行稳定后评估 `IMFMediaEngine` + `IMFByteStream`，以利用系统
  硬件解码；这不是消除明文临时文件的前置条件。
- 增加损坏头、无音轨、长 GOP 与尾部 `moov` fixture，并验证中途取消和进程退出。
- 发布检查可进一步拒绝非 fast-start 的大 MP4，减少远端或分块源的尾部探测。

编译进客户端的资源密钥仍按既定离线游戏模型处理；自定义 byte source 的目标是消除
持久明文副本；它不把内置密钥变成不可提取的 DRM。

## 后续性能阶段

第一阶段仍把 CVPixelBuffer/D3D surface 复制到 Bevy CPU `Image`，但复用 texture handle，
不会在 main world 保留第二份像素。完成语义与生命周期验收后再推进零拷贝：

- macOS：`CVMetalTextureCache` 把 CVPixelBuffer 包装为 Metal texture；
- Windows：IMFMediaEngine frame-server 输出 DXGI surface，并与 wgpu 外部纹理互操作；
- 两端都必须先证明 adapter/format/同步和设备丢失处理可维护，再替换 CPU 上传路径。

零拷贝不能改变 Screen blend、颜色空间、sRGB 采样或 Bevy render-world 清理语义。
