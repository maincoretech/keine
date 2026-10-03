# 兼容输入

原生 Eiyashou 是当前作者格式；兼容输入通过 Loader adapter 生成同一个 typed Program。
不会将兼容 JSON、WebGAL wrapper 或第三方宿主扩展直接交给 Engine。

## LetsGal

- 检测 Studio project，读取 chapters/scenes/characters、资源清单与默认壳配置。
- 已接入舞台 track/key/event、镜头、场景层/分层立绘、皮肤/视口高度、对焦、音频和退格。
- 时间轴共用一个时钟；camera、character、sceneLayer 与 79 个 StageProperty 的回归在 fixtures。
- 支持跨 fragment/章节调用、waitForInput、repeat/muted/playbackRate 与已映射默认壳行为。
- 完整官方示例是忽略的本机内容，不提交商业资源。
- Editor 中兼容 JSON 只读；`migrate` 输出原生清单/脚本，转换失败不发布目标。
- 迁移按源文件分组输出 `.shou`，保留文件名与子目录；同源文件的多个 fragment 仍放在一起。LetsGal `project.json` 生成的入口/调度放在小型 `main.shou`，重名路径加数字后缀。跨文件 `goto` / `call` 仍按全项目 scene ID 连接，初始变量放在入口文件。
- 对象 ID 重命名后通过 `objects.yaml` 恢复引擎分组；粒子纹理进资源清单。
- 场景背景按源资源分类进入 `backgrounds`；分层绘制仍使用 `sprite`，不因此改成立绘资源。同一分类下的源文件只复制一次。
- 资源文件保留源文件名与分类内子目录，引用 ID 独立编号；大小写重名加短数字后缀，不覆盖。后续格式转换仅改变扩展名。
- 重置镜头迁移为一条 `camera.reset(all)`，保留动画时长、缓动与等待；不展开默认特效字段。
- tweenFields、blocking 时序及已支持的随机度必须保留，原子镜头动作仍对应一个 Block。

宿主仅提供已注册能力；未知/缺失能力明确诊断，不设计新的 Studio/Electron 扩展。
支持范围以 [`letsgal adapter`](../../crates/loader/src/adapter/editor/letsgal.rs) 与回归输入为准。

## WebGAL（冻结）

保持现有兼容行为，只修安全、崩溃、数据丢失和 Kēne 引入的明确回归。
不新增 WebGAL semantics 或继续追求版本 parity。

```text
现有合同
├── say / changeBg / changeFigure / choose / setVar / callScene / changeScene
├── 音视频、转场、动画/变换的已实现 Rust 子集
├── -when：安全表达式子集；-next：现有非阻塞 Flow
└── 未知命令：已有对白 fallback；保留字可产生 unsupported warning
```

已知边界：不执行任意 JavaScript/对象表达式；不完整支持 `-continue`、CSS/React 模板、
自定义 WebGAL animation table、所有 easing/filter、Steam 桥接和上游输入验证参数。
`showVars`、`applyStyle`、`callSteam` 不伪造成功。
资源图像与格式通过 Kēne 的现有媒体路径；解析成功不等于视觉或听觉完全等价。

保留已知差异：重复参数本地以后项覆盖；标签取最后定义；gallery 不保存完整 series 元数据。
`setFilter` 属于已有 Kēne 行为，不计上游 parity；不根据漂移的上游文档猜新别名。
实现与测试入口：[`webgal`](../../crates/loader/src/adapter/script/webgal.rs)、
[`coverage`](../../tests/coverage.rs)、[`fixtures`](../../tests/fixtures/webgal-showcase/)。
