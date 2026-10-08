# Bevy audio output recovery patch

Source: official [bevy_audio 0.19.1](https://crates.io/crates/bevy_audio/0.19.1).
Upstream MIT/Apache licenses and decoder/sink APIs are retained. Kēne additions
in `recovery.rs` use the repository's Defold License 1.0.

- `audio_output.rs` uses the persistent output's mixer; queued sources wait for
  hardware availability. Existing Players/SpatialPlayers are never rebuilt.
- `lib.rs` exports the shared `AudioOutput` resource and includes recovery.
- `recovery.rs` owns the mixer reader and default hardware stream on a worker.
  One `try_lock` per output buffer keeps source iterators outside the stream's
  lifetime. Sample-rate/channel conversion reuses Rodio's converters.
- Windows detects default endpoint ID changes without waiting for stream errors.
  CoreAudio DefaultOutput keeps automatic system routing while callbacks remain
  healthy. AAudio disconnects and callback stalls request bounded retries.
  Error callbacks never stop/close/reopen a stream; see [AAudio disconnected
  streams](https://developer.android.com/ndk/guides/audio/aaudio/aaudio#disconnected_audio_stream).
- Output absence freezes source consumption. Submitted playback time drives
  engine fades; availability notifications wake the native event loop.
- `audio_source.rs` retains upstream `std::io::Cursor`: Rust 1.97 Clippy suggests
  the unstable `core::io` API; one module-level lint expectation records this.

No new shipping dependency. Cargo.toml is the official normalized manifest.
The workspace member's optional upstream codec features add lockfile entries
when resolving all features. They do not expand production game features.

On upgrade, compare these three upstream files and reapply or remove the patch.
Tests cover idle/new queued playback, retained paused Players across format
changes, no decoding during outages, reconnect, bounded retry, and endpoint
change policies. They do not replace native Bluetooth/USB/wired listening tests.
Format changes may discard a few converter look-ahead samples; driver buffers
and exact physical playback latency cannot be recovered.
