//! Command-line surface and strict argument parsing.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy)]
pub(super) struct BenchmarkOptions {
    pub(super) seconds: f32,
    pub(super) cursor: Option<usize>,
    pub(super) cameras: crate::ui::performance::BenchmarkCameras,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum InteractiveMode {
    Shipping,
    Development,
    CompiledPreview,
    Benchmark(BenchmarkOptions),
}

impl InteractiveMode {
    pub(super) const fn development(self) -> bool {
        matches!(self, Self::Development)
    }

    pub(super) const fn benchmark(self) -> Option<BenchmarkOptions> {
        match self {
            Self::Benchmark(options) => Some(options),
            _ => None,
        }
    }

    pub(super) const fn requires_single_instance(self) -> bool {
        matches!(self, Self::Shipping | Self::CompiledPreview)
    }
}

#[derive(Debug)]
pub(super) enum CliCommand {
    Adapters,
    Check {
        project: PathBuf,
    },
    Compile {
        project: PathBuf,
        output: Option<PathBuf>,
    },
    Package {
        project: PathBuf,
        output: PathBuf,
    },
    Run {
        project: PathBuf,
        mode: InteractiveMode,
        editor_sync: bool,
    },
}

struct CommandHelp {
    binary_name: &'static str,
    cargo_name: &'static str,
    args: &'static str,
    summary: &'static str,
}

const COMMANDS: &[CommandHelp] = &[
    CommandHelp {
        binary_name: "adapters",
        cargo_name: "adapters",
        args: "",
        summary: "Enable or disable built-in adapters",
    },
    CommandHelp {
        binary_name: "check",
        cargo_name: "validate",
        args: "<project>",
        summary: "Validate without opening a window",
    },
    CommandHelp {
        binary_name: "compiler",
        cargo_name: "compiler",
        args: "<project> [--output <path>]",
        summary: "Compile source scripts into a program.bin artifact",
    },
    CommandHelp {
        binary_name: "package",
        cargo_name: "package",
        args: "<project> [--output <dir>]",
        summary: "Package an encrypted release build",
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
        args: "<project> [seconds] [cursor] [profile]",
        summary: "Record a performance sample",
    },
];

const CARGO_ONLY_COMMANDS: &[(&str, &str, &str)] =
    &[("preview", "<project>", "Run an optimized preview")];
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
        Some("adapters") => {
            require_no_extra_args(args, 1, "keine adapters")?;
            Ok(CliCommand::Adapters)
        }
        Some("check") => {
            let project = required_path(args, 1, "keine check <project>")?;
            require_no_extra_args(args, 2, "keine check <project>")?;
            Ok(CliCommand::Check { project })
        }
        Some("compiler") if args.get(1).is_some_and(|arg| arg == "preview") => {
            let project = required_path(args, 2, "keine compiler preview <project>")?;
            require_no_extra_args(args, 3, "keine compiler preview <project>")?;
            Ok(run(project, InteractiveMode::CompiledPreview))
        }
        Some("compiler") => {
            let project = required_path(args, 1, "keine compiler <project>")?;
            let output = parse_output_option(&args[2..])?;
            Ok(CliCommand::Compile { project, output })
        }
        Some("package") => {
            let project = required_path(args, 1, "keine package <project>")?;
            let output = parse_output_option(&args[2..])?
                .unwrap_or_else(|| PathBuf::from(crate::package::DEFAULT_OUTPUT));
            Ok(CliCommand::Package { project, output })
        }
        Some("dev") => parse_development(args),
        Some("benchmark") => parse_benchmark(args),
        Some("validate" | "perf" | "preview") => anyhow::bail!(
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

pub(super) fn resolve_project_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
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
    const USAGE: &str = "keine benchmark <project> [seconds] [cursor] [profile]";
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
    let cursor = args
        .get(3)
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .context("benchmark cursor must be a non-negative action index")
        })
        .transpose()?;
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
            cursor,
            cameras,
        }),
    ))
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

fn parse_output_option(args: &[OsString]) -> Result<Option<PathBuf>> {
    match args {
        [] => Ok(None),
        [flag, value] if flag == "--output" && !value.is_empty() => Ok(Some(PathBuf::from(value))),
        [flag] if flag == "--output" => anyhow::bail!("--output requires a path argument"),
        [argument, ..] => anyhow::bail!("unexpected argument {argument:?}"),
    }
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
    if cargo {
        for (name, args, summary) in CARGO_ONLY_COMMANDS {
            println!("  {:<60}{}", format!("cargo {name} {args}"), summary);
        }
    } else {
        println!(
            "  {:<60}Run a packaged or directory project",
            "keine <project>"
        );
        println!(
            "  {:<60}Run an existing compiled program",
            "keine compiler preview <project>"
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
