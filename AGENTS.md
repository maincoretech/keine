# Kēne 行动指南

> 本指南对整个项目生命周期有效，适用于本仓库的每一次改动，
> 无论任务大小。所有会话开始工作时默认遵守。

## 代码质量原则

1. **写优雅的代码。** 结构清晰、命名准确、职责单一。避免散落的 `unwrap`、
   魔法数字和重复逻辑；布局、格式、边界这类有单一事实的代码收敛到一个类型
   或函数里，让 schema 演进只改一处。
2. **积极地反思结构。** 动手前先想清模块边界与公共 API 形状；实现后回头
   审视是否出现了更干净的形态（类型收敛、错误路径统一、职责边界正确）。
   发现更好的结构就重构后再提交，不要带着已知的结构问题交付。
3. **上网查证再定稿。** 对设计决策（序列化格式、二进制 envelope 惯例、依赖
   行为、错误处理、性能假设）先查官方文档、issue 与社区实践，确认方案是否
   合适再实现。查证结论应体现为代码注释或提交说明，不能只停留在口头。

## 完成标准

每次改动提交前必须全部通过：

```text
cargo check --workspace
cargo clippy --workspace --all-targets        # 0 warning / 0 error
cargo fmt --all --check                        # 干净
cargo test --workspace                         # 全过
cargo validate projects/test-project           # 涉及项目加载/适配器时
```

涉及性能的改动还必须带前后基准（`cargo bench --workspace` 或 `cargo perf`），并把结果写进
`dev/docs/performance-baseline.md`。

## 提交约定

- 本项目直接提交 `main`，不走 PR。
- 每次提交完成后立即推送到 `origin/main`（`git push`），不积压本地提交。
- 一个 commit 只解决一个热点；提交信息说明改了什么、为什么、验证结果。
- 工作树有无关改动时先 `git status` 核对范围，不顺手提交无关文件。
