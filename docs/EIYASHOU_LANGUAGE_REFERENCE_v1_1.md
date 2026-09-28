# Eiyashou v1.1 增量语法

v1.1 在 [v1](EIYASHOU_LANGUAGE_REFERENCE_v1.md) 上增加命令，不改变对白、`choice`、`if`、`loop`、变量、列表、`background`、`sprite`、`hide`、`move`、`bgm`、`se` 和 `video` 的原写法。每条点号命令各自 lower 为有类型的 Engine Action；Block View 显示同一份 `.shou` 源码的短行，花括号内的行按层级缩进。参数中的时间使用 `ms` 或 `s`，坐标为 1920×1080 设计空间。未出现的稀疏字段保持当前值。

## 命令树

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
│   ├── background(asset | none, transition: ..., transform_*: ...)
│   ├── background.transform(x: ..., y: ..., alpha: ..., ...)
│   ├── sprite(id, asset, position: ..., layout: ..., blend: ..., transform_*: ...)
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
│   ├── hide(id | prefix* | *, transition: ...)
│   ├── move(id, left | center | right, anchor_offset: ..., y: ..., duration: ..., easing: ..., blocking: ...)
│   ├── avatar.show(asset) / avatar.hide()
│   ├── scene.parallax(amplitude_percent: ..., edge_ease_percent: ..., return_to_center_on_leave: ..., scale: ...)
│   └── scene.parallax.stop()
├── 镜头、特效与舞台
│   ├── camera.move(scene | characters | all | none, transform..., tween: [...], duration: ..., easing: ..., blocking: ...)
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

## 字段与语义

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

引擎内部跳转/标签、冻结的 WebGAL 兼容包装以及真正第三方扩展的 `HostCommand` 不成为原生作者命令。对应的逐项 Action 清单见 [Action 与 Block 对应树](EIYASHOU_ACTION_BLOCK_MAPPING_PROPOSAL.md)。

## 镜头随机度与逐字段补间（2026-09-28）

```text
camera.shake(...) / event.camera.shake(...)
├── amplitude_randomness、frequency_randomness：0–1，默认 0；Inspector 显示 0–100%
├── 振幅随机度：在基础振幅的 (1-r) 到 (1+r) 范围内平滑变化，并保留原衰减包络
├── 频率随机度：平滑改变节奏；按程序、执行位置与时间确定，重复预览结果一致
├── 两项均为 0 时使用原有 ShakeCamera / CameraShake 事件和旧采样顺序
└── 不增加逐字提示音 blip；用户明确排除
camera.move(...) / camera.effect(...) / camera.effect.v2(...)
├── tween: [x, scale_x]：列出的数值字段使用本命令 duration / easing；其余立即生效
├── 省略 tween：保留原来的整条命令补间行为
├── tween: []：全部立即生效，不因 duration 额外阻塞
├── 不重复或跨命令引用字段；枚举、资源、布尔等离散字段不参与
├── 同一条源码仍生成一个有类型 Action；即时字段与动画起点同时应用
├── Inspector：◆ 随时长补间，◇ 立即生效；直接修改同一份源码
└── 可选数值没有前值时（如 focal_distance: none → number）直接取目标值
LetsGal 导入
├── tweenFields：保留显式选择；offsetX / offsetY → x / y，zoom → scale_x / scale_y
├── 普通震动随机度为百分比；Stage 事件随机度为 0–1
├── 新震动使用顶部 duration，兼容旧 shakeDuration
└── 缺少新字段保持原有动作；旧 Save v11 布局、原动作 Postcard 索引不变
```

```shou
scene example {
  camera.move(scene, x: 120, scale_x: 1.5, tween: [x], duration: 1s),
  camera.effect(scene, blur_amount: 4, bloom_intensity: 0.5, tween: [blur_amount], duration: 500ms),
  camera.shake(all, amplitude: 8, frequency: 12, amplitude_randomness: 0.3, frequency_randomness: 0.2, duration: 300ms)
}
```
