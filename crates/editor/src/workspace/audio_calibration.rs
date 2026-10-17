//! Offline measurement only. Runtime receives a bounded fixed gain.
use keine_core::config::{EiyashouAssetManifest, valid_audio_gain};
use noyalib::{Value, cst::parse_document};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub(crate) struct Measurement {
    pub integrated: Option<f32>,
    pub true_peak: f32,
}
impl Measurement {
    // A conservative authoring starting point, not a delivery standard.
    // Short/silent clips without integrated loudness never receive automatic boost.
    pub fn suggested(&self) -> f32 {
        let gain = self
            .integrated
            .map_or(0.0, |i| -18.0 - f64::from(i))
            .min(-1.0 - f64::from(self.true_peak))
            .clamp(-60.0, 12.0);
        // Use the displayed precision, rounding down to preserve peak headroom.
        ((gain * 10.0).floor() / 10.0) as f32
    }
}

pub(crate) fn measure(root: &Path, relative: &Path) -> Result<Measurement, String> {
    let path = super::file_ops::confined_existing(root, relative).map_err(|e| e.to_string())?;
    let executable = super::file_ops::ffmpeg_executable()
        .map_err(|_| "Install FFmpeg to analyze loudness".to_owned())?;
    let mut command = Command::new(executable);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-nostats",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            "ogg,wav,mp3,flac,aac,mov",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-af",
            "loudnorm=I=-18:TP=-1:LRA=11:dual_mono=true:print_format=json",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stderr = child.stderr.take().ok_or("Could not read FFmpeg output")?;
    // Drain the pipe, retaining only the bounded tail containing the measurement.
    let reader = std::thread::spawn(move || {
        let mut tail = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(n) = stderr.read(&mut buffer) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..n]);
            if tail.len() > 16384 {
                tail.drain(..tail.len() - 16384);
            }
        }
        tail
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(120) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            other => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match other {
                    Err(e) => e.to_string(),
                    _ => "Analysis timed out".into(),
                });
            }
        }
    };
    let output = reader
        .join()
        .map_err(|_| "Could not read analysis output")?;
    if !status?.success() {
        return Err("Could not analyze this audio file".into());
    }
    parse_measurement(&String::from_utf8_lossy(&output))
}

fn parse_measurement(output: &str) -> Result<Measurement, String> {
    let start = output
        .rfind('{')
        .ok_or("No loudness measurement was returned")?;
    let value: serde_json::Value = serde_json::Deserializer::from_str(&output[start..])
        .into_iter::<serde_json::Value>()
        .next()
        .ok_or("No loudness measurement was returned")?
        .map_err(|_| "Could not read loudness measurement")?;
    let number = |key| {
        value[key]
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
    };
    Ok(Measurement {
        integrated: number("input_i"),
        true_peak: number("input_tp").ok_or("No measurable audio (silence)")?,
    })
}

pub(crate) fn edit_gain(
    source: &str,
    namespace: &str,
    id: &str,
    path: &str,
    gain: f32,
) -> Result<String, String> {
    if !matches!(namespace, "bgm" | "voices" | "se") || !valid_audio_gain(gain) {
        return Err("Invalid audio gain".into());
    }
    let manifest = EiyashouAssetManifest::from_yaml(source).map_err(|e| e.to_string())?;
    let entries = match namespace {
        "bgm" => &manifest.bgm,
        "voices" => &manifest.voices,
        _ => &manifest.effects,
    };
    let entry = entries
        .get(id)
        .filter(|e| e.path() == path)
        .ok_or("Audio entry changed; select it again")?;
    let key = format!("{namespace}[{}]", serde_json::to_string(id).unwrap());
    let mut doc = parse_document(source).map_err(|e| e.to_string())?;
    // Keep f32's shortest decimal instead of exposing its f64 expansion in YAML.
    let decimal: f64 = gain.to_string().parse().map_err(|_| "Invalid audio gain")?;
    if matches!(entry, keine_core::config::EiyashouAssetEntry::Path(_)) {
        if gain == 0.0 {
            return Ok(source.to_owned());
        }
        // A scalar shorthand becomes a flow mapping; retaining the inline comment.
        let value = serde_json::json!({"path": path, "gain_db": decimal}).to_string();
        doc.set(&key, &value).map_err(|e| e.to_string())?;
    } else {
        doc.set_path(&format!("{key}.gain_db"), &Value::from(decimal))
            .map_err(|e| e.to_string())?;
    }
    let result = doc.to_string();
    EiyashouAssetManifest::from_yaml(&result).map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires FFmpeg; exercises actual offline measurement"]
    fn offline_measurement_preserves_audio_and_handles_silence() {
        let root = std::env::temp_dir().join(format!("keine-loudness-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ffmpeg = super::super::file_ops::ffmpeg_executable().unwrap();
        for (name, filter) in [
            ("tone.wav", "sine=frequency=1000:duration=3"),
            ("silence.wav", "anullsrc=r=48000:cl=stereo"),
        ] {
            let file = root.join(name);
            assert!(
                Command::new(&ffmpeg)
                    .args([
                        "-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i", filter, "-t", "3"
                    ])
                    .arg(&file)
                    .status()
                    .unwrap()
                    .success()
            );
            let before = std::fs::read(&file).unwrap();
            let result = measure(&root, Path::new(name));
            assert_eq!(std::fs::read(&file).unwrap(), before);
            if name == "tone.wav" {
                let m = result.unwrap();
                assert!((-20.0..-16.0).contains(&m.integrated.unwrap()), "{m:?}");
                assert!((-19.0..-17.0).contains(&m.true_peak), "{m:?}");
                assert!(m.true_peak + m.suggested() <= -1.0);
            } else {
                assert!(result.unwrap_err().contains("silence"));
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn measurement_and_peak_limited_gain() {
        let m = parse_measurement("info\n{\"input_i\":\"-25.0\",\"input_tp\":\"-3.0\"}\n").unwrap();
        assert_eq!(m.suggested(), 2.0);
        assert_eq!(
            Measurement {
                integrated: Some(-9.0),
                true_peak: 0.0
            }
            .suggested(),
            -9.0
        );
        assert_eq!(
            Measurement {
                integrated: None,
                true_peak: -12.0
            }
            .suggested(),
            0.0
        );
        assert!(parse_measurement("{\"input_i\":\"-inf\",\"input_tp\":\"-inf\"}").is_err());
    }
    #[test]
    fn audio_gain_edits_preserve_paths_tags_and_comments() {
        for source in [
            "bgm:\n  track: assets/music.opus # keep\nse: {}\n",
            "bgm:\n  track:\n    path: assets/music.opus\n    tags: [calm] # keep\nse: {}\n",
        ] {
            let changed = edit_gain(source, "bgm", "track", "assets/music.opus", -3.9).unwrap();
            assert!(changed.contains("-3.9"));
            assert!(!changed.contains("-3.9000"));
            assert!(changed.contains("# keep"));
            assert!(changed.contains("se: {}"));
            let parsed = EiyashouAssetManifest::from_yaml(&changed).unwrap();
            assert_eq!(parsed.bgm["track"].gain_db(), -3.9);
            let reset = edit_gain(&changed, "bgm", "track", "assets/music.opus", 0.0).unwrap();
            assert_eq!(
                EiyashouAssetManifest::from_yaml(&reset).unwrap().bgm["track"].gain_db(),
                0.0
            );
        }
        assert!(edit_gain("bgm: {}", "bgm", "gone", "a.opus", 2.0).is_err());
        assert!(edit_gain("bgm: {}", "bgm", "gone", "a.opus", f32::NAN).is_err());
    }
}
