#!/usr/bin/env python3
"""Capture a real project and native call stacks; never rewrite project input."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time

BUNDLE_ROOT = Path(__file__).resolve().parent
PACKAGED = (BUNDLE_ROOT / "keine-benchmark.conf").is_file()
ROOT = BUNDLE_ROOT if PACKAGED else Path(__file__).resolve().parents[2]
TRACE_FIELDS = ("elapsed_seconds", "frame", "interval_ms", "update_to_render_ms",
                "budget_ms", "scene", "next_cursor", "source", "activity", "line",
                "focused", "width", "height", "exclusion")


def read_frames(log):
    frames = []
    for line in log.splitlines():
        workload = {}
        if line.startswith("RAWFRAME\t"):
            values = line.split("\t")[1:]
            if values[0] == "workload":
                continue
            if len(values) != 5 + len(TRACE_FIELDS):
                raise ValueError("incomplete RAWFRAME row")
            workload = dict(zip(("workload", "run", "total_runs", "target", "camera_profile"), values[:5]))
            values = values[5:]
        elif line.startswith("KEINE_TRACE\t"):
            values = line.split("\t")[1:]
        else:
            continue
        if len(values) != len(TRACE_FIELDS):
            raise ValueError("incomplete KEINE_TRACE row")
        frame = dict(zip(TRACE_FIELDS, values))
        for key in ("elapsed_seconds", "interval_ms", "update_to_render_ms"):
            frame[key] = float(frame[key])
        frame["budget_ms"] = None if frame["budget_ms"] == "unknown" else float(frame["budget_ms"])
        for key in ("frame", "next_cursor", "line", "width", "height"):
            frame[key] = int(frame[key])
        frame["focused"] = frame["focused"] == "true"
        frame.update(workload)
        frames.append(frame)
    return frames


def rank_locations(frames):
    """Slow-frame attribution, not exclusive CPU time or proof of causation."""
    locations = {}
    for frame in frames:
        budget = frame["budget_ms"]
        if frame["exclusion"] != "none" or budget is None or frame["interval_ms"] <= budget:
            continue
        key = (frame["scene"], frame["source"], frame["line"], frame["activity"],
               frame.get("workload"), frame.get("camera_profile"))
        item = locations.setdefault(key, dict(zip(("scene", "source", "line", "activity", "workload", "camera_profile"), key),
                                             over_budget_frames=0, excess_ms=0.0, worst_interval_ms=0.0))
        item["over_budget_frames"] += 1
        item["excess_ms"] += frame["interval_ms"] - budget
        item["worst_interval_ms"] = max(item["worst_interval_ms"], frame["interval_ms"])
    return sorted(locations.values(), key=lambda item: item["excess_ms"], reverse=True)[:20]


def stack_command(pid, duration, output):
    if platform.system() == "Darwin" and shutil.which("sample"):
        # sample(1): interval is milliseconds. 10 ms avoids the 1 kHz default.
        return ["sample", str(pid), str(duration), "10", "-mayDie", "-file", str(output / "stacks.txt")]
    if platform.system() == "Linux" and shutil.which("perf"):
        return ["perf", "record", "-F", "99", "-g", "--call-graph", "dwarf", "-p", str(pid),
                "-o", str(output / "perf.data"), "--", "sleep", str(duration)]
    return None


def file_digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def source_digests(project):
    if (project / "game.haku").is_file():
        # Compiled bundles have no authoring sources. Hash the immutable store.
        files = [project / "game.haku"] + sorted((project / "data").glob("*.taku"))
        return {str(p.relative_to(project)): file_digest(p) for p in files}
    return {str(p.relative_to(project)): file_digest(p) for p in sorted(project.rglob("*"))
            if p.is_file() and p.suffix in {".shou", ".yaml", ".yml"}}


def readable_stacks(output):
    tool = shutil.which("c++filt") or shutil.which("llvm-cxxfilt")
    if not tool:
        return "unavailable: no Rust-capable symbol filter installed"
    raw = (output / "stacks.txt").read_text(encoding="utf-8", errors="replace")
    try:
        result = subprocess.run([tool], input=raw, capture_output=True, text=True,
                                encoding="utf-8", timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        return f"unavailable: {error}"
    if result.returncode != 0 or result.stdout == raw:
        return "unavailable: symbol filter did not resolve Rust symbols; raw stack retained"
    (output / "stacks-readable.txt").write_text(result.stdout, encoding="utf-8")
    return "stacks-readable.txt"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path, nargs="?", default=BUNDLE_ROOT if PACKAGED else None)
    executable = "keine.exe" if platform.system() == "Windows" else "keine"
    parser.add_argument("--binary", type=Path, default=(BUNDLE_ROOT if PACKAGED else ROOT / "target/profiling") / executable)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, help="convert a suite's existing report to JSON without rerunning it")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--mode", choices=("runtime", "continuous"), default="runtime")
    parser.add_argument("--scene")
    parser.add_argument("--cursor", type=int)
    parser.add_argument("--timeline")
    parser.add_argument("--hz", type=float)
    parser.add_argument("--window", help="WIDTHxHEIGHT or fullscreen; actual physical size is recorded")
    parser.add_argument("--camera", choices=("runtime", "scene", "scene-ui", "scene-dialog"), default="runtime")
    parser.add_argument("--stacks", choices=("auto", "off"), default="auto")
    args = parser.parse_args()
    if args.report:
        frames = read_frames(args.report.read_text(encoding="utf-8"))
        if not frames:
            parser.error("report has no attributed frame records")
        args.output.mkdir(parents=True, exist_ok=False)
        (args.output / "frames.json").write_text(json.dumps(frames, ensure_ascii=False), encoding="utf-8")
        (args.output / "slow-locations.json").write_text(json.dumps(rank_locations(frames), ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"Converted {len(frames)} report intervals: {args.output}")
        return
    if args.project is None:
        parser.error("project is required outside a benchmark package")
    if not math.isfinite(args.seconds) or not 1 <= args.seconds <= 3600:
        parser.error("--seconds must be between 1 and 3600")
    if args.hz is not None and (not math.isfinite(args.hz) or not 1 <= args.hz <= 1000):
        parser.error("--hz must be between 1 and 1000")
    if args.cursor is not None and args.cursor < 0:
        parser.error("--cursor must be nonnegative")
    if args.timeline and (args.scene or args.cursor is not None):
        parser.error("--timeline cannot be combined with --scene/--cursor")
    if args.camera != "runtime" and args.mode != "continuous":
        parser.error("camera decomposition requires --mode continuous")
    binary, project, output = args.binary.resolve(), args.project.resolve(), args.output.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error("build the profiling binary first; see dev/docs/testing.md")
    if not project.is_dir():
        parser.error("project does not exist")
    output.mkdir(parents=True, exist_ok=False)
    entry = project / "game.haku" if (project / "game.haku").is_file() else project
    command = [str(binary), "perf", str(entry), "--seconds", str(args.seconds), "--mode", args.mode, "--raw"]
    for flag, value in (("--scene", args.scene), ("--cursor", args.cursor), ("--timeline", args.timeline),
                        ("--hz", args.hz), ("--camera", args.camera), ("--window", args.window)):
        if value is not None:
            command += [flag, str(value)]
    source_hashes = source_digests(project)
    metadata = {"command": command, "binary_sha256": file_digest(binary), "platform": platform.platform(),
                "git_commit": "see engine build identity" if PACKAGED else subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "git_status": "packaged build" if PACKAGED else subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True),
                "source_sha256": source_hashes, "cache": "not controlled; no cold-cache claim",
                "stacks": {"status": "disabled" if args.stacks == "off" else "unavailable",
                           "reason": "no automatic native sampler on this platform, or sampling disabled"},
                "scope": "render submission intervals; update-to-render latency includes scheduling; stack samples include startup"}
    (output / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
    env = os.environ.copy()
    # The portable child's marker also hides its window. Raw output must not
    # change foreground/focus behavior when profiling the real runtime.
    env.pop("KEINE_RUNTIME_BENCHMARK_CHILD", None)
    sampler = None
    with (output / "engine.stdout.log").open("w", encoding="utf-8") as stdout, \
            (output / "engine.stderr.log").open("w", encoding="utf-8") as stderr, \
            (output / "sampler.log").open("w", encoding="utf-8") as sampler_log:
        engine = subprocess.Popen(command, stdout=stdout, stderr=stderr, env=env)
        try:
            stack = stack_command(engine.pid, math.ceil(args.seconds + 3), output) if args.stacks == "auto" else None
            if stack:
                metadata["stacks"] = {"status": "running", "command": stack, "interval": "approximately 100 Hz"}
                try:
                    sampler = subprocess.Popen(stack, stdout=sampler_log, stderr=subprocess.STDOUT)
                except OSError as error:
                    metadata["stacks"] = {"status": "unavailable", "reason": str(error)}
            start = time.monotonic()
            code = engine.wait(timeout=args.seconds + 90)
            metadata["process_wall_seconds"] = time.monotonic() - start
        except BaseException as error:
            metadata["capture_error"] = str(error)
            engine.terminate()
            try:
                engine.wait(timeout=5)
            except subprocess.TimeoutExpired:
                engine.kill()
                engine.wait()
            raise
        finally:
            if sampler:
                try:
                    sampler.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    sampler.terminate()
                    try:
                        sampler.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        sampler.kill()
                        sampler.wait()
                metadata["stacks"]["exit_code"] = sampler.returncode
                artifact = output / ("stacks.txt" if platform.system() == "Darwin" else "perf.data")
                captured = sampler.returncode == 0 and artifact.is_file() and artifact.stat().st_size > 0
                metadata["stacks"]["status"] = "captured" if captured else "failed; inspect sampler.log"
                if captured and platform.system() == "Darwin":
                    metadata["stacks"]["readable"] = readable_stacks(output)
            metadata["engine_exit_code"] = engine.poll()
            (output / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
    metadata["engine_exit_code"] = code
    log = (output / "engine.stderr.log").read_text(encoding="utf-8", errors="replace")
    frames = read_frames(log)
    (output / "frames.json").write_text(json.dumps(frames, ensure_ascii=False), encoding="utf-8")
    (output / "slow-locations.json").write_text(json.dumps(rank_locations(frames), ensure_ascii=False, indent=2), encoding="utf-8")
    metadata["retained_intervals"] = len(frames)
    metadata["engine_errors"] = [line for line in log.splitlines() if " ERROR " in line]
    metadata["engine_summary"] = [line for line in log.splitlines() if any(
        tag in line for tag in ("CAPTURE  |", "FRAME    |", "BUDGET   |", "PROCESS  |", "EXCLUDED |", "GPU_TIME |",
                               "GPUINFO  |", "GPU      │", "WINDOWSYS |", "SAMPLING |", "ASSETS   |", "DISPLAY  |", "MEMORY   |", "RENDER   |", "UPDATE   |", "build identity ·"))]
    metadata["source_unchanged"] = source_digests(project) == source_hashes
    (output / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
    if not metadata["source_unchanged"]:
        raise SystemExit("source changed during capture; results are not comparable")
    if code != 0 or metadata["engine_errors"] or "KEINE_RENDER_SAMPLE " not in log or not frames:
        raise SystemExit(f"capture failed; inspect {output / 'engine.stderr.log'}")
    print(f"Capture: {output}")
    print(f"Intervals: {len(frames)}; call stacks: {metadata['stacks']['status']}")


if __name__ == "__main__":
    main()
