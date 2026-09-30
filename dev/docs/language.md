# Eiyashou v1 + v1.1

当前原生脚本为 UTF-8 `.shou`，保留 v1 写法；v1.1 增加点号命令。
每条作者命令 lower 为 typed Engine Action，Editor 只编辑同一份源码。
`script.version: 1` 仍是当前配置值；v1.1 是增量命令集合。

## 工程

```text
project/
├── config.yaml        project.id、adapter.script: keine、script.entry
├── assets.yaml        backgrounds / figures / voices / bgm / se / videos / particles
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
  version: 1
  entry: start
  assets: assets.yaml
  characters: characters.yaml
```

清单条目可写 `room: assets/room.webp`，或 `{ path: assets/room.webp, tags: [interior] }`。
资源/角色/对象使用裸 ID；普通工程不需要对象映射。保留 `none`、`*` 等命令上下文哨兵。
脚本 ID 中的下划线合法；仓库文件布局规则不限制作者 ID。

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
scene name { ... }
├── 原 v1
│   ├── 对白、旁白、choice、if、loop、let/赋值、列表方法
│   ├── goto、call、return、wait
│   └── background、sprite、hide、move、bgm、se、video
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
│   ├── background(asset | none, transition: ..., blocking: ..., transform_*: ...)
│   ├── background.transform(x: ..., y: ..., alpha: ..., ...)
│   ├── sprite(id, asset, position: ..., layout: ..., blend: ..., blocking: ..., transform_*: ...)
│   ├── sprite.update(id, asset, position: ..., layout: ..., scale: ..., ...)
│   ├── sprite.offset(id, x: ..., y: ..., duration: ...)
│   ├── sprite.transform(id, x: ..., alpha: ..., ...)
│   ├── sprite.filter(id, blur: ..., brightness: ..., contrast: ..., saturation: ...)
│   ├── sprite.animate(id, preset, duration: ...)
│   ├── sprite.transition(id, enter: preset, exit: preset, duration: ...)
│   ├── sprite.keyframes(id, repeat: ..., blocking: ...) { frame(duration: ..., easing: ..., transform...), ... }
│   ├── sprite.sequence(id, fps: ..., loop: ...) { frame(asset), ... }
│   ├── sprite.sequence.timed(id, loop: ...) { frame(asset, duration: ...), ... }
│   ├── sprite.select(id, variable, default: asset) { case("value", asset), ... }
│   ├── sprite.select.when(id, default: asset) { case(strict_bool_expression, asset), ... }
│   ├── sprite.focus.configure(characters: [...], speaking: style(...), others: style(...), narration: style(...), ...)
│   ├── sprite.focus(speaker_id | none)
│   ├── hide(id | prefix* | *, transition: ..., blocking: ...)
│   ├── move(id, left | center | right, anchor_offset: ..., y: ..., duration: ..., easing: ..., blocking: ...)
│   ├── avatar.show(asset) / avatar.hide()
│   ├── scene.parallax(amplitude_percent: ..., edge_ease_percent: ..., return_to_center_on_leave: ..., scale: ...)
│   └── scene.parallax.stop()
├── 镜头、特效与舞台
│   ├── camera.move(scene | characters | all | none, transform..., optional_effect_fields..., tween: [...], duration: ..., easing: ..., blocking: ...)
│   ├── camera.shake(targets, amplitude: ..., frequency: ..., amplitude_randomness: ..., frequency_randomness: ..., duration: ..., axis: ..., falloff: ..., blocking: ...)
│   ├── camera.bind(id, distance: ...) / camera.unbind(id, distance: ...)
│   ├── camera.effect(targets, sparse_effect_fields..., tween: [...], duration: ..., easing: ..., blocking: ...)
│   ├── camera.effect.v2(targets, all_18_v2_fields..., tween: [...], duration: ..., easing: ..., blocking: ...)
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
│   ├── se.loop(id, asset, volume: ...) / se.stop(id | *)
│   ├── vocal.play(asset, volume: ...) / vocal.stop()
│   ├── video.play(id, asset, loop: ..., muted: ..., alpha: ..., skippable: ..., wait: ..., mode: fullscreen | mixed)
│   ├── video.stop(id | *, fade: ...)
│   ├── particle.show(id, preset, texture: ..., count: ..., wind: ..., gravity: ..., fade_in: ...)
│   ├── particle.hide(id | *, duration: ...)
│   └── particle.layers.clear()
└── 交互、系统与资源
    ├── input.simple(variable, title: "...", button: "...")
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
├── position = left | center | right；anchor_offset 与 y 可为负数
├── layout = natural | viewport_height | scene | composite
│   ├── viewport_height：layout_height
│   ├── scene：layout_fit、layout_x/y、layout_anchor_x/y、可选 layout_width/height 成对
│   └── composite：layout_canvas_width/height、可选 layout_rect_x/y/width/height 四项成组、layout_height_ratio
├── sprite(...)：blend = alpha | add | multiply | screen；z 为整数
└── 初始 transform_x/y/alpha/scale_x/scale_y/rotation/blur/width/height
    └── 与 transition 在同一 ShowSprite/ShowBg Action 中生效；后续 sprite.transform/background.transform 为稀疏更新

sprite.focus.configure(...)
├── characters 是 stage sprite/角色 ID 列表，不创建另一种 portrait 对象
├── speaking、others、narration 均为 style(...)
│   └── 各有 scale、brightness、saturation、contrast、blur、alpha 六个可选字段
└── Block Inspector 将三组 style 的字段分别编辑，写回原有 style(...) 源码

camera.effect(...)
├── PostProcessPatch 的全部 73 个字段由 Inspector 的共享字段清单提供
├── focal_distance 与 lut_preset：不写=保持；none=清除；值=设置
└── camera.effect.v2(...) 是完整状态，18 个 V2 字段必须全部显式填写

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
├── type、title、description、placeholder、confirm_text、required_text、required
├── min_length、max_length、min_value、max_value、step
└── true_text、false_text
```

`sprite.select.when` 使用严格 Eiyashou bool 表达式，运行时也按严格类型求值。`sprite.select` 的变量未赋值时使用 `default` 资源。`sprite.animate` 与 `sprite.transition` 的内建 preset 可写标识符；自定义 preset 写带引号的 ID。`text.style` 与段落 style 的自定义 ID 也可写带引号的字符串。

`se.stop(id)` 停止指定循环音效；`se.stop(*)` 调用引擎现有的无 ID Stop 效果事件。`hide(prefix*)` 匹配所有相同前缀的立绘 ID；`hide(*)` 匹配全部。此处的 `*` 不泛化到其他命令参数。



## 默认值与补间

```text
原 v1
├── background：默认 instant；background(none) 清背景
├── sprite：默认 center / instant / z: 0；同 slot 替换图像
├── hide(id | prefix* | *)：前缀或全部立绘共用 hide
├── move：省略 duration 为瞬移；easing 为 linear / ease_in / ease_out / ease_in_out
├── bgm(asset, volume: 1, fade: 0ms, loop: true)：新曲 crossfade；bgm(none, fade: ...) 停止
├── se(asset, volume: 1)：一次性音效；se(none) 停止
└── video(asset, skippable: true)：非循环、fullscreen、阻塞到完成/跳过
v1.1
├── background / sprite / hide：blocking 默认 true；false 允许过渡时继续
├── camera.move 可同时设置变换与特效，仍为一个原子 Action / Block
├── tween: [x, blur_amount]：仅列出的数值字段补间，其余立即生效
│   ├── 省略：原整条命令补间；[]：立即生效且不额外阻塞
│   └── 未知/重复字段报错；离散字段不补间；仅对本命令提供值的通道生效
└── camera.shake / event.camera.shake
    ├── amplitude_randomness / frequency_randomness：0–1，默认 0
    ├── 平滑变化；按程序、执行位置与时间确定，可重复预览
    └── 两项为 0 时保持原采样；不提供逐字 blip
```

## 迁移映射

`objects.yaml` 的 `objects` 将新裸 ID 映射到原引擎 ID，`prefixes` 保留
`scene-layer:` / `character-layer:` 分组与 wildcard。Loader 解析一次再生成 Program；
未映射 ID 原样保留，别名不递归，未知字段/空目标/重复引擎 ID 拒绝。
粒子纹理登记在 `assets.yaml` 的 `particles`，命令使用资源 ID，不内嵌文件路径。
关键帧会保留等待/恢复初值等已有引擎字段；以解析器与 Inspector 共享 schema 为准。

完整参数 owner：[`native`](../../crates/loader/src/adapter/script/native.rs)、
[`v11`](../../crates/loader/src/adapter/script/native/v11.rs)、
[`typed model`](../../crates/core/src/model/action.rs)。字段列表与诊断在代码中维护，
不在文档复制另一套 73 字段表。未知宿主扩展和 WebGAL wrapper 不成为新原生命令。
