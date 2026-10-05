# Kēne

Native visual-novel engine and source-backed Editor. Rust 2024, Bevy 0.19, GPUI.
Native authoring uses Eiyashou `.shou`; LetsGal Studio and frozen WebGAL compatibility inputs are supported.
Shipping projects use Hakutaku v1, WebP images and Ogg Opus audio.

[简体中文](dev/docs/readme.zh.md)

<img src="src/assets/branding/keine-portrait.png" width="180" alt="Kēne character artwork">

## Run

```sh
cargo editor projects/tday
cargo validate tests/fixtures/native-smoke
cargo dev tests/fixtures/native-smoke
cargo migrate /path/to/letsgal/tday projects/tday
cargo bundle <project> --output <release-directory>
```

`projects/` contains ignored local authoring inputs; `tests/fixtures/` contains the tracked regression projects.
The Editor controls a separate native Engine window. Text, Blocks and Inspector edit one source document.
Compatibility JSON is read-only; migration is explicit and preserves its input.

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

Current version: 0.13.0. macOS is the current acceptance focus; Windows/Linux runtime acceptance is deferred.
Build/test success and manual input/display/media acceptance are separate gates.

## License

Original Kēne code and documentation use the [Defold License 1.0](LICENSE).
Commercial games are allowed; commercialisation of the engine or Editor as a
Game Engine Product is restricted. This is source-available software.
Third-party dependencies and assets retain their own licenses; see [NOTICE](NOTICE).
Contributions are accepted under the same license unless separately agreed.
