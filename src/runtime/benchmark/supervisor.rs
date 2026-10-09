//! Wall-clock limits and incremental evidence for the desktop benchmark suite.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

pub(super) const SUITE_LIMIT: Duration = Duration::from_secs(30 * 60);
pub(super) const SAMPLE_LIMIT: Duration = Duration::from_secs(60);
pub(super) const IO_LIMIT: Duration = Duration::from_secs(120);
const LOG_LIMIT: u64 = 64 * 1024 * 1024;
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct BenchmarkSupervisor {
    started: Instant,
    limit: Duration,
    sample: usize,
    total: usize,
    checkpoint: Option<(File, usize, usize)>,
}

impl BenchmarkSupervisor {
    pub(super) fn new(total: usize, limit: Duration, report: Option<&Path>) -> Result<Self> {
        Ok(Self {
            started: Instant::now(),
            limit,
            sample: 0,
            total,
            checkpoint: report
                .map(|path| File::create(path).map(|file| (file, 0, 0)))
                .transpose()
                .context("failed to create incremental benchmark report")?,
        })
    }

    pub(super) fn check_budget(&self) -> Result<Duration> {
        let remaining = self.limit.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            bail!(
                "suite exceeded {:.0}s wall-clock limit; remaining samples were not run, coverage is INCOMPLETE",
                self.limit.as_secs_f64()
            );
        }
        Ok(remaining)
    }

    /// Append only new evidence, avoiding a growing report rewrite per sample.
    pub(super) fn checkpoint(&mut self, report: &str, raw: &str) -> Result<()> {
        if let Some((file, report_end, raw_end)) = &mut self.checkpoint {
            file.write_all(&report.as_bytes()[*report_end..])?;
            file.write_all(&raw.as_bytes()[*raw_end..])?;
            file.flush()?;
            *report_end = report.len();
            *raw_end = raw.len();
        }
        Ok(())
    }

    pub(super) fn run(
        &mut self,
        command: &mut Command,
        label: &str,
        limit: Duration,
    ) -> Result<(Output, Duration)> {
        let remaining = self.check_budget()?;
        let limit = limit.min(remaining);
        self.sample += 1;
        println!(
            "PROGRESS | sample {}/{} · suite {:.1}s / {:.0}s limit · {label}",
            self.sample,
            self.total,
            self.started.elapsed().as_secs_f64(),
            self.limit.as_secs_f64()
        );
        // Files cannot deadlock a verbose child on a full stdout/stderr pipe.
        // They also retain its last messages if initialization/render/exit hangs.
        let logs = ChildLogs::new()?;
        command
            .stdout(Stdio::from(File::create(logs.0.join("stdout"))?))
            .stderr(Stdio::from(File::create(logs.0.join("stderr"))?));
        let child = command.spawn();
        // Command retains the configured handles after spawn. Release them before
        // deleting logs, including on spawn failure (Windows forbids open-file removal).
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        let mut child = child.with_context(|| format!("failed to start {label}"))?;
        let started = Instant::now();
        let outcome = (|| -> Result<bool> {
            loop {
                if child.try_wait()?.is_some() {
                    return Ok(false);
                }
                if started.elapsed() >= limit || self.started.elapsed() >= self.limit {
                    return Ok(true);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })();
        if !matches!(outcome, Ok(false)) {
            // Reap on timeout and polling errors; never leave the measured child running.
            let _ = child.kill();
        }
        let status = child.wait().context("failed to reap benchmark child")?;
        let stdout = logs.read("stdout")?;
        let stderr = logs.read("stderr")?;
        let timed_out = outcome.context("failed to poll benchmark child")?;
        if timed_out {
            bail!(
                "{label} timed out after {:.1}s (limit {:.1}s, suite {:.1}s); child terminated; partial output:\nstdout:\n{}\nstderr:\n{}",
                started.elapsed().as_secs_f64(),
                limit.as_secs_f64(),
                self.started.elapsed().as_secs_f64(),
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
        Ok((
            Output {
                status,
                stdout,
                stderr,
            },
            started.elapsed(),
        ))
    }
}

struct ChildLogs(PathBuf);

impl ChildLogs {
    fn new() -> Result<Self> {
        for _ in 0..100 {
            let sequence = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "keine-benchmark-child-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        bail!("failed to reserve benchmark child logs")
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(self.0.join(name))?
            .take(LOG_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LOG_LIMIT {
            bail!("benchmark {name} exceeded the 64 MiB output limit");
        }
        Ok(bytes)
    }
}

impl Drop for ChildLogs {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn portable_benchmark_supervisor_times_out_and_retains_partial_output() {
        let logs = ChildLogs::new().unwrap();
        let report = logs.0.join("report");
        let mut suite =
            BenchmarkSupervisor::new(2, Duration::from_secs(10), Some(&report)).unwrap();
        suite.checkpoint("before\n", "RAWFRAME\n").unwrap();
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'started'; exec sleep 5"]);
        let error = suite
            .run(&mut command, "blocked renderer", Duration::from_millis(200))
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(error.to_string().contains("started"));
        suite
            .checkpoint("before\nFAILED\n", "RAWFRAME\nframe\n")
            .unwrap();
        assert_eq!(
            fs::read_to_string(report).unwrap(),
            "before\nRAWFRAME\nFAILED\nframe\n"
        );
        suite.limit = suite.started.elapsed() + Duration::from_millis(200);
        let error = suite
            .run(&mut command, "suite deadline", Duration::from_secs(5))
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(error.to_string().contains("started"));
        assert!(
            suite
                .run(&mut Command::new("false"), "not launched", SAMPLE_LIMIT)
                .is_err()
        );
        assert_eq!(suite.sample, 2);
    }

    #[test]
    fn portable_benchmark_supervisor_drains_output_larger_than_a_pipe() {
        let mut suite = BenchmarkSupervisor::new(1, Duration::from_secs(10), None).unwrap();
        let mut command = Command::new("sh");
        command.args(["-c", "head -c 262144 /dev/zero; printf 'done' >&2"]);
        let (output, _) = suite
            .run(&mut command, "verbose child", Duration::from_secs(5))
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 262144);
        assert_eq!(output.stderr, b"done");
    }
}
