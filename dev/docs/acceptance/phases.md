# Phase 1–7 验收清单

已完成阶段的手工验收步骤集中于此；LetsGal Studio 集成仍保留独立清单。

## Phase 1 桌面端验收提纲

### 启动

```bash
cargo validate projects/test-project
cargo dev projects/test-project
```

无窗口检查的当前基线是 `11 scene(s) · 184 action(s) · 2 source(s) · 0 warning(s)`。
窗口必须先进入标题页；点击 **START** 后显示樱花背景、左侧小夜和九项展示菜单。

### 核心流程

选择 **01 · 流程、变量与选择**：

1. 第一行显示 `chapter=1`、数组第二项 `true` 和全局展示版本 `8`；speaker 正确插值为“小夜”。
2. 条件为真的句子出现；带 `-next` 的句子不建立独立等待点，直接衔接下一句。
3. 立绘平滑偏移、缩放、旋转、变透明并模糊，随后恢复；稀疏 patch 不能重置未出现字段。
4. Choice 中“隐藏选项”完全不可见，“可见但禁用”置灰且无法用鼠标、键盘或手柄确认。
5. 选择 **当前场景 label** 后显示 label 说明并结束分项。
6. 再次进入，选择 **嵌套 callScene**；右侧访客退场后准确返回选择的下一条 action。
7. 两种路径都自然回到九项主菜单，不返回标题，也不重新执行入口初始化。

### changeScene 与 end

在主菜单选择 **结束并返回标题**：

1. `changeScene` 清除调用栈并进入 `phase1_end.txt`；不能回到展示菜单。
2. 新背景通过 dissolve 切入，右侧角色出现。
3. 推进最后一句后，`end` 清理舞台、音频、视频、对话和交互状态并返回标题页。

### 通用回归

- 调整窗口后背景、立绘、Textbox 和 Choice 继续锁定 1920×1080 设计空间；
- Choice 只有每个选项自身的局部 blur，禁用项不会进入焦点；
- Q·SAVE/Q·LOAD、Dialog 和全屏菜单不能让舞台快捷键穿透；
- 空 speaker 隐藏姓名框，有 speaker 时姓名框只包住文本；
- 重复进入分项不会残留上一轮动画、Choice、场景栈或音频。

---

## Phase 3 验收清单

### 准备

1. 在仓库根目录运行：

   ```bash
   cargo dev projects/test-project
   ```

2. 点击 `START`，推进到出现姓名、正文和语音的对话。
3. 测试时不要手动修改 `projects/test-project/saves/`；需要检查持久化时先正常退出引擎。

### A. Backlog 与回想

1. 连续推进至少 6 句对话。
2. 点击上栏 `file-text` 图标进入 Backlog。
3. 确认 Backlog 快速淡入，背景模糊但所有历史文字保持清晰。
4. 滚动列表，确认方向自然、无跳动，姓名与正文没有被裁切。
5. 点击有语音记录的重播按钮，确认播放对应语音。
6. 点击较早记录的回想按钮，确认场景、立绘、变量和对话回到该节点。
7. 按 `Esc` 或关闭按钮退出，确认快速淡出并恢复游戏输入。

### B. 已读历史与 Skip

1. 在 SYSTEM 中把 Skip Mode 设为 `READ`。
2. 回到游戏，按 `S` 开启 Skip；确认遇到未读对话时自动停止。
3. 再次读过同一段后开启 Skip，确认已读内容可以跳过。
4. 按 `Shift+S` 切换为 `ALL`，再按 `S`；确认未读内容也会跳过。
5. 退出并重新启动，打开 SYSTEM，确认 Skip Mode 保留上次选择。

### C. SAVE

1. 点击底栏 `SAVE`，确认出现 20 个页码与 5×2 的 10 张槽位卡片。
2. 点击页码 2，确认显示槽位 11–20，并有快速错峰进入动画。
3. 返回第 1 页，点击空槽 1；空槽应直接保存，不出现覆盖确认。
4. 再次打开 SAVE，确认槽位 1 显示：槽号、时间、场景截图、姓名和正文。
5. 确认截图是纯场景画面，没有把文本框、按钮或存档面板截进去。
6. 再次点击槽位 1，确认出现覆盖提示；点取消后原存档不变。
7. 再次覆盖并确认，重新打开 SAVE，确认时间、文本和截图已更新。

### D. LOAD 与删除

1. 推进几句使当前画面明显不同，然后打开 `LOAD`。
2. 点击空槽，确认没有任何响应，也不会破坏当前状态。
3. 点击槽位 1，确认先出现读取确认；取消后当前游戏不变。
4. 再次读取并确认，确认场景、立绘、变量、对话和游标恢复到保存位置。
5. 打开 SAVE 或 LOAD，把鼠标放在槽位 1 上并右键。
6. 确认出现删除提示；先取消，槽位仍存在。
7. 再次右键并确认删除，确认卡片变回空槽，截图同时消失。
8. 重新启动引擎，确认已删除槽位不会恢复。

### E. SYSTEM

1. 点击底栏 `SYSTEM`，确认游戏暂停，面板显示四项设置。
2. 把 Master Volume 降到 0%，确认当前语音和后续语音静音；恢复后声音重新出现。
3. 调整 Text Speed 到低值，观察新对话逐字显示明显变慢；调高后明显变快。
4. 调整 Auto Delay，开启 `AUTO`，确认每句完整显示后的等待时间随设置变化。
5. 点击 Skip Mode，确认在 `READ` 和 `ALL` 间切换，并停止当前 Skip。
6. 检查边界：音量不能低于 0% 或高于 100%；文字速度保持 10–120；Auto 延迟保持 0.5–5.0 秒。
7. 按 `Esc` 关闭 SYSTEM，确认恢复游戏输入。
8. 正常退出并重新启动，确认四项设置全部保留。

### F. TITLE 与组合回归

1. 点击 `TITLE`，取消一次，确认游戏状态不变。
2. 再次点击并确认，确认返回标题画面且不残留 Backlog、SAVE/LOAD、SYSTEM 或 Dialog。
3. 从标题重新开始，依次打开 Backlog、SAVE、LOAD、SYSTEM，确认没有界面重叠或输入穿透。
4. 在窗口 1280×720 和一个更宽/更高的可调整尺寸下重复打开各面板，确认布局不越过游戏视口。

### 问题记录格式

记录“章节与步骤编号、截图或录屏、预期、实际、是否稳定复现”。例如：

`D-7 / 删除后截图仍显示 / 预期为空槽 / 实际保留旧图 / 稳定复现`。

---

## Phase 4 音频与视频验收

运行 `cargo dev projects/test-project`，选择 **06 · 音频与流式视频**。

1. Opus BGM 在约 0.6 秒内淡入并循环。
2. 第一个 H.264/AAC 视频全屏播放；BGM/voice 被 duck，视频结束后 BGM 恢复。双击可跳过，
   第一次点击不能结束视频。
3. 第二个视频由 `-next` 非阻塞启动，约 1.2 秒后 `playVideo:none` 主动停止；脚本和视频
   时钟不能互相重启。
4. 第三个视频带 `-skipOff`，双击无效，必须完整播放四秒。
5. 带 `-vocal` 的句子只播放一次；控制栏 Replay 和 Backlog 语音按钮都从头重播。
6. 无 id 音效只播放一次；`phase4-loop` 按 id 循环并由同 id 的 `none` 立即停止。
7. CONFIG → AUDIO 的 MASTER、VOICE、BGM、SOUND EFFECT 滑杆实时作用于对应总线，拖动
   不重启音轨。
8. 最后 BGM 在约 0.6 秒内淡出；退出分项后无剧情媒体残留。

通过标准：视频增量解码和上传，不能把完整容器或 PCM 展开进内存；动画与媒体时钟不依赖
刷新率；损坏资源产生明确错误而不锁死脚本。

---

## Phase 5 演出验收

运行 `cargo preview projects/test-project`，分别选择菜单的 **02**、**03**、**04**。

### 02 · 转场、遮幅与混合

1. `intro` 黑屏居中显示两页文字，`-hold` 只能点击推进。
2. filmMode 黑边锁定 16:9 内容视口。
3. 依次看到 Fade、Wipe、Dissolve、Crossfade 和 Instant；wipe 不压缩纹理，dissolve 不是
   普通 alpha fade，crossfade 期间新旧图同时存在。
4. 临时立绘从右侧滑入、向左侧滑出，覆盖 Slide From Right/Left 两种 transition。
5. `setTransition` 让指定立绘从左入场并按 Exit 规则退场。
6. Alpha/Add/Multiply/Screen 四种合成同时可辨，透明边缘正常。

### 03 · 全部动画与滤镜

依次检查 17 个原生 preset：Enter、Exit、三个方向入场、Shake、Move Front And Back、Blur、
Shockwave In/Out、Old/Dot/Reflection/Glitch/RGB/Godray Film 和 Remove Film。

- 每项完成后恢复基础 transform，Exit 仅在最后一帧移除目标；
- `setTempAnimation` 与 `setComplexAnimation` 使用同一帧率无关时间线；
- filter 同时降低亮度/饱和度、提高对比并增加 blur，clear 后立即恢复；
- 低帧率、窗口拖动或短暂失焦不会改变总时长。

### 04 · 全部粒子风格

依次显示 LIGHT/MODERATE/HEAVY SNOW、LIGHT/MODERATE/HEAVY RAIN、FIREFLY 和
FALLEN_LEAVES。粒子应具有柔边、速度/尺寸/漂移差异；切换时替换 WebGAL emitter，
`pixiInit` 后完全清空。单 emitter 数量始终不超过 256，并且只对应一个 ECS 实体和一个
动态网格；HEAVY RAIN/SNOW 不应再因数百个独立 Sprite 的逐帧更新与提取造成 CPU 峰值。

---

## Phase 6 文本增强验收

运行 `cargo preview projects/test-project`，选择 **05 · 富文本、头像与输入**。

1. 第一行同时出现普通、樱花色粗体、背景色、放大、斜体和删除线；样式只影响各自区段。
2. `蟹 / 桜 / 物語` 的假名叠加在正文正上方，不占用独立行高；正文基线保持一致，
   相邻长注音通过水平占位避让而不重叠。
3. `<br>` 强制换行；第二行 ruby 不与第一行重叠。
4. `[wait=650]` 不占字形位置，在前后文本之间产生约 0.65 秒打字停顿。
5. mini avatar 淡入时 textbox 左边缘平滑右移，隐藏后恢复；旁白使用完整宽度。
6. 打字机逐字显示时，尚未出现的字保留排版位置但不可见，ruby 与正文同步出现。
7. `-notend` 在文字完成后自动执行 shake；动画完成前不跳入下一句。
8. `-next` 与 `-concat` 合并为“前半句与后半句合并。”，speaker 不丢失。
9. `setTextbox:hide` 在 0.7 秒等待期间持续隐藏，`show` 后恢复；`:` 只隐藏到下一句。
10. 输入框键入中日文、Backspace 删除；空输入不能确认。确认后变量插值到下一句。

通过标准：字号切换、Hide、菜单淡出和全局扩散阴影继续作用于每个富文本/ruby glyph，且一行只在内容改变时重建。

---

## Phase 7 工程能力逐步验收

### 运行时

1. 运行测试项目并选择 **08 · UI、存档与工程能力**；`comment:` 不产生对话或警告。
2. 分别用鼠标/Space、触控、手柄 South 推进；手柄 West 切换 Auto，右扳机切换 Skip。
3. 场景结束返回标题后打开 **EXTRA**，应显示 CG 卡片与 BGM 列表；分页、全屏 CG、
   上一曲/播放/下一曲/停止均可用，重启引擎后解锁内容仍存在。
4. 进入 **07 · 音频与流式视频**：三个 `calibration_*.opus` 从 `content/shared/audio`
   播放，`calibration_pan.mp4` 从主资源根增量解码，控制台没有 asset-not-found、Opus
   或 FFmpeg 错误。完整的逐项预期见
   [`projects/test-project/ACCEPTANCE.md`](../../../projects/test-project/ACCEPTANCE.md)。

### Hexz

```bash
export HEXZ_PASSWORD='<test-only password>'
cargo package <project> --output target/phase7-release
target/phase7-release/run.sh
```

`<project>` 可以是带 `config.yaml` 的原生工程或 LetsGal（`project.json`）工程；
LetsGal 的配置在打包时物化为 `config.yaml`，`projects/test-project` 可直接打包。

5. `target/phase7-release` 包含引擎、启动脚本和 encrypted `game.hxz`；归档内含
   `.keine/compiled/program.bin`，config.yaml 为 `compiled_program: require`，且不包含
   `saves/`、`imported_assets/`、`*.meta` 或源项目 `.keine` 缓存。
6. 直接运行 `.hxz` 时脚本、图片、字体和多来源音频行为与目录项目一致；运行期间不得出现
   staging、ready marker 或明文资源目录。截断或破坏归档后应由 `hexz_k` 校验拒绝启动。
7. 执行 `cargo test --workspace --all-targets`，并确认运行时通过 `hexz_k::ResourceFile`
   直接 seek/read，不生成解包目录或完整文件副本。

### 桌面包与 CI

```bash
bash dev/scripts/bundle-macos.sh projects/test-project keine-demo
open target/bundle/macos/keine-demo.app
```

8. `.app` 可脱离工作目录启动内置项目。
9. 普通推送后 `ci.yml` 在 Linux/macOS/Windows 执行 fmt、Clippy、测试和 release build。
   配置仓库 Secret `HEXZ_PASSWORD` 后手动运行 `encrypted-release` workflow；三个平台
   artifact 均应只包含引擎、encrypted `game.hxz` 和启动脚本，日志不显示密钥。

### 外部媒体适配边界

10. 视频由项目扫描按需启用 `video-ffmpeg`；Live2D 已明确暂缓，Spine 与 Steam 仍是可选
    集成。发行前必须完成目标平台依赖打包与真机验收，不能用静态占位冒充外部媒体。
