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

## 三个独立下载渠道

| 用途 | 下载标签 | 触发方式 | 内容 |
| --- | --- | --- | --- |
| 性能定位 | `benchmark-latest` | main 通过 CI 后自动；也可手动 | 三平台自运行真实负载、报告与 profiler 工具 |
| 作者开发 | `editor-latest` | main 通过 CI 后自动；也可手动 | 三平台独立 Editor + 同版本 Preview Engine，无游戏素材 |
| tday 试玩 | `tday-latest` | **仅手动** | 三平台 ZIP + Android ARM64 APK，含 shiftz 素材版权说明 |

三个渠道都是预发布，互不覆盖。Editor 和 tday 使用 release 优化；benchmark 使用
同样优化但保留符号的 profiling。macOS 当前 runner 产出 Apple Silicon，Linux/Windows
为 x64；不声称已经覆盖 Intel Mac。macOS Editor 下载是一个 `Kēne Editor.app`，内部带
Preview Engine；Windows/Linux 是相邻的 Editor/Engine executable 和运行库，须完整解压。
Editor 的 Build 临时试玩导出不需要 Cargo/密钥；规范媒体播放含对应平台视频后端。
转换非规范音视频仍须另装 FFmpeg executable，SDK 动态库不是转换工具。

CI / 发布使用官方稳定 Actions 并固定完整发布 SHA；JavaScript Actions 使用 Node 24。
Runner 显式使用 `ubuntu-26.04`、`macos-26`（arm64）和 `windows-2025`（x64）；
Rust 原生构建缓存按 runner image / architecture 隔离，
避免系统迁移时恢复其他镜像的链接产物。artifact 下载沿用新版默认的哈希校验失败即报错。
版本与 runner 要求依据 [GitHub Node 20 退役公告](https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/)
及 [官方 runner images](https://github.com/actions/runner-images#available-images)。

维护者操作：

1. 推送 main 后等待 **CI Desktop**，成功后 **Release Engine** 更新 benchmark，**Release Editor** 更新 Editor。
2. 发布 tday：Actions → **Release Engine** → **Run workflow** → 选目标 ref → `package=tday`。
   同一次触发并行构建桌面三平台和安卓 release APK，游戏资源已内置；普通推送和自动 CI 完成不会触发 tday 打包。此渠道只用隔离临时 identity，不读正式发行密钥。安卓使用测试应用签名，见 [Android](android.md)。
3. 手动补发 benchmark：同一入口选 `package=benchmark`；补发 Editor 则运行 **Release Editor**。
4. 下载更新完成后核对标签对应的 SHA、三个平台 ZIP、tday 的安卓 APK 和包内 provenance；代码推送、CI 成功、
   Release 附件更新分别确认。正式游戏发行继续使用下方 Project CI/CLI 与稳定 identity。

自动构建固定使用通过 CI 的 SHA；PR、fork、失败的 CI 不发布。新 main 已出现时，
旧自动构建只保留 Actions artifact，不覆盖滚动下载。三个 ZIP 在原平台打好再上传，保留
Unix executable 权限；benchmark 必须三个桌面平台成功，tday 必须四个平台成功后才发布。标签/附件更新失败会让发布 job 失败。
macOS 包是开发签名，未做 Apple 公证；完整跨平台运行验收与硬件性能测试仍是独立关卡。

默认 native-benchmark 三平台使用同一套真实负载，包含媒体。缺少必测时间轴时返回
INCOMPLETE 并保留报告，不能当作完整跑分。包内 `profile-runtime.py` 的额外调用栈采集
需要 Python 3 和平台采样工具；直接跑 benchmark 无需 Python。Windows 附 PDB，macOS
有 dSYM 时附带。它不在 CI 主机上执行 GPU 性能验收，也不是正式游戏发行。

游戏打包保存 Cargo 缓存前清理 Engine/Loader 的 release/profiling 密钥产物，保留依赖
与不含密钥的 publisher runner；必要时补写 `target/CACHEDIR.TAG`，清理失败阻止缓存保存
和发布。Editor 使用独立 `target/authoring` 缓存，不启用 publisher/hardened，也不接收 identity。
发布依据见 [workflow_run](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)
与 [Release 上传 action](https://github.com/softprops/action-gh-release)。

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
不能套用 Rust wrapper 的 WTFPL；macOS 使用系统 AVFoundation；Windows/Linux Editor 包附 FFmpeg 播放库及 SDK 版权说明，
不附带转换 executable。正式发行仍须核对实际 native SDK 的许可和来源。
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

第一条打包脚本使用预构建二进制，产出一个 `Kēne Editor.app`，同版本 Engine 内置于
`Contents/MacOS/keine`；目标必须是新目录。跨平台 CI 使用 `package-authoring.py` 包装，
记录 SHA/features，检查实际安装 runtime 并校验 native-smoke；不能混用不同提交的二进制。
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
验收包括 `codesign --verify --deep --strict`、ZIP 解压后启动、独立安装后的内置 Engine discovery。

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
