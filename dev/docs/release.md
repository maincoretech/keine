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

Cargo aliases 与 executable 使用同名动词；具体参数以 `--help` 为准。
正式内容先验证/编译，再加载或创建 publisher identity；失败保留已有可运行发布目录。
identity 与内嵌 key 不记录、不提交、不缓存，也不传给无需它的 child build。

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
