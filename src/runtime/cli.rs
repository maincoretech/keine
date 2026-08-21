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
    Development,
    Benchmark(BenchmarkOptions),
    StartupBenchmark(StartupBenchmarkOptions),
}

impl InteractiveMode {
    pub(super) const fn development(&self) -> bool {
        matches!(self, Self::Development)
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
    Configure,
    Check {
        project: PathBuf,
    },
    AssetsPack {
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
    RemapAssets {
        project: PathBuf,
        rules: Vec<(String, String)>,
        yes: bool,
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
    binary_name: &'static str,
    cargo_name: &'static str,
    args: &'static str,
    summary: &'static str,
}

const COMMANDS: &[CommandHelp] = &[
    CommandHelp {
        binary_name: "configure",
        cargo_name: "configure",
        args: "",
        summary: "Configure built-in engine capabilities",
    },
    CommandHelp {
        binary_name: "check",
        cargo_name: "validate",
        args: "<project>",
        summary: "Validate without opening a window",
    },
    CommandHelp {
        binary_name: "assets",
        cargo_name: "assets",
        args: "--pack <project> [--output <dir>] | --remap <project> <old=new>... [-y]",
        summary: "Pack project assets or remap their references",
    },
    CommandHelp {
        binary_name: "bundle",
        cargo_name: "bundle",
        args: "<project> [--output <dir>] [--benchmark]",
        summary: "Build a complete distributable game",
    },
    CommandHelp {
        binary_name: "dev",
        cargo_name: "dev",
        args: "<project> [--sync]",
        summary: "Run with hot reload and video",
    },
    CommandHelp {
        binary_name: "benchmark",
        cargo_name: "perf",
        args: "<project> [seconds] [timeline|cursor] [profile]",
        summary: "Record a performance sample",
    },
    CommandHelp {
        binary_name: "benchmark-startup",
        cargo_name: "startup-perf",
        args: "<project> [runs]",
        summary: "Repeat process-cold startup measurements",
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
        && COMMANDS.iter().any(|command| command.binary_name == name)
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
        Some("configure" | "adapters") => {
            require_no_extra_args(args, 1, "keine configure")?;
            Ok(CliCommand::Configure)
        }
        Some("check") => {
            let project = required_path(args, 1, "keine check <project>")?;
            require_no_extra_args(args, 2, "keine check <project>")?;
            Ok(CliCommand::Check { project })
        }
        Some("assets") => parse_assets(args),
        Some("bundle") => parse_bundle(args),
        Some("package") => anyhow::bail!(
            "`keine package` was split by responsibility; use `keine assets --pack <project>` for a resource package or `keine bundle <project>` for a complete game"
        ),
        Some("dev") => parse_development(args),
        Some("benchmark") => parse_benchmark(args),
        Some("benchmark-startup") => parse_startup_benchmark(args),
        Some("__benchmark-package") => {
            let project = required_path(args, 1, "internal package benchmark")?;
            require_no_extra_args(args, 2, "internal package benchmark")?;
            Ok(CliCommand::PackageBenchmark { project })
        }
        Some("validate" | "perf" | "startup-perf") => anyhow::bail!(
            "{command:?} is a Cargo alias, not a keine subcommand; run `cargo {}` or `keine --help`",
            command.to_string_lossy()
        ),
        Some(name) if name.starts_with('-') => anyhow::bail!("unknown option {name:?}"),
        _ => {
            require_no_extra_args(args, 1, "keine <project>")?;
            Ok(run(PathBuf::from(command), InteractiveMode::Shipping))
        }
    }
}

fn parse_assets(args: &[OsString]) -> Result<CliCommand> {
    match args.get(1).and_then(|argument| argument.to_str()) {
        Some("--pack") => parse_asset_pack(args),
        Some("--remap") => parse_asset_remap(args),
        Some(argument) => anyhow::bail!(
            "unknown assets operation {argument:?}; use `keine assets --pack ...` or `keine assets --remap ...`"
        ),
        None => anyhow::bail!(
            "missing assets operation; use `keine assets --pack ...` or `keine assets --remap ...`"
        ),
    }
}

fn parse_asset_pack(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine assets --pack <project> [--output <dir>]";
    let project = required_path(args, 2, USAGE)?;
    let mut output = None;
    let mut index = 3;
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
            None => anyhow::bail!("assets argument is not UTF-8; usage: {USAGE}"),
        }
    }
    Ok(CliCommand::AssetsPack {
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

fn parse_asset_remap(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine assets --remap <project> <old=new>... [-y]";
    let project = required_path(args, 2, USAGE)?;
    let mut rules = Vec::new();
    let mut yes = false;
    for argument in &args[3..] {
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
    Ok(CliCommand::RemapAssets {
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

fn parse_benchmark(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine benchmark <project> [seconds] [timeline|cursor] [profile]";
    let project = required_path(args, 1, USAGE)?;
    require_no_extra_args(args, 5, USAGE)?;
    let seconds = match args.get(2) {
        Some(value) => value
            .to_string_lossy()
            .parse::<f32>()
            .context("benchmark duration must be a number of seconds")?,
        None => 15.0,
    };
    if !seconds.is_finite() || seconds < 1.0 {
        anyhow::bail!("benchmark duration must be at least one second");
    }
    let target = args.get(3).map(|value| {
        let value = value.to_string_lossy();
        value.parse::<usize>().map_or_else(
            |_| BenchmarkTarget::Timeline(value.into_owned()),
            BenchmarkTarget::Cursor,
        )
    });
    let cameras = match args.get(4).and_then(|value| value.to_str()) {
        None | Some("full") => crate::ui::performance::BenchmarkCameras::Full,
        Some("scene-ui") => crate::ui::performance::BenchmarkCameras::SceneUi,
        Some("scene-dialog") => crate::ui::performance::BenchmarkCameras::SceneDialog,
        Some("scene") => crate::ui::performance::BenchmarkCameras::SceneOnly,
        Some(value) => anyhow::bail!(
            "unknown benchmark camera profile {value:?}; expected full, scene-ui, scene-dialog, or scene"
        ),
    };
    Ok(run(
        project,
        InteractiveMode::Benchmark(BenchmarkOptions {
            seconds,
            target,
            cameras,
        }),
    ))
}

fn parse_startup_benchmark(args: &[OsString]) -> Result<CliCommand> {
    const USAGE: &str = "keine benchmark-startup <project> [runs]";
    let project = required_path(args, 1, USAGE)?;
    require_no_extra_args(args, 3, USAGE)?;
    let runs = match args.get(2) {
        Some(value) => value
            .to_string_lossy()
            .parse::<usize>()
            .context("startup benchmark runs must be an integer")?,
        None => 7,
    };
    if !(1..=50).contains(&runs) {
        anyhow::bail!("startup benchmark runs must be between 1 and 50");
    }
    Ok(run(
        project,
        InteractiveMode::StartupBenchmark(StartupBenchmarkOptions { runs }),
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
    let (prefix, name) = if cargo {
        ("cargo", command.cargo_name)
    } else {
        ("keine", command.binary_name)
    };
    if command.args.is_empty() {
        format!("{prefix} {name}")
    } else {
        format!("{prefix} {name} {}", command.args)
    }
}

fn print_help() {
    let cargo = cargo_invocation();
    let prefix = if cargo { "cargo" } else { "keine" };
    println!("Kēne {VERSION}");
    println!("A native visual-novel engine with WebGAL and LetsGal compatibility.");
    println!("\nUsage: {prefix} <command> [args]\n\nCommands:");
    for command in COMMANDS {
        println!("  {:<60}{}", command_usage(command, cargo), command.summary);
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
        .find(|command| command.binary_name == name)
        .expect("caller matched a known command");
    println!("Kēne {VERSION}");
    println!("\nUsage: {}", command_usage(command, cargo_invocation()));
    println!("\n{}", command.summary);
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
    fn assets_pack_has_a_resource_only_default_output() {
        let command = parse(&[
            "assets".into(),
            "--pack".into(),
            "projects/test-project".into(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            CliCommand::AssetsPack { output, .. }
                if output == Path::new(DEFAULT_ASSET_PACKAGE_OUTPUT)
        ));
    }

    #[test]
    fn assets_modes_are_explicit_and_mutually_exclusive() {
        assert!(parse(&["assets".into()]).is_err());
        assert!(
            parse(&[
                "assets".into(),
                "--pack".into(),
                "projects/test-project".into(),
                "--remap".into(),
            ])
            .is_err()
        );
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
