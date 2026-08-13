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

Use `cargo run -- dev projects/test-project` when FFmpeg development libraries
are unavailable. The visual acceptance checklist is in
[`projects/test-project/ACCEPTANCE.md`](projects/test-project/ACCEPTANCE.md).

## Commands

| Command | Purpose |
|---|---|
| `cargo validate <project>` | Validate a project without opening a window |
| `cargo dev <project>` | Run a project with development tools and hot reload |
| `cargo dev <project> --sync` | Follow an open LetsGal project |
| `cargo bundle <project>` | Build a distributable game |
| `cargo adapters` | Configure built-in adapters |
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

The default release is written to `target/release-package/`:

```text
target/release-package/
├── keine[.exe]
├── game.haku
├── data/
│   └── <content-id>.taku
└── run.sh | run.bat
```

`cargo bundle` invokes Hakutaku automatically; game developers do not need to
run a separate packer. The first bundle creates
`.keine/publisher.hakutaku-key`. Back up this file and keep it outside the
distributed game. Later bundles reuse unchanged content segments when the
previous output is available.

Create a macOS application bundle with:

```bash
dev/scripts/bundle-macos.sh path/to/project
```

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
recommended distributed audio format.

After converting source assets offline, references can be migrated without a
runtime fallback. The command supports audio and image rules together, checks
that every converted target exists, and always previews before writing:

```bash
cargo bundle --remap-assets path/to/project wav=opus png=webp
cargo bundle --remap-assets path/to/project wav=opus png=webp -y
```

The default mode prints a path/size/reference table and asks for confirmation;
`-y` applies the reviewed plan without prompting. Apply mode backs up every changed source below
`.keine/asset-remap-backups/`, uses same-directory atomic replacements, then
reopens and validates the project. A failed validation restores the originals.

## Documentation

- [Project structure](dev/docs/PROJECT.md)
- [Content loader](dev/docs/architecture/07-content-loader.md)
- [Rendering](dev/docs/architecture/03-render-pipeline.md)
- [Saves and rollback](dev/docs/architecture/04-rollback-and-save.md)
- [Hakutaku packaging](dev/docs/architecture/06-hakutaku-packaging.md)
- [LetsGal integration](dev/docs/architecture/08-letsgal-studio.md)
- [WebGAL compatibility](dev/docs/webgal-compatibility/README.md)
- [Current work](dev/docs/TODO.md)
