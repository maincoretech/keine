# Kēne

**English** · [简体中文](README_CN.md)

<p align="center">
  <img src="assets/branding/keine-portrait.png" width="220" alt="Kēne character artwork">
</p>

Kēne is a Rust/Bevy desktop visual-novel engine with a separate native editor.
Eiyashou (`.shou`) is its native authoring format. WebGAL scripts and LetsGal
Studio projects are supported through compatibility loaders; WebGAL support is
maintained, not being expanded toward full parity.

## Open the editor

Install Rust 1.97.1, then run from the repository root:

```bash
cargo editor projects/test-project
```

`cargo editor` opens an empty workbench; one or more project paths open project
windows. The editor can browse and edit native Eiyashou projects. In those
projects, `config.yaml`, configured manifests, and `scripts/**/*.shou` are
writable; compatibility projects and other documents are read-only.

Preview needs a compatible, prebuilt Engine executable. For a source checkout,
build it once before pressing Start in Preview:

```bash
cargo build -p keine --bin keine
cargo editor projects/test-project
```

The two development binaries then sit together in `target/debug/`. The editor
also accepts `KEINE_ENGINE=/path/to/keine` if the Engine is elsewhere. Editor
and Engine must use the same authoring protocol; mismatched builds fail the
handshake. The standalone macOS packaging instructions are in
[T24](docs/tasks/T24-project-release-hardening.md).

**`cargo dev editor` is not an editor command.** `cargo dev <project>` runs a
game project. Use `cargo editor <project>` for the authoring UI.

## Run and release a game

```bash
cargo validate projects/test-project
cargo dev projects/test-project
cargo bundle projects/test-project
```

`cargo dev` enables hot reload and both native and FFmpeg video features, so
it needs the relevant development libraries. If FFmpeg is unavailable, run
without its video features:

```bash
cargo run --features hot-reload -- dev projects/test-project
```

The sample project's [acceptance checklist](projects/test-project/ACCEPTANCE.md)
is for hands-on editor and runtime checks. `cargo bundle` writes a complete
desktop release to `target/bundle/` by default:

```text
target/bundle/
├── keine[.exe]
├── game.haku
└── data/
    └── <content-id>.taku
```

Distribute the whole directory. With no project argument, the release
executable finds the sibling `game.haku`; it does not need to be launched from
that directory. Production image assets must be WebP and standalone audio must
be Ogg Opus. Other supported development inputs do not silently enter a
shipping build. The first operation requiring a publisher identity creates
`.keine/publisher.hakutaku-key`; back it up and never distribute or commit it.
See [Project CI](docs/project-ci-release.md) for formal releases.

## Commands

These `cargo` commands are repository aliases from `.cargo/config.toml`:

| Command | What it does |
|---|---|
| `cargo editor [project ...]` | Open the editor; Preview uses a separate Engine |
| `cargo validate <project>` | Check a project without opening a window |
| `cargo dev <project> [--sync]` | Run with hot reload; `--sync` follows an open LetsGal project |
| `cargo bundle <project> [--output <dir>]` | Build the complete game release |
| `cargo pack <project> [--output <dir>]` | Build only Hakutaku resources, without the Engine |
| `cargo migrate <source> <target>` | Convert supported compatibility content into a new Eiyashou project |
| `cargo remap <project> <old=new>... [-y]` | Update references after separately converting assets |
| `cargo perf <project> [options]` | Measure runtime frames or startup (`--startup`) |

`cargo migrate` reads its source without rewriting it and refuses a conversion
that cannot preserve the source semantics. `cargo remap` changes references,
not media files. `cargo bundle <project> --benchmark` builds a separate
benchmark package. Run `cargo <command> --help` for command-specific options.

The installed `keine` executable exposes only commands compiled into that
build; publisher and hot-reload commands are not part of the normal game
release. The editor is a separate executable, not a `keine` subcommand.

## Projects

| Input | Root entry | Authoring status |
|---|---|---|
| Native Eiyashou | `config.yaml` and `.shou` scripts | Editable in the editor |
| WebGAL directory | `config.yaml` | Compatibility input; read-only in the editor |
| LetsGal Studio | `project.json` | Compatibility input; read-only in the editor |
| Hakutaku package | `game.haku` with `data/` | Packaged runtime input |

Game saves, settings, and profiles live in the platform user-data directory
identified by the project's stable ID, not beside a read-only release.
Supported project formats are detected from project content and configuration;
the old machine-wide `engine.conf` adapter/video switches are not used.

On macOS, `dev/scripts/bundle-macos.sh <project> <app-name> <bundle-id>` creates
a game `.app`. Native projects require a reverse-DNS bundle ID in argument 3;
LetsGal projects can derive it from `project.json.id`. This is separate from
the standalone Editor/Engine application pair.

## Game controls

| Shortcut | Action |
|---|---|
| `Ctrl+A` / `Ctrl+K` | Auto / Skip |
| `Ctrl+B` / `Ctrl+R` | Backlog / replay voice |
| `Ctrl+H` | Hide or restore the textbox |
| `Ctrl+Q` / `Ctrl+L` | Quick save / quick load |
| `Ctrl+S` / `Ctrl+O` | Save / load |
| `Ctrl+,` / `Ctrl+T` | Settings / title |
| Hold `Ctrl` | Fast-forward |
| `Esc` | Close or go back |

These are game shortcuts; the editor has its own editing shortcuts.

## Develop Kēne

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
cargo validate projects/test-project
```

`crates/core/` owns the Bevy-free model and execution; `crates/loader/` owns
project and format adapters; `crates/authoring/` owns the Editor–Engine
protocol; `crates/editor/` owns the workbench. The root `src/` owns the Bevy
runtime, scene, UI, media, and storage. Project state and acceptance limits are
tracked in [docs/PROJECT_STATE.md](docs/PROJECT_STATE.md).

Further reading: [project structure](docs/PROJECT.md),
[resource limits](docs/resource-limits.md),
[content loading](docs/architecture/07-content-loader.md),
[saves](docs/architecture/04-rollback-and-save.md),
[Hakutaku packaging](docs/architecture/06-hakutaku-packaging.md), and
[WebGAL compatibility](docs/webgal/README.md).
