use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    const WINDOWS_ICON: &str = "assets/icons/keine.ico";

    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=crates");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-env-changed=KEINE_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    track_git_head();
    println!("cargo:rustc-env=KEINE_BUILD_TIME={}", build_time());
    println!("cargo:rustc-env=KEINE_BUILD_COMMIT={}", build_commit());
    println!("cargo:rustc-env=KEINE_BUILD_FEATURES={}", build_features());
    println!("cargo:rerun-if-changed={WINDOWS_ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    winresource::WindowsResource::new()
        .set_icon(WINDOWS_ICON)
        .compile()
        .expect("failed to embed the Kēne Windows icon");
}

fn track_git_head() {
    let git = Path::new(".git");
    let head = git.join("HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    let Ok(contents) = std::fs::read_to_string(&head) else {
        return;
    };
    let Some(reference) = contents.trim().strip_prefix("ref: ") else {
        return;
    };
    println!("cargo:rerun-if-changed={}", git.join(reference).display());
}

fn build_commit() -> String {
    let configured = std::env::var("KEINE_BUILD_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("GITHUB_SHA")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    let commit = configured
        .or_else(git_head)
        .unwrap_or_else(|| "unknown".into());
    let mut identity = commit.trim().chars().take(12).collect::<String>();
    if git_dirty() {
        identity.push_str("-dirty");
    }
    identity
}

fn git_head() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

fn build_features() -> String {
    let mut features = std::env::vars()
        .filter_map(|(name, _)| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .map(|name| name.to_ascii_lowercase().replace('_', "-"))
        .collect::<Vec<_>>();
    features.sort();
    features.join(",")
}

fn build_time() -> String {
    let seconds = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before the Unix epoch")
                .as_secs()
        });
    format_utc(seconds)
}

fn format_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    // Convert days since 1970-01-01 to the proleptic Gregorian calendar.
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}
