# Kēne

Native visual-novel engine and source-backed Editor. Rust 2024 (MSRV 1.99.0), Bevy 0.19.1, GPUI.
Native authoring uses Eiyashou `.shou`; LetsGal Studio and frozen WebGAL compatibility inputs are supported.
Shipping projects use Hakutaku v2, WebP images and Ogg Opus audio.

[简体中文](dev/docs/readme.zh.md)

<img src="src/assets/branding/keine-portrait.png" width="180" alt="Kēne character artwork">

## Downloads

- [`editor-latest`](https://github.com/maincoretech/keine/releases/tag/editor-latest): standalone Editor with a matching Preview Engine; updated after successful main-branch Desktop CI.
- [`tday-latest`](https://github.com/maincoretech/keine/releases/tag/tday-latest): tday demo; built only by manually running **Release Engine**.

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
2026-10-11, using the 0.14.3 working tree and optimized Cargo benches. These are historical measurements, not a rerun on the current release.
Single-threaded, warm in-memory input, 10 samples:

| Workload | Approximate result |
| --- | ---: |
| EYS parsing: 100,000 text statements | 115 ms / 870k actions/s |
| EYS parsing: 100,001 expression/text actions | 132 ms / 760k actions/s |
| tday parser corpus: 14 scenes, 2,665 actions | 2.85 ms |
| Core execution: assignment, condition and interpolated dialogue | 225k dialogue turns/s |
| Core execution: non-blocking Comment actions | 108M actions/s |

Execution excludes Program installation; each mixed turn contains three authored actions
and immediately advances dialogue. These CPU microbenchmarks exclude disk I/O, media,
scene/UI updates and rendering; the desktop was not isolated. Reproduction details:
[testing guide](dev/docs/testing.md).

For context, these DSLs use different loading/execution paths:

| Engine / DSL | Script path |
| --- | --- |
| Kēne / Eiyashou | Source lowered to typed IR; shipping games load a compiled Program and execute through Rust Core |
| [Ren'Py](https://www.renpy.org/doc/html/language_basics.html) | `.rpy` compiled to `.rpyc`; script expressions use Python |
| [WebGAL](https://docs.openwebgal.com/en/tech/) | `.txt` parsed into executable scene objects, with a Pixi stage |

Ren'Py and WebGAL were not measured on the same machine/workload. This is an
architecture comparison, not a cross-engine speed ranking.

## Repository

```text
crates/       core, loader, authoring protocol, Editor, media
src/          engine; all embedded resources in src/assets/
tests/        integration, fixtures, bench, fuzz, video acceptance
dev/docs/     current documentation
dev/scripts/  build and fixture scripts
```

## Documentation

- [Architecture and data contracts](dev/docs/architecture.md)
- [Eiyashou v2.0](dev/docs/language.md)
- [Editor](dev/docs/editor.md)
- [Development, testing and acceptance](dev/docs/testing.md)
- [Publishing](dev/docs/release.md)
- [Android build and device acceptance](dev/docs/android.md)
- [Compatibility limits](dev/docs/compatibility.md)

Current version: 0.16.1. macOS is the current acceptance focus; Windows/Linux runtime acceptance is deferred.
Build/test success and manual input/display/media acceptance are separate gates.

## License

Original Kēne code and documentation use the [Defold License 1.0](LICENSE).
Commercial games are allowed; commercialisation of the engine or Editor as a
Game Engine Product is restricted. This is source-available software.
Third-party dependencies and assets retain their own licenses; see [NOTICE](NOTICE).
The tday demo's scripts and media are original works by shiftz and retain their
separate copyright; see [tday's notice](projects/tday/LICENSE).
Contributions are accepted under the same license unless separately agreed.
