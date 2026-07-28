# crabgal

crabgal is a purpose-built visual novel engine written in Rust on top of
Bevy 0.19 and wgpu. It provides MainCore projects with a consistent UI, native
desktop rendering, and single-binary distribution while loading WebGAL scripts,
LetsGal 1.9 projects, local directories, and standard Hexz resource packages
through adapters.

The project is currently in the `0.8.1` development series and prioritizes
desktop platforms. WebGAL and LetsGal compatibility is defined by executable
tests and compatibility matrices; successful parsing alone is not treated as
end-to-end support.

## Features

- Fixed 1920×1080 design space, 16:9 clipping, and letterboxing. Window size
  does not alter animation speed or blur strength.
- Native Bevy/wgpu backgrounds, portraits, timelines, transitions, filters,
  blend modes, and bounded GPU particle effects.
- WebGAL K-inspired Title, Textbox, Choice, Backlog, Save/Load, Config, and
  Extra screens.
- Rich text, ruby annotations, text input, typewriter animation, Auto, Skip,
  rollback, and persistent read history.
- Separate BGM, voice, SE, and UI audio buses. Ogg Opus is the recommended
  distribution format.
- Versioned Postcard saves with dual CRC32 checks, program fingerprints,
  atomic writes, and WebP previews.
- Layered resource sources, scene-aware prefetching, development hot reload,
  and seekable access to standard Hexz archives.
- Native LetsGal 1.9 project loading, typed Action compilation, and
  synchronization through its open project state.
- Optional FFmpeg video backend and project-aware Cargo feature selection for
  audio and video codecs.
- macOS, Windows, and Linux CI plus standalone binary and encrypted resource
  package workflows.

## Quick start

A stable Rust toolchain is required. Builds without video do not need FFmpeg.
`cargo dev`, `cargo preview`, and `cargo studio-sync` enable the native video
backend by default and therefore require local FFmpeg development libraries.
The bundled Opus decoder requires CMake during its first build.

```bash
# Validate the default acceptance project.
cargo validate projects/test-project

# Run with hot reload and video support.
cargo dev projects/test-project

# Run without FFmpeg. Projects containing video report the missing capability.
cargo dev-lite projects/test-project
```

The engine opens on the title screen. Follow
[`projects/test-project/ACCEPTANCE.md`](projects/test-project/ACCEPTANCE.md) for
the numbered end-to-end acceptance test.

Global keyboard shortcuts use only `Ctrl` combinations. Holding `Ctrl` performs
fast-forward, while `Esc` consistently closes the current overlay or returns
to the previous screen. Mouse, touch, gamepad, and choice-menu navigation are
interaction controls rather than global shortcuts.

| Shortcut | Action |
|---|---|
| `Ctrl+A` / `Ctrl+K` | Auto / Skip |
| `Ctrl+B` / `Ctrl+R` | Backlog / replay current voice |
| `Ctrl+H` | Hide or restore the textbox |
| `Ctrl+Q` / `Ctrl+L` | Q·SAVE / Q·LOAD |
| `Ctrl+S` / `Ctrl+O` | SAVE / LOAD |
| `Ctrl+,` / `Ctrl+T` | CONFIG / return to title |
| `Esc` | Close a dialog, Backlog, Extra, or menu |

### Commands

| Command | Purpose |
|---|---|
| `cargo adapters` | Interactively enable or disable built-in adapters |
| `cargo validate <project>` | Parse and validate a project without opening a window |
| `cargo dev <project>` | Development runtime with asset watching and hot reload |
| `cargo dev-lite <project>` | Development runtime without FFmpeg |
| `cargo preview <project>` | Interactive preview using release optimization |
| `cargo perf <project> [seconds] [Action index] [full\|scene-ui\|scene-dialog\|scene]` | Reproducible performance sample |
| `cargo studio-sync <LetsGal project>` | Read-only synchronization with the current LetsGal project and step position |

Validation and runtime commands fail immediately when the path does not exist
or lacks the expected `config.yaml` or `project.json`; they never silently fall
back to another project.

## Adapter selection

Run:

```bash
cargo adapters
```

Controls:

- `↑` / `↓`: select an entry
- `←` / `→` or `Space`: enable or disable it
- `Enter`: save
- `Esc`: cancel

Built-in adapters are grouped by capability:

| Category | Built-in implementations |
|---|---|
| Asset | `auto`, `fs`, `hexz` |
| Script | `webgal` |
| Project | `hexz`, `letsgal` |
| Store | `crabgal` |

This selection limits which built-in implementations the final CLI may use; it
does not replace project configuration. Asset, Script, and Store always retain
at least one implementation. Newly introduced adapters are enabled by default.

The configuration is stored in the platform user-data directory:

- macOS: `~/Library/Application Support/crabgal/adapters.conf`
- Linux: `$XDG_CONFIG_HOME/crabgal/adapters.conf` or
  `~/.config/crabgal/adapters.conf`
- Windows: `%APPDATA%\crabgal\adapters.conf`

Set `CRABGAL_ADAPTER_CONFIG` to use a different file temporarily. Library hosts
using `run_with_loader` or `build_app_with_loader` do not read this global
configuration and may register only the capabilities they need, starting from
`LoaderRegistry::empty()`.

## Supported project inputs

### Native crabgal and WebGAL projects

```text
my-game/
├── config.yaml
├── scripts/
└── assets/
    ├── background/
    ├── figure/
    ├── audio/
    ├── video/
    └── fonts/
```

`config.yaml` selects enabled Asset, Script, and Store adapters. Resource
sources are ordered overlays: a later source replaces an earlier source with
the same logical path.

```yaml
adapter:
  asset:
    - path: "."
      format: fs
    - path: "content/shared"
      format: fs
    - path: "packs/route.hxz"
      format: hexz
  script: webgal
  store: crabgal
```

`layout.sprite_y_offset` defines a project-wide portrait baseline offset in
1920×1080 design pixels. It is applied before the relative
`transform.position.y` value authored by a script.

### LetsGal 1.9 projects

crabgal directly opens LetsGal projects containing `project.json`, chapters,
characters, scenes, variables, and the resource manifest. Supported editor
blocks are compiled into the engine's neutral typed Action representation.

```bash
cargo validate '/absolute/path/to/LetsGal project'
cargo studio-sync '/absolute/path/to/LetsGal project'
```

Synchronization uses the project's open JSON files and
`.studio/state.json`. It does not install a Studio extension, inject Electron,
modify ASAR files, start a local server, or control the original Studio player.
Normal `cargo dev` sessions and release builds do not poll Studio.

### Hexz packages

Standard `.hxz` packages are validated, decrypted, indexed, and read through
seekable random access by `hexz_k`; the runtime never needs to extract the
entire archive first.

```bash
target/release/crabgal /path/to/game.hxz
```

## Build and distribution

Build the desktop engine:

```bash
cargo build --release
```

Select the smallest codec feature set required by a project and create an
encrypted Hexz distribution:

```bash
CRABGAL_HEXZ_PASSWORD='your-password' \
  dev/scripts/package-release.sh projects/test-project target/release-package
```

This workflow requires the `hexz_k` command-line tool with its CLI feature.
Output is always written below `target/` and contains the engine binary,
`game.hxz`, platform launchers, and required runtime libraries.

Create a macOS application bundle:

```bash
dev/scripts/bundle-macos.sh projects/test-project crabgal
```

Video support is provided by the `video-ffmpeg` feature. Linux requires FFmpeg,
ALSA, udev, and pkg-config development packages. Windows CI obtains
`ffmpeg:x64-windows` through vcpkg and copies the required DLLs into release
artifacts. macOS may use Homebrew FFmpeg. Android/iOS video backends and
pixel-level mobile acceptance remain future work.

## Architecture

```text
crabgal-core   <- crabgal-loader <- crabgal
state/runtime      content adapters   Bevy runtime, rendering, UI, storage
```

```text
crabgal/
├── src/                 Engine, ECS, rendering, UI, media, and storage
├── crates/
│   ├── core/            Bevy-independent configuration, Action, State, runtime
│   └── loader/          Asset/Script/Project/Store adapters and hot reload
├── projects/
│   └── test-project/    Single end-to-end visual acceptance project
├── tests/               Adapter, IR, and coverage regressions
├── dev/docs/            Architecture, compatibility, acceptance, and TODO
└── dev/scripts/         Codec feature detection, packaging, and app bundling
```

The dependency direction is fixed at `core <- loader <- engine`. The loader
does not depend on Bevy. Each adapter only converts an external format into
neutral configuration, logical resource mounts, and Actions; adapter-specific
concepts do not enter the renderer or UI. The engine remains runnable when any
specific adapter is removed.

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo validate projects/test-project
```

CI runs formatting, Clippy, tests, and release builds on macOS, Windows, and
Ubuntu, with additional Linux and Windows checks for the FFmpeg feature.

## Current boundaries

- The command-level WebGAL support and fallback behavior are documented in
  [`dev/docs/webgal-compatibility/semantic-matrix.md`](dev/docs/webgal-compatibility/semantic-matrix.md).
- The LetsGal 1.9 project and synchronization contract is documented in
  [`dev/docs/architecture/08-letsgal-studio.md`](dev/docs/architecture/08-letsgal-studio.md).
- Live2D, Spine, Steam, mobile video, Safe Area handling, and physical mobile
  device acceptance are deferred.
- crabgal is a purpose-built engine and does not plan to provide a theme system
  or runtime skinning.
- [`dev/docs/TODO.md`](dev/docs/TODO.md) is the single source of truth for
  current progress and remaining work.

## Documentation

- [Project structure and boundaries](dev/docs/PROJECT.md)
- [Content loader and adapters](dev/docs/architecture/07-content-loader.md)
- [Rendering pipeline](dev/docs/architecture/03-render-pipeline.md)
- [Save and rollback model](dev/docs/architecture/04-rollback-and-save.md)
- [Performance baseline](dev/docs/performance-baseline.md)
- [Acceptance suites](dev/docs/acceptance/phases.md)

## Credits

The regional GPU blur pipeline was informed by
[bevy_blur_regions](https://github.com/atbentley/bevy_blur_regions) by
atbentley, particularly its separable Gaussian blur and region-mask design.
