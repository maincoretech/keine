# Kēne

Native visual-novel engine and source-backed Editor. Rust 2024 (MSRV 1.99.0), Bevy 0.19.1, GPUI.
Native authoring uses Eiyashou `.shou`; LetsGal Studio and frozen WebGAL compatibility inputs are supported.
Shipping projects use Hakutaku v2, WebP images and Ogg Opus audio.

[简体中文](dev/docs/readme.zh.md)

<img src="src/assets/branding/keine-portrait.png" width="180" alt="Kēne character artwork">

## Downloads

- [`editor-latest`](https://github.com/maincoretech/keine/releases/tag/editor-latest): standalone Editor with a matching Preview Engine; updated after successful main-branch Desktop CI.
- [`tday-latest`](https://github.com/maincoretech/keine/releases/tag/tday-latest): tday demo; built only by manually running **Build tday**.

See [publishing](dev/docs/release.md) for package layouts, signatures and project releases.

## Run

```sh
cargo build --locked -p keine --bin keine --features video-native,video-ffmpeg
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
```

Use the pinned toolchain and install the native SDKs listed in the [development guide](dev/docs/testing.md#开发环境). The development alias above enables FFmpeg; macOS prefers its native video backend.
Build the Engine before starting Editor Preview: Editor launches an existing executable and does not run Cargo.
Migration and formal bundling are separate [publisher operations](dev/docs/release.md#cli).

`projects/tday/` is a sealed, playable demo with tracked WebP/Opus media. Use a copy for experiments; its PNG/WAV originals and other local projects are ignored. `tests/fixtures/` contains the regression projects.
The Editor controls a separate native Engine window. Text, Blocks and Inspector edit one source document.
Compatibility JSON is read-only; migration is explicit and preserves its input.

## Script performance

Measured on **Apple M5 Pro (15 logical CPUs), 24 GiB RAM, macOS 27.0.1**,
2026-10-16, using the 0.16.3 working tree with parser allocation improvements
included in 0.16.4.
With the same optimized Criterion benchmark, 100,000 text statements improved
from **85.26 to 75.09 ms**; the 2,626-action tday corpus improved from **2.27 to 2.13 ms**.

Same-machine CPU microbenchmarks below use equivalent content in each language,
single-threaded, warm in-memory input. Each result is the median of three process
medians, each with five warm-ups and ten samples; output destruction is excluded.

| Native script path | Parse 10k text statements | Parse 10k assignments | Execute 10k increments |
| --- | ---: | ---: | ---: |
| Kēne / EYS, optimized 0.16.3 | 5.98 ms | 4.40 ms | 0.65 ms |
| TyranoScript `c8dbfd4`, V8 / Node 24.22 | 2.50 ms | 7.01 ms | 0.77 ms |
| Suika3 26.07.13, C tag path | 1.06 ms | 1.77 ms | 16.49 ms |
| Ren'Py 8.5.3, SDK Python 3.12 | 81.65 ms | 63.23 ms | 3.74 ms |

The parsers produce different structures and perform different semantic checks.
Execution uses prepared scripts and native command handlers/Core; host persistence,
frame scheduling, rollback, UI, media and rendering are excluded. Suika3's NoctLang/JIT
is unmeasured. These are specific-path measurements, not a whole-engine ranking or
startup comparison: shipping Kēne and Ren'Py can load compiled scripts. The desktop
was not isolated. Exact contracts, versions, raw results and reproduction commands:
[testing guide](dev/docs/testing.md#0163-解析优化与四引擎脚本对照).

## Repository

```text
crates/       core, loader, authoring protocol, Editor, media
src/          engine; all embedded resources in src/assets/
tests/        integration, fixtures, bench, fuzz, video acceptance
dev/docs/     current documentation
dev/scripts/  build and release scripts
```

## Documentation

- [Architecture and data contracts](dev/docs/architecture.md)
- [Eiyashou v2.0](dev/docs/language.md)
- [Editor](dev/docs/editor.md)
- [Development, testing and acceptance](dev/docs/testing.md)
- [Publishing](dev/docs/release.md)
- [Android build and device acceptance](dev/docs/android.md)
- [Compatibility limits](dev/docs/compatibility.md)

Current version: 0.16.5. macOS is the current acceptance focus; Windows/Linux runtime acceptance is deferred.
Build/test success and manual input/display/media acceptance are separate gates.

## License

Original Kēne code and documentation use the [Defold License 1.0](LICENSE).
Commercial games are allowed; commercialisation of the engine or Editor as a
Game Engine Product is restricted. This is source-available software.
Third-party dependencies and assets retain their own licenses; see [NOTICE](NOTICE).
The tday demo's scripts and media are original works by shiftz and retain their
separate copyright; see [tday's notice](projects/tday/LICENSE).
Contributions are accepted under the same license unless separately agreed.
