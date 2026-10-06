# Eiyashou v2.0

当前原生脚本为 UTF-8 `.shou`，命令按对象职责分组。
sprite.update、transform 与 camera 稀疏修改中省略的字段保留当前值；创建使用默认值。
每条作者命令 lower 为 typed Engine Action，Editor 只编辑同一份源码。
EYS v2.0 对应 `script.version: 2`；省略时默认 2，显式旧版本会被拒绝。已合并的旧命令不提供别名。

## 工程

```text
project/
├── config.yaml        project.id、adapter.script: keine、script.entry
├── assets.yaml        backgrounds / figures / voices / bgm / se / videos / particles / luts
├── characters.yaml    characters: { rin: { name: 凛 } }
├── objects.yaml       可选迁移对象/前缀映射
├── scripts/*.shou
└── assets/            清单里的工程相对路径
```

```yaml
title: Example
project:
  id: my-game
adapter:
  script: keine
script:
  version: 2
  entry: start
  assets: assets.yaml
  characters: characters.yaml
```

清单条目可写 `room: assets/room.webp`，或 `{ path: assets/room.webp, tags: [interior] }`。
资源/角色/对象使用裸 ID；普通工程不需要对象映射。保留 `none`、`*` 等命令上下文哨兵。
脚本 ID 中的下划线合法；仓库文件布局规则不限制作者 ID。

### 多文件与章节连接

Loader 递归读取 `scripts/` 下所有 `.shou`；scene ID 全项目唯一，不需要 import，
不按文件名/目录顺序执行。`script.entry` 指定入口 scene，文件不必叫 `main.shou`。

```shou
// scripts/开场.shou；config.yaml 中 script.entry: opening
scene opening {
  "故事开始了。",
  goto(chapter1)
}
```

```shou
// scripts/第一章.shou
scene chapter1 {
  call(aside),
  "支线结束，继续第一章。",
  goto(chapter2)
}
scene aside {
  "一段可返回的支线。",
  return
}
```

```shou
// scripts/第二章.shou
scene chapter2 {
  "故事结束了。",
  story.end()
}
```

- 顺序章节在尾部写 `goto(下一章场景)`；它替换当前场景，不压入返回栈。
- 支线/公共片段用 `call(场景)`；`return` 或被调用场景到末尾会返回调用后的语句。
- 普通入口场景到末尾不会自动进入其他文件；结局显式写 `story.end()`。
- `migrate` 按实际源文件分组，保留原剧情的跳转和调用；拆文件本身不增加连接语句。

## 基础语法

```shou
scene start {
  let courage = 0,
  background(room),
  sprite(rin_stage, rin_smile, position: right),
  rin: "早上好。[wait=1000]你来了。", greeting,
  "当前好感度：${courage}",
  choice("去哪里？") {
    "天台" when (courage >= 3): goto(rooftop),
    "教室": { courage += 1, call(classroom) }
  },
  story.end()
}
```

```text
基础
├── scene id { ... }：项目内唯一；goto 替换、call 调用、return 返回
├── id: "对白" / "旁白"；id: { "第一句", "第二句" }
│   ├── 每条字符串单独等待推进；可在后面加一个 voices 资源 ID
│   │   └── return / break 保留为控制语句，不作为语音 ID
│   ├── 尾部可选 volume: 0–1、concat: bool、auto: bool、inherit_speaker: bool
│   │   └── 默认 1 / false / false / false；concat 拼接前句，inherit_speaker 沿用前句角色
│   ├── @source_id 前置注解可赋稳定源身份；没有时由语义内容派生
│   └── [wait=N] 在同句打字中等待 N 毫秒，相邻标记累加，不占字形位置
├── let name = value：全项目一处声明，只在尚未初始化时初始化
├── 赋值 = / += / -= / *= / /= / %=；无 ++ / --
├── if (bool) { ... } else if (bool) { ... } else { ... }
├── loop { ... } / break：最近一层；无 while / for / continue
├── choice("可选提示") { "选项" when (bool): statement | { ... } }
│   └── when=false 隐藏；分支结束后汇合；全部隐藏是运行时错误
├── wait(300ms)：计时；wait.advance()：等待玩家推进
└── 注释 // ... 与 /* ... */；逗号分隔 statement，字符串与嵌套块不靠行号切分
```

类型为 bool/int/float/string 与同类型 list；没有 truthy、JavaScript 或隐式字符串回退。
表达式支持算术、比较、`and/or/not`、list 索引、`in`；混合比较/逻辑类别使用括号。
`/` 为实数除法，`%` 仅整数；除零、溢出、越界报错。
字符串使用双引号、转义与 `${纯表达式}`；list 不可直接插值。
列表修改是 statement：`items.append(x)`、`remove(x)`、`clear()`、`insert(index, x)`，
`pop(items, into: declared_variable)` 或 `pop(items, index, into: declared_variable)`。
空列表/越界 pop 不修改目标变量。列表方法不是可嵌入插值的表达式。

## 作者命令

时间用 `ms` / `s`，坐标用 1920×1080 设计空间，音量 0–1。
未出现的稀疏更新字段保持当前值；不支持的参数报诊断。

```text
常用命令选择
├── background(room)：换背景；background(none)：清背景
├── sprite(hero, face, position: right(x: 500), layout: viewport(height: 0.85))：显示立绘；默认开启 light
├── hide(hero) / hide(hero*)：隐藏指定立绘或前缀组
├── move(hero, left, duration: 300ms)：改变舞台锚点位置
├── sprite.transform(hero, x: 20, alpha: 0.8)：只修改给出的变换字段
├── sprite / sprite.transform 的 scale: 1.2 同时缩放两轴；不能与 scale_x/scale_y 混写
├── sprite.update(hero, face)：只换图，保留位置、布局、两轴缩放；目标不存在时跳过
│   └── 显式 position: center、layout: natural、scale: 1 才重设这些字段
├── camera.move(all, x: 20, duration: 300ms)：移动镜头
├── camera.effect(all, blur_amount: 2)：修改镜头特效
├── bgm(theme) / bgm(none)：播放循环音乐 / 停止音乐
├── se(click)：播放一次音效
└── wait(300ms) / wait.advance()：等待时间 / 等待玩家推进
```

```text
scene name { ... }
├── 基础：对白、旁白、choice、if、loop、let/赋值、列表方法、goto、call、return、wait
├── 剧情与文字
│   ├── story.end()
│   ├── text.box(visible: bool, auto: bool)
│   ├── text.style(style) / text.paragraph.style(style, ...)
│   ├── text.presentation(paragraph | dialogue)
│   ├── text.retract(source: "...", keep: "...")
│   ├── text.intro(hold: bool) { page("..."), ... }
│   ├── text.float("...", x: number, y: number, ...)
│   ├── text.float.configure(id: id, infinite: bool) / text.float.hide(id)
│   └── wait.advance()
├── 场景与立绘
│   ├── background(asset | none, transition: ..., blocking: ..., x: ..., y: ..., alpha: ..., ...)
│   ├── background.transform(x: ..., y: ..., alpha: ..., ...)
│   ├── sprite(id, asset, position: ..., layout: ..., blend: ..., blocking: ..., x: ..., y: ..., alpha: ..., ...)
│   ├── sprite.update(id, asset, position: ..., layout: ..., scale: ..., ...)
│   ├── sprite.transform(id, x: ..., alpha: ..., brightness: ..., contrast: ..., saturation: ..., ...)
│   ├── sprite.animate(id, preset, duration: ...)
│   ├── sprite.transition(id, enter: preset, exit: preset, duration: ...)
│   ├── sprite.keyframes(id, repeat: ..., blocking: ...) { frame(duration: ..., easing: ..., transform...), ... }
│   ├── sprite.sequence(id, fps: ..., loop: ...) { frame(asset), ... }
│   │   ├── 或省略 fps，每帧写 frame(asset, duration: ...)，所有帧必须有时长
│   │   └── mode: blink, interval: 3s / mode: talk, speaker: "角色名"
│   ├── sprite.select(id, variable, default: asset) { case("value", asset), ... }
│   ├── sprite.select.when(id, default: asset) { case(strict_bool_expression, asset), ... }
│   ├── sprite.focus.configure(characters: [...], speaking: style(...), others: style(...), narration: style(...), ...)
│   ├── sprite.focus(speaker_id | none)
│   ├── hide(id | prefix* | *, transition: ..., blocking: ...)
│   ├── move(id, left(...) | center(...) | right(...), duration: ..., easing: ..., blocking: ...)
│   ├── avatar.show(asset) / avatar.hide()
│   ├── scene.parallax(amplitude_percent: ..., edge_ease_percent: ..., return_to_center_on_leave: ..., scale: ...)
│   └── scene.parallax.stop()
├── 镜头、特效与舞台
│   ├── camera.move(scene | characters | all | none, transform..., optional_effect_fields..., tween: [...], duration: ..., easing: ..., blocking: ...)
│   ├── camera.reset(targets, duration: 0ms, easing: linear, blocking: true)
│   ├── camera.shake(targets, amplitude: ..., frequency: ..., amplitude_randomness: ..., frequency_randomness: ..., duration: ..., axis: ..., falloff: ..., blocking: ...)
│   ├── camera.bind(id, distance: ...) / camera.unbind(id, distance: ...)
│   ├── camera.effect(targets, sparse_effect_fields..., tween: [...], duration: ..., easing: ..., blocking: ...)
│   ├── stage.mask.show(id, typed_mask_fields..., duration: ..., blocking: ...)
│   ├── stage.mask.hide(id, duration: ..., blocking: ...)
│   └── stage.animate(id, duration: ..., repeat: ..., infinite: ..., playback_rate: ..., blocking: ...) {
│       ├── track(camera | character(id) | scene_layer(id), property, image: ..., muted: ...) {
│       │   └── key(time: ..., value: ..., easing: ...)
│       ├── event.camera.shake(time: ..., amplitude: ..., frequency: ..., amplitude_randomness: ..., frequency_randomness: ..., duration: ..., axis: ..., falloff: ...)
│       ├── event.camera.patch(time: ..., targets: ..., sparse_effect_fields...)
│       ├── event.particle(id, preset, time: ..., duration: ..., fade_out: ..., ...)
│       ├── event.scene(scene_id, time: ..., transition: ..., reset_camera: ..., fit: ..., ...) {
│       │   └── layer(id, asset, distance: ..., x: ..., y: ...)
│       └── event.audio(id, bgm | effect | vocal, asset, time: ..., volume: ..., loop: ..., ...)
│   }
├── 音视频与粒子
│   ├── se.loop(id, asset, volume: ..., fade: ...) / se.stop(id | *, fade: ...)
│   ├── vocal.play(asset, volume: ...) / vocal.stop()
│   ├── video.play(id, asset, loop: ..., muted: ..., alpha: ..., skippable: ..., wait: ..., mode: fullscreen | mixed)
│   ├── video.stop(id | *, fade: ...)
│   ├── particle.show(id, preset, texture: ..., count: ..., wind: ..., gravity: ..., fade_in: ..., size: ..., speed: ..., alpha: ..., spin: ..., drift: ..., drag: ..., color: ...)
│   ├── particle.hide(id | *, duration: ...)
│   └── particle.layers.clear()
└── 交互、系统与资源
    ├── input.request(variable, type: string | number | bool, title: "...", ...)
    ├── ui.show(slot) / ui.hide(slot) / ui.message(alert | confirm, title: "...", message: "...", ...)
    ├── playback.auto(bool)
    ├── screen.film(bool)
    ├── screen.curtain.show(color: rgba(r, g, b, a), duration: ...)
    ├── screen.curtain.hide(color: rgba(r, g, b, a), duration: ...)
    ├── gallery.unlock(cg | bgm, asset, name: "...")
    └── assets.loading(mode: auto | manual, lookahead: ..., blocking: ...) {
        └── resource(asset, kind: background | figure)
    }
```

## 字段

```text
sprite(...) / sprite.update(...)
├── position: left | center | right；可写 right(x: 500, y: 20)
│   └── 组内 x/y 是锚点偏移；顶层 x/y 始终是变换偏移；move 使用同一位置模型
├── layout（创建省略 = natural；换图省略 = 保留）
│   ├── natural：资源原始尺寸
│   ├── viewport(height: 0.85)：视口高度比例，必须大于 0
│   ├── scene(fit: cover, x: 960, y: 540, anchor: point(x: 0.5, y: 1), width: 700, height: 900)
│   │   └── fit 默认 by_height；x/y 默认 0；anchor 默认 (0.5, 0.5)；width/height 必须成对
│   │       fit = by_height | by_width | cover | contain | stretch | center
│   └── composite(canvas: size(width: 1920, height: 1080), rect: rect(x: 10, y: 20, width: 700, height: 900), height: 0.85)
│       └── canvas 必填；rect 四项成组，可省略；height 是可选视口高度比例
├── sprite.update 只作用于已显示对象；省略 position/layout/scale 保留当前值
│   └── 显式 position: center、layout: natural、scale: 1 可重设
├── sprite(...)：blend = alpha | add | multiply | screen；z 为整数
└── 初始 x/y/alpha/scale（或 scale_x/scale_y）/rotation/blur/width/height 与 brightness/contrast/saturation
    └── 与 transition 原子生效；后续 sprite.transform/background.transform 为稀疏更新
    └── duration/easing 只控制变换；颜色字段即时应用，纯颜色修改不接受非零 duration

sprite.focus.configure(...)
├── characters 是 stage sprite/角色 ID 列表，不创建另一种 portrait 对象
├── 配置一次后自动跟随对白角色，旁白自动使用 narration；无需逐句 sprite.focus(none)
│   └── characters.yaml 角色键与 sprite ID 一致；objects.yaml 的运行时别名由 Loader 解析
│       inherit_speaker 或无角色的 concat 延续上一句聚焦；显式 sprite.focus 可覆盖紧接着的一句
├── speaking、others、narration 均为 style(...)
│   └── 各有 scale、brightness、saturation、contrast、blur、alpha 六个可选字段
└── Block Inspector 将三组 style 的字段分别编辑，写回原有 style(...) 源码

camera.reset(...)
├── 一条 Block 恢复镜头位置/缩放和全部特效，立即停止震动；不修改场景视差或镜头绑定
└── duration/easing 控制位置与特效同步恢复；blocking 仅在组尾等待一次，省略时立即恢复

camera.effect(...)
├── 与 camera.move 使用同一 PostProcessPatch；包含镜面破碎与速度线，所有效果均可稀疏更新
├── focal_distance 与 lut_preset：不写=保持；none=清除；值=设置
└── 一个作者命令始终是一个原子 Action / Block，不因效果种类拆开

stage.mask.show(...)
├── mode、plane、scope、targets、shape、image、image_channel、image_fit
├── center_x/y、size_x/y、rotation、radius、visibility、feather、opacity
├── fill_mode、color、gradient_start/end、gradient_direction
└── texture、texture_fit/blend/scale/opacity、blur、vignette_*、noise_*、hue、saturation、brightness

text.paragraph.style(...)
├── typewriter_speed
└── reveal_duration/effect/distance/scale/rotation/blur；任一 reveal_* 出现才生成 TextRevealConfig

text.retract(source: "原文", keep: "保留前缀")
├── 复用引擎现有 RetractDialogue，不新增退格语法或执行语义
├── source 为空时使用当前对白；keep 为空时回删整句
├── keep 必须是执行时原文的前缀；回删保持完整 Unicode 字素
├── 回删完成后等待新的推进输入；同一结束帧的点击不复用
├── 可以连续执行多次；动画中途存档仍按现有 Save v11 恢复
└── Block 为独立单行“原文 → 保留前缀”
    ├── 摘要中的换行显示为 ↵；原文与 Inspector 保持完整多行文字
    ├── Inspector 编辑 Full text / Keep prefix；Enter 提交、Shift+Enter 换行
    ├── Replay 使用现有 Engine seek/show；不会把动画替换为静态截断
    └── Text Ending 关联只处理 text.box / hide，不吸收退格步骤

input.request(...)
├── 可只写 input.request(variable)；默认 string，其他字段沿用引擎默认值
├── type、title、description、placeholder、confirm_text、required_text、required
├── min_length、max_length、min_value、max_value、step
└── true_text、false_text
```

`sprite.select.when` 使用严格 Eiyashou bool 表达式，运行时也按严格类型求值。`sprite.select` 的变量未赋值时使用 `default` 资源。`sprite.animate` 与 `sprite.transition` 的内建 preset 可写标识符；自定义 preset 写带引号的 ID。`text.style` 与段落 style 的自定义 ID 也可写带引号的字符串。

`se.stop(id, fade: 200ms)` 按 ID 淡出并停止单次或循环音效；`se.stop(*)` 立即停止所有单次音效，不接受非零 fade；循环音效按 ID 停止。`hide(prefix*)` 匹配所有相同前缀的立绘 ID；`hide(*)` 匹配全部。此处的 `*` 不泛化到其他命令参数。

## 默认值与补间

```text
默认
├── background：默认 instant；background(none) 清背景
├── sprite：默认 center / instant / z: 0；同 slot 替换图像
├── hide(id | prefix* | *)：前缀或全部立绘共用 hide
├── move：省略 duration 为瞬移；easing 为 linear / ease_in / ease_out / ease_in_out
├── bgm(asset, volume: 1, fade: 0ms, loop: true)：新曲 crossfade；bgm(none, fade: ...) 停止
├── se(asset, volume: 1, id: optional_id, fade: 0ms)：一次性音效；ID 不使其循环；se(none) 停止
└── video(asset, skippable: true)：简写，非循环、fullscreen、阻塞到完成/跳过；video.play(id, asset, ...) 用于带 ID 的视频层
过渡与补间
├── background / sprite / hide：blocking 默认 true；false 允许过渡时继续
├── camera.move 可同时设置变换、特效与 shake: shake(amplitude: 4, frequency: 2)，仍为一个原子 Action / Block
│   ├── shake 的 duration 省略时沿用外层 duration；振幅、频率和时长必须齐全
│   ├── tween: [shake_amplitude, shake_frequency] 控制从当前震动值补间；没有正在震动时从 0 开始
│   └── shake 内还可写 axis、falloff、amplitude_randomness、frequency_randomness；频率变化时连续积累相位
├── tween: [x, blur_amount]：仅列出的数值字段补间，其余立即生效
│   ├── 省略：原整条命令补间；[]：立即生效且不额外阻塞
│   └── 未知/重复字段报错；离散字段不补间；仅对本命令提供值的通道生效
└── camera.shake / event.camera.shake
    ├── amplitude_randomness / frequency_randomness：0–1，默认 0
    ├── 平滑变化；按程序、执行位置与时间确定，可重复预览
    └── 两项为 0 时保持原采样；不提供逐字 blip
```

## 常用动态效果

```shou
sprite(eyes, eyes_open),
sprite.sequence(eyes, mode: blink, interval: 3s, fps: 10) {
  frame(eyes_open), frame(eyes_closed), frame(eyes_open)
},
sprite(mouth, mouth_closed),
sprite.sequence(mouth, mode: talk, speaker: "少女", fps: 12) {
  frame(mouth_closed), frame(mouth_open)
},
particle.show(snow, LIGHT_SNOW, count: 80, size: 12, speed: 100,
  alpha: 0.7, spin: 20, drift: 10, color: rgba(1, 0.95, 0.9, 1)),
```

眼睛、嘴等部件使用普通 sprite，可复用已有位置、布局、层级和变换；不引入第二套立绘模型。
动态序列至少两帧，第一帧是静止帧；眨眼在间隔后播放一遍，说话帧跟随指定角色 ID（或显示姓名）的逐字显示，
不是音频口型识别。对白完成后回到第一帧。换图或隐藏沿用 sprite 的序列清理规则。
动态模式自行控制重复，不能同时写 `loop`；仍可用统一的每帧 `duration` 替代 `fps`。

粒子省略字段沿用预设：size 是设计像素，speed 是像素/秒，spin 是度/秒，
drift 是横向摆动幅度，drag 是阻力，alpha 为 0–1，color 使用 rgba。
速度和大小保留预设的透视层差异；数量仍有每个发射器 256 的上限。
新粒子参数和动态模式不改变 Save v11 的状态布局，存档走现有剧本回放恢复路径。

## 迁移映射

`objects.yaml` 的 `objects` 将新裸 ID 映射到原引擎 ID，`prefixes` 保留
`scene-layer:` / `character-layer:` 分组与 wildcard。Loader 解析一次再生成 Program；
未映射 ID 原样保留，别名不递归，未知字段/空目标/重复引擎 ID 拒绝。
粒子纹理登记在 `assets.yaml` 的 `particles`，命令使用资源 ID，不内嵌文件路径。
关键帧会保留等待/恢复初值等已有引擎字段；以解析器与 Inspector 共享 schema 为准。

完整参数 owner：[`native`](../../crates/loader/src/adapter/script/native.rs)、
[`author commands`](../../crates/loader/src/adapter/script/native/v11.rs)、
[`typed model`](../../crates/core/src/model/action.rs)。字段列表与诊断在代码中维护，
不在文档复制另一套字段表。未知宿主扩展和 WebGAL wrapper 不成为新原生命令。

## 手写与维护

```text
写作习惯
├── 一句对白 / 一条命令一行；较长参数按行展开，仍用逗号分隔
├── 常用命令只写必要字段；资源和角色采用有意义的 ID
├── 镜头：camera.move 写变换，camera.effect 写特效；可在 move 中原子组合
├── 立绘：创建与 transform 共用变换/颜色名称；move 改锚点，transform 的 x/y 改变换偏移
├── 序列：固定帧率用 fps；不规则节奏用每帧 duration，不能混用
├── 输入：统一 input.request；需要时再增加验证字段
└── 复用剧情使用 scene/call；不要复制整组长参数到每句对白
```

不合并语义不同的项：sprite 创建与 update 的“仅修改已显示对象”、预设动画与时间轴、
每句 auto 与全局 playback.auto、屏幕视频与多层 video.play、资源变量选择与条件选择。
它们有不同的生命周期或执行约束，统一名称会隐藏作者必须理解的差别。
已删除 sprite.offset；它的 x/y、duration、easing 与 sprite.transform 完全同义，统一使用后者。

## 立绘环境光照

```text
config.yaml → layout.environment_light
├── 默认 1.0，范围 0–1；0 全局关闭；只对 native keine 项目生效
├── 逐立绘 Block：sprite(hero, face, light: false) 关闭该立绘，省略默认开启
│   ├── sprite.transform(hero, light: true) 可重新开启；省略保留当前状态
│   └── 跟随该对象的换图、存档和回退；再次 sprite 创建同 ID 时按新 Block 的设置重置
├── 图片准备时在线性颜色空间取最多 256 个样本，忽略透明像素
├── 先计算样本亮度 Y = 0.2126R + 0.7152G + 0.0722B，用第 75 百分位估计环境亮度
│   ├── 开平方压缩明暗变化，最暗增益限制为 0.65，避免把材质阴影当成无光环境
│   └── 用第 50–95 百分位区域估计色偏，独立归一化亮度；满强度通道偏移最多 25%
├── 背景过渡按进度混合；无背景时使用最底层可见 scene-layer 原图
├── 没有可取色场景时回退中性白；不保留整张 CPU 图片或回读 GPU
└── 只染色普通 alpha 立绘，不作用于背景、加法/乘法层、头像、文字或 UI
```

这是温和的背景色调适配，不产生方向光、阴影或法线重照明。
现有立绘滤镜/说话者聚焦仍生效，镜头效果和 LUT 再按原渲染顺序处理。
取色不包含镜头调色，避免反馈式重复染色；已有强手工调色可将该配置设为 0。
已发布的旧编译包需要重新编译为 IR schema v6；Save v11 的状态布局不变，
修改脚本后的存档仍受 Program fingerprint 检查。

## Editor 补全与角色维护

Text 输入命令或参数时显示原生候选菜单；↑/↓ 选择，Enter 插入，Esc 关闭。
Ctrl+Space 或 Alt+/（macOS Option+/）手动打开候选；灰色补全仍可用右方向键接受。
若 Ctrl+Space 被系统输入法快捷键占用，使用 Alt+/。候选支持命令、上下文参数、
合法枚举、资源 ID/原文件名、角色、章节和已索引的变量；右侧显示签名、值提示与例子，
命令/参数也可悬停查看。中文组词期间不查询补全或诊断，提交输入后才更新。
Inspector 切换 blink/talk 会原子移除冲突的 loop/interval/speaker；talk 默认选当前目标对应角色，
否则第一位角色。每帧 duration 与 fps 互斥，已有 duration 的序列不显示可新增 fps。

Characters 可新增、修改姓名/颜色、删除并定位对白引用；角色 ID 保持稳定。
选注册的立绘图片建立表情，多个图片 ID 按顺序作为帧列表；Assets 多选图片后，可一次登记多个表情。
头像也是注册的立绘图片。维护数据写入 characters.yaml 的可撤销源文档，Ctrl+S 保存：

```yaml
characters:
  hero:
    name: 少女
    color: '#BAEBFF'
    avatar: hero_avatar
    expressions:
      smile: [hero_smile]
      blink: [eyes_open, eyes_closed, eyes_open]
```

先在剧本选择插入位置，再点表情 Show/Change（多帧还可 Blink/Talk）或 Insert avatar。
Show 创建 sprite，Change/Blink/Talk 修改已有同 ID sprite；没有该 sprite 时应先 Show。
帧序列默认 10 fps，blink 默认间隔 3s，插入后通过 Text/Inspector 调整。
这些清单字段仅是作者预设；按钮插入显式 EYS，清单改动不会重写已插入的剧本。
资源 ID 重命名同步更新头像/表情预设；缺失图片显示问题。仅关联清单但未插入剧本的素材，
仍属于剧情未使用资源，不扩大正式打包 allowlist。非标准 flow-style 角色 YAML 可在 Text 编辑，
界面无法精确定位时拒绝修改。画面构图、时间轴和变量调试本轮暂缓。
