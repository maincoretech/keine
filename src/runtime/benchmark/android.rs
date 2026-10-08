//! ADB requests and result files for the separate Android benchmark package.
//! Shipping games never read these requests. Each sample uses a fresh process.
#[cfg(any(target_os = "android", all(test, feature = "publisher")))]
use super::cli::{CliCommand, InteractiveMode, parse};
#[cfg(any(target_os = "android", all(test, feature = "publisher")))]
use anyhow::{Context, Result, bail};

#[cfg(any(target_os = "android", all(test, feature = "publisher")))]
pub(super) enum Request {
    Render(InteractiveMode),
    Package,
}

#[cfg(any(target_os = "android", all(test, feature = "publisher")))]
fn parse_request(bytes: &[u8]) -> Result<Request> {
    if bytes.len() > 4096 {
        bail!("benchmark request exceeds 4096 bytes");
    }
    let request: serde_json::Value = serde_json::from_slice(bytes)?;
    if request["schema"] != 1 {
        bail!("unsupported benchmark request schema");
    }
    if request["kind"] == "package" {
        return Ok(Request::Package);
    }
    if request["kind"] != "render" && request["kind"] != "startup" {
        bail!("invalid benchmark request kind");
    }
    let args = request["args"]
        .as_array()
        .context("benchmark arguments missing")?;
    if args.len() > 24 {
        bail!("too many benchmark arguments");
    }
    let args = args
        .iter()
        .map(|arg| {
            let arg = arg.as_str().context("benchmark argument is not a string")?;
            if arg.len() > 128 {
                bail!("benchmark argument too long");
            }
            Ok(std::ffi::OsString::from(arg))
        })
        .collect::<Result<Vec<_>>>()?;
    let CliCommand::Run {
        mode,
        editor_sync: false,
        ..
    } = parse(&args)?
    else {
        bail!("only performance requests are allowed");
    };
    match &mode {
        InteractiveMode::Benchmark(options)
            if request["kind"] == "render"
                && options.seconds <= 60.0
                && options.raw
                && options.window == super::cli::BenchmarkWindow::Default => {}
        InteractiveMode::StartupBenchmark(options)
            if request["kind"] == "startup" && options.runs == 1 => {}
        _ => bail!("unsupported Android benchmark mode"),
    }
    Ok(Request::Render(mode))
}

#[cfg(all(target_os = "android", feature = "startup-metrics"))]
mod device {
    use super::*;
    use std::{
        fs::{self, File},
        io::{Read, Write},
        path::Path,
        sync::OnceLock,
    };
    static REPORT: OnceLock<File> = OnceLock::new();
    static DIRECTORY: OnceLock<std::path::PathBuf> = OnceLock::new();

    pub(in crate::runtime) fn begin(data: &Path) -> Result<Request> {
        let directory = data.join("benchmark");
        fs::create_dir_all(&directory)?;
        let file = File::create(directory.join("sample.txt"))?;
        REPORT
            .set(file)
            .map_err(|_| anyhow::anyhow!("benchmark already initialized"))?;
        DIRECTORY
            .set(directory.clone())
            .map_err(|_| anyhow::anyhow!("benchmark already initialized"))?;
        let _ = fs::remove_file(directory.join("finished"));
        write_line(&format!(
            "build identity · Kēne {} · commit {} · built {} · features {} · Android profiling",
            env!("CARGO_PKG_VERSION"),
            env!("KEINE_BUILD_COMMIT"),
            env!("KEINE_BUILD_TIME"),
            env!("KEINE_BUILD_FEATURES")
        ));
        let mut bytes = Vec::new();
        File::open(directory.join("request.json"))?
            .take(4097)
            .read_to_end(&mut bytes)?;
        parse_request(&bytes)
    }
    pub(crate) fn report_file() -> Option<&'static File> {
        REPORT.get()
    }
    pub(crate) fn write_line(line: &str) -> bool {
        if let Some(mut file) = REPORT.get() {
            return writeln!(file, "{line}").is_ok();
        }
        false
    }
    pub(crate) fn complete() {
        if let (Some(file), Some(directory)) = (REPORT.get(), DIRECTORY.get()) {
            // A marker is written only after the measured result. The host still
            // validates samples, source resolution, focus and engine errors.
            if file.sync_all().is_ok() {
                let _ = fs::write(directory.join("finished"), b"complete\n");
            }
        }
    }
    pub(crate) fn failure(error: &anyhow::Error) {
        write_line(&format!("ERROR benchmark: {error:#}"));
        complete();
    }
}
#[cfg(all(target_os = "android", feature = "startup-metrics"))]
pub(super) use device::begin;
#[cfg(all(target_os = "android", feature = "startup-metrics"))]
pub(crate) use device::{complete, failure, report_file, write_line};

#[cfg(all(test, feature = "publisher"))]
mod tests {
    use super::*;
    #[test]
    fn requests_are_bounded_and_only_allow_one_supported_sample() {
        let request = |kind, args| {
            serde_json::to_vec(&serde_json::json!({"schema":1,"kind":kind,"args":args})).unwrap()
        };
        for (kind, args, valid) in [
            (
                "render",
                vec!["perf", "embedded", "--raw", "--timeline", "bench_baseline"],
                true,
            ),
            (
                "startup",
                vec!["perf", "embedded", "--startup", "--runs", "1"],
                true,
            ),
            (
                "startup",
                vec!["perf", "embedded", "--startup", "--runs", "7"],
                false,
            ),
            (
                "render",
                vec!["perf", "embedded", "--raw", "--seconds", "61"],
                false,
            ),
            (
                "render",
                vec!["perf", "embedded", "--raw", "--window", "1280x720"],
                false,
            ),
            ("render", vec!["dev", "embedded"], false),
            ("render", vec!["validate", "embedded"], false),
        ] {
            assert_eq!(
                parse_request(&request(kind, args.clone())).is_ok(),
                valid,
                "{kind} {args:?}"
            );
        }
        assert!(matches!(
            parse_request(&request("package", Vec::<&str>::new())).unwrap(),
            Request::Package
        ));
        assert!(parse_request(&vec![b' '; 4097]).is_err());
        assert!(parse_request(b"{\"schema\":2}").is_err());
        // Exercise the payload, too, so the platform-independent boundary test
        // covers the mode consumed by the NativeActivity build.
        let Request::Render(mode) =
            parse_request(&request("render", vec!["perf", "embedded", "--raw"])).unwrap()
        else {
            panic!("mode missing")
        };
        assert!(mode.benchmark().is_some());
    }
    #[test]
    fn plan_accounts_for_every_desktop_workload_with_explicit_platform_skips() {
        let plan = super::super::bootstrap::android_benchmark_plan();
        let samples = plan["samples"].as_array().unwrap();
        let required = samples
            .iter()
            .filter(|s| s["required"] == true)
            .collect::<Vec<_>>();
        assert_eq!(required.len(), 78);
        assert_eq!(required.iter().filter(|s| s["skip"].is_null()).count(), 73);
        assert_eq!(required.iter().filter(|s| !s["skip"].is_null()).count(), 5);
        for sample in samples.iter().filter(|s| s["skip"].is_null()) {
            let mut request = sample.clone();
            request["schema"] = 1.into();
            assert!(
                parse_request(&serde_json::to_vec(&request).unwrap()).is_ok(),
                "{sample}"
            );
        }
    }
}
