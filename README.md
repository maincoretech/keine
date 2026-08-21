# Kēne

**English** · [简体中文](README_CN.md)

<p align="center">
  <img src="assets/branding/keine-portrait.png" width="220" alt="Kēne character artwork">
</p>

Kēne (/keːne/) is a native visual novel engine written in Rust. It uses Bevy
and wgpu for rendering and supports directory-based development projects and
self-contained desktop releases.

## Features

- Native rendering, audio, video, UI, saves, backlog, Auto, Skip, and rollback.
- Backgrounds, portraits, layers, transitions, filters, particles, and regional blur.
- Frame-rate-independent dialogue, animation, and timeline playback.
- WebGAL scripts and LetsGal projects through independent loader adapters.
- Encrypted incremental release packages powered by Hakutaku.

## Quick start

Kēne requires Rust 1.97.1.

```bash
git clone https://github.com/maincoretech/keine.git
cd keine
cargo validate projects/test-project
cargo dev projects/test-project
```

Use `cargo run --features hot-reload -- dev projects/test-project` when FFmpeg
development libraries are unavailable. The visual acceptance checklist is in
[`projects/test-project/ACCEPTANCE.md`](projects/test-project/ACCEPTANCE.md).

## Commands

| Command | Purpose |
|---|---|
| `cargo validate <project>` | Validate a project without opening a window |
| `cargo dev <project>` | Run a project with development tools and hot reload |
| `cargo dev <project> --sync` | Follow an open LetsGal project |
| `cargo assets --pack <project>` | Build only the encrypted Hakutaku resource package |
| `cargo assets --remap <project> <old=new>...` | Safely migrate converted asset references |
| `cargo bundle <project>` | Build a distributable game |
| `cargo bundle <project> --benchmark` | Build a separate `-benchmark` performance package |
| `cargo configure` | Configure built-in content adapters and runtime capabilities |
| `cargo perf <project>` | Capture a runtime performance sample |

## Game projects

| Project type | Root entry |
|---|---|
| Native / WebGAL directory | `config.yaml` |
| LetsGal project | `project.json` |
| Packaged game | `game.haku` with sibling `data/` |

Development runs read the editable project directory directly. A normal game
workflow is:

1. Edit the native/WebGAL directory or LetsGal project.
2. Run `cargo validate <project>` after script or configuration changes.
3. Iterate with `cargo dev <project>`; add `--sync` for an open LetsGal project.
4. Build the release with `cargo bundle <project>`.
5. Run the generated release before distributing the complete output directory.

The default release is written to `target/bundle/`:

```text
target/bundle/
├── keine[.exe]
├── game.haku
└── data/
    └── <content-id>.taku
```

Start the release directly: double-click `keine.exe` on Windows, or run
`./keine` on macOS/Linux. The executable locates the sibling `game.haku`
without depending on the current working directory.

`cargo assets --pack` writes only `game.haku` and `data/*.taku` under
`target/package/`; it never builds or copies an engine. `cargo bundle` reuses
the same asset-pack pipeline and then builds and assembles the matching
runtime. The first operation that needs a publisher identity creates
`.keine/publisher.hakutaku-key`. Back up this file and keep it outside the
distributed game. Later bundles reuse unchanged content segments when the
previous output is available.

The manual GitHub **Release** workflow defaults to a temporary publisher
identity, so forks, `test-project`, and benchmark bundles need no secret setup.
Temporary identities are deleted with the runner and are not suitable for
shipping updates. For a stable production lineage, run `cargo assets --pack`
once on a trusted machine, base64-encode the generated identity, store it as
the `HAKUTAKU_IDENTITY_BASE64` repository secret, and select `stable` in the
workflow. Never commit either form of the identity.

Create a macOS application bundle with:

```bash
dev/scripts/bundle-macos.sh path/to/project
```

### Remap converted assets

After converting audio or images offline, migrate their project references
without adding a runtime fallback:

```bash
# Preview the path and size changes, then ask for confirmation.
cargo assets --remap path/to/project wav=opus png=webp

# Print the same preview and apply it without prompting.
cargo assets --remap path/to/project wav=opus png=webp -y
```

Every converted target must already exist beside its source. The preview shows
the old path in red, the new path in green, `old size → new size`, and the size
change percentage. Apply mode backs up every changed source below
`.keine/asset-remap-backups/`, replaces files atomically, and reopens the
project for validation. Failed validation restores the originals. The command
does not convert, rename, delete, or select fallback assets.

## Controls

| Shortcut | Action |
|---|---|
| `Ctrl+A` / `Ctrl+K` | Auto / Skip |
| `Ctrl+B` / `Ctrl+R` | Backlog / replay voice |
| `Ctrl+H` | Hide or restore the textbox |
| `Ctrl+Q` / `Ctrl+L` | Quick save / quick load |
| `Ctrl+S` / `Ctrl+O` | Save / load |
| `Ctrl+,` / `Ctrl+T` | Settings / title |
| hold `Ctrl` | Fast-forward |
| `Esc` | Close or go back |

## Repository

| Path | Contents |
|---|---|
| `src/` | Runtime, rendering, UI, media, and storage |
| `crates/core/` | Typed action model and execution state |
| `crates/loader/` | Project, script, asset, and save adapters |
| `projects/test-project/` | End-to-end acceptance project |
| `dev/` | Architecture notes and platform scripts |

The runtime depends on `keine-loader` and `keine-core`; the core crate remains
independent from Bevy.

## Build and test

```bash
cargo build --release
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate projects/test-project
```

Video builds require the relevant native media libraries. Ogg Opus is the
recommended distributed audio format: project Opus assets use a reopenable,
seekable incremental decoder, including bounded-memory loops. WAV, MP3, Ogg
Vorbis, and FLAC remain memory-backed; their shared byte-length-aware decoder
provides duration, bounded-memory loops, and random-access seeking in the Extra
gallery without loading a second compressed copy.

## Documentation

- [Project structure](dev/docs/PROJECT.md)
- [Resource, package, and storage limits](docs/resource-limits.md)
- [Content loader](dev/docs/architecture/07-content-loader.md)
- [Rendering](dev/docs/architecture/03-render-pipeline.md)
- [Saves and rollback](dev/docs/architecture/04-rollback-and-save.md)
- [Hakutaku packaging](dev/docs/architecture/06-hakutaku-packaging.md)
- [LetsGal integration](dev/docs/architecture/08-letsgal-studio.md)
- [WebGAL compatibility](dev/docs/webgal-compatibility/README.md)
- [Current work](dev/docs/TODO.md)
