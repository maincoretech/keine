# Kēne

**English** · [中文 (Chinese)](README_CN.md)

<p align="center">
  <img src="assets/branding/keine-portrait.png" width="220" alt="Kēne character artwork">
</p>

**Kēne** (/keːne/) is a native visual novel engine built with Rust, Bevy, and wgpu. It
uses a fixed 1920×1080 design space and translates external project formats
through independent adapters.

The product and executable are named Kēne/`keine`. Compatibility identifiers
such as the `keine` project key, save adapter, file formats, environment
variables, and internal crate names remain stable so existing games and saves
continue to work.

## Highlights

- Native rendering, audio, video, UI, saves, and single-binary distribution.
- Frame-rate-independent transitions, typewriter text, timelines, and
  particles.
- Backgrounds, portraits, layers, filters, blend modes, camera transforms, and
  regional blur.
- Dialogue, narration, ruby text, choices, backlog, Auto, Skip, and rollback.
- Directory and Hexz asset overlays with development hot reload.
- WebGAL scripts and LetsGal projects compiled into one typed action model.
- Optional codec features; Ogg Opus is recommended for distributed audio.

## Run

```bash
cargo validate projects/test-project
cargo dev projects/test-project
```

Use `cargo dev-lite projects/test-project` when FFmpeg is unavailable. The
numbered visual test is in
[`projects/test-project/ACCEPTANCE.md`](projects/test-project/ACCEPTANCE.md).

| Command | Purpose |
|---|---|
| `cargo adapters` | Enable or disable built-in adapters |
| `cargo validate <project>` | Validate without opening a window |
| `cargo compiler <project> [--output <path>]` | Compile source scripts into a `program.bin` artifact |
| `cargo startup <project> [--compiled]` | Print startup segment timing (T-1..T7) and exit |
| `cargo dev <project>` | Run with hot reload and video |
| `cargo dev-lite <project>` | Run without FFmpeg |
| `cargo preview <project>` | Run an optimized preview |
| `cargo perf <project> [seconds] [cursor] [profile]` | Record a performance sample |
| `cargo dev <project> --sync` | Follow an open LetsGal project and step |

Invalid project paths fail immediately.

### Compiled program artifact

`cargo compiler <project>` parses and validates the project exactly like
`cargo validate`, then writes a versioned binary program to
`.keine/compiled/program.bin` (override with `--output <path>`). The artifact
uses a fixed envelope (magic, versions, lengths, CRC32, program fingerprint)
so release packages can skip source-script parsing at startup; the fingerprint
matches the program built from source, so saves remain compatible. Development
runs still read source scripts for diagnostics and hot reload. Use
`cargo compiler preview <project>` to run the compiled-loading path against any
project that has a program.bin. Release packaging runs this step automatically
and pins `compiled_program: require` in the packaged config.

`cargo startup <project>` measures startup in eight segments (T-1 CLI setup,
T1 project open, T2 script language, T3 store setup, T4 app assembly, T5 app
start, T6 scene loading, T7 first frame, plus a total row) and exits after
the first frame. Add `--compiled` to force the compiled `program.bin` loading
path; run it twice (with and without `--compiled`) for a source-vs-compiled
A/B. The T6 scene-loading segment is the only one that differs between the
two paths.

## Project inputs

| Input | Entry |
|---|---|
| Native / WebGAL directory | `config.yaml` |
| LetsGal project | `project.json` |
| Packaged game | `game.hxz` |

A directory project can combine ordered asset sources:

```yaml
adapter:
  asset:
    - { path: ".", format: fs }
    - { path: "content/shared", format: fs }
    - { path: "packs/route.hxz", format: hexz }
  script: webgal
  store: keine
```

Later sources override earlier files with the same logical path. LetsGal
synchronization reads open project files and `.studio/state.json`; Kēne
remains a separate native process and does not modify Studio.

Optional shell features are disabled by default. A native `config.yaml` can
enable the Extra CG/BGM gallery explicitly:

```yaml
features:
  extra: true
```

LetsGal projects use the equivalent project-level object in `project.json`:

```json
{
  "keine": {
    "features": {
      "extra": true
    }
  }
}
```

Built-in adapters:

| Capability | Implementations |
|---|---|
| Assets | `auto`, `fs`, `hexz` |
| Scripts | `webgal` |
| Editor projects | `letsgal` |
| Packages | `hexz` |
| Saves | `keine` |

## Architecture

### Project to screen

```mermaid
flowchart LR
    P["Project<br/>WebGAL · LetsGal · Hexz"] --> L["Loader<br/>adapters · validation · resources"]
    L --> C["Core<br/>Config · Action · State"]
    C --> R["Runtime<br/>Bevy · rendering · UI · media"]
    R --> O["Player<br/>window · audio · saves"]
```

External formats stop at the loader. The runtime only sees typed actions and
logical resources.

### Code dependencies

```mermaid
flowchart LR
    R["Kēne runtime<br/>src/"] --> L["keine-loader<br/>crates/loader/"]
    R --> C["keine-core<br/>crates/core/"]
    L --> C
```

`core` is Bevy-free, and adapter models never enter rendering or UI code.

| Path | Responsibility |
|---|---|
| `src/` | Runtime, rendering, scenes, UI, media, and storage |
| `crates/core/` | Typed engine model and execution state |
| `crates/loader/` | Asset, script, project, and save adapters |
| `projects/test-project/` | End-to-end visual acceptance |
| `tests/` | Compiler, adapter, runtime, and coverage regressions |
| `dev/` | Documentation, packaging, and platform scripts |

## Controls

Global shortcuts use `Ctrl`; `Esc` closes or returns.

| Shortcut | Action |
|---|---|
| `Ctrl+A` / `Ctrl+K` | Auto / Skip |
| `Ctrl+B` / `Ctrl+R` | Backlog / replay voice |
| `Ctrl+H` | Hide or restore textbox |
| `Ctrl+Q` / `Ctrl+L` | Quick save / quick load |
| `Ctrl+S` / `Ctrl+O` | Save / load |
| `Ctrl+,` / `Ctrl+T` | Configuration / title |
| hold `Ctrl` | Fast-forward |
| `Esc` | Close or go back |

## Build

```bash
cargo build --release
cargo build --release --features video-ffmpeg
```

The bundled Opus decoder requires CMake. Video builds require FFmpeg
development libraries.

Package an encrypted Hexz game:

```bash
HEXZ_PASSWORD='your-password' \
  dev/scripts/package-release.sh path/to/native-project target/release-package
```

Release packaging requires a native project with `config.yaml`; LetsGal
`project.json` projects need the native conversion described in
[`docs/project-and-assets-spec.md`](docs/project-and-assets-spec.md). The
pipeline compiles `.keine/compiled/program.bin`, pins `compiled_program: require`,
and keeps runtime state and caches out of the archive.

Create a macOS app bundle:

```bash
dev/scripts/bundle-macos.sh projects/test-project
```

## Validate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo validate projects/test-project
```

## Documentation

- [Project structure](dev/docs/PROJECT.md)
- [Content loader](dev/docs/architecture/07-content-loader.md)
- [Rendering](dev/docs/architecture/03-render-pipeline.md)
- [Saves and rollback](dev/docs/architecture/04-rollback-and-save.md)
- [LetsGal integration](dev/docs/architecture/08-letsgal-studio.md)
- [WebGAL compatibility](dev/docs/webgal-compatibility/README.md)
- [Current work](dev/docs/TODO.md)
