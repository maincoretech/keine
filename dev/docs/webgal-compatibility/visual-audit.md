# 视觉审计与验收矩阵

## 当前结论

仓库目前没有可复现的自动视觉回归基线，也不保留某个示例项目的人工截图作为引擎基线。单元测试可以证明布局数学、状态迁移和部分渲染辅助逻辑，但不能证明最终画面、动画观感、音频或跨平台一致性。

因此跨平台视觉一致性仍不能由自动测试证明。`dev/docs/acceptance/` 只定义步骤，不代表已经
执行通过；下方人工验收记录只对明确列出的环境和状态有效。

## 2026-08-27 macOS 人工验收

- 基线：`1db8e15`，Apple M5 Pro / Metal，本地打包的 shipping `.app`，通过 Computer Use
  直接操作真实窗口；证据截图保存在 `/private/tmp/keine-T03-captures-20260827/`，不进入仓库。
- 已检查标题页（CONTINUE 禁用和有存档两态）、四种语言、CONFIG 往返、空 EXTRA、舞台、
  Backlog、SAVE/LOAD、Q·SAVE 与返回标题 Dialog、续玩恢复、系统 Zoom、原生全屏及全屏
  CONFIG。未发现可稳定复现的布局、层级、blur、输入隔离或视口缺陷。
- 系统 Zoom 恢复到非 16:9 窗口时，画面保持居中的 16:9 letterbox；原生全屏填满 16:9
  显示区域，退出和续玩没有永久坐标偏移。
- 工具只能可靠保留动作完成后的稳定帧，未对 0.2 秒淡入、pressed 中间帧或纯 hover
  中间帧做逐帧判定。Windows 需要用户输入远程凭据，Linux 和 1× DPI 目标当前不可用；这些
  均记为未覆盖，而不是模拟通过。

## 现有自动证据

| 视觉域 | 已有证据 | 尚不能证明 |
|---|---|---|
| 16:9 视口 | wide/tall viewport 数学测试 | 完整窗口帧、DPI 与命中区域 |
| 背景和立绘 | 图片尺寸上限、cover/fit 辅助逻辑测试 | GPU 输出、透明边缘与混合效果 |
| Textbox | ruby 碰撞、旁白宽度、逐字与伸缩动画测试 | 字体栅格、阴影、blur 与真实多行画面 |
| Choice | 条件、禁用、键盘与数字键状态测试 | hover/pressed/touch 和多尺寸布局 |
| transition/transform/filter | 状态机、稀疏 patch 与采样逻辑测试 | 0/50/100% GPU 关键帧 |
| Backlog/Save/Load/Settings/Extra/Dialog | 存储、输入 scope 与 UI helper 测试 | 完整页面动效、层级与视觉一致性 |
| BGM/voice/SE | 状态与解码测试 | 可听结果、设备切换与响度 |

## 必测 UI 组合

| UI 面 | 状态组合 | 必查项 |
|---|---|---|
| Title | 无存档/有存档；EXTRA 空/已解锁 | enabled/disabled、Continue 预览、旧舞台不透出 |
| Stage/Textbox | 旁白/角色、ruby/style、mini avatar、hide | 字体、阴影、模糊、伸缩、内容裁切、输入作用域 |
| Choice | normal/hover/pressed/selected/disabled | 禁用项不可触发、焦点移动、无输入穿透 |
| Backlog | 空/长列表、有/无 voice、rollback | 滚动、重播、恢复、关闭动画 |
| Save/Load | 空槽/满槽/分页/覆盖/删除/Dialog | 预览、卡片信息、分页动画、遮罩层级 |
| Settings | System/Display/Audio/About | hover/pressed、滑杆、下拉、持久化 |
| Dialog/Input | confirm/cancel、文字输入、叠在各 screen 上 | caret、按钮统一样式、模糊、焦点和快捷键隔离 |
| Extra | CG 全屏、BGM 播放与 seek | 控件、进度、退出清理、左右布局 |

## 尺寸与输入覆盖

至少验证 1920×1080、1280×720、2560×1080、1280×1024，并分别检查 1× 与高 DPI。输入覆盖鼠标、键盘、触控和手柄；移动端 SafeArea 仍需独立验收。

## Golden 建议

1. 以 1920×1080 为首个像素基线，其他比例先做归一化几何断言。
2. 固定时钟，在动画 0%、50%、100% 采样；等待资源就绪且 UI activity 稳定后截图。
3. 字体区域允许小 tolerance 或 mask，舞台材质区域使用更严格阈值。
4. 每个 artifact 记录 keine commit、OS、GPU/driver、逻辑/物理尺寸、DPI、脚本行和时间点。
5. 不同平台使用独立基线，不能相互覆盖。

建议目录：

```text
dev/docs/webgal-compatibility/artifacts/
  YYYY-MM-DD/<platform>/<case>/
    metadata.json
    actual.png
    expected.png
    diff.png
```

该目录目前不存在；这里只定义后续格式。2026-08-27 的人工截图保持为临时任务证据，未被
提升为 golden 基线。
