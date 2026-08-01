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
| macOS | AVPlayer + AVPlayerItemVideoOutput | AVPlayer 单时钟 | CVPixelBuffer BGRA → 稳定 Bevy Image | 已接入，待真机素材验收 |
| Windows | FFmpeg + rodio | 引擎 elapsed + 独立音频源 | 软件 RGBA → 稳定 Bevy Image | 可用回退；下一阶段迁移 IMFMediaEngine |
| Linux | FFmpeg + rodio | 引擎 elapsed + 独立音频源 | 软件 RGBA → 稳定 Bevy Image | 保留 |
| Android/iOS | 无承诺 | — | — | 延期 |

macOS 发行按内容检测启用 `video-native`，不要求安装或分发 FFmpeg。开发别名同时启用
`video-native,video-ffmpeg`，使同一命令在 macOS 选择 AVFoundation、在 Windows/Linux
选择 FFmpeg。`video-ffmpeg` 仍可单独用于回退验证。

首选发行容器为 MP4/M4V，编码为 H.264 + AAC。MOV 可作为开发输入；WebM/MKV 只保证
FFmpeg 后端。透明视频不进入第一阶段承诺，现有 `VideoMode::Mixed` 是黑底素材的 Screen
混合，不等同于 alpha channel。

## 当前数据路径

FS mount 直接把真实文件路径交给平台播放器。Hexz 或其他非文件 mount 由后台 source
worker 顺序复制到带原扩展名的 `NamedTempFile`，播放器关闭后自动删除。这个过渡路径有
以下明确限制：

- 播放前需要完整复制，首帧延迟与视频大小成正比；
- 临时文件是明文，进程崩溃或被强杀时只能依赖系统临时目录回收；
- 峰值磁盘占用至少等于压缩后媒体文件大小；
- 它没有利用 Hexz 已具备的 seekable `ResourceFile`。

因此该实现只作为平台播放器接入阶段的兼容层，不作为最终的加密资源方案。

## Hexz / hexz_k 待办暂存

以下任务需要 KÄne 与 `maincoretech/hexz_k` 协同，先记录在这里，不在当前改动中隐式
扩张 `ContentMount` 或复制 Hexz 协议：

- [ ] 在 `hexz_k` 提供稳定、线程安全、O(1) clone 的随机读取合同：长度查询、`read_at`
  / seek、短读、EOF、取消和错误类型；不得要求把 entry 整体复制到内存。
- [ ] 明确同一媒体 entry 的并发 range read 与 block cache 上限，避免平台播放器预读造成
  解压 block 抖动或无界缓存。
- [ ] macOS 实现自定义 URL scheme + `AVAssetResourceLoaderDelegate`，把 content info、byte
  range 和取消请求映射到上述读取合同。
- [ ] Windows 实现 `IMFByteStream`，必要时配合 `IMFMediaEngineExtension`/自定义 scheme，
  保证异步 `BeginRead/EndRead`、seek、长度和 shutdown 生命周期完整。
- [ ] 为 MP4 要求可 seek 的 `moov` 元数据；发布打包阶段检查 fast-start 布局，避免播放器
  为读取尾部索引产生不必要的全文件扫描。
- [ ] 对播放器请求做 MIME/container 映射，不依据解密后的临时文件名猜测格式。
- [ ] 增加取消、进程退出、错误中断和循环播放测试，验证归档句柄、解压 block 与平台回调
  不在 session 结束后存活。
- [ ] `hexz_k` 合并所需 API 后，KÄne 只更新固定 git rev 并在同一提交记录 API 版本；
  不同时依赖浮动分支和旧 rev。
- [ ] 准备小型 H.264/AAC、无音轨、损坏头、长 GOP、尾部 `moov` 与 Hexz 加密 fixture；
  fixture 不包含发行项目素材或真实密钥。

编译进客户端的资源密钥仍按既定离线游戏模型处理；自定义 byte source 的目标是消除持久
明文临时文件并收紧生命周期，不把内置密钥宣传为不可提取的 DRM。

## 后续性能阶段

第一阶段仍把 CVPixelBuffer/D3D surface 复制到 Bevy CPU `Image`，但复用 texture handle，
不会在 main world 保留第二份像素。完成语义与生命周期验收后再推进零拷贝：

- macOS：`CVMetalTextureCache` 把 CVPixelBuffer 包装为 Metal texture；
- Windows：IMFMediaEngine frame-server 输出 DXGI surface，并与 wgpu 外部纹理互操作；
- 两端都必须先证明 adapter/format/同步和设备丢失处理可维护，再替换 CPU 上传路径。

零拷贝不能改变 Screen blend、颜色空间、sRGB 采样或 Bevy render-world 清理语义。
