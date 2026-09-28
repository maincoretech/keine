# Eiyashou Engine Action 与 Block 对应树（v1.1）

状态：**75 项逐项映射**。原有 [Eiyashou v1 语法](EIYASHOU_LANGUAGE_REFERENCE_v1.md) 保留，新增命令见 [v1.1 参考](EIYASHOU_LANGUAGE_REFERENCE_v1_1.md)。原生作者命令已接入 parser、typed lowering 和源码 Block 投影；冻结的兼容层、编译器内部 Action 与第三方 HostCommand 仍标明各自边界。代表性嵌套 Block 和 Inspector 源码回写已在运行中的 Editor 目测。

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

`hide(rin_*)`（一般形式 `hide(prefix*)`）匹配所有以 `rin_` 开头的 sprite ID，lower 为 `HideSprites { prefix: "rin_", ... }`；`hide(*)` 继续表示全部。`hide(rin_stage)` 仍精确隐藏一个 ID。其他点号命令按下文各自的 Action 映射。

Block View 保持 Scene section 和结构缩进。动作卡显示类型、主值与关键参数；花括号中的 Page、Frame、Case、Resource、Track、Key、Event 和 Layer 有独立源范围与缩进子卡，其中 Key 使用紧凑单行。Inspector 提供可编辑输入、常用值选项和可展开的未设置参数，仍直接修改同一份 `.shou` 源码。当前构建已目测代表性的 Camera、Focus rule、Stage Track/Key/Event 卡及 Key 时间预设的源码写回。

## 树中记号

- **现有**：v1 已可写，拼写和语义不变。
- **现有 v1.1**：新增命令已接入 parser、Block 和 typed lowering。
- **内部/兼容**：无直接原生命令；说明它由哪段原生结构表达，或为什么不该暴露 legacy 语义。它仍逐项列在树中，不冒充已覆盖。
- `target` 是运行时目标 ID，`asset` 是 `assets.yaml` 相应 namespace 中的资源 ID，`duration` 沿用 `ms`/`s`，`targets` 取 `scene`、`characters`、`all` 或 `none`。树中 `字段…` 指该 typed payload 的全部字段；不是 opaque JSON/RON 参数。
- 一条独立命令按源码顺序改变当时的运行状态。若 core 只有“显示时设置”字段而无运行中更新 Action，就必须补 typed Action 与 runtime 消费者；不能把命令回填到先前的 `sprite(...)` 或 `background(...)`。

## sprite 与 portrait 的关系

当前引擎只维护一批 stage sprites。`sprite(...)` 按 ID 显示/替换一个图像对象；相关 sprite Action 直接控制其位置、布局、层级、变换和滤镜。`ConfigurePortraits` **不会创建第二种图像或第二个渲染层**；它存一条规则，列出要受影响的角色 ID 和 speaking/others/narration 三套样式。`FocusPortrait` 根据 speaker ID 对已显示且命中的 sprites 批量应用样式：scale/alpha 写进 transform，blur/brightness/contrast/saturation 写进 filter。它因此会覆盖这些 sprite 的相同字段。

所以原生源码把聚焦写作 `sprite.focus.configure(...)` 与 `sprite.focus(...)`，Block 也归在 Sprite 的聚焦分支；不存在独立的 Portrait 资源或平行的 Portrait 画面视图。`sprite.transform`/`sprite.filter` 与批量聚焦共同作用于同一批 sprite，脚本顺序决定当前结果。`MiniAvatar` 是文本框旁的 UI 头像，不属于 stage sprite 的这套聚焦规则。

## 逐项映射树（75 个 Action）

每个 Action 节点下面依次列出源码写法和 Block 呈现。

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
│   │   │   ├── .shou：现有 v1.1 story.end()；与 scene 自然到末尾不同
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
│   │   │   ├── .shou：现有 background(asset, transition: fade(300ms), transform_alpha: 0.5)；运行中改变状态用 background.transform(字段…)
│   │   │   └── Block：Background 与 Transform 各一行；初始 transform 与 transition 在同一 Action 中生效
│   │   └── HideBg
│   │       ├── .shou：现有 background(none, transition: fade(300ms))
│   │       └── Block：Background · clear；transition
│   ├── 立绘对象
│   │   ├── ShowSprite
│   │   │   ├── .shou：现有 sprite(id, asset, position: right, transition: fade(300ms), z: 10, layout: scene, blend: add, transform_scale_x: 1.1)
│   │   │   └── Block：Figure；id/image/position/transition/z/layout/blend/初始 transform 在同一 Action 中生效
│   │   ├── HideSprite
│   │   │   ├── .shou：现有 hide(id, transition: fade(300ms))
│   │   │   └── Block：Hide；id、transition
│   │   ├── HideSprites
│   │   │   ├── .shou：现有 hide(*)、hide(prefix*, transition: fade(300ms))
│   │   │   └── Block：同一 Hide 短行；Inspector 可编辑目标和 transition；不用 hide_prefix 新命令
│   │   ├── MoveSprite
│   │   │   ├── .shou：现有 move(id, center, anchor_offset: 20, y: 0, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Move；id、position、anchor_offset、y、duration、easing、blocking；原写法仍可用
│   │   ├── SetTransform
│   │   │   ├── .shou：现有 v1.1 sprite.offset(id, x: 24, y: 0, duration: 300ms)、sprite.transform(id, alpha: 0.5, scale_x: 1.1, duration: 300ms)；background.transform(...) 用同一 typed patch
│   │   │   └── Block：Offset / Transform；id、TransformPatch 全九字段、duration、easing；未写字段保持现值
│   │   ├── Animate
│   │   │   ├── .shou：现有 v1.1 sprite.animate(target, shake, duration: 300ms)
│   │   │   └── Block：Animate；target、AnimationPreset（含 custom ID）、duration
│   │   ├── SetTransition
│   │   │   ├── .shou：现有 v1.1 sprite.transition(target, enter: ..., exit: ..., duration: 300ms)
│   │   │   └── Block：Transition rule；target、可选 enter/exit preset、duration；区别原 sprite(..., transition: ...) 的局部过渡
│   │   ├── SetFilter
│   │   │   ├── .shou：现有 v1.1 sprite.filter(target, blur: 0.2, brightness: 1.1, contrast: 1, saturation: 1)
│   │   │   └── Block：Filter；target、VisualFilter 四字段
│   │   ├── AnimateKeyframes
│   │   │   ├── .shou：现有 v1.1 sprite.keyframes(target, repeat: 0, blocking: true) { frame(duration: 300ms, easing: ease_out, 字段…) }
│   │   │   └── Block：Keyframes 主行 + 缩进 Frame；target、有序 TransformKeyframe、repeat、blocking
│   │   ├── UpdateSprite
│   │   │   ├── .shou：现有 v1.1 sprite.update(id, asset, position: center, layout: ..., scale: 1, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Update figure；只修改已在场目标，全部八字段；与 sprite(...) 的可入场替换不同
│   │   ├── SelectSpriteImage
│   │   │   ├── .shou：现有 v1.1 sprite.select(id, variable, default: asset) { case("value", asset) }
│   │   │   └── Block：Select image 主行 + 缩进 Case；id/variable/default_image/有序 variants
│   │   ├── SelectSpriteImageByCondition
│   │   │   ├── .shou：兼容层使用旧表达式求值，无原生写法；原生严格 bool 使用下方新 Action
│   │   │   └── Block：兼容项目只读
│   │   ├── EiyashouSelectSpriteImageByCondition
│   │   │   ├── .shou：现有 v1.1 sprite.select.when(id, default: asset) { case(mood == "happy", asset) }
│   │   │   └── Block：Select by condition 主行 + Case；有序 typed bool expression→image，严格类型检查
│   │   ├── ConfigureSpriteSequence
│   │   │   ├── .shou：现有 v1.1 sprite.sequence(id, fps: 12, loop: true) { frame(asset) }
│   │   │   └── Block：Sequence 主行 + Frame；id/有序 frames/fps/looped
│   │   └── ConfigureTimedSpriteSequence
│   │       ├── .shou：现有 v1.1 sprite.sequence.timed(id, loop: true) { frame(asset, duration: 120ms) }
│   │       └── Block：Timed sequence 主行 + Frame；id、frames 与 frame_durations 一一对应、looped
│   ├── 头像 UI
│   │   ├── MiniAvatar
│   │   │   ├── .shou：现有 v1.1 avatar.show(asset)
│   │   │   └── Block：Avatar；image
│   │   └── HideMiniAvatar
│   │       ├── .shou：现有 v1.1 avatar.hide()
│   │       └── Block：Hide avatar；无参数
│   └── 同一批立绘上的聚焦规则
│       ├── ConfigurePortraits
│       │   ├── .shou：现有 sprite.focus.configure(enabled: true, characters: [rin], speaking: style(scale: 1.1), others: style(brightness: 0.7), narration: style(), duration: 300ms, easing: ease_out)
│       │   └── Block：Focus rule 短行；Inspector 编辑 characters 与 speaking/others/narration 下各六个 style 字段，原源码仍为 style(...)；不创建新 sprite
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
│   │   │   ├── .shou：现有 se(asset, volume: ...) / se(none)；v1.1 se.loop(id, asset, volume: ...) 与 se.stop(id | *) 覆盖 id 语义
│   │   │   └── Block：SE / Loop SE / Stop SE；file/none、volume、id；无 ID Stop 是引擎原有的效果停止事件
│   │   └── Vocal
│   │       ├── .shou：现有 v1.1 vocal.play(asset, volume: ...) / vocal.stop()，仅指对白外独立语音；对白语音仍写在对白行尾
│   │       └── Block：Vocal / Stop vocal；file/none、volume；不改 v1 的行级 Voice 规则
│   └── 视频
│       ├── PlayVideo
│       │   ├── .shou：现有 video(asset, skippable: true) 固定 fullscreen、non-loop、blocking；v1.1 video.play(id, asset, loop: ..., muted: ..., alpha: ..., skippable: ..., wait: ..., mode: ...) 覆盖完整 VideoSpec
│       │   └── Block：Video / Video play；id/file/looped/muted/alpha/skippable/wait_for_finished/mode
│       └── StopVideo
│           ├── .shou：现有 v1.1 video.stop(id, fade: 300ms) 或 video.stop(*, fade: 300ms)
│           └── Block：Stop video；可选 id、fade_out；* 仅此命令的 all sentinel
├── 文字、交互与系统 UI
│   ├── 文本与呈现
│   │   ├── Intro
│   │   │   ├── .shou：现有 v1.1 text.intro(hold: true) { page("...") }
│   │   │   └── Block：Intro 主行 + Page；有序 pages、hold
│   │   ├── FilmMode
│   │   │   ├── .shou：现有 v1.1 screen.film(true) / screen.film(false)
│   │   │   └── Block：Film bars；enabled
│   │   ├── SetTextbox
│   │   │   ├── .shou：现有 v1.1 text.box(visible: true, auto: false)
│   │   │   └── Block：Text box；visible、auto
│   │   ├── Curtain
│   │   │   ├── .shou：现有 v1.1 screen.curtain.show(color: rgba(...), duration: 300ms) / screen.curtain.hide(color: rgba(...), duration: 300ms)
│   │   │   └── Block：Curtain；visible、RGBA、duration；hide 也保留 Action 中的 color 值
│   │   ├── FloatingText
│   │   │   ├── .shou：现有 v1.1 text.float("...", x: 960, y: 540, font_size: 48, color: rgba(...), fade_in: 300ms, hold: 1s, fade_out: 300ms, blocking: true)
│   │   │   └── Block：Floating text；text/position/font_size/color/fade_in/hold/fade_out/blocking
│   │   ├── HideFloatingText
│   │   │   ├── .shou：现有 v1.1 text.float.hide(id) / text.float.hide()
│   │   │   └── Block：Hide floating text；可选 id
│   │   ├── ConfigureFloatingText
│   │   │   ├── .shou：现有 v1.1 text.float.configure(id: ..., infinite: true)
│   │   │   └── Block：Floating text config；可选 id、infinite，作用于当前浮动文字
│   │   ├── SetDialogueStyle
│   │   │   ├── .shou：现有 v1.1 text.style(cinematic)
│   │   │   └── Block：Dialogue style；DialogueStyle 含 custom ID
│   │   ├── SetParagraphStyle
│   │   │   ├── .shou：现有 v1.1 text.paragraph.style(literary, typewriter_speed: ..., reveal_duration: ..., reveal_effect: ..., reveal_distance: ..., reveal_scale: ..., reveal_rotation: ..., reveal_blur: ...)
│   │   │   └── Block：Paragraph style；style、可选 typewriter_speed、完整 TextRevealConfig
│   │   ├── SelectTextPresentation
│   │   │   ├── .shou：现有 v1.1 text.presentation(paragraph) / text.presentation(dialogue)
│   │   │   └── Block：Text presentation；paragraph bool
│   │   ├── RetractDialogue
│   │   │   ├── .shou：现有 v1.1 text.retract(source: "完整文本", keep: "保留前缀")
│   │   │   └── Block：Retract text；source、keep；随后一次明确推进
│   │   └── WaitForAdvance
│   │       ├── .shou：现有 v1.1 wait.advance()
│   │       └── Block：Wait advance；无参数
│   └── 交互与界面
│       ├── UserInput
│       │   ├── .shou：现有 v1.1 input.simple(variable, title: "...", button: "...")
│       │   └── Block：Input；variable/title/button
│       ├── RequestInput
│       │   ├── .shou：现有 v1.1 input.request(variable, type: string, title: "...", 字段…)
│       │   └── Block：Input request；UserInputSpec 的全部字段及类型专属校验
│       ├── Unlock
│       │   ├── .shou：现有 v1.1 gallery.unlock(cg, asset, name: "...") 或 gallery.unlock(bgm, asset, name: "...")
│       │   └── Block：Unlock；kind/file/name
│       ├── SetAutoplay
│       │   ├── .shou：现有 v1.1 playback.auto(true) / playback.auto(false)
│       │   └── Block：Autoplay；enabled
│       ├── SetSystemUi
│       │   ├── .shou：现有 v1.1 ui.show(save) / ui.hide(save)
│       │   └── Block：System UI；SystemUiSlot 七类、visible
│       └── SystemMessage
│           ├── .shou：现有 v1.1 ui.message(alert, title: "...", message: "...", confirm_text: "...", cancel_text: "...", result: variable)
│           └── Block：System message；mode/title/message/confirm_text/cancel_text/result_variable
├── 镜头、舞台、粒子与资源
│   ├── 粒子
│   │   ├── ShowParticles
│   │   │   ├── .shou：现有 v1.1 particle.show(id, preset, texture: asset, count: 100, wind: 0, gravity: 0, fade_in: 300ms)
│   │   │   └── Block：Particles；id 与 ParticleEffect 的 texture/preset/count/wind/gravity/fade_in
│   │   ├── HideParticles
│   │   │   ├── .shou：现有 v1.1 particle.hide(id, duration: 300ms) / particle.hide(*, duration: 300ms)
│   │   │   └── Block：Hide particles；可选 id、duration
│   │   └── HideParticleLayers
│   │       ├── .shou：现有 v1.1 particle.layers.clear()
│   │       └── Block：Clear particle layers；无参数；区别于停止 emitter
│   ├── 镜头
│   │   ├── SetPostProcess
│   │   │   ├── .shou：现有 v1.1 camera.effect(targets, bloom_intensity: 0.6, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Camera effect；targets、PostProcessPatch 的每个稀疏字段、duration/easing/blocking；可空字段区分未设置/清除/设值
│   │   ├── SetPostProcessV2
│   │   │   ├── .shou：现有 v1.1 camera.effect.v2(targets, 全部 18 个 V2 字段, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Camera effect V2；全部 18 个字段强制显式填写，避免缺省项清零旧效果
│   │   ├── SetCameraBinding
│   │   │   ├── .shou：现有 v1.1 camera.bind(target, distance: 1.5) / camera.unbind(target, distance: 1.5)
│   │   │   └── Block：Camera binding；target/bound/distance；解绑时仍保留 distance
│   │   ├── SetCameraTransform
│   │   │   ├── .shou：现有 camera.move(targets, x: 20, y: 0, scale_x: 1.1, scale_y: 1.1, duration: 300ms, easing: ease_out, blocking: true)
│   │   │   └── Block：Camera move；targets、TransformPatch 九字段、duration/easing/blocking；示例 x/y 精确映射 offset_x/offset_y
│   │   └── ShakeCamera
│   │       ├── .shou：现有 camera.shake(targets, amplitude: 8, frequency: 12, duration: 300ms, axis: both, falloff: linear, blocking: true)
│   │       └── Block：Camera shake；targets、CameraShakeSpec 五字段、blocking
│   └── 舞台和资源
│       ├── StageAnimation
│       │   ├── .shou：现有 v1.1 stage.animate(id, duration: 2s, repeat: 0, infinite: false, playback_rate: 1, blocking: true) { track(...) { key(...) }, event.camera.shake(...), event.camera.patch(...), event.particle(...), event.scene(...) { layer(...) }, event.audio(...) }
│       │   └── Block：Stage 主行 + Track/Key/Event 缩进行；StageAnimation 的 tracks/events 全部有源范围和字段
│       ├── StageMask
│       │   ├── .shou：现有 v1.1 stage.mask.show(id, 字段…, duration: 300ms, blocking: true) / stage.mask.hide(id, duration: 300ms, blocking: true)
│       │   └── Block：Mask；id、完整 StageMask 或 none、duration/blocking；显示/删除独立行
│       ├── ConfigureLoading
│       │   ├── .shou：现有 v1.1 assets.loading(mode: manual, lookahead: 20, blocking: false) { resource(asset, kind: figure) }
│       │   └── Block：Loading 主行 + Resource；strategy mode/lookahead/blocking/有序 AssetHint
│       └── ConfigureSceneMouseParallax
│           ├── .shou：现有 v1.1 scene.parallax(amplitude_percent: 4, edge_ease_percent: 0, return_to_center_on_leave: true, scale: 1.08) / scene.parallax.stop()
│           └── Block：Parallax；可选 SceneMouseParallax 四字段
└── 第三方兼容 IR
    └── 仅记录已有引擎事实
        └── HostCommand
            ├── .shou：现有兼容/宿主 IR，无 .shou 写法提议。LetsGal adapter 保留真正第三方 extension 的 namespace/command/payload 事件；内建能力使用 typed Action
            └── Block：原生 Block 无此项；兼容项目只读；不新增插件功能
```

## 完整字段与验收边界

1. `ShowBg`/`ShowSprite` 的初始 transform、layout、blend 与 transition 在同一 Action lowering，避免阻塞过渡后的独立命令改变第一帧。后续 `sprite.transform`/`background.transform` 使用九字段稀疏 patch；未写字段保持当前值。`Position` 支持 left/center/right、anchor offset 与 y。布局支持 natural、viewport_height、scene 和 composite 的全部 core 字段。
2. `PostProcessPatch` 的 73 字段可在 `camera.effect` 与 Stage 的 `event.camera.patch` 中按需填写；`focal_distance` 和 `lut_preset` 区分不写、`none` 与值。`PostProcessV2` 的 18 字段必须在 `camera.effect.v2` 一次填全。`StageMask`、`UserInputSpec`、`PortraitStyle` 和 `TextRevealConfig` 也有逐字段 typed parser，没有 JSON/RON 逃生口。
3. `StageAnimation` 使用共享时钟，Track、Key、CameraShake、CameraPatch、Particle、Scene/Layer 和 Audio 事件均有源范围、Block 缩进行与 Inspector 字段。`StageProperty` 的 79 个枚举值均有可写名称；列表顺序与时间不被 Block 投影重排。
4. `sprite.select.when` lower 为尾部新增的严格 Eiyashou Action，并使用严格 bool 求值；旧兼容 `SelectSpriteImageByCondition` 的 truthiness 不进入原生语法。`HostCommand` 仍只服务真正第三方扩展。
5. 原 v1 拼写、未触碰文件的字节、注释和未知节点保持原样。`cargo migrate` 对不支持的兼容语义继续 fail closed；此树不意味着 WebGAL 兼容范围扩大。
6. 当前实现仍需完整工作区门禁、原生工程 validate、Editor 实际视觉与交互验收。编译和单元测试通过不能替代运行态目测。

## Approved camera additions (2026-09-28)

```text
Camera
├── SetCameraTween
│   ├── .shou：camera.move / camera.effect / camera.effect.v2(..., tween: [numeric_field, ...])
│   ├── 单条命令同时应用即时字段与补间起点；未选字段立即生效
│   ├── Block：保持原命令的紧凑行；Inspector：◆ / ◇ 直接写回列表
│   └── 省略列表仍使用原有 SetCameraTransform / SetPostProcess / SetPostProcessV2
├── ShakeCameraRandomized
│   ├── .shou：camera.shake(..., amplitude_randomness: 0.3, frequency_randomness: 0.2)
│   ├── Block：Camera shake；Inspector：两个 0–100% 滑块
│   └── 两项为零仍使用 ShakeCamera，保持原有采样
└── StageEventKind::CameraShakeRandomized
    ├── .shou：event.camera.shake(..., amplitude_randomness: ..., frequency_randomness: ...)
    └── Stage 原有时间、缩进与执行位置对应关系不变
Excluded
└── blip：用户明确不添加
```
