# LetsGal Studio 1.9.0 完全同步验收

本轮只验收原生工程同步，不安装或启用任何 Studio 扩展。Studio 原版 Player 可以关闭；
画面以独立 crabgal 窗口为准。

本文的“调试位置”只表示 Studio 当前选中的 fragment/block，不是鼠标指针。个性化鼠标指针
不在兼容范围内，窗口应始终使用操作系统默认指针。

## A. 启动与隔离

1. 在 Studio 打开待测工程，确认工程根有 `project.json` 与 `.studio/state.json`。
2. 在 crabgal 仓库运行：

   ```bash
   cargo studio-sync '/absolute/path/to/LetsGal project'
   ```

3. 预期直接出现一个正常可缩放的 crabgal 原生窗口；没有安装提示、CRABGAL 扩展按钮、灰屏、
   端口占用或 10 秒自动退出。
4. Studio 与 crabgal 分别切到后台 15 秒。预期窗口继续渲染且不重启、不抢焦点。
5. 关闭 Studio。预期 crabgal 保持运行；重新打开 Studio 并继续编辑后仍能收到文件变化。

## B. 调试位置与确定性状态

6. 在同一 fragment 依次选择 10 个 block。预期 crabgal 每次停在所选 block，不多走一格。
7. 选择一条 dialogue。预期姓名、文字与 textbox 出现；只选择 block 不应先闪标题页。
8. 选择 narration/storyParagraph。预期没有空姓名黑框。
9. 选择含多个底层 Action 的 scene/camera/particle block。预期该 block 的完整结果都出现，
   不停在半完成状态。
10. 在前一 fragment 设置背景、角色、BGM、变量，直接选择后一 fragment 的对白。预期 runtime
    从入口重放，继承路径上的全部状态，而不是只孤立运行后一 fragment。
11. 在两个分支间来回选择。预期每次从项目默认变量重建，不继承上一次预览的临时分支值。
12. 修改 `project.variables.json` 的 slot/shared 默认值，保存后重新选择引用它的 block。预期
    表达式与文本使用新值；角色属性使用 `<角色 UUID>.<属性名>` 的当前默认值。

## C. 内容与资源热重载

13. 修改当前 block 文本并保存。预期无需重启，重新编译后仍停在当前 block，显示新文本。
14. 新增/删除/重排 block。预期 action span 随新 index 更新，调试位置不沿用旧 Action 偏移。
15. 修改 character expression、scene layer、camera、particle、sound、video props。预期保存后
    对应 typed Action 更新，无 `unsupported LetsGal block type`。
16. 替换已加载图片/音频文件但保持逻辑路径。预期 Bevy handle 原位热重载；不扫描无关资源。
17. 新增资源并让 manifest 与 block 引用它。预期 manifest 保存后 config alias 更新，资源可加载。
18. 快速连续保存 chapter 和 `.studio/state.json`。预期没有永久停留在旧画面；短暂 JSON 读取
    冲突会自动跨帧恢复；即使文件通知被系统合并，200 ms 调试位置轮询也会兜底。

## D. 规定动作与 1.9.0 新语义覆盖

19. 逐项覆盖：`dialogue`、`narration`、`storyParagraph`、`showCharacter`、
    `removeCharacter`、`switchDialogueStyle`、`portraitStyleRule`、`floatingText`、`scene`、
    `destroyScene`、`curtain`、`camera`、`resetCamera`、`animateSprite`、`particle`、`sound`、
    `stopSound`、`video`、`stopVideo`、`wait`、`setver`、`if`、`branch`、`callFragment`、
    `endChapter`、`returnToEntry`、`enterAutoPlay`、`exitAutoPlay`、`playerInput`、`comment`、
    `showExtensionUI`、`hideExtensionUI`、`callExtensionFunction`、`stageAnimation`。
20. 每项至少检查一次资源、参数、等待/非等待和目标清理。内置类型不得降级为第三方
    `HostCommand`；真正的第三方 extension capability 未安装 host plugin 时只记录明确警告。
    `shiftz.backspace/backspace-to` 和旧
    `maincore.backspace-to/backspace-to` 必须直接编译为原生句尾退格：按打字速度反向删除到
    `keep`，删完后只接受一次新的推进输入，连续退格不吞原文；同步定位到退格块时保留动画，
    定位到后续块时则确定性完成之前的退格，不加载扩展 bundle。
21. 为 `stageAnimation` 建立 camera、character、scene layer 三类 track，分别覆盖首关键帧插值、
    相邻关键帧 easing、hold-last、倍率、有限循环、无限循环和 muted track；预期三类目标共用一个
    时钟，低帧率下也不发生相互漂移。
22. 在同一时间轴加入 camera patch、camera shake、particle cue、scene cue 与 audio cue。
    预期事件只在越过时间点时触发；有限循环逐轮重复，下一轮开始时上一轮 patch 不泄漏；
    audio 的 BGM/SE/VOCAL、音量、循环、持续时间和淡入淡出由同一时钟驱动；muted event 不播放；
    blocking 仅在有限且 `waitForComplete=true` 时阻塞剧情。
23. 添加 `wait` 并启用 `waitForInput`。预期持续时间被忽略，停留任意时间都不会自动继续；
    明确点击一次后才进入下一 block。
24. 给角色配置全局 `graphics.heightRatio`、表情级 `graphicsOverride.heightRatio`、
    `portraitSkinConfig` 与 `skinAssets`。预期表情级高度优先；未锁定皮肤时按角色属性选图，
    block 明确指定 `skin` 时固定使用该皮肤。
25. 从一个章节调用另一个章节的 fragment。预期按稳定 fragment id 进入并在结束后返回原调用点，
    不依赖当前文件名或 Studio 的运行对象。

## E. 同步控制权与窗口

26. 在 crabgal 窗口单击、滚轮、按 Enter/Space。预期打字和动画可以继续，但剧情执行位置不会
    脱离 Studio 当前 block；推进必须在 Studio 选择新 block。
27. 调整 crabgal 窗口大小并移动到另一显示器。预期 1920×1080 逻辑视口等比居中，背景、
    scene layer、角色、粒子、textbox 使用同一裁切区。
28. 打开/关闭 SAVE、LOAD、CONFIG、BACKLOG。预期不改变 Studio block；返回后画面仍是同一
    确定性预览状态；确认存档/清除/设置等持久化操作不应在工程中产生新文件。

## F. 自动回归

```bash
cargo test -p crabgal-loader adapter::editor::letsgal
cargo test --lib editor_seek
cargo check --workspace --all-targets
```

失败记录格式：

```text
步骤：15
fragment UUID：...
block index/type：12 / scene
现象：...
修改的文件：chapters/xxx.json
日志最后 20 行：...
```
