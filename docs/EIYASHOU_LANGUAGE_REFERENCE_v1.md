# Eiyashou v1 — 完整语法与实现对齐参考

> **状态**：语言设计合同（v1）+ 已实现代码对齐参考
>
> **设计收口日期**：2026-09-20
> **实现基线**：当前集成树（Eiyashou v1 typed adapter/runtime、source-first Editor 与 source-free release boundary）
> **用途**：作者参考、Editor 语法支持、Eiyashou loader/compiler 实现、Code Review 与后续协作者交接。

---

## 0. 本文档如何阅读

本文档严格区分三类事实：

1. **语言合同**：已经确认的 Eiyashou v1 语法和语义。实现不得静默改写。
2. **当前 Engine 能力目录**：从审计基线代码中读取的真实 runtime/core 能力，例如 Transition、Easing、Position、媒体格式。
3. **实现对齐项**：当前代码如何承载语言合同，以及哪些能力明确在 v1 范围外。

Eiyashou 是作者源语言。它编译到 runtime-neutral `Action` / `Program`，但 **`Action` / `Program` 不是作者文档模型，也绝不能反向覆盖 `.shou` 源文件**。

---

# 1. 项目结构

## 1.1 标准目录

```text
<project>/
├── config.yaml
├── assets.yaml
├── characters.yaml
├── assets/
│   ├── background/
│   ├── figure/
│   ├── vocal/
│   ├── bgm/
│   ├── se/
│   └── video/
└── scripts/
    ├── opening.shou
    ├── common.shou
    └── chapter01/
        └── day01.shou
```

v1 脚本扩展名固定为：

```text
.shou
```

默认递归发现：

```text
scripts/**/*.shou
```

v1 **没有 `import`**。所有脚本共同形成一个项目级 scene 命名空间。

一个 `.shou` 文件可以包含一个或多个 scene。

---

## 1.2 `config.yaml`

Eiyashou 作者侧配置至少需要：

```yaml
adapter:
  script: keine

script:
  version: 1
  entry: opening
  assets: assets.yaml
  characters: characters.yaml
```

`entry` 指定入口 scene。

manifest 路径相对于项目根。默认值分别为 `assets.yaml` 与 `characters.yaml`，因此常规项目
可以省略这两个字段；脚本不重复声明角色。

项目已有的标题、项目 ID、bundle identifier、asset/store adapter 等 Engine 配置仍可由 `config.yaml` 承担；Eiyashou 实现不得要求作者同时维护第二份等价脚本配置。

### 当前代码库对齐说明

`GameConfig` 继续以 `adapter.script` 作为唯一脚本适配器选择入口：

```yaml
adapter:
  script: keine
script:
  version: 1
  entry: opening
  assets: assets.yaml
  characters: characters.yaml
```

Eiyashou 没有第二套项目探测/配置所有者。Loader 从同一个配置读取 manifest 路径，将作者侧：

```text
config.yaml + assets.yaml + characters.yaml + scripts/**/*.shou
```

转换/编译为 runtime 所需的 `GameConfig + Program`。旧内嵌 `GameConfig.assets` 仅保留给兼容
路径；Eiyashou 作者资源以独立 manifest 为准。

---

# 2. `assets.yaml`

## 2.1 基本结构

资源 ID 按类型拥有独立命名空间：

```yaml
backgrounds:
  day_school: assets/background/day_school.webp
  sunset: assets/background/sunset.webp

figures:
  rin_smile: assets/figure/rin/smile.webp
  rin_angry: assets/figure/rin/angry.webp

voices:
  ch01_001: assets/vocal/ch01/001.opus
  ch01_002: assets/vocal/ch01/002.opus

bgm:
  summer: assets/bgm/summer.opus

se:
  door: assets/se/door.opus
  step: assets/se/step.opus

videos:
  opening_movie: assets/video/opening.mp4
```

因此下面两个 ID 可以同时存在：

```yaml
backgrounds:
  school: assets/background/school.webp

figures:
  school: assets/figure/school.webp
```

解析永远由参数位置决定类型，不跨 namespace 猜测。

例如：

```eiyashou
background(school)
```

只查 `backgrounds.school`。即使 `figures.school` 存在，也不能兜底替代。

---

## 2.2 标准 namespace

v1 标准 authoring namespace：

```text
backgrounds
figures
voices
bgm
se
videos
```

当前 Engine 还具有粒子、LUT 等更广泛的资源能力；它们不属于这份基础 Eiyashou v1 的固定作者语法，可由后续 Engine/DSL 协作者扩展。

未知 namespace：

- 保留原始 YAML；
- 给 warning；
- 真正引用时必须由当前 Engine/extension capability 识别，否则报错。

未使用资源只产生低严重度诊断，不是构建错误。

---

## 2.3 路径规则

`assets.yaml` 中路径：

- 相对于项目根；
- 必须使用项目内资源；
- 禁止绝对路径；
- 禁止通过 `..` 越出项目根；
- symlink 最终解析后若逃出项目根，同样无效；
- 打包阶段仍服从当前 Kēne 内容挂载与发布安全边界。

---

## 2.4 当前 Engine 媒体格式

下表是 **当前代码库能力**，不是 DSL 永久写死的扩展名名单。

| 类别 | 生产/发行规范 | 开发兼容 |
|---|---|---|
| 背景 / 立绘 / 粒子图片 | WebP | PNG、JPEG |
| LUT | WebP | PNG |
| Voice / BGM / SE | Ogg Opus `.opus` | WAV、MP3、Vorbis (`.ogg/.oga/.spx`)、FLAC |
| Video | MP4/M4V，H.264 + AAC 为跨平台 canonical | FFmpeg 后端可接受更多容器；macOS AVFoundation 侧以 MP4/MOV 为主 |

发行流水线目前要求项目图片使用 WebP、独立音频使用 Opus；兼容格式不应偷偷扩大 shipping engine feature set。

DSL 本身不把这些扩展名写死。Editor 应从当前 Engine/capability 与发布规则显示权威支持情况。

---

# 3. `characters.yaml`

## 3.1 Schema

```yaml
characters:
  rin:
    name: "凛"
    color: "#86A8D8"

  china:
    name: "千夏"
```

规则：

- mapping key 是角色 ID；
- `name`：v1 唯一必填字段；
- `color`：可选，仅 `#RRGGBB`；
- 无 `color` 时由 UI/theme 决定；
- v1 没有 `short_name`；
- YAML 不绑定 runtime 变量，`name` 是静态 metadata；
- 未定义角色 ID 用作 speaker 是编译错误；
- 未知字段 warning，但应保留/忽略，避免未来扩展被破坏；疑似拼写错误应提供更明确诊断。

例如：

```eiyashou
rin: "早上好。"
```

这里的 `rin` 必须存在于 `characters.yaml`。

### 当前代码库对齐说明

Eiyashou compiler 在项目边界解析角色 ID，将静态 `name` 与 `color` 写入 typed dialogue；
runtime 保留该 presentation 数据，MainCore speaker UI 使用作者颜色。未知角色仍是编译错误。

---

# 4. 词法与基础格式

## 4.1 标识符

标识符：

- Unicode XID；
- 大小写敏感；
- 源码必须是 NFC；
- 非 NFC 是错误，不静默规范化；
- 关键字不能作为普通 identifier；
- Editor 应警告视觉混淆字符；
- 资源 ID 推荐 ASCII `snake_case`。

合法示例：

```eiyashou
scene 学校天台 {
  绫: "早上好。"
}
```

---

## 4.2 字符串

v1 只使用双引号：

```eiyashou
"hello"
```

不支持单引号、反引号、三引号。

标准 escape：

```text
\"   双引号
\\   反斜杠
\n   换行
\r   回车
\t   制表
```

Unicode 直接写源码，不提供 `\uXXXX` / `\u{...}`。

未知 escape 是错误。

字符串本身不物理跨行；单条对白需要换行时使用 `\n`。

### 字面量 `${`

`${...}` 是插值起始序列。需要显示字面量 `${` 时，在它前面加 `/`：

```eiyashou
"价格模板是 /${amount}。"
```

显示结果为 `价格模板是 ${amount}。`，不会执行插值。`/${` 是一个完整的插值禁用序列；
其余 `/` 和不组成 `${` 的单个 `$` 都按普通字符处理。这样不增加新的反斜杠 escape，
也不会为本来就能直接书写的 `$` 再造一条规则。

---

## 4.3 注释

行注释：

```eiyashou
// comment
```

块注释：

```eiyashou
/*
  comment
*/
```

块注释允许嵌套。

---

## 4.4 UTF-8 与源码保真

Native authoring source 采用 UTF-8。

Editor 的核心原则：

- 源码是权威；
- 不因打开/关闭重写文件；
- 保存时不自动格式化整个文件；
- Text View 与 Card View 必须编辑同一个 `SourceDocument`；
- whitespace、comment、unknown node、原始顺序必须通过无损 CST 保存；
- unknown syntax 可显示诊断，但不能静默丢掉；
- runtime `Action` 不能作为写回源文件的依据。

---

# 5. 分隔符、块与调用

## 5.1 块

使用 `{}`：

```eiyashou
scene opening {
  "第一句。",
  "第二句。"
}
```

缩进与换行只影响可读性，不影响语义。

---

## 5.2 逗号

逗号分隔：

- 相邻 statement；
- 参数；
- list 元素；
- choice entry；
- dialogue block entry。

**任何列表或块的最后一项都不允许尾逗号。**

合法：

```eiyashou
scene opening {
  "A。",
  "B。"
}
```

非法：

```eiyashou
scene opening {
  "A。",
  "B。",
}
```

顶层 scene 声明彼此之间不加逗号：

```eiyashou
scene opening {
  "A。"
}

scene ending {
  "B。"
}
```

---

## 5.3 普通调用

普通 command/function 统一使用 `()`：

```eiyashou
background(day_school)
wait(300ms)
goto(rooftop)
```

参数规则：

1. 常用必填参数按位置写；
2. 可选参数用命名参数；
3. 所有位置参数在命名参数之前；
4. 必填位置参数不能从中间跳过；
5. 命名参数顺序不影响语义；
6. 重复命名参数是错误。

```eiyashou
sprite(
  rin_stage,
  rin_smile,
  position: center,
  transition: fade(300ms),
  z: 10
)
```

Editor 可以显示不写入源码的 inlay hint。

---

# 6. Scene 与项目控制流

## 6.1 Scene

```eiyashou
scene opening {
  "开始。"
}
```

规则：

- scene 名全项目唯一；
- scene v1 不接受参数；
- scene v1 不返回值；
- 入口 scene 来自 `config.yaml` 的 `script.entry`。

---

## 6.2 `goto`

```eiyashou
goto(ending)
```

永久转移到目标 scene，不返回当前位置。

---

## 6.3 `call`

```eiyashou
call(side_story)
```

进入目标 scene；目标自然到达末尾或执行合法 `return` 后，回到 `call(...)` 的下一条 statement。

直接或间接递归 `call` 在 v1 中禁止。

---

## 6.4 `return`

```eiyashou
return
```

只用于从 `call(...)` 返回。

调用栈为空时执行 `return` 是语义/runtime error，不等价于“结束游戏”。

---

## 6.5 Scene 自然结束

- 被 `call` 的 scene 自然到末尾：自动 return；
- 入口/`goto` 进入的 scene 到末尾且调用栈为空：故事结束。

---

## 6.6 跨 scene 环与执行预算

通过 `goto` 构造跨 scene cycle 允许，但所有控制流环都必须保证每一圈经过至少一个
**yield action**：对白/旁白、choice、正时长 `wait` 或阻塞 video。

编译器在整个项目 CFG 上检查 strongly connected component。只要存在一条可以绕开所有
yield action 回到自身的路径，就是编译错误，不生成 Program；这同时适用于显式 `loop`、
同 scene jump 和跨 scene `goto` cycle。当前 core 每次推进最多 1024 个 Action 的限制继续
作为 fail-closed 保险；触发后必须报告可定位 runtime error，不自动重试。

---

# 7. `if / else if / else`

```eiyashou
if (affection >= 5) {
  "最高分支。"
} else if (affection >= 3) {
  "中间分支。"
} else {
  "普通分支。"
}
```

条件必须严格为 `bool`。

非法：

```eiyashou
if (affection) {
  ...
}
```

必须写为：

```eiyashou
if (affection > 0) {
  ...
}
```

v1 没有 truthy/falsy 隐式转换。

---

# 8. `loop` 与 `break`

v1 唯一循环结构是无限 `loop`：

```eiyashou
loop {
  call(daily_event),

  if (day >= 7) {
    break
  }
}
```

规则：

- 到达 `loop` 块末尾后回到第一条 statement；
- `break` 退出最近一层 loop；
- v1 没有 `while`；
- v1 没有 `for`；
- v1 没有 `continue`；
- v1 没有 `break 2` 等多层 break。

---

# 9. 对白、旁白、语音

## 9.1 单条对白

```eiyashou
rin: "早上好。"
```

`speaker` 必须是 `characters.yaml` 中已定义角色 ID。

---

## 9.2 旁白

裸字符串 statement 就是旁白：

```eiyashou
"风从窗缝里吹了进来。"
```

不使用 `narrate(...)` 或 `narration:`。

---

## 9.3 连续对白块

```eiyashou
rin: {
  "第一句。",
  "第二句。",
  "最后一句。"
}
```

块中每个字符串都是独立的一次玩家推进，不会自动拼成一个文本框。

---

## 9.4 行级 Voice

没有 `voice(...)` command。

语音资源直接附在对白/旁白后：

```eiyashou
rin: "早上好。", ch01_001
```

旁白同样支持：

```eiyashou
"那年夏天结束了。", narrator_001
```

规则：

- voice slot 只能有一个资源 ID；
- 严格查 `voices` namespace；
- 没有语音就完全省略槽位；
- 不允许 `, none` 作为假 voice；
- 不提供单行 volume/pan/fade 参数；
- 玩家推进到下一行时，当前行 voice 立即停止。

---

## 9.5 对白块中的 Voice

```eiyashou
rin: {
  "第一句。", ch01_001,
  "第二句。", ch01_002,
  "最后一句。"
}
```

语音只绑定它所在的那一个 dialogue entry。

---

# 10. Stable ID

## 10.1 显式 ID

需要跨编辑稳定时，在目标节点前写裸 `@identifier`：

```eiyashou
@ch01_rin_001
rin: "早上好。", ch01_001
```

旁白：

```eiyashou
@ch01_narration_001
"雨停了。"
```

choice option：

```eiyashou
choice {
  @choice_rooftop
  "去天台": goto(rooftop),

  @choice_classroom
  "回教室": goto(classroom)
}
```

显式 stable ID：

- 全项目唯一；
- v1 只允许用于对白、旁白、choice option；
- 不是任意 statement annotation 系统。

---

## 10.2 对白块中的 ID

每个 entry 独立：

```eiyashou
rin: {
  @rin_001
  "第一句。", ch01_001,

  @rin_002
  "第二句。", ch01_002
}
```

---

## 10.3 自动派生 ID

没有显式 `@id` 时，默认由：

```text
scene + 语义内容 + 同内容出现序号
```

派生。

语义内容不包含：

- whitespace；
- comment；
-纯格式变化。

因此格式化不会改变派生 ID。

如果同一 scene 有多条完全相同文本，在前面插入另一条相同文本可能导致后续 occurrence number 改变。这是接受的限制；真正需要稳定的地方使用显式 `@id`。

### 当前代码库对齐说明

当前 read history 使用 `DialogueKey { scene, action_index }`，choice/runtime 也没有 source stable ID 字段。要兑现本合同，需要 compiler/runtime persistence 增加 stable source identity 或稳定映射；不能继续把 action index 当作长期作者 ID。

---

# 11. 文本插值

使用：

```eiyashou
${...}
```

示例：

```eiyashou
rin: "欢迎回来，${player_name}。"
rin: "当前好感度：${affection + 1}。"
```

插值上下文只允许：

- 读取变量；
- 纯表达式；
- 无副作用操作。

`bool / int / float / string` 会自动转为文本。

list 不能直接插值。

例如：

```eiyashou
"好感度：${affection}"
```

合法。

但：

```eiyashou
"物品：${inventory}"
```

非法。

---

# 12. 变量

## 12.1 全局状态

v1 所有作者变量都是项目全局剧情状态，没有 scene-local variable。

---

## 12.2 `let`

首次声明：

```eiyashou
let courage = 0
```

后续赋值：

```eiyashou
courage = 3
courage += 1
```

对不存在变量直接赋值是错误。

---

## 12.3 `let` 的初始化语义

```eiyashou
let courage = 0
```

语义不是“每次执行都写 0”，而是：

> 如果 `courage` 尚未初始化，则初始化为 `0`；否则保留当前/存档中的值。

同一个变量全项目只能存在一处 `let`。

因此：

```eiyashou
loop {
  let count = 0,
  count += 1
}
```

第一次迭代初始化，后续迭代不会把 `count` 重置为 0。

如果 `let` 位于条件路径，静态分析必须做 definite-initialization：

```eiyashou
if (condition) {
  let key = true
}

if (key) {
  ...
}
```

第二处必须诊断“`key` 可能未初始化”。

---

# 13. 类型系统

v1 标量：

```text
bool
int
float
string
```

加同类型 list：

```text
list<bool>
list<int>
list<float>
list<string>
```

源码不写显式变量类型：

```eiyashou
let score = 0
let name = "Rin"
```

v1 完全类型推断。

不允许嵌套 list。

---

# 14. List

## 14.1 非空 literal

```eiyashou
let items = ["key", "ticket"]
let scores = [1, 3, 5]
```

元素必须同类型。

`int` 和 `float` 混合时自动提升：

```eiyashou
let values = [1, 2.5]
```

类型为 `list<float>`。

---

## 14.2 空 list

首次声明空 list 时使用特殊构造：

```eiyashou
let inventory = list(string)
let flags = list(bool)
```

允许的 type name：

```text
bool
int
float
string
```

`list(type)` 是特殊初始化/type-construction 形式，不是一般 runtime expression。

例如下面这种写法非法：

```eiyashou
list(string) == list(string)
```

---

## 14.3 读取与长度

```eiyashou
items[0]
items.length
```

`.length` 只读。

负索引禁止：

```eiyashou
items[-1] // error
```

越界：

- 静态可证明时编译错误；
- 否则 runtime error。

---

## 14.4 索引赋值

```eiyashou
items[0] = "new"
scores[0] += 1
```

v1 lvalue 只有：

```text
variable
variable[index]
```

---

## 14.5 修改方法

```eiyashou
items.append("key")
items.remove("key")
items.clear()
items.insert(0, "key")
pop(items, into: last)
pop(items, 2, into: removed)
```

语义：

- `append(x)`：尾部加入；无返回值；
- `remove(x)`：只删第一个匹配项；无返回值；
- `clear()`：清空；无返回值；
- `insert(index, x)`：在位置插入；允许 `index == length`；更大 index 报错；
- `pop(list, into: target)`：删除最后一项并写入已声明的 `target`；
- `pop(list, index, into: target)`：删除指定项并写入已声明的 `target`；
- 空 list 或越界 `pop`：runtime error，目标变量保持不变；
- `remove(x)` 找不到：不改变 list，但产生 warning；静态可证明则编译期 warning，否则 runtime warning。

所有 list 修改都是独立 statement，按源码从左到右执行；它们不是 expression。使用
`pop` 前，目标变量必须已通过唯一的 `let` 初始化为相同元素类型：

```eiyashou
let last = "",
pop(items, into: last)
```

因此 choice condition、`if` condition、赋值右侧和 `${...}` 插值都保持无副作用。

---

## 14.6 `in`

```eiyashou
"key" in items
```

`in` 只用于 list membership。

不用于字符串子串搜索。

---

## 14.7 List 比较

同类型 list 支持：

```eiyashou
==
!=
```

按元素顺序逐项比较。

---

# 15. 表达式

## 15.1 算术

```text
+  -  *  /  %
```

一元负号：

```eiyashou
-3
-value
```

`%` 只允许 `int % int`。

`/` 是实数除法：

```eiyashou
5 / 2 // 2.5
```

除零是 runtime error。

整数溢出是 runtime error，不回绕。

NaN / Infinity 不属于语言值；任何产生非有限 float 的操作报错。

---

## 15.2 数字 literal

支持：

```eiyashou
0
12
-5
0.5
3.14
1e6
2.5e-3
```

v1 不支持：

```text
0xff
0b1010
1_000
```

---

## 15.3 比较

```text
==  !=  <  <=  >  >=
```

数值允许 `int`/`float` 跨类型比较：

```eiyashou
1 == 1.0 // true
```

不同非数值类型之间的比较是类型错误：

```eiyashou
1 == "1"  // error
true == 1 // error
```

string 只支持：

```text
== !=
```

不支持 string 的 `< <= > >=`。

不支持链式比较：

```eiyashou
0 < x < 10 // error
```

应写：

```eiyashou
(x > 0) and (x < 10)
```

---

## 15.4 逻辑

```text
and
or
not
```

条件严格要求 bool。

`and` / `or` 短路求值。

```eiyashou
(index < items.length) and (items[index] == "key")
```

当左侧已为 false 时，右侧不执行，因此不会产生无意义的越界读取。

---

## 15.5 字符串拼接

允许：

```eiyashou
"hello " + "world"
```

允许：

```eiyashou
name += "!"
```

不做任意隐式 stringify：

```eiyashou
"score: " + score // error
```

应写：

```eiyashou
"score: ${score}"
```

---

## 15.6 复合赋值

```text
+=
-=
*=
/=
%=
```

`%=` 仅 int。

不支持：

```text
++
--
```

---

## 15.7 优先级

v1 只依赖最小优先级规则：

1. 一元 `not`、`-`；
2. `* / %`；
3. `+ -`。

比较、membership、逻辑等不同类别混合时要求显式括号，不建立一套让作者猜测的完整隐式优先级表。

推荐：

```eiyashou
(a > 1) and (b < 2)
```

而不是依赖：

```eiyashou
a > 1 and b < 2
```

---

# 16. Choice

## 16.1 带 prompt

```eiyashou
choice("去哪里？") {
  "教室": goto(classroom),
  "天台": goto(rooftop)
}
```

---

## 16.2 无 prompt

```eiyashou
choice {
  "教室": goto(classroom),
  "天台": goto(rooftop)
}
```

---

## 16.3 分支 block

```eiyashou
choice("怎么办？") {
  "留下": {
    courage += 1,
    goto(rooftop)
  },
  "离开": goto(classroom)
}
```

branch 可以是单条 statement 或 block。

如果 branch 没有通过 `goto`、`return`、`break` 等离开，执行完后默认汇合并继续 choice 后的 statement。

v1 不提供额外 `stop` keyword。

---

## 16.4 `when`

```eiyashou
choice("去哪里？") {
  "天台" when (affection >= 3): goto(rooftop),
  "教室": goto(classroom)
}
```

condition 为 false 时，该 option **隐藏**。

`when` 只属于 choice option，不能写：

```eiyashou
background(sunset) when (evening) // error
```

普通条件使用 `if`。

“显示但禁用”的 choice 语法在 v1 **暂缓**，没有为了它发明额外语法。当前 core 虽然已有 `enable_when` runtime 能力，但基础 Eiyashou v1 不暴露。

---

## 16.5 Choice 边界

- 只有一个 option：允许，但 warning；
- 运行时所有 option 都被过滤掉：runtime error；
- option stable ID 可用 `@id`；
- option text 是普通纯文本字符串。

---

# 17. 标准视觉命令

## 17.1 `background`

```eiyashou
background(day_school)
```

替换当前背景。

带 transition：

```eiyashou
background(
  sunset,
  transition: fade(300ms)
)
```

清空背景：

```eiyashou
background(none)
```

`none` 在这里是 command-context sentinel，不是普通资源 ID 或一般值。

默认 transition：`instant`。

---

## 17.2 `sprite`

```eiyashou
sprite(rin_stage, rin_smile)
```

第一个参数是 **stage sprite slot/instance ID**，不是强制 character ID。

完整示例：

```eiyashou
sprite(
  rin_stage,
  rin_smile,
  position: right,
  transition: fade(300ms),
  z: 10
)
```

规则：

- 同一 slot 再次 `sprite(...)`：替换该 slot 当前图像；
- 默认 `position: center`；
- 默认 `transition: instant`；
- 默认 `z: 0`；
- z 越大越靠前；
- 同 z 时，最近一次显示/更新的 sprite 在前。

位置名称来自 Engine capability；审计基线当前基础 `Position` 提供：

```text
left
center
right
```

v1 不暴露任意坐标位置。未来若扩展二维值，语法应保持轻量，但不因此把 tuple 强塞进当前 v1 类型系统。

---

## 17.3 `hide`

隐藏一个 slot：

```eiyashou
hide(rin_stage)
```

带 transition：

```eiyashou
hide(rin_stage, transition: fade(300ms))
```

隐藏全部 sprite：

```eiyashou
hide(*)
```

也可以：

```eiyashou
hide(*, transition: fade(300ms))
```

`*` 不是一般表达式，只是明确支持 wildcard 的 command 参数 token；v1 当前只在 `hide(*)` 使用。

---

## 17.4 `move`

移动已存在的 sprite 使用独立 command，不复用 `sprite()`：

```eiyashou
move(rin_stage, right)
```

无 `duration` 时瞬移。

动画移动：

```eiyashou
move(
  rin_stage,
  right,
  duration: 300ms,
  easing: ease_out
)
```

当前 Engine easing catalog 见第 20 节。

### 当前代码库对齐说明

现有 core 有 `SetTransform` 和 `UpdateSprite`，但没有一个无需重新提供 image、直接把现存 sprite 的 base `Position` 移到另一命名 anchor 的干净 Action。为了保持 `move(slot, named_position)` 的已定语义，Eiyashou implementation 应增加 typed `MoveSprite` 等价能力，或提供语义完全一致的 typed lowering；不应通过猜当前 image 来伪造 `UpdateSprite`。

---

# 18. Transition

transition 是视觉 command 的局部命名参数，不是影响后续命令的全局 ambient state。

审计基线 `Transition` 的真实能力：

```eiyashou
instant
fade(300ms)
slide_from_left(300ms)
slide_from_right(300ms)
crossfade(300ms)
wipe(300ms)
dissolve(300ms)
```

对应 current core：

```text
Instant
Fade
SlideFromLeft
SlideFromRight
Crossfade
Wipe
Dissolve
```

未来 Engine 增加 transition 时，由 capability/reference 同步补充；不要在 DSL 文档里凭空声明不存在的名称。

---

# 19. Duration 与 `wait`

## 19.1 Duration literal

```eiyashou
250ms
1s
1.5s
```

裸数字不自动代表毫秒或秒。

---

## 19.2 `wait`

```eiyashou
wait(500ms)
wait(1.5s)
```

阻塞剧情执行相应的 real-time duration。

---

# 20. Easing

审计基线 `Easing` 的真实能力：

```text
linear
ease_in
ease_out
ease_in_out
in_out_quad
out_cubic
in_out_cubic
out_back
out_bounce
```

例如：

```eiyashou
move(
  rin_stage,
  center,
  duration: 400ms,
  easing: out_cubic
)
```

对应 current core：

```text
Linear
EaseIn
EaseOut
EaseInOut
InOutQuad
OutCubic
InOutCubic
OutBack
OutBounce
```

---

# 21. BGM

## 21.1 播放

```eiyashou
bgm(summer)
```

完整形式：

```eiyashou
bgm(
  summer,
  volume: 0.8,
  fade: 1s,
  loop: true
)
```

规则：

- `volume` 范围 `0.0 .. 1.0`；
- 默认 `volume: 1.0`；
- 默认 `fade: 0ms`；
- 默认 `loop: true`；
- 只有一个 BGM bus；
- 播放新 BGM 会替换旧 BGM；
- 新 BGM 指定 `fade` 时，语言语义是旧曲淡出、新曲淡入的 crossfade。

---

## 21.2 停止

```eiyashou
bgm(none)
```

淡出：

```eiyashou
bgm(none, fade: 1s)
```

`none` 是 command-context sentinel。

### 当前代码库对齐说明

当前 `Action::Bgm` 只有：

```text
file
volume
fade_seconds
```

runtime 固定使用 `PlaybackMode::Loop`，所以 `loop: false` 尚无 typed lowering。

同时，当前切换到新 BGM 时会先 despawn 旧 player，再让新 player fade-in，并非本文档已经确定的真正 old/new crossfade。要兑现 Eiyashou 语义，需要扩展 BGM runtime state/action。

`bgm(none, fade: ...)` 的 fade-out 当前已有自然对应能力。

---

# 22. SE

播放一次性音效：

```eiyashou
se(door)
```

音量：

```eiyashou
se(door, volume: 0.8)
```

规则：

- `volume` 范围 `0.0 .. 1.0`；
- 多次同资源调用允许重叠；
- v1 SE 不 loop；
- v1 SE 不提供 pan。

停止当前所有 one-shot SE：

```eiyashou
se(none)
```

这个操作预计很少使用，但为严谨性保留。

当前 `Action::Effect { file: None, id: None }` 已有对应 stop-all-one-shots 行为。

---

# 23. Video

## 23.1 播放

```eiyashou
video(opening_movie)
```

默认：

```text
fullscreen
skippable: true
blocking
```

显式：

```eiyashou
video(
  opening_movie,
  skippable: false
)
```

---

## 23.2 收敛语义

v1 不支持 `loop`、`wait` 或 `video(none)` 参数/形式。播放始终阻塞剧情，直到视频自然
结束，或玩家在 `skippable: true` 时跳过。当前 core 的 `VideoSpec` 更宽；Native adapter
固定 lowering 为 `looped: false`、`wait_for_finished: true`、`VideoMode::Fullscreen`，不把
兼容层的非阻塞/循环能力泄漏进 v1。

---

# 24. 纯文本原则

Eiyashou v1 string 是纯文本。

不内置：

- HTML；
- Markdown；
- `[color]`；
- WebGAL markup；
- 自定义富文本标签语言。

未来需要富文本时，由 Engine 侧协作者扩展 DSL。

### 当前代码库对齐说明

审计基线 `Action::Say` runtime 仍会调用现有 `compile_rich_text(...)`，这是兼容层/现有 runtime presentation 行为。Eiyashou v1 要兑现“纯文本”，需要为 Native Say 提供 plain-text 标记/路径或可靠 escaping，不能让作者无意写出的符号被旧兼容 markup 解释。

---

# 25. 诊断规则

## 25.1 Error

至少包括：

- 未闭合字符串、括号、大括号；
- 缺少分隔逗号；
- 尾逗号；
- 未知角色；
- 未知/错误类型资源；
- 未知 scene；
- 未声明变量赋值；
- 变量可能未初始化；
- 同变量多处 `let`；
- 类型不一致；
- 非 bool condition；
- 参数数量/slot/type 错误；
- 重复命名参数；
- 资源路径越界；
- 负 list index；
- 静态可证明的 list 越界；
- 非法递归 `call`；
- 空调用栈可达的 `return`；
- 当前 DSL version 不支持的语法；
- unknown CST node 在语义编译阶段存在；
- 不支持的 capability 被实际使用。

有 compile/semantic error 时，不生成可运行 Program。

---

## 25.2 Warning

至少包括：

- 单 option choice；
- `remove(x)` 静态可证明找不到；
- 含 yield 的跨 scene `goto` cycle（无 yield cycle 是 error）；
- 未使用资源；
- 未知 YAML 扩展字段/namespace；
- 不可达 scene/statement（建议）；
- 视觉混淆 identifier。

warning 不阻止构建/运行。

当前 authoring IPC 的 wire-level diagnostic 只有：

```text
Warning
Error
```

因此“unused asset”之类低严重度信息在当前协议中可使用非阻塞 Warning，并由 Editor 视觉上弱化；以后协议若增加 Info/Hint，不改变 DSL 语义。

---

## 25.3 Runtime error

例如：

- 除零；
- int overflow；
- 非 finite float；
- 动态 list index 越界；
- 空 list `pop`；
- choice 运行时没有任何可见 option；
- `return` 时调用栈为空。

Eiyashou 语义要求这些错误停止当前正常剧情推进并提供可定位诊断，不能静默猜测结果后继续。

---

# 26. Source-preserving Editor 合同

推荐管线：

```text
原始字节
  -> token + trivia
  -> 无损 CST
  -> 诊断后的语义模型
  -> runtime Action / Program
```

CST 要保存：

- byte range；
- line/column；
- whitespace；
- comment；
- unknown node；
- 原始顺序。

Editor：

- Text View 和 Card View 修改同一个 `SourceDocument`；
- Card 只重写目标已知节点的 source range；
- 未知节点显示只读诊断卡，可跳回 Text View；
- formatter 必须保留 comment 与 unknown node；
- 保存不自动全文件格式化；
- 未修改文件打开再关闭，文件 hash 必须不变；
- 不建立隐藏 JSON sidecar 作为第二正文；
- 不从 runtime `Action` 重建源码。

当前 `DocumentManager` 对 `.shou`、`config.yaml` 与配置指定 manifests 统一提供 exact text、
revision、undo/redo、UTF-8、冲突安全原子保存与 recovery。Card View 直接投影 Loader 的
lossless source ranges；普通打开/保存不迁移，也不建立第二正文。

---

# 27. 显式迁移原则

普通打开/保存：

```text
绝不自动迁移源语法
```

需要升级时：

1. 用户显式执行 migration command；
2. 工具生成 diff preview；
3. 用户确认；
4. 才应用源文件变化。

不能静默升级或把兼容输入与 Eiyashou 做双向同步。

WebGAL 继续作为冻结兼容输入；LetsGal 继续只读。

---

# 28. Editor ↔ Engine IPC

项目 DSL 和 Editor IPC 是两层，不能混用。

审计基线 Phase 3 已实现：

- Postcard binary；
- 4-byte big-endian length prefix；
- 256 KiB message upper bound；
- protocol version 1；
- Engine child per project；
- Hello / OpenProject / Validate / lifecycle / Ping / Shutdown；
- capability handshake；
- structured diagnostics。

当前 capability enum：

```text
native_project
letsgal_project
webgal_project
validate
no_frame_preview
lifecycle
```

这与 Eiyashou“人类可维护文本”和 IPC“有界、版本化强类型二进制协议”的分层目标一致。

---

# 29. 当前代码库 lowering / 实现对齐表

这一节记录 Eiyashou v1 在当前集成树中的实际 owner 与完成状态。

| Eiyashou v1 能力 | 当前 main | 说明 |
|---|---|---|
| `.shou` parser / lossless CST | **已实现** | 独立 lexer/parser 保留所有 token/trivia/unknown byte range |
| `assets.yaml` 独立 manifest | **已实现** | 配置路径受 confinement 检查，资源 ID lowering 到既有 AssetMap |
| `characters.yaml` | **已实现** | 角色 ID 编译期解析为 name/color，presentation 贯穿 runtime/UI |
| `scene` -> Program scene | **已有 core 基础** | `Program` 已是 scene -> actions 映射 |
| `goto` | **可直接 lowering** | `Action::ChangeScene` |
| `call` | **可直接 lowering** | `Action::CallScene` |
| 提前 `return` | **已补齐** | `ReturnScene` 恢复最近 call frame；entry-aware validation 拒绝空栈可达路径，runtime 仍有 fail-closed error backstop |
| `loop` / `if` / branch block | **已实现** | lowering 使用 compiler-private Label/Jump，非 yielding cycle 编译期拒绝 |
| hidden `choice when` | **已实现** | 使用 Eiyashou typed expression evaluator，不经过 legacy truthiness |
| disabled choice | **core 有能力，v1 不暴露** | `Choice.enable_when` 已存在 |
| choice 空可见项 => runtime error | **已实现** | Eiyashou menu fail closed；兼容 adapter 行为不扩张 |
| dialogue -> Say | **可直接 lowering** | `Action::Say` |
| inline voice | **可直接 lowering** | `SayOptions.vocal` |
| stable source ID | **已实现** | 显式/确定性 ID 进入 dialogue、choice、read-history 与 rollback identity |
| `${...}` | **已实现** | typed interpolation；`/${` 仅转义字面 `${` |
| strict bool condition | **已实现** | 类型检查与 runtime 都拒绝 truthy coercion |
| `and/or/not` + short-circuit | **已实现** | dedicated typed evaluator 真正短路 |
| string + string only | **已实现** | 非 string 拼接为类型错误 |
| `/` real division | **已实现** | 结果为 Float，除零/非有限值 fail closed |
| `%` int-only | **已实现** | 编译期约束并有 runtime backstop |
| scientific notation | **已实现** | Native lexer 原生识别 `e` / `E` 指数 |
| `in` list membership | **已实现** | typed list membership |
| typed homogeneous list | **已实现** | 编译期同质约束，int/float 仅按明确 numeric promotion 归一化 |
| `append/remove/clear/insert/pop(..., into: target)` | **已实现** | typed statement IR；修改操作不进入 expression |
| `let` once-only initialization | **已实现** | declaration/type/stable-ID 分析覆盖合并后的项目级 native scene 集，跨 `.shou` 文件共享变量；definite-init 校验与 once-init runtime 语义 |
| background show/hide | **可直接 lowering** | `ShowBg` / `HideBg` |
| sprite show/replace | **可直接 lowering** | `ShowSprite` 替换同 ID state |
| hide(slot) | **可直接 lowering** | `HideSprite` |
| hide(*) | **可直接 lowering** | `HideSprites` 使用空 prefix 即可表达 all，或实现专用 lowering |
| move(slot, named anchor) | **已补齐** | 独立 `MoveSprite` 只更新现有 sprite 的 base Position，可瞬移或用 typed easing 阻塞动画，不重传 image/layout/scale |
| transition catalog | **已有** | 7 个，见第 18 节 |
| easing catalog | **已有** | 9 个，见第 20 节 |
| wait | **可直接 lowering** | `Action::Wait` |
| BGM play/stop/fade | **已实现** | typed Eiyashou BGM action carries volume/fade/playback mode |
| BGM `loop:false` | **已实现** | runtime uses one-shot/despawn playback mode |
| BGM crossfade | **已实现** | 旧 player fade-out 与新 player fade-in 并行存在 |
| SE one-shot / stop all | **已有** | `Action::Effect` 对应 one-shot / stop event |
| Video | **已有基础** | Native 固定 lowering 为 fullscreen、non-loop、blocking，只公开 `skippable` |
| plain-text Say | **已实现** | Eiyashou dialogue bypasses rich-text authoring semantics |
| no-auto source migration | **已实现** | 普通 open/save byte-preserving；显式迁移先预览再确认 |

### 实现原则

不要为了“少改 core”而改变已经确认的作者语法。

如果现有 `Action` 无法忠实表达 Eiyashou 语义，应：

1. 优先增加 typed、runtime-neutral core 表示；
2. 更新 compiled Program envelope/version 与必要的 persistence safety；
3. 让 WebGAL/LetsGal compatibility adapter 保持原合同；
4. Eiyashou compiler 做严格 type/flow/capability validation；
5. 绝不能把 legacy evaluator 的更宽松行为泄漏成 Eiyashou 语义。

---

# 30. Native expression 与 legacy evaluator 的边界

审计基线 `crates/core/src/runtime/expression.rs` 是兼容/runtime 既有 evaluator，不等同于本 DSL 规范。

它当前：

- token 使用 `&& || !`；
- 条件可 truthy；
- string 与非 string `+` 会 display 拼接；
- array 本身可异构；
- int exact division 可返回 Int；
- float `%` 可执行；
- 没有 `in`；
- 没有 Native list method；
- current binary evaluation 先解析右侧，因此不能直接承诺 Native `and/or` 的 short-circuit。

因此 Eiyashou implementation **不得直接把作者 expression 原样交给该 evaluator**。

可接受实现方向包括：

- Native parser 产生 typed expression AST，再扩展 core 为 typed expression IR；或
- Native compiler 将语义完整 lowering 到等价的 runtime representation。

无论内部选哪种，实现结果必须符合本文档，而不是 legacy evaluator 的偶然行为。

---

# 31. Choice block 的推荐内部 lowering

作者写：

```eiyashou
choice {
  "A": {
    x += 1,
    call(a)
  },
  "B": {
    x += 2
  }
},
"after"
```

当前 core `ChoiceTarget` 只能直接指向 Label / ChangeScene / CallScene。

Compiler 可以内部生成不可见 label：

```text
Menu -> __keine_choice_A / __keine_choice_B
__keine_choice_A:
  ...
  Jump __keine_choice_merge
__keine_choice_B:
  ...
  Jump __keine_choice_merge
__keine_choice_merge:
  ...
```

这些 label 是 compiler IR 细节，不进入作者语言，也不形成用户可依赖的稳定名称。

---

# 32. 完整项目示例

## 32.1 `config.yaml`

```yaml
title: "Example"

project:
  id: example-game

adapter:
  script: keine

script:
  version: 1
  entry: opening
  assets: assets.yaml
  characters: characters.yaml
```

---

## 32.2 `assets.yaml`

```yaml
backgrounds:
  day_school: assets/background/day_school.webp
  rooftop: assets/background/rooftop.webp

figures:
  rin_smile: assets/figure/rin/smile.webp
  rin_thinking: assets/figure/rin/thinking.webp

voices:
  ch01_001: assets/vocal/ch01/001.opus
  ch01_002: assets/vocal/ch01/002.opus

bgm:
  summer: assets/bgm/summer.opus

se:
  door: assets/se/door.opus

videos:
  opening_movie: assets/video/opening.mp4
```

---

## 32.3 `characters.yaml`

```yaml
characters:
  rin:
    name: "凛"
    color: "#86A8D8"

  china:
    name: "千夏"
```

---

## 32.4 `scripts/opening.shou`

```eiyashou
scene opening {
  let courage = 0,
  let inventory = list(string),

  bgm(summer, volume: 0.8, fade: 1s),
  background(day_school, transition: fade(300ms)),

  sprite(
    rin_stage,
    rin_smile,
    position: right,
    transition: fade(300ms),
    z: 10
  ),

  @opening_rin_001
  rin: "早上好。", ch01_001,

  "风从窗边吹进来。",

  rin: {
    @opening_rin_002
    "今天也很热呢。", ch01_002,

    @opening_rin_003
    "要不要去别的地方？"
  },

  choice("去哪里？") {
    @choice_rooftop
    "天台" when (courage >= 3): goto(rooftop),

    @choice_classroom
    "教室": {
      inventory.append("ticket"),
      call(classroom)
    }
  },

  goto(rooftop)
}

scene classroom {
  se(door),
  move(rin_stage, center, duration: 300ms, easing: ease_out),
  rin: "这里只待一会儿。",
  return
}

scene rooftop {
  background(rooftop, transition: dissolve(400ms)),

  if ("ticket" in inventory) {
    courage += 1
  },

  "风很大。",
  bgm(none, fade: 1s)
}
```

---

# 33. Loop 示例

```eiyashou
scene daily_hub {
  let day = 1,

  loop {
    choice("今天做什么？") {
      "去教室": call(classroom),
      "结束今天": break
    }
  },

  day += 1,
  goto(next_day)
}
```

---

# 34. Video 示例

阻塞过场：

```eiyashou
video(opening_movie)
```

不可跳过：

```eiyashou
video(opening_movie, skippable: false)
```

---

# 35. 非正式 EBNF 骨架

这部分描述语法形状；完整 parser 应以本文各节语义规则为准。

```text
file              := top_level*

top_level         := scene_decl
scene_decl         := "scene" identifier block

block             := "{" [statement ("," statement)*] "}"

statement         := dialogue
                   | narration
                   | choice_stmt
                   | if_stmt
                   | loop_stmt
                   | let_stmt
                   | assignment_stmt
                   | list_mutation_stmt
                   | command_stmt
                   | "return"
                   | "break"

dialogue          := [stable_id] identifier ":" string ["," voice_id]
                   | identifier ":" dialogue_block

dialogue_block    := "{" dialogue_entry ("," dialogue_entry)* "}"
dialogue_entry    := [stable_id] string ["," voice_id]

narration         := [stable_id] string ["," voice_id]
stable_id         := "@" identifier
voice_id          := identifier

choice_stmt       := "choice" ["(" string ")"]
                     "{" choice_entry ("," choice_entry)* "}"
choice_entry      := [stable_id] string ["when" "(" pure_expr ")"]
                     ":" (statement | block)

if_stmt           := "if" "(" pure_expr ")" block
                     { "else" "if" "(" pure_expr ")" block }
                     [ "else" block ]

loop_stmt         := "loop" block

let_stmt          := "let" identifier "=" initializer
initializer       := expr | list_type_init
list_type_init    := "list" "(" scalar_type ")"
scalar_type       := "bool" | "int" | "float" | "string"

assignment_stmt   := lvalue assignment_op expr
assignment_op     := "=" | "+=" | "-=" | "*=" | "/=" | "%="
lvalue            := identifier | identifier "[" expr "]"

list_mutation_stmt:= identifier "." (append_call | remove_call | clear_call | insert_call)
                   | "pop" "(" identifier ["," expr] "," "into" ":" identifier ")"

command_stmt      := identifier "(" [argument ("," argument)*] ")"
argument          := expr | identifier ":" expr

pure_expr         := expr_without_mutating_operations
```

### 解析注意

对白/旁白的 trailing voice 使用逗号，但不会与 statement separator 冲突：普通裸 identifier 本身不是合法 statement；在 dialogue/narration entry 后紧跟的单个 bare identifier 按 voice slot 解析，其后的逗号才继续分隔外层 statement/entry。

---

# 36. 保留 keyword / sentinel（v1）

语言关键字至少包括：

```text
scene
choice
when
if
else
loop
break
let
return
and
or
not
true
false
```

标准 command 名包括：

```text
background
sprite
hide
move
wait
goto
call
bgm
se
video
list
```

特殊 command-context token：

```text
none
*
```

其中：

- `none` 不是 null 值；
- `*` 不是一般 wildcard expression。

---

# 37. 明确不属于 v1 的内容

基础 v1 不包含：

- rich-text/markup 语言；
- dictionary/map；
- nested list；
- tuple；
- scene 参数/返回值；
- scene-local variable；
- while/for/continue；
- recursive call；
- bitwise operator；
- `++/--`；
- negative list index；
- string ordering；
- chained comparison；
- arbitrary numeric sprite coordinates；
- disabled-visible choice author语法；
- SE loop/pan；
- video loop/non-blocking playback；
- script-driven player settings；
- Live2D/Spine/GIF 专用 DSL 语法；
- runtime UI skinning DSL；
- 任意 JS/Python/Lua/Rhai/Rust 执行；
- implicit source migration；
- import system。

Engine 已经存在但未进入基础 DSL 的高级 runtime Action（camera/post-process/particle/stage timeline 等），由后续 Engine-side DSL collaborator 按真实需求扩展；不要为了“暴露全部 core enum”污染基础作者语法。

---

# 38. 实现完成标准（Eiyashou v1）

只有同时满足下面条件，才能声称 Eiyashou v1 已在代码层完成：

1. `keine` script adapter/loader 能递归读取 `.shou`；
2. `config.yaml` authoring schema、`assets.yaml`、`characters.yaml` 能被验证并 lowering；
3. lossless CST 保存 whitespace/comment/unknown nodes；
4. Text/Card 共用同一 `SourceDocument`；
5. strict type checker 实现本文表达式规则；
6. `let` once-init 与 definite initialization 正确；
7. list statement API、明确的 `pop(..., into: target)` 与 runtime error 行为正确；
8. choice hidden/filter/merge/empty error 正确；
9. `goto/call/return/loop/break` 精确符合本文；
10. stable ID 贯穿 translation/read-history/voice/source diagnostics 所需 identity；
11. background/sprite/hide/move/transition 与 runtime 语义对齐；
12. BGM `loop:false` 与真正 crossfade 补齐；
13. SE、阻塞非循环 Video 与 voice 行为对齐；
14. Native Say 保证 plain text；
15. Editor writable boundary 更新到 `.shou` / manifests；
16. authoring protocol 能报告 Eiyashou validation diagnostics；
17. compile error fail-closed，不跳过 unknown statement；
18. 普通 open/save 不迁移源码；
19. explicit migration 先 diff preview；
20. WebGAL frozen compatibility 与 LetsGal read-only 不被破坏；
21. compiled Program/Save schema 变化按现有 strict-version policy 正式升版，不做 best-effort legacy decode。

---

# 39. 给后续实现者的最终约束

- **不要重新设计本文已确认语法。**
- 遇到现有 core 不够表达时，补 typed runtime-neutral 表示，而不是把 authoring 语义偷偷改成 legacy evaluator 的行为。
- “当前 Engine 支持什么”必须从代码/capability 得出；Transition/Easing/media 等目录不得凭想象添加。
- Eiyashou 是 source-first；`Program` 是编译产物。
- Editor/Card View 是 source projection，不是第二正文。
- migration 永远显式。
- compatibility adapter 不成为 Eiyashou 的语法模板。
- v1 保持小而严格；高级镜头、滤镜、粒子、timeline、富文本等由后续 Engine-side collaborator 基于真实产品需求扩展。

---

## 审计涉及的主要当前代码路径

```text
crates/core/src/model/action.rs
crates/core/src/model/types.rs
crates/core/src/runtime/expression.rs
crates/core/src/runtime/step.rs
crates/core/src/config.rs
crates/loader/src/report.rs
crates/authoring-protocol/src/lib.rs
crates/editor/src/document.rs
crates/editor/src/app.rs
src/scene/audio.rs
src/storage/settings.rs
src/storage/read_history.rs
docs/project-and-assets-spec.md
docs/editor-phase2.md
docs/PROJECT_STATE.md
```

这份参考同时是冻结的 v1 语言合同和当前实现对齐记录。未来代码变化应更新“能力目录/实现
对齐”章节，但不能无声改写已经冻结的 v1 语言合同。
