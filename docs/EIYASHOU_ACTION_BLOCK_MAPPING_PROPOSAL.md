# Eiyashou 全部 Engine Action 的源码与 Block 对应草案

状态：**74 项逐项映射草案，部分已实现**。基线为当前 `main` 的 74 个 `Action` variant。原有 [Eiyashou v1 语法](EIYASHOU_LANGUAGE_REFERENCE_v1.md) 保留；新操作使用独立命令和独立 Block 短行。标为“现有”的条目已接入源码 parser 和 Block 投影；标为“提议”的条目仍只是候选，不算引擎功能覆盖。本树不把兼容层/编译器内部 Action 强行变成作者语法。

## 已实现的调用形式

```eiyashou
scene example {
  sprite(rin_stage, rin_smile, position: right),
  camera.move(scene, x: 20, duration: 300ms),
  camera.shake(scene, amplitude: 8, frequency: 12, duration: 300ms),
  hide(rin_*),
  hide(*)
}
```

`hide(rin_*)`（一般形式 `hide(prefix*)`）匹配所有以 `rin_` 开头的 sprite ID，lower 为 `HideSprites { prefix: "rin_", ... }`；`hide(*)` 继续表示全部。`*` 只在 `hide` 的目标末尾有通配意义。`hide(rin_stage)` 仍精确隐藏一个 ID。已实现的点号命令是 `camera.move()`、`camera.shake()`、`sprite.focus.configure()`、`sprite.focus()`；其他点号写法仍待实现。

Block View 保持现有的 Scene section、轻量短行、结构缩进。上例中的 Figure、Camera move、Camera shake 和两条 Hide 均显示为可编辑短行，Inspector 修改对应源码范围。下文其他 Block 名称仍是提议，尤其时间轴的缩进子行尚未实现。

## 树中记号

- **现有**：当前 v1 已可写，拼写和语义不变；若只覆盖 Action 的一个子集，明确写出缺口。
- **提议**：未来 `.shou` 命令，全部需要 parser、Block、Inspector、lowering、迁移无损验证。
- **内部/兼容**：无直接原生命令；说明它由哪段原生结构表达，或为什么不该暴露 legacy 语义。它仍逐项列在树中，不冒充已覆盖。
- `target` 是运行时目标 ID，`asset` 是 `assets.yaml` 相应 namespace 中的资源 ID，`duration` 沿用 `ms`/`s`，`targets` 取 `scene`、`characters`、`all` 或 `none`，所有候选枚举拼写仍待逐一校定。树中 `字段…` 表示该具名 payload 的**全部**字段，具体无损要求见末节；不是 opaque JSON/RON 参数。
- 一条独立命令按源码顺序改变当时的运行状态。若 core 只有“显示时设置”字段而无运行中更新 Action，就必须补 typed Action 与 runtime 消费者；不能把命令回填到先前的 `sprite(...)` 或 `background(...)`。

## sprite 与 portrait 的关系

当前引擎只维护一批 stage sprites。`sprite(...)` 按 ID 显示/替换一个图像对象；相关 sprite Action 直接控制其位置、布局、层级、变换和滤镜。`ConfigurePortraits` **不会创建第二种图像或第二个渲染层**；它存一条规则，列出要受影响的角色 ID 和 speaking/others/narration 三套样式。`FocusPortrait` 根据 speaker ID 对已显示且命中的 sprites 批量应用样式：scale/alpha 写进 transform，blur/brightness/contrast/saturation 写进 filter。它因此会覆盖这些 sprite 的相同字段。

所以原生源码把聚焦写作 `sprite.focus.configure(...)` 与 `sprite.focus(...)`，Block 也归在 Sprite 的聚焦分支；不存在独立的 Portrait 资源或平行的 Portrait 画面视图。普通 `sprite.transform`/`sprite.filter` 仍是未实现的直接编辑候选，`sprite.focus(speaker)` 是已实现的批量聚焦。若未来两者混用，脚本顺序决定当前结果。`MiniAvatar` 是文本框旁的 UI 头像，不属于 stage sprite 的这套聚焦规则。

## 逐项映射树（74 个 Action）

每个 Action 节点下面依次列出源码写法和 Block 呈现；“现有”可运行，“提议”尚未实现。

```text
Action
├── 剧本
│   ├── 对白和选择
│   │   ├── Say
│   │   │   ├── .shou：兼容：普通纯文本由 EiyashouSay 承担；rich-text/concat/auto-advance 等 SayOptions 需未来明确的 text.rich 能力，不能隐式塞入 角色: "文本"
│   │   │   └── Block：兼容输入只读；若开放 Rich text 行，需 speaker/text/vocal/volume/concat/auto_advance/inherit_speaker 全字段与新的语言版本
│   │   ├── EiyashouSay
│   │   │   ├── .shou：现有 rin: "早上好。", ch01_001、"旁白"、前置 @id
│   │   │   └── Block：Text；speaker、纯文本、行尾 voice、稳定 ID；角色名/颜色由 characters.yaml 给出
│   │   ├── Menu
│   │   │   ├── .shou：兼容：选择功能由 EiyashouMenu 的 choice 负责；enable_when（显示但禁用）需未来扩展 choice option 及原生 typed/runtime 表示
│   │   │   └── Block：Choice/Option；legacy prompt/choices/show_when/enable_when/target 不可因转换成现有 choice 而丢失
│   │   └── EiyashouMenu
│   │       ├── .shou：现有 choice("问题") { "选项" when (条件): goto(scene) }
│   │       └── Block：Choice/Option 与缩进分支；typed prompt/text/visibility、target、稳定 ID
│   ├── 场景和流程
│   │   ├── Jump
│   │   │   ├── .shou：内部：choice、if、loop 的编译器跳转
│   │   │   └── Block：展示原 Choice/If/Loop 行；不公开 compiler-private label
│   │   ├── Label
│   │   │   ├── .shou：内部：choice、if、loop 的编译器标签
│   │   │   └── Block：同上；不公开 label(...)
│   │   ├── EiyashouJumpIf
│   │   │   ├── .shou：内部：严格 bool 的 if、loop、choice when lowering
│   │   │   └── Block：If/Loop/Option；condition、jump_when 由源码结构决定，私有 label 不出现
│   │   ├── ChangeScene
│   │   │   ├── .shou：现有 goto(scene)
│   │   │   └── Block：Goto；scene
│   │   ├── CallScene
│   │   │   ├── .shou：现有 call(scene)
│   │   │   └── Block：Call；scene
│   │   ├── ReturnScene
│   │   │   ├── .shou：现有 return
│   │   │   └── Block：Return；无参数，空调用栈报错
│   │   ├── End
│   │   │   ├── .shou：提议 story.end()；与 scene 自然到末尾不同
│   │   │   └── Block：End story；无参数；必须调用栈语义一致
│   │   ├── Flow
│   │   │   ├── .shou：兼容：when 用原生 if/choice when；next 的非阻塞行为需在具体命令单独表达，不作为通用 legacy wrapper
│   │   │   └── Block：原结构行；不提供 flow(...) { Action } 或任意 Action 包装
│   │   ├── Wait
│   │   │   ├── .shou：现有 wait(300ms)
│   │   │   └── Block：Wait；seconds
│   │   └── Comment
│   │       ├── .shou：内部/无操作：作者写 // ... 或 /* ... */，保留源码，不生成运行时 Comment
│   │       └── Block：Comment/Source trivia；不提供 comment() 命令
│   └── 变量与列表
│       ├── Set
│       │   ├── .shou：兼容：原生使用严格类型的 let/赋值，lowering 为 EiyashouSet；不暴露 legacy truthiness 的 set
│       │   └── Block：Let/Set；legacy name/expression/global 的迁移须逐项证明等价，否则拒绝
│       ├── EiyashouSet
│       │   ├── .shou：现有 let x = ...、x = ...、x += ...、x[i] = ...
│       │   └── Block：Let/Set；target、typed expression、operation、initialize_once 均由原语法决定
│       └── EiyashouList
│           ├── .shou：现有 items.append(x)、.remove、.clear、.insert、pop(items, into: x)
│           └── Block：List operation；variable、operation、index/value/result target
├── 场景图像与角色
│   ├── 背景
│   │   ├── ShowBg
│   │   │   ├── .shou：现有 background(asset, transition: fade(300ms))；运行中改变 transform 用提议 background.transform(字段…)
│   │   │   └── Block：Background 与 Transform 各一行；image、transition、完整 transform。非默认初始 transform 与阻塞 transition 同时生效尚无无损独立命令表达，见末节缺口
│   │   └── HideBg
│   │       ├── .shou：现有 background(none, transition: fade(300ms))
│   │       └── Block：Background · clear；transition
│   ├── 立绘对象
│   │   ├── ShowSprite
│   │   │   ├── .shou：现有 sprite(id, asset, position: right, transition: fade(300ms), z: 10)；运行中改变状态另用 sprite.layout、sprite.blend、sprite.transform
│   │   │   └── Block：Figure 加各操作短行；id/image/position/transition/z；非默认初始 layout/blend/transform 与阻塞 transition 同时生效尚有无损缺口
│   │   ├── HideSprite
│   │   │   ├── .shou：现有 hide(id, transition: fade(300ms))
│   │   │   └── Block：Hide；id、transition
│   │   ├── HideSprites
│   │   │   ├── .shou：现有 hide(*)、hide(prefix*, transition: fade(300ms))
│   │   │   └── Block：同一 Hide 短行；Inspector 可编辑目标和 transition；不用 hide_prefix 新命令
│   │   ├── MoveSprite
│   │   │   ├── .shou：现有 move(id, center, duration: 300ms, easing: ease_out)；提议同一 Move 操作增加 anchor_offset、y、blocking 命名参数以覆盖完整 Position
│   │   │   └── Block：Move；id、position、duration、easing、blocking；新参数不改变原写法
│   │   ├── SetTransform
│   │   │   ├── .shou：提议 sprite.offset(id, x: 24, y: 0, duration: 300ms)、sprite.transform(id, alpha: 0.5, scale_x: 1.1, duration: 300ms)；background.transform(...) 用同一 typed patch
│   │   │   └── Block：Offset / Transform；id、TransformPatch 全九字段、duration、easing；未写字段保持现值
│   │   ├── Animate
│   │   │   ├── .shou：提议 sprite.animate(target, shake, duration: 300ms)
│   │   │   └── Block：Animate；target、AnimationPreset（含 custom ID）、duration
│   │   ├── SetTransition
│   │   │   ├── .shou：提议 sprite.transition(target, enter: ..., exit: ..., duration: 300ms)
│   │   │   └── Block：Transition rule；target、可选 enter/exit preset、duration；区别原 sprite(..., transition: ...) 的局部过渡
│   │   ├── SetFilter
│   │   │   ├── .shou：提议 sprite.filter(target, blur: 0.2, brightness: 1.1, contrast: 1, saturation: 1)
│   │   │   └── Block：Filter；target、VisualFilter 四字段
│   │   ├── AnimateKeyframes
│   │   │   ├── .shou：提议 sprite.keyframes(target, repeat: 0, blocking: true) { frame(duration: 300ms, easing: ease_out, 字段…) }
│   │   │   └── Block：Keyframes 主行 + 缩进 Frame；target、有序 TransformKeyframe、repeat、blocking
│   │   ├── UpdateSprite
│   │   │   ├── .shou：提议 sprite.update(id, asset, position: center, layout: ..., scale: 1, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Update figure；只修改已在场目标，全部八字段；与 sprite(...) 的可入场替换不同
│   │   ├── SelectSpriteImage
│   │   │   ├── .shou：提议 sprite.select(id, variable, default: asset) { case("value", asset) }
│   │   │   └── Block：Select image 主行 + 缩进 Case；id/variable/default_image/有序 variants
│   │   ├── SelectSpriteImageByCondition
│   │   │   ├── .shou：提议 sprite.select.when(id, default: asset) { case("typed condition", asset) }
│   │   │   └── Block：Select by condition 主行 + Case；id/default_image/有序 expression→image，严格表达式检查
│   │   ├── ConfigureSpriteSequence
│   │   │   ├── .shou：提议 sprite.sequence(id, fps: 12, loop: true) { frame(asset) }
│   │   │   └── Block：Sequence 主行 + Frame；id/有序 frames/fps/looped
│   │   └── ConfigureTimedSpriteSequence
│   │       ├── .shou：提议 sprite.sequence.timed(id, loop: true) { frame(asset, duration: 120ms) }
│   │       └── Block：Timed sequence 主行 + Frame；id、frames 与 frame_durations 一一对应、looped
│   ├── 头像 UI
│   │   ├── MiniAvatar
│   │   │   ├── .shou：提议 avatar.show(asset)
│   │   │   └── Block：Avatar；image
│   │   └── HideMiniAvatar
│   │       ├── .shou：提议 avatar.hide()
│   │       └── Block：Hide avatar；无参数
│   └── 同一批立绘上的聚焦规则
│       ├── ConfigurePortraits
│       │   ├── .shou：现有 sprite.focus.configure(enabled: true, characters: [rin], speaking: style(scale: 1.1), others: style(brightness: 0.7), narration: style(), duration: 300ms, easing: ease_out)
│       │   └── Block：Focus rule 短行已实现；Inspector 当前编辑 characters 列表与 style(...) 源码片段，逐字段控件仍待实现；不创建新 sprite
│       └── FocusPortrait
│           ├── .shou：现有 sprite.focus(speaker_id) / sprite.focus(none)
│           └── Block：Focus speaker；对已显示、被规则选中的 sprite 应用 scale/alpha/filter；可选 speaker_id
├── 音频与视频
│   ├── 背景音乐与音效
│   │   ├── Bgm
│   │   │   ├── .shou：兼容：原生仍写 bgm(asset, volume: 0.8, fade: 1s) 并 lower 为 EiyashouBgm；不新增 bgm_legacy
│   │   │   └── Block：BGM；legacy file/volume/fade 可迁入原命令，loop 默认为 true
│   │   ├── EiyashouBgm
│   │   │   ├── .shou：现有 bgm(asset, volume: ..., fade: ..., loop: ...) / bgm(none, fade: ...)
│   │   │   └── Block：BGM；file/none、volume、fade_seconds、looped
│   │   ├── Effect
│   │   │   ├── .shou：现有 se(asset, volume: ...) / se(none)；提议 se.loop(id, asset, volume: ...) 与 se.stop(id) 覆盖 id 语义
│   │   │   └── Block：SE / Loop SE / Stop SE；file/none、volume、id；stop-all 与 stop-one 分开显示
│   │   └── Vocal
│   │       ├── .shou：提议 vocal.play(asset, volume: ...) / vocal.stop()，仅指对白外独立语音；对白语音仍写在对白行尾
│   │       └── Block：Vocal / Stop vocal；file/none、volume；不改 v1 的行级 Voice 规则
│   └── 视频
│       ├── PlayVideo
│       │   ├── .shou：现有 video(asset, skippable: true) 固定 fullscreen、non-loop、blocking；提议 video.play(id, asset, loop: ..., muted: ..., alpha: ..., skippable: ..., wait: ..., mode: ...) 覆盖完整 VideoSpec
│       │   └── Block：Video / Video play；id/file/looped/muted/alpha/skippable/wait_for_finished/mode；新命令是后续语言扩展
│       └── StopVideo
│           ├── .shou：提议 video.stop(id, fade: 300ms) 或 video.stop(*, fade: 300ms)
│           └── Block：Stop video；可选 id、fade_out；* 仅此命令的 all sentinel
├── 文字、交互与系统 UI
│   ├── 文本与呈现
│   │   ├── Intro
│   │   │   ├── .shou：提议 text.intro(hold: true) { page("...") }
│   │   │   └── Block：Intro 主行 + Page；有序 pages、hold
│   │   ├── FilmMode
│   │   │   ├── .shou：提议 screen.film(true) / screen.film(false)
│   │   │   └── Block：Film bars；enabled
│   │   ├── SetTextbox
│   │   │   ├── .shou：提议 text.box(visible: true, auto: false)
│   │   │   └── Block：Text box；visible、auto
│   │   ├── Curtain
│   │   │   ├── .shou：提议 screen.curtain.show(color: rgba(...), duration: 300ms) / screen.curtain.hide(color: rgba(...), duration: 300ms)
│   │   │   └── Block：Curtain；visible、RGBA、duration；hide 也保留 Action 中的 color 值
│   │   ├── FloatingText
│   │   │   ├── .shou：提议 text.float("...", x: 960, y: 540, font_size: 48, color: rgba(...), fade_in: 300ms, hold: 1s, fade_out: 300ms, blocking: true)
│   │   │   └── Block：Floating text；text/position/font_size/color/fade_in/hold/fade_out/blocking
│   │   ├── HideFloatingText
│   │   │   ├── .shou：提议 text.float.hide(id) / text.float.hide()
│   │   │   └── Block：Hide floating text；可选 id
│   │   ├── ConfigureFloatingText
│   │   │   ├── .shou：提议 text.float.configure(id: ..., infinite: true)
│   │   │   └── Block：Floating text config；可选 id、infinite，作用于当前浮动文字
│   │   ├── SetDialogueStyle
│   │   │   ├── .shou：提议 text.style(cinematic)
│   │   │   └── Block：Dialogue style；DialogueStyle 含 custom ID
│   │   ├── SetParagraphStyle
│   │   │   ├── .shou：提议 text.paragraph.style(literary, typewriter_speed: ..., reveal: ...)
│   │   │   └── Block：Paragraph style；style、可选 typewriter_speed、完整 TextRevealConfig
│   │   ├── SelectTextPresentation
│   │   │   ├── .shou：提议 text.presentation(paragraph) / text.presentation(dialogue)
│   │   │   └── Block：Text presentation；paragraph bool
│   │   ├── RetractDialogue
│   │   │   ├── .shou：提议 text.retract(source: "完整文本", keep: "保留前缀")
│   │   │   └── Block：Retract text；source、keep；随后一次明确推进
│   │   └── WaitForAdvance
│   │       ├── .shou：提议 wait.advance()
│   │       └── Block：Wait advance；无参数
│   └── 交互与界面
│       ├── UserInput
│       │   ├── .shou：提议 input.simple(variable, title: "...", button: "...")
│       │   └── Block：Input；variable/title/button
│       ├── RequestInput
│       │   ├── .shou：提议 input.request(variable, type: string, title: "...", 字段…)
│       │   └── Block：Input request；UserInputSpec 的全部字段及类型专属校验
│       ├── Unlock
│       │   ├── .shou：提议 gallery.unlock(cg, asset, name: "...") 或 gallery.unlock(bgm, asset, name: "...")
│       │   └── Block：Unlock；kind/file/name
│       ├── SetAutoplay
│       │   ├── .shou：提议 playback.auto(true) / playback.auto(false)
│       │   └── Block：Autoplay；enabled
│       ├── SetSystemUi
│       │   ├── .shou：提议 ui.show(save) / ui.hide(save)
│       │   └── Block：System UI；SystemUiSlot 七类、visible
│       └── SystemMessage
│           ├── .shou：提议 ui.message(alert, title: "...", message: "...", confirm_text: "...", cancel_text: "...", result: variable)
│           └── Block：System message；mode/title/message/confirm_text/cancel_text/result_variable
├── 镜头、舞台、粒子与资源
│   ├── 粒子
│   │   ├── ShowParticles
│   │   │   ├── .shou：提议 particle.show(id, preset, texture: asset, count: 100, wind: 0, gravity: 0, fade_in: 300ms)
│   │   │   └── Block：Particles；id 与 ParticleEffect 的 texture/preset/count/wind/gravity/fade_in
│   │   ├── HideParticles
│   │   │   ├── .shou：提议 particle.hide(id, duration: 300ms) / particle.hide(*, duration: 300ms)
│   │   │   └── Block：Hide particles；可选 id、duration
│   │   └── HideParticleLayers
│   │       ├── .shou：提议 particle.layers.clear()
│   │       └── Block：Clear particle layers；无参数；区别于停止 emitter
│   ├── 镜头
│   │   ├── SetPostProcess
│   │   │   ├── .shou：提议 camera.effect(targets, bloom_intensity: 0.6, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Camera effect；targets、PostProcessPatch 的每个稀疏字段、duration/easing/blocking；可空字段区分未设置/清除/设值
│   │   ├── SetPostProcessV2
│   │   │   ├── .shou：提议 camera.effect(targets, mirror_shatter_intensity: 0.6, speed_lines_intensity: 0.3, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：同一 Camera effect 行；当前 PostProcessV2 是完整 18 字段状态，示例只写两项并不能无损表达其余 16 项；需全部显式写出或先补 typed patch Action
│   │   ├── SetCameraBinding
│   │   │   ├── .shou：提议 camera.bind(target, distance: 1.5) / camera.unbind(target, distance: 1.5)
│   │   │   └── Block：Camera binding；target/bound/distance；解绑时仍保留 distance
│   │   ├── SetCameraTransform
│   │   │   ├── .shou：现有 camera.move(targets, x: 20, y: 0, scale_x: 1.1, scale_y: 1.1, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Camera move；targets、TransformPatch 九字段、duration/easing/blocking；示例 x/y 精确映射 offset_x/offset_y
│   │   └── ShakeCamera
│   │       ├── .shou：现有 camera.shake(targets, amplitude: 8, frequency: 12, duration: 300ms, axis: both, falloff: linear, blocking: true)
│   │       └── Block：Camera shake；targets、CameraShakeSpec 五字段、blocking
│   └── 舞台和资源
│       ├── StageAnimation
│       │   ├── .shou：提议 stage.animate(id, duration: 2s, repeat: 0, infinite: false, playback_rate: 1, blocking: true) { track(...) { key(...) }, event(...) }
│       │   └── Block：Stage 主行 + Track/Key/Event 缩进行；StageAnimation 的 tracks/events 全部有源范围和字段
│       ├── StageMask
│       │   ├── .shou：提议 stage.mask.show(id, 字段…, duration: 300ms, blocking: true) / stage.mask.hide(id, duration: 300ms, blocking: true)
│       │   └── Block：Mask；id、完整 StageMask 或 none、duration/blocking；显示/删除独立行
│       ├── ConfigureLoading
│       │   ├── .shou：提议 assets.loading(mode: manual, lookahead: 20, blocking: false) { resource(asset, kind: figure) }
│       │   └── Block：Loading 主行 + Resource；strategy mode/lookahead/blocking/有序 AssetHint
│       └── ConfigureSceneMouseParallax
│           ├── .shou：提议 scene.parallax(amplitude_percent: 4, edge_ease_percent: 0, return_to_center_on_leave: true, scale: 1.08) / scene.parallax.stop()
│           └── Block：Parallax；可选 SceneMouseParallax 四字段
└── 第三方兼容 IR
    └── 仅记录已有引擎事实
        └── HostCommand
            ├── .shou：现有兼容/宿主 IR，无 .shou 写法提议。LetsGal adapter 保留真正第三方 extension 的 namespace/command/payload 事件；内建能力使用 typed Action
            └── Block：原生 Block 无此项；兼容项目只读；不新增插件功能
```

## 复杂字段无损规则与实现门槛

1. `SpriteTransform`/`TransformPatch`：`offset_x/offset_y/alpha/scale_x/scale_y/rotation/blur/width/height`；稀疏 patch 每项区分缺席和显式 0。`Position` 的 left/center/right anchor offset 与 y、`SpriteLayout` 的 natural/viewport-height/scene/composite 各自完整字段必须有受限 Inspector 控件。
2. `PostProcessPatch`、`PostProcessV2`、`StageMask`、`UserInputSpec`、`PortraitStyle`、`TextRevealConfig`、`StageAnimation` 的字段全集以当前 core 结构为准。实现时必须有明确的字段清单与覆盖检查；不能用通用 JSON/RON 文本框代替。`PostProcessPatch.focal_distance` 和 `lut_preset` 是“未设置/清除/设值”三态；`PostProcessV2` 是完整状态，单独命令不可暗中把另一项特效清零。同一个 `camera.effect` 同时写普通 patch 与 V2 字段时，在有确定的原子 lowering 前应报错。
3. `StageAnimation` 的 StageTarget/StageProperty、每个 keyframe、CameraShake/CameraPatch/Particle/Scene/Audio 五种 event payload、Scene layers 及 Audio cue 都有独立可编辑子行和稳定 source range。列表顺序、时间、repeat/infinite/playback_rate/blocking 不得被省略或重排。
4. 每个树中“提议”命令先实现 strict parser/validation、source-preserving Block projection/Inspector、typed lowering、runtime 运行态验证，最后才开放 `cargo migrate` 的对应规则。原始 v1 语法、未触碰文件的字节、注释和未知节点保持原样。当前迁移对不支持的能力继续 fail closed。
5. 逐项审核时仍需确认命令名、资源 namespace、参数类型/默认值及是否进入基础 v1 或后续版本；本文的候选拼写不是已批准语言合同。
6. **尚待解决的无损缺口**：`ShowBg`/`ShowSprite` 的非默认初始 transform/layout/blend 若伴随阻塞过渡，过渡之后才执行的独立命令不能复现初始帧；`SetPostProcessV2` 用完整状态而非稀疏 patch；`Curtain`/`SetCameraBinding` 关闭时仍携带颜色/距离。实现必须保持这些字段和时序，必要时增加明确的 core Action 或一次性显示参数；不能仅为了表面一命令一行而宣称已等价。
