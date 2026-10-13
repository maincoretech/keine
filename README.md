# Kēne

Native visual-novel engine and source-backed Editor. Rust 2024, Bevy 0.19.1, GPUI.
Native authoring uses Eiyashou `.shou`; LetsGal Studio and frozen WebGAL compatibility inputs are supported.
Shipping projects use Hakutaku v1, WebP images and Ogg Opus audio.

[简体中文](dev/docs/readme.zh.md)

<img src="src/assets/branding/keine-portrait.png" width="180" alt="Kēne character artwork">

## 下载

- `editor-latest`：独立 Editor，内含同版本 Preview；main 通过 CI 后自动更新。
- `tday-latest`：tday 试玩，Actions → Release Engine → Run workflow，仅手动。

下载与正式发行流程见 [发布文档](dev/docs/release.md)。

## Run

```sh
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
cargo migrate /path/to/letsgal/tday projects/tday
cargo bundle <project> --output <release-directory>
```

`projects/tday/` contains the playable authoring demo with WebP/Opus media. Its PNG/WAV originals and other local projects are ignored; `tests/fixtures/` contains the regression projects.
The Editor controls a separate native Engine window. Text, Blocks and Inspector edit one source document.
Compatibility JSON is read-only; migration is explicit and preserves its input.

## Script performance

Measured on **Apple M5 Pro (15 logical CPUs), 24 GiB RAM, macOS 27.0.1**,
2026-10-11, using the 0.14.3 working tree and optimized Cargo benches.
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
- [Compatibility limits](dev/docs/compatibility.md)

Current version: 0.16.0. macOS is the current acceptance focus; Windows/Linux runtime acceptance is deferred.
Build/test success and manual input/display/media acceptance are separate gates.

## License

Original Kēne code and documentation use the [Defold License 1.0](LICENSE).
Commercial games are allowed; commercialisation of the engine or Editor as a
Game Engine Product is restricted. This is source-available software.
Third-party dependencies and assets retain their own licenses; see [NOTICE](NOTICE).
The tday demo's scripts and media are original works by shiftz and retain their
separate copyright; see [tday's notice](projects/tday/LICENSE).
Contributions are accepted under the same license unless separately agreed.
