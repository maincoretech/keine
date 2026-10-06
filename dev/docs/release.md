# 发布

## CLI

```text
validate <project>          配置、脚本、引用校验
dev <project> [--sync]      开发运行；sync 跟随 LetsGal 工程
migrate <source> <target>
                           只读源工程 → 新原生项目，验证后发布
bundle <project> --output <dir> [--benchmark]
                           编译、资源校验、hardened Engine 与完整发行目录
pack <project>             高级 Hakutaku 资源打包
remap ...                  高级资源引用迁移
perf <project> [--startup]  开发测量
```

`bundle --benchmark` 使用与 release 相同优化的 `profiling` 构建，保留符号，并关闭
阻止系统 profiler 附加的反调试措施；普通发行仍使用 hardened release。
测试包使用临时 identity。包内 `BENCHMARK.txt` 写明运行和采样步骤。

main 的推送通过 **CI** 后，**Release** 自动构建 Linux、macOS、Windows 测试包，
更新 [benchmark-latest](https://github.com/maincoretech/keine/releases/tag/benchmark-latest)
的三个下载。自动构建固定使用通过 CI 的提交、native-benchmark 与临时 identity；PR、fork、
失败的 CI 不发布，新 main 已出现时旧提交仅保留 Actions artifact，不覆盖滚动下载。
它不在 CI 主机上执行 GPU 性能验收，也不是正式游戏发行。
手动运行仍可选择项目/identity，并勾选 `benchmark` 更新相同测试包下载。
常规推送 CI 做编译与运行回归；完整 release 优化编译及打包在 Release /
Project Release 中执行，正式发行前必须核对所发行提交的这些结果。
“代码已推送”“CI 通过”“Release 下载已更新”是三个独立状态；下载更新完成须核对
Release 的提交 SHA 和三平台附件，不能仅以 CI 成功或 Actions artifact 上传作结论。
默认 native-benchmark 在三个平台使用同一套原生真实负载，包含媒体。
所选工程缺少必测时间轴时，测试返回 INCOMPLETE 并保留报告，不能当作完整跑分。
每个测试包包含 `profile-runtime.py`；直接运行 Engine 无需 Python，额外采集调用栈时
需要 Python 3 和平台采样工具。Windows 包附 PDB，macOS 有 dSYM 时一并附带。
工作流保存 Cargo 缓存前按 package 清理 Engine/Loader 的 release/profiling 产物，
避免内嵌运行密钥份额进入缓存；其他依赖和不含发行密钥的 publisher runner 保留。
缓存同时保留 `target/CACHEDIR.TAG`；若还原或独立 runner 先创建了 target 目录，
清理前补写标准标记，满足 Cargo 1.97 的显式目录清理检查。清理失败仍阻止缓存保存和发布。

Cargo aliases 与 executable 使用同名动词；具体参数以 `--help` 为准。
正式内容先验证/编译，再加载或创建 publisher identity；失败保留已有可运行发布目录。
identity 与内嵌 key 不记录、不提交、不缓存，也不传给无需它的 child build。

## 许可证与发行署名

Kēne 原创代码与文档采用 [Defold License 1.0](https://defold.com/license/)，
完整条款见根目录 `LICENSE`；允许商业发行游戏，限制将引擎/Editor 本身作为游戏引擎产品商业化。
游戏剧本和素材可使用作者自己的许可。第三方组件保持各自许可；提交贡献默认遵循 DL1，
不要求转让贡献者的著作权。Cargo 使用 `license-file` 声明这个非标准 SPDX 许可证。

用户仍按原流程导出/打包，保留输出中的 `LICENSE`、`NOTICE`、`FONT-LICENSES.txt` 即可；
macOS app 内放在 `Contents/Resources`。这些文件由打包器写入，不需要手动复制。
试玩导出不等于正式发行许可审查完成。

维护者每次升级或加入依赖时运行（CI 使用 cargo-deny 0.20.2）：

```sh
cargo deny --locked --all-features check advisories licenses bans sources
```

未知许可/来源与未固定 revision 的 Git 依赖会失败；上游重复版本和现有路径依赖的
通配声明仅警告。DL1 只在本工程六个 crate 上按原文哈希识别，不能误判为 Apache；
修改许可或允许列表必须核对完整上游条款，不能用宽泛 ignore 绕过。

正式发行前，维护者按实际目标平台与 features 补齐第三方依赖的完整版权/许可文本，
为 MPL 组件提供实际使用版本的源代码获取方式及必要的修改源码；`NOTICE` 是概览，
不是全部依赖的许可文本集合。外部 FFmpeg 的 LGPL/GPL 和 codec 义务由实际构建配置决定，
不能套用 Rust wrapper 的 WTFPL；目前 app 打包脚本不附带 FFmpeg。
`cargo-deny` 检查声明，不证明所有二进制与素材的发行义务已满足。

合成字体来源与完整 OFL/MIT 文本见 `src/assets/fonts/FONT-LICENSES.txt`。
Maven Pro 合成版内部名称已改为 Kene Text，字形与排版数据未变；CJK 来源由项目所有者确认
为 HanaMinAFDKO，作者仓库声明遵循 GlyphWiki 数据许可。已附 Mozilla 分发的 GlyphWiki 历史许可原文；
当前上游页面返回 403，未验证其当前版本。原始合成/图标转换配方和确切源版本未保存，
不声称已重建来源链。

## 密钥与更新

默认身份文件为 `<project>/.keine/publisher.key`，首次有效打包时生成，后续自动复用；旧 `.hakutaku-key` 名自动迁移并保留相同身份。`KEINE_HAKUTAKU_IDENTITY` 可指定工程外的身份路径。

身份文件包含签名私钥和内容根密钥，须私下备份，不随游戏发行。客户端仅嵌入验签公钥及根密钥的两份随机拆分材料；每次拆分变化不等于密钥轮换。客户端需要解密资源，因此不承诺抵抗逆向提取；拆分材料不含签名私钥。

发行步骤仍为 `cargo bundle <project> --output target/bundle/<game>`，macOS 沿用下方 app 打包脚本。更新继续使用同一身份与稳定 `project.id`；CI 继续将同一身份文件的 base64 放在 `HAKUTAKU_IDENTITY_BASE64` secret，无需用户手动处理每次生成的运行时材料。

显式轮换只在确有需要时进行：备份原身份，指定新身份并使用全新输出目录完整打包，分发配套新 Engine 和资源。当前客户端只信任一组密钥；新旧身份的程序/包不可任意混用。包允许合法版本回退，slot 仍要求剧本 fingerprint 匹配。

## 发行前的用户准备

1. 确认发行平台、剧情/媒体验收结果与稳定 `project.id`，交给维护者打包。
2. 首次有效打包生成 `.keine/publisher.key` 后私下备份；更新时继续使用同一文件。
   使用游戏仓库 CI 发行时，把该文件的 base64 放入 `HAKUTAKU_IDENTITY_BASE64` secret。
   运行时材料由打包器生成，无需手动拆分密钥。
3. 正式发行 macOS 版时，在发行机器的 Keychain 安装带私钥的
   **Developer ID Application** 证书，使用 `xcrun notarytool store-credentials <profile-name>`
   按本机提示私下保存 Apple 公证凭据。交给维护者证书名称与 profile 名称；密码和私钥不发到聊天或仓库。

`publisher.key` 用于游戏内容签名/加密；Apple 证书与 profile 用于 macOS 应用签名/公证，
两者分别保管。维护者负责媒体规范化、执行下方打包命令、检查公证/安装/更新结果。
正式公证前只具备开发包证据；完整分工和完成标准见 [验收分工](testing.md#剩余事项与分工)。

## macOS

```sh
cargo build --release --locked -p keine-editor --bin editor
cargo build --release --locked -p keine --bin keine --no-default-features --features audio-all,ui-sounds,video-native
bash dev/scripts/package-authoring-macos.sh target/release/editor target/release/keine /path/to/fresh-output
bash dev/scripts/bundle-macos.sh <project> <app-name> <reverse-dns-bundle-id>
```

第一条打包脚本使用预构建二进制，产出相邻 `Kēne Editor.app` / `Kēne Engine.app`。
目标必须是新目录；Engine discovery 与协议必须匹配。
Editor 导入图片无需外部工具；导入非规范音视频需要可执行的 FFmpeg（libopus/libx264），
可安装到 PATH 或放在 Editor executable 旁/Resources 内。当前打包脚本不附带 FFmpeg；
Engine 播放规范媒体不依赖这项导入工具。
两条 macOS 打包脚本默认使用 ad hoc 开发签名。正式发行可设置：

```sh
export KEINE_CODESIGN_IDENTITY="Developer ID Application: <name> (<team>)"
export KEINE_NOTARY_PROFILE="<notarytool Keychain profile>"
```

共用 `sign-macos.sh`：正式签名启用 hardened runtime 与 timestamp；有 profile 时提交
公证、staple/validate ticket 并执行 Gatekeeper assessment，全部成功后才安装新包。
profile 通过 `xcrun notarytool store-credentials` 私下存入 Keychain，不把密码/私钥写进工程。
仅设置签名证书时只签名；不设置两项时仍为开发包。实际正式公证需有效 Apple Developer
证书与凭据，本机尚未执行正式公证。依据 [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)。
验收包括 `codesign --verify --deep --strict`、ZIP 解压后启动、独立安装后 sibling discovery。

## Project CI

先本机创建稳定的 Hakutaku identity，将其 base64 放进游戏仓库 Actions secret
`HAKUTAKU_IDENTITY_BASE64`。同一发布 lineage 始终复用同一身份。

```yaml
name: Release
on:
  workflow_dispatch:
  push:
    tags: ["v*"]
permissions:
  contents: read
jobs:
  release:
    uses: maincoretech/keine/.github/workflows/project-release.yml@<40-character-keine-commit>
    with:
      project-path: .
      artifact-name: my-game-linux-x64
      retention-days: 14
    secrets:
      HAKUTAKU_IDENTITY_BASE64: ${{ secrets.HAKUTAKU_IDENTITY_BASE64 }}
```

完整 SHA 同时确定 workflow 与 Engine；branch/tag/短 SHA 被拒绝。
身份只在 runner 临时私有路径恢复，上传前无条件清理。
缓存只有 Cargo downloads 与 Kēne build trees，不含游戏 checkout、完整发行包或身份。
产物包含 executable、`game.haku`、`data/`、非秘密 provenance。
mutable ref、路径逃逸、缺失身份、非生产媒体、构建失败或未解析动态库均 fail closed，不上传半成品。

当前复用工作流面向 Linux x64；macOS/Windows 本机运行态验收不由它代替。
入口：[`project-release.yml`](../../.github/workflows/project-release.yml)、
[`publisher`](../../src/publisher.rs)、[`构建脚本`](../scripts/)。
