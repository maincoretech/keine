# Desktop video acceptance fixtures

These files contain only FFmpeg-generated test patterns and tones. The
established `playback.mp4` is the AVFoundation-compatible seed. Regenerate all
edge variants from that seed at the repository root with:

```text
dev/tools/video_fixtures.sh
```

`FFMPEG_BIN` may select another FFmpeg executable. The checked-in edge set was
generated with FFmpeg 9.0.1 and `libx264`; the script uses stream copies where
possible and fixes the long-GOP duration, level, encoder thread count,
keyframe interval, and metadata.

| Fixture | Contract exercised |
| --- | --- |
| `playback.mp4` | 1 s H.264 Baseline + stereo AAC, fast-start |
| `no-audio.mp4` | 1 s H.264 Baseline with no audio stream |
| `long-gop.mp4` | 4 s H.264 Baseline + AAC with one 96-frame GOP |
| `tail-moov.mp4` | stream-copy of `playback.mp4` with `moov` after `mdat` |
| `damaged-header.mp4` | first 128 bytes of `playback.mp4`, truncated inside its header |

The acceptance binary decodes each valid fixture to EOF, rewinds it, and does
the same through an encrypted Hakutaku mount. Invoke it with `--expect-error`
for `damaged-header.mp4`; rejection is required for both source types.
