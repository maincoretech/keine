# Kēne Editor — Editor / File / Asset 分 Phase 实施合同

> **状态**：2026-09-21 产品问答收口版
> **用途**：交给 Codex / 后续实现者，作为 Kēne Editor 的 Card Editor、File、Asset、Inspector 与资源拖放的实施合同。
> **基线参考**：`docs/editor/questions/assets.md`、`docs/EIYASHOU_LANGUAGE_REFERENCE_v1.md`。
> **优先级**：本文“已决定”覆盖旧 QA 中与之冲突的建议或候选方向。不要重新讨论已经收口的产品选择。
> **重要**：本文只规定产品行为、数据合同、Phase 边界和验收条件；不包含此前临时讨论的 Rust 实现建议。

> **2026-09-23 后续决定**：此仓库副本在原文基础上同步用户后续确认；下载目录原件保留。界面只保留 `Text` / `Blocks`，旧称 `Card View` 在本文指当前 `Blocks`。Blocks 直接管理当前文件的 Scene，但不增加 Scene 页面或第二级 tab；高频参数仍在 Inspector。媒体转换后置，目前只允许合规格式进入，不为此增加新的长期状态。视觉保持暗色、统一、少文字、轻量过渡，性能优先。以下与这些决定冲突的旧条款均以后续决定为准。

---

# 0. 产品骨架

Kēne Editor 的主工作流始终是 **Editor**。`File` 与 `Asset` 位于左侧辅助槽位，`Inspector` 位于右侧并跟随当前选择。

```text
┌──────────────────────────────────────────────────────────────┐
│ Top Bar / Global actions                                     │
├───┬──────────────────┬───────────────────────────┬───────────┤
│   │ File / Asset     │          Editor           │ Inspector │
│ A │                  │                           │           │
│ c │ 同一左侧槽位切换 │ 始终为主要工作区域          │ 当前选择   │
│ t │                  │                           │ 的属性     │
│ i │                  │                           │           │
│ v │                  │                           │           │
│ i │                  │                           │           │
│ t │                  │                           │           │
│ y │                  │                           │           │
├───┴──────────────────┴───────────────────────────┴───────────┤
│ Problems / Tasks / Global Toast / global status              │
└──────────────────────────────────────────────────────────────┘
```

## 0.1 File

`File` 是 VS Code Explorer 风格的当前 workspace 文件树。

负责：

- 项目文件导航；
- 打开文件到 Editor；
- 新建文件 / 文件夹；
- 重命名；
- 移动；
- 复制 / 粘贴；
- 删除；
- 文件夹内拖动；
- Reveal 等普通文件操作；
- 接收来自 Finder / Explorer 等系统文件管理器的外部拖放。

不负责：

- 作为 VN 内容素材选择器；
- 把 File 中已有资源直接拖到内容 Editor 创建 VN Block。

## 0.2 Asset

`Asset` 是 `assets.yaml` 的可视化管理入口。

负责：

- 物理资源文件与逻辑 Asset ID 的 1:1 映射；
- Asset Type；
- Tags；
- 文件与逻辑引用之间的关系；
- 引用/健康状态；
- Asset 级重命名、Type 修改、删除、Remap；
- 作为向内容 Editor 投放 Asset 的唯一来源。

## 0.3 Editor

`Editor` 始终是主要创作区域。

负责：

- `.shou` Card View / Text View；
- Block 创建、编辑、选择、移动、复制、粘贴；
- Scene 的文件内组织；
- 接收 Asset 拖放；
- 显示 source-preserving 的 unknown/unsupported 节点。

## 0.4 Inspector

`Inspector` 跟随当前主选择。

- 选中 Asset → Asset 属性。
- 多选 Asset → common / mixed 属性与允许的批量编辑。
- 选中 Editor Block → 当前 Block 的结构化属性。
- 多选 Block → common / mixed 属性；不适合批量修改的字段隐藏或只读。
- Text Block 正文本身不在 Inspector 重复编辑。

---

# 1. 权威模型与不可破坏的边界

## 1.1 Source-first

Eiyashou 源文件和 manifests 是权威。

```text
.shou / config.yaml / assets.yaml / characters.yaml
```

Card View 是同一个 `SourceDocument` 的投影，不建立第二份正文，不从 runtime `Action` 反向重建作者源码。

## 1.2 `assets.yaml`

现有 v1 合同：

```yaml
backgrounds:
  day_school: assets/background/day_school.webp

figures:
  rin_smile: assets/figure/rin/smile.webp

voices:
  ch01_001: assets/vocal/ch01/001.opus

bgm:
  summer: assets/bgm/summer.opus

se:
  door: assets/se/door.opus

videos:
  opening_movie: assets/video/opening.mp4
```

资源 ID 按类型 namespace 独立。

本轮允许兼容扩展单条 Asset entry，以支持 Tags：

```yaml
figures:
  chinatsu: assets/figure/chinatsu.webp

  rina_summer:
    path: assets/figure/rina_summer.webp
    tags:
      - rina
      - summer
```

逻辑合同：

```text
AssetEntry =
    PathString
  | {
      path: PathString,
      tags?: Tag[]
    }
```

旧纯字符串写法必须继续有效。

## 1.3 不进入 `assets.yaml` 的派生信息

以下信息不作为持久 Asset metadata：

- 文件大小；
- 文件修改时间；
- 引用数量；
- 缩略图；
- 健康状态；
- 可重新计算的媒体信息。

这些可以实时或缓存计算，但缓存不是工程事实源。

## 1.4 Resource ↔ Asset 严格 1:1

```text
1 个物理资源文件
↕
1 条 assets.yaml 记录
↕
1 个逻辑 Asset ID
```

允许一个 Asset 被作品多处引用。

不允许同一个物理资源文件映射到多个 Asset ID。

## 1.5 自动登记范围

只有可作为资源使用的媒体文件进入 Asset 系统。

普通项目文件例如：

```text
.shou
.yaml
.md
```

不自动成为 Asset。

资源文件进入 workspace 时走自动规范化与登记流程；不存在用户手动 `Register Asset` 的主工作流。

---

# 2. 已覆盖 / 废止的旧 QA 方向

同时阅读旧 `docs/editor/questions/assets.md` 时，以下内容已被本轮产品决定覆盖：

1. **“文件存在但未登记”不再作为普通长期状态。**
   用户显式解除 Asset 映射时使用 `.unmapped`。

2. **“有损转换必须逐次展示参数与结果并确认”已废止。**
   当前策略是按既有默认规范化策略执行，不显示 Import Review。

3. **当前 Asset Browser“点击后打开并定位 Manifest”不是最终 UX。**
   最终由 Inspector 承担 Asset 属性查看与修改。

4. **Asset 不是独立中央工作区。**
   Editor 始终为主要工作区域。

5. **产品名称使用 `File`，不使用本轮早期误称的 `File Library`。**

6. **File 不是 Asset 投放入口。**
   File 中已有资源不能直接拖入内容 Editor；Asset 才是资源投放来源。

7. **不建立隐藏的“unregistered override”数据库。**
   用户保留但取消映射的文件通过 `.unmapped` 显式表示。

---

# 3. Phase 总览

| Phase | 目标 | 主要交付 | Gate |
|---|---|---|---|
| Phase 0 | 固定 source / manifest 合同 | `assets.yaml` 扩展、1:1 invariant、source-preserving 基线 | 老项目无损打开，新 schema 可 round-trip |
| Phase 1 | Card Editor 核心交互 | Scene section、Text Block、Block Picker、选择/移动/复制、Text↔Card 同步、Unknown Block | 不依赖 Asset 也能完整编辑 `.shou` |
| Phase 2 | File 与自动资源导入 | File 常规操作、外部资源规范化、自动登记、Toast/Tasks | 文件与资源进入 workspace 的路径稳定 |
| Phase 3 | Asset 浏览与查询 | Type/Folder/Unmapped、List/Grid、Search/Filter/Sort、多选 | 大小项目均可稳定查找与选择 |
| Phase 4 | Inspector 中的 Asset 管理 | Tags、引用、ID Rename、Type 修改、批量属性 | Asset 级编辑安全、结果可审查 |
| Phase 5 | Asset ↔ Editor 拖放 | 插入、替换、Voice 特例、多选插入、统一拖放动画 | Asset 使用工作流完整接入 Editor |
| Phase 6 | Delete / `.unmapped` / Remap | 删除、保留文件、系统回收站、Remap | 删除与恢复生命周期闭合 |
| Phase 7 | 集成验证与收口 | 回归、失败一致性、跨平台、大项目性能 | 不继续扩功能，只修合同偏差 |

---

# Phase 0 — Source / Manifest 合同

## 目标

先固定数据和 source-preserving 边界，避免后续 UI 反推数据层重写。

## P0.1 `assets.yaml` entry 扩展

支持：

```text
string path
OR
{ path, tags? }
```

要求：

- 旧纯字符串项目继续有效；
- 无实际 metadata 修改时，不因 Editor 打开/保存而批量迁移成 object form；
- namespace 继续决定 Asset Type；
- Asset ID 继续按类型拥有独立命名空间；
- path 继续使用项目内相对路径并遵守 confinement。

## P0.2 1:1 invariant

必须能够拒绝或诊断：

- 同一物理资源文件被多个 Asset 记录指向；
- Asset path 越出项目根；
- 半条 Asset 记录；
- Asset record 与实际资源状态不一致。

## P0.3 Source-preserving 基线

Card View / Text View：

- 共用一个 `SourceDocument`；
- 保留 whitespace / comment / unknown node / 原始顺序；
- 普通打开/保存不迁移整个文件；
- Card 只重写目标已知 source range；
- unknown syntax 不允许静默丢失。

## 完成标准

- 旧 `assets.yaml` 无损读取。
- 带 Tags 的新 entry 可 round-trip。
- `.shou` 普通打开/关闭不产生非必要改写。
- 后续 UI 不需要第二正文或 Editor 私有事实源。

## 本 Phase 禁止

- 不做 Asset UI。
- 不做拖放。
- 不顺手增加 `path/tags` 之外的新持久字段。
- 不引入通用插件/数据库框架。

---

# Phase 1 — Card Editor 核心交互

## 目标

在不依赖 Asset 面板的情况下，让 `.shou` Card Editor 成为可实际写作、移动、复制和切换 Text View 的主工作区。

## P1.1 文件级 Scene 组织

打开一个 `.shou` 文件时，Card Editor 连续显示该文件内全部 Scene：

```text
chapter01.shou

▼ scene morning
  [Block]
  [Block]

▼ scene rooftop
  [Block]
  [Block]

▶ scene evening
```

规则：

- Scene 是可折叠 section；
- 不新增“Scene 页面”或第二级文档 tab；
- File 打开什么 `.shou`，Editor 就展示这个文件；
- Scene 名仍属于全项目 namespace，不因为 UI section 改变语言语义。

## P1.2 Text Block

最高频写作路径：

```text
Enter
→ 产生当前插入位置的空 Text 编辑行
→ 直接输入
→ 默认 Narration
```

Text Block 统一承载：

```text
Text
Speaker: Narrator / Character
Voice
Stable ID
```

默认：

```text
Speaker = Narrator
```

修改 Speaker 后，同一个 Text Block 可以投影为角色 Dialogue。

Card 内直接编辑正文。

Inspector 不重复提供正文输入，只管理 Speaker / Voice / Stable ID 等结构化属性。

空 Text 编辑行是编辑交互，不得迫使源码写入无效 Eiyashou statement；没有有效内容时不得创造第二正文或不可恢复的假 source node。

## P1.3 Block Picker

在可插入位置按 `Tab` 打开完整 Block Picker。

Picker 是完整功能入口，不为了“轻量”牺牲能力。

支持：

- 分类浏览；
- 搜索；
- 键盘输入过滤；
- `↑ / ↓` 导航；
- `Tab` 补全；
- `Enter` 确认；
- 鼠标选择；
- Favorites；
- 类别顺序自定义；
- 类别内项目顺序自定义；
- 显示 / 隐藏。

这些自定义是 **全局 Editor 偏好**，不进入项目文件，也不按项目分别保存。

隐藏影响浏览布局，但不能破坏能力本身；命令仍可通过搜索等直接入口访问。

不要把 Block Picker 强行缩成只包含少量高频命令的小 popup。

## P1.4 Block 选择模型

使用标准桌面多选模型：

```text
Click       → 单选
Ctrl / Cmd  → 增减离散选择
Shift       → 连续范围选择
点击空白    → 清空选择
```

多选后允许：

- 删除；
- 复制；
- 移动；
- Inspector 显示 common / mixed 属性。

## P1.5 Block 移动 / 重排

已有 Block 支持直接拖动重排。

单选或多选 Block：

- 多选作为一个整体移动；
- 内部顺序按 **Editor 中原始顺序** 保持；
- 不按选择顺序重新排列。

拖放视觉语言与 Asset → Editor 插入共用同一套组件和动画：

```text
drag hover
→ 插入位置显示平滑淡入/淡出的 insertion line
→ 不做另一套 live-reflow 动画

drop
→ 一次完成目标位置的布局让位/落位
```

目标是只有一套明确动作，不为“移动 Block”和“插入 Asset”制造两种完全不同的运动语言。

## P1.6 Block 复制 / 粘贴

复制的是 Block 对应的 **完整源码节点**，不是只复制当前 Card 可见文字。

支持：

- 单 Block；
- 多 Block；
- 保持原顺序；
- 跨 Scene；
- 跨 `.shou` 文件。

粘贴不得无声丢失 Card 上未展开的合法属性。

显式 stable ID 必须继续满足全项目唯一；粘贴不得产生已知的重复显式 ID 后还宣称成功。

## P1.7 Card View ↔ Text View 精确同步

Card → Text：

```text
当前选中 Block
→ 对应 source range
→ 光标/viewport 定位
```

Text → Card：

```text
光标位于可识别节点
→ 选中对应 Block
→ 滚动到可见
```

如果光标位于：

- comment；
- 纯空白；
- Card 当前无法表达的 unknown syntax；

则尽量保持附近 viewport，不错误吸附到无关 Block。

## P1.8 Unknown / Unsupported Source

Card View 不允许跳过 unknown source。

显示为 **短小的只读错误 Block**，避免破坏编辑区简洁度。

示意：

```text
┌ Unsupported source ───────────────┐
│ Unsupported syntax · L84–L91  →   │
└───────────────────────────────────┘
```

要求：

- 不展开大段源码；
- 不占大面积；
- 有简短错误摘要；
- 可直接跳转对应 Text View / source range；
- 保持它在真实 Block 顺序中的位置；
- 不能在 Card View 中静默重写或删除。


## P1.9 Nested Structure：Choice / If / Loop

`choice`、`if / else if / else`、`loop` 等嵌套结构采用：

> **轻量结构 header + 缩进子 Block**

不使用厚重的大容器 Card，也不进入独立子页面。

示意：

```text
Choice: 怎么办？

  去天台
    [ Block ]
    [ Block ]

  回教室
    [ Block ]

If affection >= 3
  [ Block ]

Else
  [ Block ]

Loop
  [ Block ]
  [ Block ]
```

层级主要通过以下元素表达：

- 缩进；
- 细引导线；
- 分支标题；
- 轻量 header；
- 必要时折叠。

目标是保持长篇编辑时的纵向可读性，避免“大盒子套小盒子”。

## P1.10 非 Text Block 的编辑职责

除 Text Block 外，其他 Block 默认采用：

```text
Card       → 简洁摘要
Inspector  → 完整结构化参数
```

例如 Card 可以显示：

```text
[ Background · day_school ]
[ Sprite · rin_smile ]
[ Wait · 500ms ]
[ Goto · rooftop ]
[ BGM · summer ]
```

但完整参数，例如 transition、z、easing、volume、condition、target 等，统一由 Inspector 编辑。

不要为了“就地编辑方便”把大量参数重新塞回 Card。

Text Block 保持例外：正文直接在 Card 内编辑。

## P1.11 Scene 管理边界

Blocks 直接承担当前 `.shou` 文件内 Scene 的结构性管理。

Scene header 负责：

- 显示；
- 折叠 / 展开；
- 编辑 Scene 内部 Block；
- 新建、重命名、删除及文件内顺序调整。

入口保持轻量：常驻只显示一个图标入口，其余操作进 Scene 标题右键菜单；新建可从 Blocks 顶部的单个 `+` 图标进入。编辑名称只在操作时显示输入框，不增加常驻控制行。

Scene 名仍在全项目 namespace 内。重命名须检查全项目冲突并更新可确定的 `goto` / `call` 引用；无法安全定位时阻止。删除须确认并让剩余引用由正常诊断暴露。移动保持完整 Scene 源码节点及文件内顺序。

不为 Scene 再造独立页面、文档 tab 或重复的管理面板。


## 完成标准

- 一个 `.shou` 文件可在 Card View 连续编辑多个 Scene。
- Enter → 写旁白路径不需要打开 Picker。
- Tab Picker 完整可用并支持键盘工作流。
- Narration ↔ Dialogue 可通过 Speaker 改变。
- Block 标准多选、复制、移动可用。
- Card/Text 切换位置稳定。
- Unknown syntax 在 Card View 可见且不污染布局。
- Choice / If / Loop 以轻 header + 缩进子 Block 清晰表达嵌套。
- 非 Text Block 保持 Card 摘要 / Inspector 参数的统一职责。
- Blocks 可在当前文件直接完成 Scene 新建、重命名、删除和移动。

## 本 Phase 禁止

- 不建立第二正文。
- 不为 Scene 再造页面导航系统。
- 不为 Scene 建第二套页面导航或常驻按钮行。
- 不把 Text 正文重复放进 Inspector。
- 不为了 Picker 可定制性引入项目级配置。
- 不把 Unknown Block 做成大面积源码编辑器。
- 不把大量非 Text Block 参数塞回 Card。
- 不用厚重大容器 Card 表达嵌套结构。

---

# Phase 2 — File 与资源自动导入

## 目标

让 `File` 保持普通 workspace 文件树直觉，同时让可作为 Asset 的外部资源在进入 workspace 时自动规范化和登记。

## P2.1 File 基本行为

支持正常文件操作：

- 打开；
- 新建；
- 重命名；
- 移动；
- 复制 / 粘贴；
- 删除；
- 目录内拖动；
- Reveal。

资源文件在 File 中仍然是正常文件。

File 中已有资源文件 **不能直接拖到内容 Editor 创建 VN Block**。

## P2.2 外部文件拖入 File

来自 Finder / Explorer 等系统文件管理器：

### 普通非资源文件

```text
external file
→ normal copy into workspace
```

### 可作为 Asset 的资源文件

```text
external resource
→ existing default normalization
→ canonical output written into workspace
→ existing type rules
→ Asset ID from filename
→ assets.yaml auto registration
```

不先保留一份开发兼容原文件，不显示 Import Review。

## P2.3 默认有损规范化

沿用项目已存在的默认规范化策略。

产品合同：

- 允许默认有损规范化；
- 不逐次询问编码参数；
- Asset/File UX 不提供高级编码器参数；
- 规范化失败即导入失败并报错。

## P2.4 File 中的资源移动 / 重命名

普通 File 操作仍需支持资源文件。

当一个已映射资源文件通过 File 被重命名或移动时，Asset 映射必须保持一致，不能留下明知失效的 `assets.yaml` path。

不要把“File 是普通文件浏览器”理解成“可以无视 Asset 1:1 映射”。

## P2.5 错误与批量任务

单次错误：

```text
Global Toast
```

与 Engine Error / Editor Error 共用全局 Toast。

批量资源导入：

```text
Importing assets  37 / 80
```

使用一个聚合任务。

完成后只给一次汇总结果，例如：

```text
Imported 78 assets, 2 failed
```

不为每个文件刷 Toast。

## 完成标准

- 普通文件外部拖入 = 普通复制。
- 资源外部拖入 = 规范化 + 自动登记。
- 失败不产生有效 Asset 或悬空 manifest。
- File 内资源移动/重命名保持映射一致。
- 批量任务使用真实聚合进度。
- File 不成为 Editor 第二套素材入口。

## 本 Phase 禁止

- 不做 Import Review。
- 不新增编码参数配置。
- 不做手动 Register。
- 不开始 Asset 搜索 UI。

---

# Phase 3 — Asset 浏览、查询与多选

## 目标

实现左侧 Asset 管理面板本身，不把它抬成独立中央 workspace。

## P3.1 一级浏览

```text
[ Type ] [ Folder ]
```

并有独立：

```text
Unmapped
```

### Type View

无 Search / Filter 时，固定按类型分组，每组可折叠。

### Folder View

无 Search / Filter 时，按资源文件在 workspace 中的真实目录结构索引。

不创建虚拟 Bin。

### Unmapped

展示 `.unmapped` 资源文件。

File 中同一文件仍按普通文件显示。

## P3.2 List / Grid

```text
[ List ] [ Grid ]
```

Type / Folder 共用同一个 `view_mode`。

不要给两个视图复制状态。

## P3.3 Search

搜索字段：

```text
Asset ID
文件名
项目内相对路径
```

**Tags 不参与文本搜索。**

匹配：

- 按空格分词；
- 轻量 fuzzy；
- 多 token 共同缩小结果；
- 不扩展成项目全文搜索。

## P3.4 Filter

结构化条件包括：

- Type；
- Tags；
- 时间；
- 大小；
- 状态；
- 其他已有可靠索引能够提供的条件。

Search 与 Filter：

```text
AND
```

## P3.5 查询后的结果形态

只要 Search 或 Filter 生效：

```text
Type View   → 拍平
Folder View → 拍平
```

不保留 Type 分组，不保留 Folder 树。

拍平结果项不额外显示原始相对路径；需要路径时看 Inspector。

## P3.6 Sort

提供一个共用 Sort：

- Name；
- Imported time（仅在已有可靠数据时）；
- Modified time；
- Size。

Type / Folder 共用同一 Sort 状态。

## P3.7 共用查询模块

产品状态应当只有一套：

```text
search
filters
sort
view_mode
source_mode
```

Type / Folder 只改变无查询时的组织方式。

不要复制 Search / Filter / Sort / List/Grid 实现。

## P3.8 多选

标准桌面模型：

```text
Click       → 单选
Ctrl / Cmd  → 增减离散选择
Shift       → 连续范围选择
点击空白    → 清空选择
```

List / Grid 一致。

## 完成标准

- Type / Folder 无查询时分别体现类型和物理目录。
- Search / Filter 激活后统一拍平。
- Tags 只通过 Filter。
- Search + Filter 为 AND。
- Search / Filter / Sort / List/Grid 只存在一套状态。
- Asset 多选符合桌面习惯。

## 本 Phase 禁止

- 不做虚拟 Bin。
- 不做全文搜索。
- 不做独立中央 Asset workspace。
- 不给 Type/Folder 各造一套 query UI。

---

# Phase 4 — Inspector 中的 Asset 管理

## 目标

Asset 列表保持浏览/选择职责；属性和高风险修改集中到 Inspector。

## P4.1 单选 Asset

至少显示/管理：

- Asset ID；
- Type；
- Path / 对应文件；
- Tags；
- 状态；
- 引用；
- 当前已有且确实需要编辑的少量资源属性。

## P4.2 多选 Asset

Inspector 显示 common / mixed 属性。

适合批量修改的字段可以编辑。

Asset ID、Path 等天然单对象字段不做批量编辑。

### Tags

采用增量语义：

```text
Add tag    → 给所有选中 Asset 添加
Remove tag → 从所有选中 Asset 移除
```

不得覆盖每个 Asset 原有完整 Tag 集合。

## P4.3 引用展示

Inspector 直接显示文件 + 行号：

```text
opening.shou          L42, L118
chapter01/day01.shou  L16, L87
common.shou           L203
+7 more
```

规则：

- 可见条数按 Inspector 当前高度动态决定；
- 超出统一显示 `+N more`；
- hover `+N more` 显示完整剩余引用；
- 文件/行号可跳源码。

## P4.4 Asset ID Rename

一个界面一次完成，不再多弹确认：

```text
Rename Asset

Asset ID
old_id
→ new_id

☑ Rename file to match
assets/.../old_id.webp
→ assets/.../new_id.webp

References to update
opening.shou              L42, L118, L203
chapter01/day01.shou      L16, L87
```

规则：

- 同步重命名物理文件默认开启；
- 用户可关闭；
- 引用预览使用“文件 + 行号”紧凑形式；
- 行号过多可显示 `+N`，hover 完整展开；
- 一次 Rename 提交同步更新已知脚本引用；
- ID 或目标文件冲突时直接阻止；
- 不自动生成 `_2`。

## P4.5 修改 Type

修改 Type 时：

```text
namespace 改变
+ 物理文件移动到该类型规范目录
+ assets.yaml path 更新
+ 重新验证引用
```

目标目录已有同名文件：

```text
→ 直接阻止
```

不自动改名、不覆盖、不弹冲突策略菜单。

## 完成标准

- Asset 属性集中于 Inspector。
- 多选 Tags 增量编辑正确。
- 引用可快速看到文件与具体行号。
- Rename 同界面预览文件名和引用影响。
- Type 变更同步物理移动。
- 所有目标冲突 fail closed。

## 本 Phase 禁止

- 不在 Asset 列表内复制一套 Inspector。
- 不加入复杂批处理语言。
- 不自动解决冲突。
- 不额外弹第二个 Rename 确认窗。

---

# Phase 5 — Asset ↔ Editor 拖放与统一动画

## 目标

让 Asset 成为内容 Editor 唯一资源投放来源，并让 Asset 插入、Asset replacement、Block reorder 使用一致的拖放语言。

## P5.1 统一 insertion / reorder 动画

拖动 Asset 或已有 Block 到可插入位置：

```text
[ Block A ]

──────────────  ← insertion line

[ Block B ]
```

规则：

- insertion line 平滑淡入 / 淡出；
- hover 不使用另一套实时 reflow 动画；
- 不提前大幅撑开布局；
- Drop 后以一次布局动作完成落位和周围 Block 让位；
- 新建 Block 内容可以随落位淡入；
- Block reorder 和 Asset insert 共用这套交互组件，不维护两种运动语言。

## P5.2 单 Asset 插入

可独立成为 statement 的 Asset 在有效插入点生成对应 Block，例如：

```text
Background → Background Block
Figure     → Sprite/Figure Block
BGM        → BGM Block
SE         → SE Block
Video      → Video Block
```

具体默认参数沿用 Eiyashou / Editor 已有规则，不由 Asset 模块重新定义语言。

## P5.3 Voice 特例

Voice 只能拖到已有 Text Block（Narration / Dialogue）上。

```text
Voice Asset
→ Text Block voice slot
```

不能：

- 在普通插入点生成独立 Voice Block；
- 自动创建空 Dialogue / Narration。

## P5.4 单 Asset replacement

单个 Asset 拖到兼容 Block 本体：

```text
compatible Block
→ replacement hover
→ Drop
→ 只替换 Asset 引用
```

保留原 Block 的其他属性，例如：

- slot；
- position；
- transition；
- z；
- 其他使用属性。

不兼容类型：

```text
→ 不进入 replacement 状态
→ Drop 无效
```

不自动转换 Block 类型。

## P5.5 Asset 多选拖入

Asset 面板多选可以一次拖入 Editor。

顺序：

```text
按用户选择顺序
```

不是当前 Sort 顺序。

规则：

- 多选只允许连续插入；
- 多选不允许 replacement；
- 一次 drag 只显示一个 insertion line；
- 整组必须在目标位置全部合法；
- 任一项不合法 → 整组无效；
- 不跳过无效项；
- 不部分插入；
- 无效时只显示不可放置状态，不额外解释具体哪一项失败。

## P5.6 Existing Block reorder

单个或多选已有 Block 可以拖动重排。

- 多选按 Editor 原始顺序整体移动；
- 使用 P5.1 同一 insertion/reorder 动画；
- Drop 后完成新的 source order；
- 不建立独立的“Block move 动画系统”。

## 完成标准

- File 不能作为资源投放入口。
- Asset 插入、Block reorder 使用一致的拖放反馈。
- Voice 只能进入 Text Block。
- compatible replacement 只改 Asset 引用。
- Asset multi-drag 按选择顺序且 all-or-nothing。
- Block multi-reorder 保持原始 Block 顺序。

## 本 Phase 禁止

- 不自动转换不兼容 Block。
- 不给 Asset 与 Block 各造一套 DnD 框架。
- 不为 multi-drag 增加排序确认。
- 不让 File 与 Asset 成为两个等价素材入口。

---

# Phase 6 — Delete / `.unmapped` / Remap

## 目标

闭合 Asset 删除、保留物理文件、物理删除与恢复生命周期。

## P6.1 Delete Asset

删除 Asset 时提供：

```text
☐ Also delete file
```

默认 **不删除物理文件**。

### 默认：保留文件

```text
assets/figure/chinatsu.webp
→ assets/figure/chinatsu.webp.unmapped

remove assets.yaml entry
```

`.unmapped` 显式表示“当前不参与 Asset 映射”。

### 勾选：删除文件

物理文件使用系统可恢复删除：

```text
Trash / Recycle Bin
```

如果当前环境不能执行可恢复删除：

```text
→ 删除失败
```

不得静默降级为永久删除。

## P6.2 有引用仍允许删除

Asset 即使仍被脚本引用，也允许删除。

删除窗口不承担引用统计/修复流程。

删除后：

```text
原引用
→ 正常 unresolved-asset diagnostics
→ Problems / 编译诊断暴露
```

## P6.3 `Asset > Unmapped`

显示 `.unmapped` 资源文件。

File 中这些文件仍正常显示。

## P6.4 Remap

```text
file.ext.unmapped
→ Remap
→ 去掉 .unmapped
→ 按现有类型规则恢复 Type
→ 从文件名生成 Asset ID
→ 写回 assets.yaml
```

正常情况一键执行，不开 Remap Review。

ID 冲突：

```text
→ 直接阻止
```

不自动 `_2`，不临时弹改名输入。

## P6.5 删除 Unmapped 文件

`Asset > Unmapped` 允许直接删除物理文件。

与 File 的物理删除使用相同的可恢复删除行为。

## 完成标准

- Delete Asset 默认保留文件并加 `.unmapped`。
- `.unmapped` 不进入正常 Asset。
- Remap 一键恢复。
- Remap 冲突 fail closed。
- 有引用 Asset 可删除，之后由正常诊断暴露。
- 物理删除无永久删除 fallback。

## 本 Phase 禁止

- 不建立隐藏 ignore/unregistered 数据库。
- 不在 Delete 流程自动修复脚本引用。
- 不自动生成冲突后缀。
- 不建立项目私有回收站来替代已决定的系统可恢复删除。

---

# Phase 7 — 集成验证与收口

## 目标

不增加产品功能，只验证 Phase 0–6 组合后是否忠实实现合同。

## P7.1 Card Editor 主流程

验证：

```text
File 打开 .shou
→ Card View 显示文件内所有 Scene
→ Enter 新 Text
→ 直接输入 Narration
→ Inspector 改 Speaker 变 Dialogue
→ Tab Block Picker 添加其他 Block
→ 多选 / copy / paste / reorder
→ Card ↔ Text 精确同步
```

验证 unknown syntax：

```text
unknown source
→ 短错误 Block
→ 不丢 source
→ 可跳 Text View
```


验证嵌套结构：

```text
Choice / If / Loop
→ 轻 header
→ 缩进子 Block
→ 分支层级清晰
→ 不出现大容器套娃
```

验证非 Text Block：

```text
Card → 摘要
Inspector → 完整参数
```

验证 Scene 管理边界：

```text
Blocks
→ 显示 / 折叠 Scene
→ 标题菜单管理 rename/delete/move
→ 轻量新建入口
→ 不增加第二 Scene 页面或独立 tab
```

## P7.2 Asset 导入主流程

```text
external resource
→ File
→ normalize
→ auto-register
→ Asset 可见
→ Inspector 可编辑
→ 可拖入 Editor
```

## P7.3 Rename

```text
Asset ID rename
→ 同界面预览物理文件与引用行号
→ physical rename 默认开
→ update assets.yaml
→ update source references
```

## P7.4 Type 修改

```text
Type change
→ conflict check
→ physical move
→ namespace/path update
→ validation
```

## P7.5 Delete / Remap

```text
Delete Asset
→ keep file(default)
→ .unmapped
→ Asset > Unmapped
→ Remap
```

以及：

```text
Delete physical file
→ system Trash / Recycle Bin
→ no permanent fallback
```

## P7.6 Editor 拖放

验证：

```text
single Asset insert
single compatible replacement
Voice → Text Block
Asset multi-insert in selection order
invalid multi set → reject all
existing Block single/multi reorder
```

全部使用同一 insertion/reorder 视觉语言。

## P7.7 大项目行为

验证：

- Asset 首屏可用性；
- 缩略图/派生信息按需加载；
- Search / Filter / Sort 不因 Type/Folder 切换复制状态；
- 查询拍平结果不卡顿；
- Inspector 大量引用仍保持紧凑；
- 长 `.shou` 中 Scene 折叠、Card/Text 定位可用；
- Block Picker 在项目变大后仍保持快速；
- 缓存失效不影响工程事实。

## P7.8 失败一致性

模拟：

- normalize 失败；
- 文件不可写；
- manifest 保存失败；
- source 写回失败；
- ID 冲突；
- Type move 冲突；
- `.unmapped` rename 失败；
- Remap ID 冲突；
- Trash 不可用；
- paste stable-ID 冲突；
- Block reorder 写回失败。

要求：

- 不留下半完成 manifest；
- 不丢 source；
- 不生成假成功 Toast；
- 不静默永久删除；
- 不自动造新 ID / 文件名绕过冲突。

## P7.9 跨平台

至少验证目标平台上的：

- 文件路径；
- 文件重命名 / 移动；
- 外部拖放；
- 系统可恢复删除；
- canonical media 导入；
- symlink / confinement；
- 全局快捷键差异（Ctrl / Cmd）。

## 完成标准

- 本文所有“决定”可通过真实 UI 操作完成。
- File / Asset / Editor / Inspector 职责没有重新混淆。
- 老项目仍可打开。
- Source/manifest 始终是权威。
- 常见失败不会产生半完成状态。
- 没有为了本功能顺手引入无关框架。

---

# 4. 产品合同速查

## Editor

```text
主工作区始终是 Editor

.shou:
一个文件页连续显示全部 Scene
Scene 可折叠
Blocks 直接管理当前文件的 Scene，不建第二页面

Enter:
空 Text 行
直接输入 = Narration

Text Block:
正文 Card 内编辑
Speaker 改为角色 = Dialogue
Voice / Stable ID 等结构化属性 → Inspector

非 Text Block:
Card 只显示摘要
完整参数 → Inspector

Choice / If / Loop:
轻 header + 缩进子 Block
不用厚重大容器 Card

Tab:
完整 Block Picker
支持搜索 / ↑↓ / Tab 补全 / Enter
支持 Favorites / 顺序 / 显示隐藏
自定义为全局 Editor 偏好

Block:
标准桌面多选
支持完整节点 copy/paste
支持拖动 reorder

Card ↔ Text:
精确同步 source range

Unknown source:
短只读错误 Block
可跳 Text View
不展开大段源码
```

## File

```text
workspace 文件树
打开/编辑文件
正常文件操作
external ordinary file → 普通复制
external resource → 默认规范化 + 自动 Asset 登记
File 内资源不能直接拖到内容 Editor
```

## Asset

```text
assets.yaml 可视化管理
严格 file ↔ asset 1:1

Type / Folder / Unmapped
List / Grid
Search / Filter / Sort 共用
标准桌面多选
Asset 是资源拖入 Editor 的唯一入口
```

## Search / Filter

```text
Search:
Asset ID
filename
relative path

不搜 Tags
分词 + 轻量 fuzzy

Filter:
Type
Tags
time
size
status
...

Search AND Filter

有 Search / Filter:
Type → 拍平
Folder → 拍平
```

## Inspector

```text
Asset single → Asset 属性
Asset multi → common/mixed + batch tags add/remove

Block single → Block 结构化属性
Block multi → common/mixed

Text 正文不在 Inspector 重复编辑

References:
file + line numbers
动态可见数量
+N more hover 完整显示
```

## Drag / Drop

```text
统一一套 insertion/reorder 动画

hover:
insertion line fade

drop:
一次布局落位 / 让位
新 Block 可淡入

single compatible Asset → replace asset reference only
incompatible → reject

Voice → Text Block only

Asset multi:
insert only
selection order
all-or-nothing

Block multi reorder:
按 Editor 原始顺序整体移动
```

## Rename / Type

```text
Rename Asset:
一个界面
默认同步文件名
可关闭
显示 reference file + line
一次提交更新引用
冲突直接阻止

Change Type:
namespace change
physical file move
path update
冲突直接阻止
```

## Delete / Unmapped

```text
Delete Asset:
默认 keep physical file
→ file.ext.unmapped

optional delete:
→ system Trash / Recycle Bin
→ unavailable = fail

有引用仍可删除
之后由正常 diagnostics 暴露

Unmapped:
Asset 中可见
File 中正常显示
Remap 一键恢复
ID conflict = block
```

---

# 5. 明确非目标

本轮不做：

- Asset 独立中央 workspace；
- DaVinci page 式整体模式切换；
- 虚拟 Bin；
- 云 Asset 库；
- 协作系统；
- 插件市场；
- 通用插件框架；
- 主题系统；
- 手动 Register Asset 主流程；
- Editor 私有数据库成为工程事实源；
- Asset 搜索变成项目全文搜索；
- Import Review；
- 每次有损转换确认；
- 编码器高级参数 UI；
- 自动文件名冲突修复；
- 删除 Asset 时自动修复全部引用；
- 为 Type/Folder 各实现一套 Search/Filter/Sort；
- 为 Asset insert 和 Block reorder 各实现一套不同动画系统；
- 为每个项目保存 Block Picker 个性化顺序；
- Scene 再套一层页面/tab 导航；
- 为 Scene 增加第二页面、第二级 tab 或常驻按钮行；
- 为 Choice / If / Loop 制作厚重大容器 Card；
- 在非 Text Block Card 内堆叠完整参数编辑器；
- 在 Card View 内做大型 unknown-source 编辑器。

---

# 6. Codex 执行约束

1. **一次只实现当前 Phase。** 当前 Phase 未通过验收，不提前进入下一 Phase。
2. **不要重新设计已决定的产品合同。** 代码现状冲突时先报告具体冲突，再做最小必要修改。
3. **不要为了未来可能需求引入通用框架。**
4. **优先复用已有基础设施。** 包括 SourceDocument / DocumentManager、Inspector、Problems、Tasks、Global Toast、现有规范化链路。
5. **File / Asset / Editor 职责不得重新混淆。**
6. **Type / Folder 不复制查询模块。**
7. **Asset insert / Block reorder 共用一套拖放视觉与基础交互。**
8. **任何顺手重构必须是当前 Phase 的必要依赖。**
9. **遇到阻塞先报告事实，不通过扩大功能范围绕过。**
10. **每个 Phase 完成后只汇报：已完成、未完成、真实阻塞、验证结果。不要自动开始下一 Phase。**
11. **source / manifests 始终是权威。** 缓存、缩略图、索引、Card projection 不能成为第二正文。
12. **不要把本轮临时 Rust 实现讨论写入产品合同或实现目标。**
13. **不要因为某个边缘场景停留过久。** 如果它不阻塞当前 Phase 的核心合同，记录为待验证项并继续完成主路径。
14. **实现前先对照当前 Phase 的“本 Phase 禁止”与“完成标准”。** 不满足完成标准不能声称 Phase 完成。
15. **嵌套结构不要用大容器套娃。** `Choice / If / Loop` 按轻 header + 缩进子 Block 实现。
16. **非 Text Block 的完整参数统一进入 Inspector。** Card 只承担摘要，不要自行扩成参数表。
17. **Blocks 直接管理当前文件的 Scene。** 入口保持轻量，不能扩成第二套 Scene 页面或重复管理面板。

---

# 7. 严格实施顺序

```text
Phase 0  Source / Manifest 合同
   ↓
Phase 1  Card Editor 核心交互
   ↓
Phase 2  File 与自动资源导入
   ↓
Phase 3  Asset 浏览与查询
   ↓
Phase 4  Inspector Asset 管理
   ↓
Phase 5  Asset ↔ Editor 拖放
   ↓
Phase 6  Delete / Unmapped / Remap
   ↓
Phase 7  集成验证与收口
```

Phase 0 是数据与 source gate。

Phase 1 先让 Editor 本身成立；不要等 Asset 才能测试 Card Editor。

Phase 2 建立资源进入 workspace 的唯一可靠路径。

Phase 3–4 完成 Asset 自身的浏览与管理。

Phase 5 才把 Asset 和已经稳定的 Editor 拖放连接起来。

Phase 6 闭合删除/恢复生命周期。

Phase 7 只验证、修合同偏差和性能问题，不继续扩产品范围。

如果某 Phase 出现“必须先实现后续 Phase 才能继续”的情况，先检查 Phase 边界是否被实现者写反，不要直接跨 Phase 开工。
