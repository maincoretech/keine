//! Command-line surface and strict argument parsing.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::ui::performance::BenchmarkTarget;

const DEFAULT_ASSET_PACKAGE_OUTPUT: &str = "target/package";
const DEFAULT_BUNDLE_OUTPUT: &str = "target/bundle";
pub(crate) const BENCHMARK_MARKER: &str = "keine-benchmark.conf";
pub(crate) const BENCHMARK_REPORT_FILE: &str = "keine-benchmark-report.txt";

#[derive(Debug, Clone)]
pub(super) struct BenchmarkOptions {
    pub(super) seconds: f32,
    pub(super) target: Option<BenchmarkTarget>,
    pub(super) cameras: crate::ui::performance::BenchmarkCameras,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StartupBenchmarkOptions {
    pub(super) runs: usize,
}

#[derive(Debug, Clone)]
pub(super) enum InteractiveMode {
    Shipping,
    #[cfg(feature = "hot-reload")]
    Development,
    Benchmark(BenchmarkOptions),
    StartupBenchmark(StartupBenchmarkOptions),
}

impl InteractiveMode {
    pub(super) const fn development(&self) -> bool {
        #[cfg(feature = "hot-reload")]
        {
            matches!(self, Self::Development)
        }
        #[cfg(not(feature = "hot-reload"))]
        {
            false
        }
    }

    pub(super) const fn benchmark(&self) -> Option<&BenchmarkOptions> {
        match self {
            Self::Benchmark(options) => Some(options),
            _ => None,
        }
    }

    pub(super) const fn startup_benchmark(&self) -> Option<StartupBenchmarkOptions> {
        match self {
            Self::StartupBenchmark(options) => Some(*options),
            _ => None,
        }
    }

    pub(super) const fn requires_single_instance(&self) -> bool {
        matches!(self, Self::Shipping)
    }
}

#[derive(Debug)]
pub(super) enum CliCommand {
    AuthoringHost {
        endpoint: String,
        token: String,
    },
    Validate {
        project: PathBuf,
    },
    Pack {
        project: PathBuf,
        output: PathBuf,
    },
    Bundle {
        project: PathBuf,
        output: PathBuf,
        benchmark: bool,
    },
    BenchmarkReport {
        project: PathBuf,
        runs: usize,
        report_path: PathBuf,
    },
    PackageBenchmark {
        project: PathBuf,
    },
    Remap {
        project: PathBuf,
        rules: Vec<(String, String)>,
        yes: bool,
    },
    Migrate {
        source: PathBuf,
        target: PathBuf,
    },
    Run {
        project: PathBuf,
        mode: InteractiveMode,
        editor_sync: bool,
    },
}

impl CliCommand {
    pub(super) const fn uses_startup_error_page(&self) -> bool {
        matches!(
            self,
            Self::Run {
                mode: InteractiveMode::Shipping,
                ..
            }
        )
    }
}

struct CommandHelp {
    name: &'static str,
    args: &'static str,
    summary: &'static str,
}

const COMMANDS: &[CommandHelp] = &[
    CommandHelp {
        name: "validate",
        args: "<project>",
        summary: "Validate without opening a window",
    },
    #[cfg(feature = "hot-reload")]
    CommandHelp {
        name: "dev",
        args: "<project> [--sync]",
        summary: "Run with hot reload",
    },
    #[cfg(feature = "publisher")]
    CommandHelp {
        name: "bundle",
        args: "<project> [--output <dir>] [--benchmark]",
        summary: "Build a complete distributable game",
    },
    #[cfg(feature = "publisher")]
    CommandHelp {
        name: "migrate",
        args: "<source-project> <target-project>",
        summary: "Convert a compatibility project to native Eiyashou",
    },
    #[cfg(feature = "publisher")]
    CommandHelp {
        name: "pack",
        args: "<project> [--output <dir>]",
        summary: "Build only a Hakutaku resource package",
    },
    #[cfg(feature = "publisher")]
    CommandHelp {
        name: "remap",
        args: "<project> <old=new>... [-y]",
        summary: "Update resource references after conversion",
    },
    CommandHelp {
        name: "perf",
        args: "<project> [options]",
        summary: "Measure frames or startup",
    },
];

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(super) fn help_or_version(args: &[OsString]) -> Option<ExitCode> {
    let first = args.first().map(|argument| argument.to_string_lossy());
    let requested_help = args
        .iter()
        .any(|argument| argument == "-h" || argument == "--help");
    let requested_version = args
        .iter()
        .any(|argument| argument == "-V" || argument == "--version");
    if let Some(name) = first.as_deref()
        && requested_help
        && COMMANDS.iter().any(|command| command.name == name)
    {
        print_command_help(name);
        return Some(ExitCode::SUCCESS);
    }
    if requested_version || first.as_deref() == Some("version") {
        println!("Kēne {VERSION}");
        return Some(ExitCode::SUCCESS);
    }
    if requested_help || first.as_deref() == Some("help") {
        print_help();
        return Some(ExitCode::SUCCESS);
    }
    None
}

pub(super) fn parse(args: &[OsString]) -> Result<CliCommand> {
    let Some(command) = args.first() else {
        return Ok(run(PathBuf::new(), InteractiveMode::Shipping));
    };
    match command.to_str() {
        Some("__authoring-host") => {
            let endpoint = required_utf8(args, 1, "internal authoring host")?;
            let token = required_utf8(args, 2, "internal authoring host")?;
            require_no_extra_args(args, 3, "internal authoring host")?;
            Ok(CliCommand::AuthoringHost { endpoint, token })
        }
        Some("validate") => {
            let project = required_path(args, 1, "keine validate <project>")?;
            require_no_extra_args(args, 2, "keine validate <project>")?;
            Ok(CliCommand::Validate { project })
        }
        Some("pack") => parse_pack(args),
        Some("remap") => parse_remap(args),
        Some("bundle") => parse_bundle(args),
        Some("migrate") => {
            const USAGE: &str = "keine migrate <source-project> <target-project>";
            let source = required_path(args, 1, USAGE)?;
            let target = required_path(args, 2, USAGE)?;
            require_no_extra_args(args, 3, USAGE)?;
            Ok(CliCommand::Migrate { source, target })
        }
        #[cfg(feature = "hot-reload")]
        Some("dev") => parse_development(args),
        #[cfg(not(feature = "hot-reload"))]
        Some("dev") => anyhow::bail!("hot reload is not compiled; run `cargo dev <project>`"),
        Some("perf") => parse_perf(args),
        Some("__benchmark-package") => {
            let project = required_path(args, 1, "internal package benchmark")?;
            require_no_extra_args(args, 2, "internal package benchmark")?;
            Ok(CliCommand::PackageBenchmark { project })
        }
        Some(name) if name.starts_with('-') => anyhow::bail!("unknown option {name:?}"),
        _ => {
            if args.len() > 1 {
                anyhow::bail!("unknown command {command:?}");
            }
            let project = PathBuf::from(command);
            if !project.exists()
                && project.components().count() == 1
                && project.extension().is_none()
            {
                anyhow::bail!("unknown command or missing project {command:?}");
            }
            Ok(run(project, InteractiveMode::Shipping))
        }
    }
}

fn parse_pack(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine pack <project> [--output <dir>]";
    let project = required_path(args, 1, USAGE)?;
    let mut output = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].to_str() {
            Some("--output") if output.is_none() => {
                let value = args.get(index + 1).filter(|value| !value.is_empty());
                output = Some(PathBuf::from(value.with_context(|| {
                    format!("--output requires a path argument; usage: {USAGE}")
                })?));
                index += 2;
            }
            Some(argument) => anyhow::bail!("unexpected argument {argument:?}; usage: {USAGE}"),
            None => anyhow::bail!("pack argument is not UTF-8; usage: {USAGE}"),
        }
    }
    Ok(CliCommand::Pack {
        project,
        output: output.unwrap_or_else(|| PathBuf::from(DEFAULT_ASSET_PACKAGE_OUTPUT)),
    })
}

fn parse_bundle(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine bundle <project> [--output <dir>] [--benchmark]";
    let project = required_path(args, 1, USAGE)?;
    let mut output = None;
    let mut benchmark = false;
    let mut index = 2;
    while index < args.len() {
        match args[index].to_str() {
            Some("--output") if output.is_none() => {
                let value = args.get(index + 1).filter(|value| !value.is_empty());
                output = Some(PathBuf::from(value.with_context(|| {
                    format!("--output requires a path argument; usage: {USAGE}")
                })?));
                index += 2;
            }
            Some("--benchmark") if !benchmark => {
                benchmark = true;
                index += 1;
            }
            Some(argument) => anyhow::bail!("unexpected argument {argument:?}; usage: {USAGE}"),
            None => anyhow::bail!("bundle argument is not UTF-8; usage: {USAGE}"),
        }
    }
    let mut output = output.unwrap_or_else(|| PathBuf::from(DEFAULT_BUNDLE_OUTPUT));
    if benchmark {
        output = benchmark_output_path(&output)?;
    }
    Ok(CliCommand::Bundle {
        project,
        output,
        benchmark,
    })
}

fn benchmark_output_path(output: &Path) -> Result<PathBuf> {
    let name = output
        .file_name()
        .context("benchmark output directory must have a final component")?;
    if name.to_string_lossy().ends_with("-benchmark") {
        return Ok(output.to_owned());
    }
    let mut benchmark_name = name.to_os_string();
    benchmark_name.push("-benchmark");
    Ok(output.with_file_name(benchmark_name))
}

fn parse_remap(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine remap <project> <old=new>... [-y]";
    let project = required_path(args, 1, USAGE)?;
    let mut rules = Vec::new();
    let mut yes = false;
    for argument in &args[2..] {
        if argument == "-y" {
            if yes {
                anyhow::bail!("-y may only be specified once");
            }
            yes = true;
            continue;
        }
        let argument = argument
            .to_str()
            .with_context(|| format!("extension rule is not UTF-8; usage: {USAGE}"))?;
        let Some((from, to)) = argument.split_once('=') else {
            anyhow::bail!("invalid extension rule {argument:?}; usage: {USAGE}");
        };
        rules.push((from.to_owned(), to.to_owned()));
    }
    if rules.is_empty() {
        anyhow::bail!("at least one extension rule is required; usage: {USAGE}");
    }
    Ok(CliCommand::Remap {
        project,
        rules,
        yes,
    })
}

pub(super) fn resolve_project_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return std::env::current_exe().ok().map_or_else(
            || PathBuf::from(".").join("game.haku"),
            |executable| packaged_project_path(&executable),
        );
    }
    if path.is_absolute() {
        return path.to_owned();
    }
    std::env::current_dir()
        .unwrap_or_else(|error| {
            log::warn!("failed to read current directory: {error}");
            PathBuf::from(".")
        })
        .join(path)
}

fn packaged_project_path(executable: &Path) -> PathBuf {
    let executable_dir = executable.parent().unwrap_or_else(|| Path::new("."));
    let sibling = executable_dir.join("game.haku");
    if sibling.is_file() {
        return sibling;
    }
    // A native macOS app launches Contents/MacOS/keine directly while its
    // signed content belongs in Contents/Resources. Keeping this fallback in
    // the executable removes the need for an app-bundle shell launcher.
    let app_resource = executable_dir
        .parent()
        .map(|contents| contents.join("Resources/game.haku"));
    app_resource
        .filter(|path| path.is_file())
        .unwrap_or(sibling)
}

fn run(project: PathBuf, mode: InteractiveMode) -> CliCommand {
    CliCommand::Run {
        project,
        mode,
        editor_sync: false,
    }
}

#[cfg(feature = "hot-reload")]
fn parse_development(args: &[OsString]) -> Result<CliCommand> {
    let project = required_path(args, 1, "keine dev <project> [--sync]")?;
    let mut editor_sync = false;
    for argument in &args[2..] {
        if argument == "--sync" && !editor_sync {
            editor_sync = true;
        } else {
            anyhow::bail!("unexpected argument {argument:?}; usage: keine dev <project> [--sync]");
        }
    }
    Ok(CliCommand::Run {
        project,
        mode: InteractiveMode::Development,
        editor_sync,
    })
}

fn parse_perf(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine perf <project> [--seconds N] [--timeline ID | --cursor N] [--camera PROFILE] | --startup [--runs N]";
    let project = required_path(args, 1, USAGE)?;
    let mut seconds = None;
    let mut target = None;
    let mut cameras = None;
    let mut startup = false;
    let mut runs = None;
    let mut index = 2;
    while index < args.len() {
        let option = args[index]
            .to_str()
            .with_context(|| format!("perf option is not UTF-8; usage: {USAGE}"))?;
        match option {
            "--startup" if !startup => {
                startup = true;
                index += 1;
                continue;
            }
            "--seconds" if seconds.is_none() => {
                let value = required_utf8(args, index + 1, USAGE)?;
                let parsed = value.parse::<f32>().context("--seconds must be a number")?;
                if !parsed.is_finite() || parsed < 1.0 {
                    anyhow::bail!("--seconds must be at least 1");
                }
                seconds = Some(parsed);
            }
            "--timeline" if target.is_none() => {
                target = Some(BenchmarkTarget::Timeline(required_utf8(
                    args,
                    index + 1,
                    USAGE,
                )?));
            }
            "--cursor" if target.is_none() => {
                target = Some(BenchmarkTarget::Cursor(
                    required_utf8(args, index + 1, USAGE)?
                        .parse::<usize>()
                        .context("--cursor must be an integer")?,
                ));
            }
            "--camera" if cameras.is_none() => {
                cameras = Some(match required_utf8(args, index + 1, USAGE)?.as_str() {
                    "runtime" => crate::ui::performance::BenchmarkCameras::Runtime,
                    "scene-ui" => crate::ui::performance::BenchmarkCameras::SceneUi,
                    "scene-dialog" => crate::ui::performance::BenchmarkCameras::SceneDialog,
                    "scene" => crate::ui::performance::BenchmarkCameras::SceneOnly,
                    _ => {
                        anyhow::bail!("--camera expects runtime, scene-ui, scene-dialog, or scene")
                    }
                });
            }
            "--runs" if runs.is_none() => {
                let parsed = required_utf8(args, index + 1, USAGE)?
                    .parse::<usize>()
                    .context("--runs must be an integer")?;
                if !(1..=50).contains(&parsed) {
                    anyhow::bail!("--runs must be between 1 and 50");
                }
                runs = Some(parsed);
            }
            _ => anyhow::bail!("unexpected perf option {option:?}; usage: {USAGE}"),
        }
        index += 2;
    }
    if startup {
        if seconds.is_some() || target.is_some() || cameras.is_some() {
            anyhow::bail!("--startup cannot be combined with frame sampling options");
        }
        return Ok(run(
            project,
            InteractiveMode::StartupBenchmark(StartupBenchmarkOptions {
                runs: runs.unwrap_or(7),
            }),
        ));
    }
    if runs.is_some() {
        anyhow::bail!("--runs requires --startup");
    }
    Ok(run(
        project,
        InteractiveMode::Benchmark(BenchmarkOptions {
            seconds: seconds.unwrap_or(15.0),
            target,
            cameras: cameras.unwrap_or(crate::ui::performance::BenchmarkCameras::Runtime),
        }),
    ))
}

pub(super) fn packaged_benchmark_command() -> Result<Option<CliCommand>> {
    let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_owned))
    else {
        return Ok(None);
    };
    let marker = root.join(BENCHMARK_MARKER);
    if !marker.is_file() {
        return Ok(None);
    }
    let bytes = crate::storage::read_limited(&marker, 32)?;
    let runs = std::str::from_utf8(&bytes)
        .context("benchmark marker is not UTF-8")?
        .trim()
        .parse::<usize>()
        .context("benchmark marker does not contain a startup run count")?;
    if !(1..=50).contains(&runs) {
        anyhow::bail!("benchmark marker run count must be between 1 and 50");
    }
    Ok(Some(CliCommand::BenchmarkReport {
        project: root.join("game.haku"),
        runs,
        report_path: root.join(BENCHMARK_REPORT_FILE),
    }))
}

fn required_path(args: &[OsString], index: usize, usage: &str) -> Result<PathBuf> {
    let value = args
        .get(index)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("missing project path; usage: {usage}"))?;
    Ok(PathBuf::from(value))
}

fn required_utf8(args: &[OsString], index: usize, usage: &str) -> Result<String> {
    args.get(index)
        .context(format!("missing argument; usage: {usage}"))?
        .to_str()
        .map(str::to_owned)
        .context("argument is not valid UTF-8")
}

fn require_no_extra_args(args: &[OsString], expected: usize, usage: &str) -> Result<()> {
    if let Some(argument) = args.get(expected) {
        anyhow::bail!("unexpected argument {argument:?}; usage: {usage}");
    }
    Ok(())
}

fn cargo_invocation() -> bool {
    std::env::var_os("CARGO").is_some()
}

fn command_usage(command: &CommandHelp, cargo: bool) -> String {
    let prefix = if cargo { "cargo" } else { "keine" };
    if command.args.is_empty() {
        format!("{prefix} {}", command.name)
    } else {
        format!("{prefix} {} {}", command.name, command.args)
    }
}

fn print_help() {
    let cargo = cargo_invocation();
    let prefix = if cargo { "cargo" } else { "keine" };
    println!("Kēne {VERSION}");
    println!("A native visual-novel engine with WebGAL and LetsGal compatibility.");
    println!("\nUsage: {prefix} <command> [args]\n\nCommands:");
    for command in COMMANDS {
        println!("  {}", command_usage(command, cargo));
        println!("      {}", command.summary);
    }
    if !cargo {
        println!(
            "  {:<60}Run a packaged or directory project",
            "keine <project>"
        );
    }
    println!("\nOptions:");
    println!("  -h, --help     Show this help");
    println!("  -V, --version  Show version");
}

fn print_command_help(name: &str) {
    let command = COMMANDS
        .iter()
        .find(|command| command.name == name)
        .expect("caller matched a known command");
    println!("Kēne {VERSION}");
    println!("\nUsage: {}", command_usage(command, cargo_invocation()));
    println!("\n{}", command.summary);
    if name == "perf" {
        println!("\nFrame options:");
        println!("  --seconds N       Sample duration (default: 15)");
        println!("  --timeline ID     Authored timeline name");
        println!("  --cursor N        Numeric cursor instead of a timeline");
        println!("  --camera PROFILE  runtime, scene-ui, scene-dialog, or scene");
        println!("\nStartup options:");
        println!("  --startup         Measure isolated launches instead of frames");
        println!("  --runs N          Number of launches (default: 7; max: 50)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_bundle_uses_a_separate_suffixed_directory() {
        let command = parse(&[
            "bundle".into(),
            "projects/test-project".into(),
            "--output".into(),
            "target/colleague".into(),
            "--benchmark".into(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            CliCommand::Bundle {
                output,
                benchmark: true,
                ..
            } if output == Path::new("target/colleague-benchmark")
        ));
    }

    #[test]
    fn normal_bundle_keeps_its_original_directory() {
        let command = parse(&["bundle".into(), "projects/test-project".into()]).unwrap();
        assert!(matches!(
            command,
            CliCommand::Bundle {
                output,
                benchmark: false,
                ..
            } if output == Path::new(DEFAULT_BUNDLE_OUTPUT)
        ));
    }

    #[test]
    fn pack_has_a_resource_only_default_output() {
        let command = parse(&["pack".into(), "projects/test-project".into()]).unwrap();
        assert!(matches!(
            command,
            CliCommand::Pack { output, .. }
                if output == Path::new(DEFAULT_ASSET_PACKAGE_OUTPUT)
        ));
    }

    #[test]
    fn migrate_requires_exactly_two_project_paths() {
        let command = parse(&[
            "migrate".into(),
            "legacy-project".into(),
            "native-project".into(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            CliCommand::Migrate { source, target }
                if source == Path::new("legacy-project")
                    && target == Path::new("native-project")
        ));
        assert!(parse(&["migrate".into(), "legacy-project".into()]).is_err());
        assert!(
            parse(&[
                "migrate".into(),
                "legacy-project".into(),
                "native-project".into(),
                "extra".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn pack_and_remap_are_separate_commands() {
        assert!(parse(&["pack".into()]).is_err());
        assert!(
            parse(&[
                "pack".into(),
                "projects/test-project".into(),
                "--remap".into(),
            ])
            .is_err()
        );
        assert!(parse(&["remap".into(), "projects/test-project".into()]).is_err());
        assert!(parse(&["configure".into()]).is_err());
    }

    #[test]
    fn benchmark_suffix_is_idempotent() {
        assert_eq!(
            benchmark_output_path(Path::new("target/game-benchmark")).unwrap(),
            Path::new("target/game-benchmark")
        );
    }

    #[test]
    fn packaged_project_supports_a_script_free_macos_bundle() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let app = std::env::temp_dir().join(format!("keine-cli-{nonce}.app/Contents"));
        let executable = app.join("MacOS/keine");
        let project = app.join("Resources/game.haku");
        std::fs::create_dir_all(project.parent().unwrap()).unwrap();
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&project, b"test").unwrap();

        assert_eq!(packaged_project_path(&executable), project);

        std::fs::remove_dir_all(app.parent().unwrap()).unwrap();
    }
}
