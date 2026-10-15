//! Bounded benchmark child processes, host metadata and report formatting.
use super::workloads::*;
use crate::runtime::benchmark_supervisor::{
    BenchmarkSupervisor, IO_LIMIT, SAMPLE_LIMIT, SUITE_LIMIT,
};
use crate::runtime::cli::BenchmarkWindow;
use crate::ui::performance::BenchmarkCameras;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub(crate) const STARTUP_BENCHMARK_CHILD_ENV: &str = "KEINE_STARTUP_BENCHMARK_CHILD";
pub(crate) const RUNTIME_BENCHMARK_CHILD_ENV: &str = "KEINE_RUNTIME_BENCHMARK_CHILD";

pub(crate) fn run_startup_suite(project_path: &Path, runs: usize) -> Result<String> {
    let mut report = String::new();
    let mut supervisor = BenchmarkSupervisor::new(runs, SUITE_LIMIT, None)?;
    collect_startup_samples(project_path, runs, &mut report, &mut supervisor)?;
    Ok(report)
}

fn collect_startup_samples(
    project_path: &Path,
    runs: usize,
    report: &mut String,
    supervisor: &mut BenchmarkSupervisor,
) -> Result<()> {
    let executable =
        std::env::current_exe().context("failed to locate the benchmark executable")?;
    let mut samples = Vec::with_capacity(runs);
    let logical_threads = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let profile = if cfg!(debug_assertions) {
        "development"
    } else {
        "release"
    };
    emit_report_line(
        report,
        format!(
            "startup baseline · Kēne {} · {profile} · {} / {} · {logical_threads} logical thread(s) · {runs} isolated process run(s) · hidden surface-backed window",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
    );
    let features = env!("KEINE_BUILD_FEATURES");
    emit_report_line(
        report,
        format!(
            "build identity · commit {} · built {} · features {}",
            env!("KEINE_BUILD_COMMIT"),
            env!("KEINE_BUILD_TIME"),
            if features.is_empty() {
                "none"
            } else {
                features
            },
        ),
    );
    append_host_environment(report, logical_threads);
    for run in 1..=runs {
        let mut command = Command::new(&executable);
        command
            .arg("perf")
            .arg(project_path)
            .args(["--startup", "--runs", "1"])
            .env(STARTUP_BENCHMARK_CHILD_ENV, "1");
        supervisor.checkpoint(report, "")?;
        let (output, wall) =
            supervisor.run(&mut command, &format!("startup {run}/{runs}"), SAMPLE_LIMIT)?;
        emit_report_line(
            report,
            format!(
                "CHILD    | startup {run}/{runs} · {:.2}s total wall",
                wall.as_secs_f64()
            ),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if run == 1
            && let Some(gpu) = stderr.lines().find(|line| line.contains("GPU      │"))
        {
            emit_report_line(report, gpu.trim());
        }
        let sample = crate::ui::performance::StartupSample::parse(&format!("{stdout}\n{stderr}"));
        if !output.status.success()
            || sample.is_none()
            || stderr.lines().any(|line| line.contains(" ERROR "))
        {
            anyhow::bail!(
                "startup child {run} failed with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                output.status,
            );
        }
        let sample = sample.expect("sample presence was checked");
        let peak_rss = sample
            .peak_rss_mib
            .map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1} MiB"));
        emit_report_line(
            report,
            format!(
                "run {run:>2} · project {:>7.2} ms · app {:>7.2} ms · first frame {:>7.2} ms · interactive {:>7.2} ms · peak RSS {peak_rss}",
                sample.project_ms, sample.app_ms, sample.first_frame_ms, sample.interactive_ms,
            ),
        );
        samples.push(sample);
        if run != runs {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    append_startup_summary(&samples, report);
    supervisor.checkpoint(report, "")?;
    Ok(())
}

fn append_host_environment(report: &mut String, logical_threads: usize) {
    let memory = physical_memory_bytes().map_or_else(
        || "unknown RAM".to_owned(),
        |bytes| format!("{:.1} GiB RAM", bytes as f64 / 1_073_741_824.0),
    );
    emit_report_line(
        report,
        format!(
            "host environment · OS {} · CPU {} · {logical_threads} logical thread(s) · {memory}",
            host_os_version(),
            host_cpu_model(),
        ),
    );
    if let Some(power) = host_power_context() {
        emit_report_line(report, format!("power context · {power}"));
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn clean_command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|output| !output.is_empty())
}

#[cfg(windows)]
fn host_cpu_model() -> String {
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(target_os = "macos")]
fn host_cpu_model() -> String {
    clean_command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn host_cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "model name").then(|| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(not(any(windows, unix)))]
fn host_cpu_model() -> String {
    "unknown".to_owned()
}

#[cfg(windows)]
fn host_os_version() -> String {
    clean_command_output("cmd", &["/C", "ver"]).unwrap_or_else(|| "Windows".to_owned())
}

#[cfg(target_os = "macos")]
fn host_os_version() -> String {
    clean_command_output("sw_vers", &["-productVersion"])
        .map_or_else(|| "macOS".to_owned(), |version| format!("macOS {version}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn host_os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|value| value.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_owned())
}

#[cfg(not(any(windows, unix)))]
fn host_os_version() -> String {
    std::env::consts::OS.to_owned()
}

#[cfg(windows)]
fn host_power_context() -> Option<String> {
    clean_command_output("powercfg", &["/getactivescheme"])
}

#[cfg(not(windows))]
fn host_power_context() -> Option<String> {
    None
}

#[cfg(all(feature = "startup-metrics", windows))]
fn physical_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` has the required size and remains writable for the call.
    let success = unsafe { GlobalMemoryStatusEx(&mut status) };
    (success != 0).then_some(status.ullTotalPhys)
}

#[cfg(all(feature = "startup-metrics", unix))]
fn physical_memory_bytes() -> Option<u64> {
    // SAFETY: `sysconf` has no pointer arguments and these names query stable
    // process-global system values.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    // SAFETY: See above; the page-size query has the same contract.
    let page_bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (pages > 0 && page_bytes > 0)
        .then(|| (pages as u64).checked_mul(page_bytes as u64))
        .flatten()
}

#[cfg(not(any(
    all(feature = "startup-metrics", windows),
    all(feature = "startup-metrics", unix)
)))]
fn physical_memory_bytes() -> Option<u64> {
    None
}

pub(crate) fn run_benchmark_report(
    project_path: &Path,
    runs: usize,
    report_path: &Path,
) -> Result<()> {
    let mut report = String::new();
    let mut raw_frames = format!(
        "RAWFRAME\tworkload\trun\ttotal_runs\ttarget\tcamera_profile\t{}\n",
        crate::ui::performance::TRACE_FIELDS,
    );
    let render_runs = portable_benchmark_render_runs();
    let mut supervisor =
        BenchmarkSupervisor::new(runs + render_runs + 1, SUITE_LIMIT, Some(report_path))?;
    emit_report_line(
        &mut report,
        format!(
            "PLAN     | {runs} startup + {render_runs} render + 1 I/O sample(s) · {:.1} min minimum render warm-up/sample time · 30 min suite wall-clock limit · 60s per startup/render, 120s I/O",
            render_runs as f64 * 8.0 / 60.0
        ),
    );
    emit_report_line(
        &mut report,
        "REPORT   | incremental evidence saved after each sample; timeout/error means INCOMPLETE, never full coverage",
    );
    supervisor.checkpoint(&report, "")?;
    let outcome = (|| -> Result<()> {
        let executable =
            std::env::current_exe().context("failed to locate benchmark executable")?;
        collect_startup_samples(project_path, runs, &mut report, &mut supervisor)?;
        // Measure real sleep/wake behavior separately from continuous throughput.
        let mut preliminary_failed = false;
        if let Err(error) = run_benchmark_workload(
            BenchmarkWorkloadCapture {
                supervisor: &mut supervisor,
                executable: &executable,
                project_path,
                label: "opening composition · runtime sleep/wake",
                target: None,
                cameras: BenchmarkCameras::Runtime,
                window: BenchmarkWindow::Default,
                continuous: false,
                run: 1,
                total_runs: 1,
            },
            &mut report,
            &mut raw_frames,
        ) {
            preliminary_failed = true;
            emit_report_line(
                &mut report,
                format!("FAILED   | runtime opening · {error:#}"),
            );
        }
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "project workload · actual packaged opening composition",
        );
        emit_report_line(
            &mut report,
            "portable render coverage · continuous window loop · runtime cameras auto-disable empty overlays · decomposition pins cameras",
        );
        let timeline_inventory = run_benchmark_workload(
            BenchmarkWorkloadCapture {
                supervisor: &mut supervisor,
                executable: &executable,
                project_path,
                label: "opening composition · runtime composition",
                target: None,
                cameras: BenchmarkCameras::Runtime,
                window: BenchmarkWindow::Default,
                continuous: true,
                run: 1,
                total_runs: 1,
            },
            &mut report,
            &mut raw_frames,
        )?
        .stderr;
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "camera decomposition · opening composition render attribution",
        );
        for (label, cameras) in CAMERA_BENCHMARK_WORKLOADS {
            run_benchmark_workload(
                BenchmarkWorkloadCapture {
                    supervisor: &mut supervisor,
                    executable: &executable,
                    project_path,
                    label,
                    target: None,
                    cameras: *cameras,
                    window: BenchmarkWindow::Default,
                    continuous: true,
                    run: 1,
                    total_runs: 1,
                },
                &mut report,
                &mut raw_frames,
            )?;
        }
        let mut completed = 0;
        let mut missing = 0;
        let mut failed = 0;
        let mut ranked = Vec::new();
        for (section, workloads) in PORTABLE_BENCHMARK_SECTIONS {
            emit_report_line(&mut report, "");
            emit_report_line(&mut report, section);
            for (label, target) in *workloads {
                supervisor.check_budget()?;
                if !timeline_is_available(&timeline_inventory, target) {
                    missing += 1;
                    emit_report_line(
                        &mut report,
                        format!("MISSING  | {label} · target {target:?}"),
                    );
                    continue;
                }
                let total_runs = benchmark_workload_runs(target);
                let mut summaries = Vec::with_capacity(total_runs);
                for run in 1..=total_runs {
                    supervisor.check_budget()?;
                    match run_benchmark_workload(
                        BenchmarkWorkloadCapture {
                            supervisor: &mut supervisor,
                            executable: &executable,
                            project_path,
                            label,
                            target: Some(target),
                            cameras: BenchmarkCameras::Runtime,
                            window: BenchmarkWindow::Default,
                            continuous: true,
                            run,
                            total_runs,
                        },
                        &mut report,
                        &mut raw_frames,
                    ) {
                        Ok(output) => summaries.push(output.summary),
                        Err(error) => {
                            emit_report_line(
                                &mut report,
                                format!("FAILED   | {label} · run {run}/{total_runs} · {error:#}"),
                            );
                            break;
                        }
                    }
                }
                if summaries.len() != total_runs {
                    failed += 1;
                    continue;
                }
                completed += 1;
                if total_runs > 1 {
                    append_render_repeat_summary(label, &summaries, &mut report);
                }
                summaries.sort_by(|a, b| a.average_ms.total_cmp(&b.average_ms));
                ranked.push((*label, *target, summaries[total_runs / 2]));
            }
        }
        // Visible windows exercise actual presentation and fill cost, separately
        // from the fixed-size hidden throughput samples used for shader attribution.
        for (label, window, target) in [
            (
                "720p particles",
                BenchmarkWindow::Size(1280, 720),
                "bench_particle_snow_256",
            ),
            (
                "1080p particles",
                BenchmarkWindow::Size(1920, 1080),
                "bench_particle_snow_256",
            ),
            (
                "runtime dialogue",
                BenchmarkWindow::Default,
                "bench_representative_dialogue",
            ),
            (
                "runtime particles",
                BenchmarkWindow::Default,
                "bench_particle_snow_256",
            ),
            (
                "runtime audio",
                BenchmarkWindow::Default,
                "bench_audio_loop",
            ),
            (
                "runtime video",
                BenchmarkWindow::Default,
                "bench_video_fullscreen",
            ),
            (
                "fullscreen stress",
                BenchmarkWindow::Fullscreen,
                "bench_stress_composition",
            ),
        ] {
            supervisor.check_budget()?;
            if !timeline_is_available(&timeline_inventory, target) {
                missing += 1;
                emit_report_line(
                    &mut report,
                    format!("MISSING  | {label} · target {target:?}"),
                );
                continue;
            }
            match run_benchmark_workload(
                BenchmarkWorkloadCapture {
                    supervisor: &mut supervisor,
                    executable: &executable,
                    project_path,
                    label,
                    target: Some(target),
                    cameras: BenchmarkCameras::Runtime,
                    window,
                    continuous: !label.starts_with("runtime "),
                    run: 1,
                    total_runs: 1,
                },
                &mut report,
                &mut raw_frames,
            ) {
                Ok(_) => completed += 1,
                Err(error) => {
                    failed += 1;
                    emit_report_line(&mut report, format!("FAILED   | {label} · {error:#}"));
                }
            }
        }
        append_hotspot_ranking(&ranked, &mut report);
        emit_report_line(
            &mut report,
            format!(
                "COVERAGE | {completed} completed · {missing} missing · {failed} failed · {} required render workloads · {}",
                PORTABLE_BENCHMARK_SECTIONS
                    .iter()
                    .map(|(_, w)| w.len())
                    .sum::<usize>()
                    + 7,
                if missing == 0 && failed == 0 {
                    "complete"
                } else {
                    "INCOMPLETE: not a full benchmark"
                }
            ),
        );
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "warm Hakutaku/cache throughput · real assets and isolated access-class stress",
        );
        emit_report_line(
            &mut report,
            "scope · package open/decrypt/cache/memory path; not a cold-cache or physical-device benchmark",
        );
        let mut package_failed = false;
        let package = (|| -> Result<String> {
            supervisor.checkpoint(&report, &raw_frames)?;
            let mut command = Command::new(&executable);
            command.arg("__benchmark-package").arg(project_path);
            let (output, wall) = supervisor.run(&mut command, "package I/O", IO_LIMIT)?;
            emit_report_line(
                &mut report,
                format!(
                    "CHILD    | package I/O · {:.2}s total wall",
                    wall.as_secs_f64()
                ),
            );
            if !output.status.success() {
                anyhow::bail!(
                    "package I/O failed: {}\n{}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            String::from_utf8(output.stdout).context("package I/O output is not UTF-8")
        })();
        match package {
            Ok(output) => {
                for line in output.lines() {
                    emit_report_line(&mut report, line);
                }
            }
            Err(error) => {
                package_failed = true;
                emit_report_line(&mut report, format!("FAILED   | package I/O · {error:#}"));
            }
        }
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "raw frame appendix · bounded tab-separated samples for offline analysis",
        );
        if missing != 0 || failed != 0 || package_failed || preliminary_failed {
            anyhow::bail!(
                "benchmark incomplete ({missing} missing, {failed} failed, package failed: {package_failed}, runtime opening failed: {preliminary_failed}); partial evidence retained in {}",
                report_path.display()
            );
        }
        Ok(())
    })();
    if let Err(error) = &outcome {
        emit_report_line(
            &mut report,
            format!("FAILED   | suite incomplete · {error:#}"),
        );
    }
    supervisor.checkpoint(&report, &raw_frames)?;
    drop(supervisor);
    report.push_str(&raw_frames);
    crate::storage::write_atomically(report_path, report.as_bytes())?;
    println!("benchmark report written to {}", report_path.display());
    outcome
}

struct BenchmarkWorkloadOutput {
    stderr: String,
    summary: crate::ui::performance::RenderSummary,
}

struct BenchmarkWorkloadCapture<'a> {
    supervisor: &'a mut BenchmarkSupervisor,
    executable: &'a Path,
    project_path: &'a Path,
    label: &'a str,
    target: Option<&'a str>,
    cameras: BenchmarkCameras,
    window: BenchmarkWindow,
    continuous: bool,
    run: usize,
    total_runs: usize,
}

fn run_benchmark_workload(
    capture: BenchmarkWorkloadCapture<'_>,
    report: &mut String,
    raw_frames: &mut String,
) -> Result<BenchmarkWorkloadOutput> {
    let BenchmarkWorkloadCapture {
        supervisor,
        executable,
        project_path,
        label,
        target,
        cameras,
        window,
        continuous,
        run,
        total_runs,
    } = capture;
    emit_report_line(report, "");
    emit_report_line(
        report,
        if total_runs == 1 {
            format!("settled render · {label} · 3.0s warm-up + 5.0s sample")
        } else {
            format!(
                "settled render · {label} · run {run}/{total_runs} · 3.0s warm-up + 5.0s sample"
            )
        },
    );
    let mut command = Command::new(executable);
    let mode = if continuous { "continuous" } else { "runtime" };
    command
        .arg("perf")
        .arg(project_path)
        .args(["--seconds", "5", "--mode", mode, "--raw"]);
    // Reactive redraws require a real window; a hidden window cannot measure idle behavior.
    if continuous && window == BenchmarkWindow::Default {
        command.env(RUNTIME_BENCHMARK_CHILD_ENV, "1");
    } else {
        command.env_remove(RUNTIME_BENCHMARK_CHILD_ENV);
    }
    match window {
        BenchmarkWindow::Default => {}
        BenchmarkWindow::Size(width, height) => {
            command.args(["--window", &format!("{width}x{height}")]);
        }
        BenchmarkWindow::Fullscreen => {
            command.args(["--window", "fullscreen"]);
        }
    }
    if let Some(target) = target {
        command.arg("--timeline").arg(target);
    }
    if cameras != BenchmarkCameras::Runtime {
        command.arg("--camera").arg(cameras.id());
    }
    supervisor.checkpoint(report, raw_frames)?;
    let (output, wall) = supervisor.run(&mut command, label, SAMPLE_LIMIT)?;
    emit_report_line(
        report,
        format!("CHILD    | {label} · {:.2}s total wall", wall.as_secs_f64()),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        anyhow::bail!(
            "{label} benchmark failed with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status,
        );
    }
    if let Some(target) = target
        && !stderr.contains("resolved cursor Some(")
    {
        anyhow::bail!("{label} benchmark did not resolve timeline {target:?}\n{stderr}");
    }
    let mut captured = 0;
    let mut focused_intervals = 0;
    let mut summary = None;
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if let Some(frame) = line.strip_prefix("KEINE_TRACE\t") {
            focused_intervals += usize::from(frame.split('\t').nth(10) == Some("true"));
            append_raw_frame(raw_frames, label, run, total_runs, target, cameras, frame);
            continue;
        }
        if let Some(sample) = crate::ui::performance::RenderSummary::parse(line) {
            summary = Some(sample);
            continue;
        }
        if benchmark_report_line(line) {
            emit_report_line(report, line);
            captured += 1;
        }
    }
    if captured == 0 {
        anyhow::bail!("{label} benchmark completed without performance output");
    }
    let summary =
        summary.context("benchmark completed without a machine-readable render sample")?;
    if stderr.lines().any(|line| line.contains(" ERROR ")) {
        anyhow::bail!("{label} reported an engine error\n{stderr}");
    }
    if !continuous && focused_intervals == 0 {
        anyhow::bail!(
            "{label} has no focused runtime intervals; leave the benchmark window foreground"
        );
    }
    if (continuous || label.starts_with("runtime ")) && summary.frames == 0 {
        anyhow::bail!("{label} has no eligible active render intervals");
    }
    supervisor.checkpoint(report, raw_frames)?;
    Ok(BenchmarkWorkloadOutput {
        stderr: stderr.into_owned(),
        summary,
    })
}

fn append_hotspot_ranking(
    results: &[(&str, &str, crate::ui::performance::RenderSummary)],
    report: &mut String,
) {
    emit_report_line(report, "");
    emit_report_line(
        report,
        "optimization candidates · controlled workload deltas, not exclusive function cost",
    );
    let Some((_, _, baseline)) = results
        .iter()
        .find(|(_, target, _)| *target == "bench_baseline")
    else {
        emit_report_line(
            report,
            "HOTSPOT  | reference unavailable; no fabricated deltas",
        );
        return;
    };
    let mut candidates = results
        .iter()
        .filter(|(_, target, s)| *target != "bench_baseline" && s.frames > 0)
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.2.average_ms.total_cmp(&a.2.average_ms));
    for (label, target, summary) in candidates.into_iter().take(15) {
        emit_report_line(
            report,
            format!(
                "HOTSPOT  | {label} · Δavg {:+.2} ms · avg {:.2} · p99 {:.2} · max {:.2} ms · 1% low {:.1} FPS · rerun: perf <package> --timeline {target:?} --mode continuous --raw",
                summary.average_ms - baseline.average_ms,
                summary.average_ms,
                summary.p99_ms,
                summary.maximum_ms,
                summary.one_percent_low_fps
            ),
        );
    }
    emit_report_line(
        report,
        "ATTRIBUTION | compare identical size/backend/cache/pacing; inspect each workload's PROCESS, MEMORY, UPDATE, RENDER and SLOW lines; frame intervals include driver/presentation/scheduling, CPU/GPU spans must not be summed; use a separate native stack capture to locate functions",
    );
}

fn append_render_repeat_summary(
    label: &str,
    summaries: &[crate::ui::performance::RenderSummary],
    report: &mut String,
) {
    let median = |select: fn(&crate::ui::performance::RenderSummary) -> f64| {
        let mut values = summaries.iter().map(select).collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    };
    let minimum_fps = summaries
        .iter()
        .map(|summary| summary.average_fps)
        .min_by(f64::total_cmp)
        .unwrap_or_default();
    let maximum_fps = summaries
        .iter()
        .map(|summary| summary.average_fps)
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    let worst_frame = summaries
        .iter()
        .map(|summary| summary.maximum_ms)
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    emit_report_line(
        report,
        format!(
            "REPEAT   | {label} · {} runs · avg FPS median {:.1} ({minimum_fps:.1}..{maximum_fps:.1}) · 1% low median {:.1} · p99 median {:.2} ms · worst frame {worst_frame:.2} ms",
            summaries.len(),
            median(|summary| summary.average_fps),
            median(|summary| summary.one_percent_low_fps),
            median(|summary| summary.p99_ms),
        ),
    );
}

fn append_raw_frame(
    output: &mut String,
    label: &str,
    run: usize,
    total_runs: usize,
    target: Option<&str>,
    cameras: BenchmarkCameras,
    frame: &str,
) {
    output.push_str("RAWFRAME\t");
    output.push_str(&tsv_text(label));
    output.push_str(&format!("\t{run}\t{total_runs}\t"));
    output.push_str(&tsv_text(target.unwrap_or("opening")));
    output.push('\t');
    output.push_str(&tsv_text(cameras.id()));
    // Already sanitized by the shared capture owner. Preserve all fields,
    // including excluded intervals, so a report can be independently audited.
    output.push('\t');
    output.push_str(frame);
    output.push('\n');
}

fn tsv_text(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn timeline_is_available(inventory: &str, wanted: &str) -> bool {
    let suffix = format!(":{wanted}");
    inventory.lines().any(|line| {
        line.split_once("TIMELINE | ")
            .is_some_and(|(_, timelines)| {
                timelines
                    .split(", ")
                    .any(|timeline| timeline.ends_with(&suffix))
            })
    })
}

fn benchmark_report_line(line: &str) -> bool {
    [
        "GPU      │",
        "GPUINFO  |",
        "WINDOWSYS |",
        "DISPLAY  |",
        "START    |",
        "CAPTURE  |",
        "FRAME    |",
        "BUDGET   |",
        "PROCESS  |",
        "SAMPLING |",
        "EXCLUDED |",
        "GPU_TIME |",
        "SLOW     |",
        "SCENE    |",
        "ASSETS   |",
        "MEMORY   |",
        "RENDER   |",
        "UPDATE   |",
        " ERROR ",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn emit_report_line(report: &mut String, line: impl AsRef<str>) {
    let line = plain_report_line(line.as_ref());
    println!("{line}");
    report.push_str(&line);
    report.push('\n');
}

fn plain_report_line(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.next_if_eq(&'[').is_some() {
            for code in characters.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        plain.push(character);
    }
    plain
}

fn append_startup_summary(samples: &[crate::ui::performance::StartupSample], report: &mut String) {
    let Some(first) = samples.first() else {
        return;
    };
    let repeat_median = |select: fn(&crate::ui::performance::StartupSample) -> f64| {
        let mut values = samples.iter().skip(1).map(select).collect::<Vec<_>>();
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        Some(values[(values.len() - 1) / 2])
    };
    let format_pair = |first: f64, repeat: Option<f64>| {
        repeat.map_or_else(
            || format!("{first:.2} / n/a ms"),
            |repeat| format!("{first:.2} / {repeat:.2} ms"),
        )
    };
    let peak_rss = samples
        .iter()
        .filter_map(|sample| sample.peak_rss_mib)
        .max_by(f64::total_cmp);
    emit_report_line(
        report,
        "first run / repeat median (cumulative from process entry)",
    );
    emit_report_line(
        report,
        format!(
            "project     · {}",
            format_pair(first.project_ms, repeat_median(|sample| sample.project_ms))
        ),
    );
    emit_report_line(
        report,
        format!(
            "app built   · {}",
            format_pair(first.app_ms, repeat_median(|sample| sample.app_ms))
        ),
    );
    emit_report_line(
        report,
        format!(
            "first frame · {}",
            format_pair(
                first.first_frame_ms,
                repeat_median(|sample| sample.first_frame_ms),
            )
        ),
    );
    emit_report_line(
        report,
        format!(
            "interactive · {}",
            format_pair(
                first.interactive_ms,
                repeat_median(|sample| sample.interactive_ms),
            )
        ),
    );
    if let Some(peak_rss) = peak_rss {
        emit_report_line(
            report,
            format!("peak RSS    · {peak_rss:.1} MiB maximum across runs"),
        );
    }
    emit_report_line(
        report,
        "cache note  · every sample is a new process; filesystem/GPU caches are intentionally not claimed cold",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_summary_separates_first_launch_from_repeat_median() {
        let sample = |app_ms, interactive_ms| crate::ui::performance::StartupSample {
            project_ms: 1.0,
            app_ms,
            first_frame_ms: interactive_ms - 5.0,
            interactive_ms,
            peak_rss_mib: Some(200.0),
        };
        let mut report = String::new();

        append_startup_summary(
            &[
                sample(1_400.0, 1_600.0),
                sample(120.0, 280.0),
                sample(140.0, 300.0),
                sample(130.0, 290.0),
            ],
            &mut report,
        );

        assert!(report.contains("first run / repeat median"));
        assert!(report.contains("app built   · 1400.00 / 130.00 ms"));
        assert!(report.contains("interactive · 1600.00 / 290.00 ms"));
    }

    #[test]
    fn portable_benchmark_ranking_does_not_fabricate_a_reference() {
        let summary = |ms| crate::ui::performance::RenderSummary {
            frames: 100,
            average_ms: ms,
            p50_ms: ms,
            p95_ms: ms,
            p99_ms: ms,
            maximum_ms: ms,
            average_fps: 1000.0 / ms,
            one_percent_low_fps: 1000.0 / ms,
            p99_equivalent_fps: 1000.0 / ms,
        };
        let mut report = String::new();
        append_hotspot_ranking(
            &[("particle", "bench_particle_snow_256", summary(20.0))],
            &mut report,
        );
        assert!(report.contains("reference unavailable"));
        assert!(!report.contains("Δavg"));
        report.clear();
        append_hotspot_ranking(
            &[
                ("baseline", "bench_baseline", summary(5.0)),
                ("particle", "bench_particle_snow_256", summary(20.0)),
                ("blur", "bench_effect_blur", summary(10.0)),
            ],
            &mut report,
        );
        assert!(report.contains("Δavg +15.00 ms"));
        assert!(
            report.find("HOTSPOT  | particle").unwrap() < report.find("HOTSPOT  | blur").unwrap()
        );
        assert!(report.contains("--timeline \"bench_particle_snow_256\""));
    }

    #[test]
    fn benchmark_report_lines_are_plain_utf8() {
        assert_eq!(
            plain_report_line("\u{1b}[2m0.1s\u{1b}[0m \u{1b}[32mINFO\u{1b}[0m Kēne"),
            "0.1s INFO Kēne"
        );
        assert_eq!(plain_report_line("普通文本"), "普通文本");
        assert!(benchmark_report_line(
            "0.1s INFO keine::performance: SLOW     | t=3.400s · 393.01 ms"
        ));
        for system in ["Wayland", "X11 (native or XWayland)", "unavailable"] {
            assert!(benchmark_report_line(&format!(
                "0.1s INFO keine::platform: WINDOWSYS | {system} · actual primary window handle"
            )));
        }
    }

    #[test]
    fn raw_frame_appendix_sanitizes_tab_separators() {
        let mut output = String::new();
        append_raw_frame(
            &mut output,
            "blur\tfamily",
            2,
            3,
            Some("bench_blur_family"),
            BenchmarkCameras::Runtime,
            "3.500000\t121\t16.750000\t6\t8.33\tchapter2\t8\tscripts/2.shou\tIdle\t12\tfalse\t1920\t1080\tsleep",
        );
        assert_eq!(
            output,
            "RAWFRAME\tblur family\t2\t3\tbench_blur_family\truntime\t3.500000\t121\t16.750000\t6\t8.33\tchapter2\t8\tscripts/2.shou\tIdle\t12\tfalse\t1920\t1080\tsleep\n"
        );
    }

    #[test]
    fn build_identity_is_available_to_portable_reports() {
        assert!(!env!("KEINE_BUILD_COMMIT").is_empty());
        assert!(!env!("KEINE_BUILD_TIME").is_empty());
    }

    #[test]
    fn portable_benchmark_matches_only_complete_authored_timeline_ids() {
        let inventory = "0.1s INFO keine::performance: TIMELINE | intro:2:bench_representative_dialogue, coverage:8:bench_blur_family\n";

        assert!(timeline_is_available(
            inventory,
            "bench_representative_dialogue"
        ));
        assert!(timeline_is_available(inventory, "bench_blur_family"));
        assert!(!timeline_is_available(inventory, "blur family"));
        assert!(!timeline_is_available("TIMELINE | ", "anything"));
    }
}
