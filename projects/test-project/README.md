# Kēne LetsGal 1.9.9 Feature Lab

默认验收工程现已收敛为一个原生 LetsGal 1.9.9 项目，直接覆盖 Kēne 已接入的完整时间轴、
立绘皮肤、视口相对高度、玩家确认等待、跨章节调用与播放控制。它不依赖 LetsGal Studio、
扩展、注入或 `dev --sync`。

```bash
cargo validate projects/test-project
cargo dev projects/test-project
```

进入标题页后点击 `START`，按画面中的 `10-00` 至 `10-10` 编号逐项验收。详细预期见
[ACCEPTANCE.md](ACCEPTANCE.md)。

## 覆盖范围

- 79 个已接入 StageProperty；
- camera、character、sceneLayer 三类目标；
- camera shake、camera patch、particle、scene、audio 五类时间事件；
- BGM/SE/VOCAL 时间轴路由、muted、持续时间与淡入淡出；
- `waitForInput` 的显式玩家确认等待；
- `chapterFolders` / `chapterTreeOrder` 虚拟目录执行顺序；
- 默认壳 `dialogueBehavior` 的打字间隔、字符淡入和文字出现效果；
- 角色皮肤属性、block 锁定皮肤和全局/表情级视口高度；
- `updateCharacter` 在场换表情、换位缓动及缺席不入场语义；
- 跨章节 fragment 调用与返回；
- muted、repeat、playbackRate、blocking；
- 原生句尾退格、连续退格和删完后等待一次新点击；
- linear、ease-in、ease-out、ease-in-out 四种插值；
- 共享时间轴上的变换、传统镜头、光学、模糊、环境、复古与遮罩效果；
- 1920×1080 设计分辨率和 16:9 视口裁切。

WebGAL 命令覆盖脚本已移至 `tests/fixtures/webgal-showcase/`，仅作为 parser/IR 自动化
回归输入，不再作为第二套可运行测试工程。这样默认测试入口只有一个，也不会重复保存背景、
音视频和运行时存档。

## Portable benchmark 场景

同一章节还包含不可从正常剧情到达的 `fragment-benchmark-journey`。它只供 benchmark
运行时直接重建，复用正式验收资源组成普通背景、单人立绘、姓名框和 textbox，并分别测量
对白构图、一次立绘移动和常见背景交叉淡入淡出。portable 报告随后逐项运行 `10-01` 至
`10-08`，覆盖全部 79 个 StageProperty 和五类时间事件，最后运行一个明确标记的组合极限场景。
日常、全能力覆盖和极限结果分区展示；相机拆分仍只属于开发诊断。

## 资源

项目只保留时间轴验收实际引用的校准资源：两张 1920×1080 背景、同一角色的两张透明立绘
与一条 Opus 时间轴提示音。资源逻辑名统一记录于 `assets/.manifest.json`，没有 WebGAL
示例资源或生成存档。
