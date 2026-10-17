#!/usr/bin/env python3
"""Run the complete supported Android suite and retrieve raw evidence via ADB."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re
import shlex
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
PACKAGE = 'moe.maincore.keine.benchmark'
TRACE_FIELDS = 14


class Adb:
    def __init__(self, serial=None):
        self.prefix = ['adb'] + (['-s', serial] if serial else [])

    def call(self, *args, data=None, check=True, timeout=30):
        result = subprocess.run([*self.prefix, *args], input=data, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=timeout)
        if check and result.returncode:
            raise RuntimeError(result.stderr.decode(errors='replace') or result.stdout.decode(errors='replace'))
        return result.stdout.decode(errors='replace')

    def shell(self, *args, **kwargs):
        return self.call('shell', '-T', shlex.join(args), **kwargs)

    def private(self, *args, **kwargs):
        return self.shell('run-as', PACKAGE, *args, **kwargs)

    def read(self, name, check=True):
        return self.private('cat', f'files/benchmark/{name}', check=check)

    def stop(self, check=True):
        self.shell('am', 'force-stop', PACKAGE, check=check)

    def context(self):
        # Outside the measured interval. Missing vendor thermal sensors are n/a.
        return {'time_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                'battery': self.shell('dumpsys', 'battery', check=False),
                'thermal': self.shell('dumpsys', 'thermalservice', check=False)}


def validate_sample(sample, log):
    if re.search(r'\bERROR\b|panicked at', log):
        raise ValueError('engine error; see sample log')
    if sample['kind'] == 'package':
        if not all(marker in log for marker in ('PACKAGE  | Hakutaku', 'HOT      |', 'NORMAL   |', 'TRANSIENT |', 'STREAM   |', 'RANDOM   |', 'PARALLEL |')):
            raise ValueError('incomplete encrypted APK I/O evidence')
        return None
    if sample['kind'] == 'startup':
        marker = 'KEINE_STARTUP_SAMPLE '
    else:
        marker = 'KEINE_RENDER_SAMPLE '
        if sample.get('target') and 'resolved cursor Some(' not in log:
            raise ValueError('requested timeline was not resolved')
    lines = [line for line in log.splitlines() if line.startswith(marker)]
    if len(lines) != 1:
        raise ValueError(f'expected one {marker.strip()} result')
    values = dict(field.split('=', 1) for field in lines[0].split()[1:])
    values = {key: None if value in ('unknown', 'n/a') else float(value) for key, value in values.items()}
    if any(value is not None and (not math.isfinite(value) or value < 0) for value in values.values()):
        raise ValueError('invalid numeric sample')
    if sample['kind'] == 'render':
        frames = [line.split('\t')[1:] for line in log.splitlines() if line.startswith('KEINE_TRACE\t')]
        if not frames or any(len(frame) != TRACE_FIELDS for frame in frames):
            raise ValueError('missing or truncated raw frame evidence')
        if not any(frame[10] == 'true' for frame in frames):
            raise ValueError('runtime sample was not foreground/focused')
        if (sample['mode'] == 'continuous' or sample['label'].startswith('runtime ')) and values.get('frames', 0) == 0:
            raise ValueError('no eligible active intervals')
    return values


def run_sample(adb, sample, directory, timeout):
    adb.stop()
    adb.private('mkdir', '-p', 'files/benchmark')
    adb.private('rm', '-f', 'files/benchmark/finished', 'files/benchmark/sample.txt')
    request = json.dumps({'schema': 1, 'kind': sample['kind'], 'args': sample['args']}).encode()
    adb.private('tee', 'files/benchmark/request.json', data=request)
    before = adb.context()
    directory.mkdir(parents=True)
    (directory / 'before.json').write_text(json.dumps(before, ensure_ascii=False, indent=2), encoding='utf-8')
    try:
        started = time.monotonic()
        launch = adb.shell('am', 'start', '-W', '-n', f'{PACKAGE}/moe.maincore.keine.EngineActivity')
        (directory / 'launch.txt').write_text(launch, encoding='utf-8')
        while time.monotonic() - started < timeout:
            if adb.read('finished', check=False).strip() == 'complete':
                break
            time.sleep(2)
        else:
            raise TimeoutError(f'sample did not finish within {timeout}s')
        log = adb.read('sample.txt')
        (directory / 'sample.txt').write_text(log, encoding='utf-8')
        return validate_sample(sample, log), log
    finally:
        # Preserve partial evidence for crashes/timeouts and stop only our app.
        if not (directory / 'sample.txt').exists():
            (directory / 'sample.txt').write_text(adb.read('sample.txt', check=False), encoding='utf-8')
        (directory / 'after.json').write_text(json.dumps(adb.context(), ensure_ascii=False, indent=2), encoding='utf-8')
        adb.stop()


def hotspot_lines(results):
    targets = {}
    for item in results:
        sample = item['sample']
        if sample['kind'] == 'render' and sample.get('target') and sample['mode'] == 'continuous' and item.get('values'):
            targets.setdefault(sample['target'], []).append(item['values']['average_ms'])
    if 'bench_baseline' not in targets:
        return ['HOTSPOTS | baseline unavailable; retain raw pass/frame evidence, no baseline delta claimed']
    baseline = statistics.median(targets['bench_baseline'])
    lines = ['HOTSPOTS | median frame interval difference from the common baseline; not exclusive CPU cost']
    for target, values in sorted(targets.items(), key=lambda entry: statistics.median(entry[1]), reverse=True)[:20]:
        median = statistics.median(values)
        lines.append(f'HOTSPOT  | {target} · median {median:.3f} ms · baseline delta {median - baseline:+.3f} ms · {len(values)} run(s)')
    return lines


def pass_hotspots(results):
    """Compare individual paths, without summing nested render/update spans."""
    paths, baseline = {}, {}
    for item in results:
        sample = item['sample']
        if sample['kind'] != 'render' or sample['mode'] != 'continuous' or not item.get('values'):
            continue
        log = Path(item['log']).read_text(encoding='utf-8')
        for path, average in re.findall(r'(?:RENDER|UPDATE)\s+\| (\S+) avg ([0-9.]+)ms', log):
            key = (sample['target'] or 'opening', path)
            paths.setdefault(key, []).append(float(average))
            if sample.get('target') == 'bench_baseline':
                baseline.setdefault(path, []).append(float(average))
    ranked = []
    for (target, path), values in paths.items():
        reference = baseline.get(path)
        if reference is None:
            continue
        average = statistics.median(values)
        ranked.append({'target': target, 'path': path, 'median_ms': average,
                       'baseline_delta_ms': average - statistics.median(reference), 'runs': len(values)})
    ranked.sort(key=lambda item: item['baseline_delta_ms'], reverse=True)
    return ranked[:30]


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk', type=Path, help='install the dedicated benchmark APK; never uninstalls a game')
    parser.add_argument('--plan', type=Path, default=ROOT / 'android-benchmark.json')
    parser.add_argument('--serial', help='adb device serial (required with multiple devices)')
    parser.add_argument('--backend', choices=['auto', 'gl', 'vulkan'], default='auto')
    parser.add_argument('--output', type=Path, default=Path('android-benchmark-report'))
    parser.add_argument('--hz', type=float, help='explicit frame-budget reference Hz; not an inferred or forced display rate')
    parser.add_argument('--timeout', type=float, default=120, help='per-sample limit; complete suite only')
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text(encoding='utf-8'))
    if plan.get('schema') != 1 or plan.get('application_id') != PACKAGE:
        parser.error('unsupported Android benchmark plan')
    if args.hz is not None and (not math.isfinite(args.hz) or not 1 <= args.hz <= 1000):
        parser.error('hz must be between 1 and 1000')
    if args.timeout < 30 or not math.isfinite(args.timeout):
        parser.error('timeout must be finite and at least 30 seconds')
    if args.output.exists():
        parser.error('output already exists; choose a new directory to preserve previous reports')
    adb = Adb(args.serial)
    devices = [line.split()[0] for line in adb.call('devices').splitlines()[1:] if '\tdevice' in line]
    if (args.serial not in devices) if args.serial else (len(devices) != 1):
        parser.error('connect and authorize one phone or select --serial from adb devices')
    args.output.mkdir(parents=True)
    if args.apk:
        adb.call('install', '-r', str(args.apk.resolve()), timeout=180)
    # Verify the installed package's plan, not just the adjacent downloaded JSON.
    adb.private('cat', '/dev/null')  # debuggable/run-as access must work
    adb.private('mkdir', '-p', 'files/benchmark')
    # APK path is returned by Android; quote it before the remote shell accesses it.
    apk_path = adb.shell('pm', 'path', PACKAGE).strip().removeprefix('package:')
    if not apk_path.startswith('/') or '\n' in apk_path:
        raise RuntimeError('expected a single installed benchmark base APK')
    import zipfile
    installed_apk = args.output / 'installed.apk'
    adb.call('pull', apk_path, str(installed_apk), timeout=180)
    with zipfile.ZipFile(installed_apk) as package:
        actual_plan = json.loads(package.read('assets/keine-benchmark.json'))
    installed_apk.unlink()
    if actual_plan != plan:
        raise RuntimeError('installed APK and plan differ; install the APK from this same bundle')
    metadata = {'plan': plan, 'backend_requested': args.backend, 'budget_reference_hz': args.hz,
                'device_properties': adb.shell('getprop'), 'display': adb.shell('wm', 'size'),
                'refresh_configuration': adb.shell('dumpsys', 'display'),
                'memory': adb.shell('cat', '/proc/meminfo'), 'initial_context': adb.context(),
                'apk_bytes': args.apk.stat().st_size if args.apk else None}
    (args.output / 'device.json').write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding='utf-8')
    adb.stop()
    adb.private('tee', 'files/render-backend', data=args.backend.encode())
    results, raw = [], []
    def emit(line):
        print(line, flush=True)
        with (args.output / 'keine-benchmark-report.txt').open('a', encoding='utf-8') as report:
            report.write(line + '\n')
    emit(f'Android full benchmark · Kēne {plan["engine_version"]} · commit {plan["commit"]} · {plan["profile"]} · backend {args.backend}')
    props = dict(re.findall(r'^\[(.*?)\]: \[(.*?)\]$', metadata['device_properties'], re.MULTILINE))
    emit('host environment · ' + ' · '.join(props.get(name, 'n/a') for name in ('ro.product.manufacturer', 'ro.product.model', 'ro.soc.model', 'ro.build.version.release')) + ' · ' + metadata['display'].strip().replace('\n', ' · '))
    emit('Native device surface, foreground Activity; fresh process per sample. Filesystem/GPU caches are not claimed cold.')
    failed, completed, skipped = 0, 0, 0
    interrupted = False
    try:
        for index, original in enumerate(plan['samples']):
            sample = dict(original)
            if args.hz is not None and sample['kind'] == 'render':
                sample['args'] = [*sample['args'], '--hz', str(args.hz)]
            if sample['skip']:
                skipped += int(sample['required'])
                emit(f'SKIP     | {sample["label"]} · {sample["skip"]}')
                continue
            success = True
            for run in range(1, sample['runs'] + 1):
                emit(f'settled {sample["kind"]} · {sample["label"]} · run {run}/{sample["runs"]}' + (' · 3s warm-up + 5s sample' if sample['kind'] == 'render' else ''))
                directory = args.output / 'samples' / f'{index:02}-{run:02}'
                raw_start = len(raw)
                try:
                    values, log = run_sample(adb, sample, directory, args.timeout)
                    results.append({'sample': sample, 'run': run, 'values': values, 'log': str(directory / 'sample.txt')})
                    for line in log.splitlines():
                        if line.startswith('KEINE_TRACE\t'):
                            raw.append('\t'.join(['RAWFRAME', sample['label'], str(run), str(sample['runs']), sample['target'] or 'opening', sample['camera'], *line.split('\t')[1:]]))
                        elif not line.startswith('KEINE_FRAME_SAMPLE '):
                            emit(line)
                except Exception as error:
                    success = False
                    results.append({'sample': sample, 'run': run, 'error': str(error), 'log': str(directory / 'sample.txt')})
                    emit(f'FAILED   | {sample["label"]} · run {run}/{sample["runs"]} · {error}')
                    if adb.call('get-state', check=False).strip() != 'device':
                        raise RuntimeError('ADB device disconnected; partial report retained')
                (args.output / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
                with (args.output / 'keine-benchmark-frames.tsv').open('a', encoding='utf-8') as frames:
                    if len(raw) > raw_start:
                        frames.write('\n'.join(raw[raw_start:]) + '\n')
            if success:
                completed += int(sample['required'])
            else:
                failed += 1
    except (KeyboardInterrupt, Exception) as error:
        interrupted = True
        emit(f'INTERRUPTED | {error} · partial evidence retained; not a full benchmark')
    finally:
        try:
            adb.stop(check=False)
            adb.private('rm', '-f', 'files/benchmark/request.json', 'files/render-backend', check=False)
        except Exception as error:
            emit(f'CLEANUP | device unavailable: {error}')
    startups = [r['values'] for r in results if r['sample']['kind'] == 'startup' and r.get('values')]
    if startups:
        for key in ('project_ms', 'app_ms', 'first_frame_ms', 'interactive_ms'):
            repeat = [s[key] for s in startups[1:]]
            emit(f'STARTUP  | {key} first {startups[0][key]:.3f} ms · repeat median {statistics.median(repeat):.3f} ms' if repeat else f'STARTUP | {key} first {startups[0][key]:.3f} ms · no successful repeat')
    for line in hotspot_lines(results):
        emit(line)
    spec = importlib.util.spec_from_file_location('keine_profile', ROOT / 'profile-runtime.py')
    collector = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(collector)
    frames = collector.read_frames('\n'.join(raw))
    (args.output / 'hotspots.json').write_text(json.dumps(collector.rank_locations(frames), ensure_ascii=False, indent=2), encoding='utf-8')
    passes = pass_hotspots(results)
    (args.output / 'pass-hotspots.json').write_text(json.dumps(passes, ensure_ascii=False, indent=2), encoding='utf-8')
    for item in passes[:15]:
        emit(f'PASS_HOTSPOT | {item["target"]} · {item["path"]} · median {item["median_ms"]:.3f} ms · baseline delta {item["baseline_delta_ms"]:+.3f} ms · no nested-path sum')
    if not any(frame['budget_ms'] is not None for frame in frames):
        emit('BUDGET | display refresh unavailable; source over-budget ranking unavailable. Use measured pass/CPU data; optionally provide --hz as an explicit reference, never treated as measured display Hz.')
    required = sum(s['required'] and s['skip'] is None for s in plan['samples'])
    complete = failed == 0 and not interrupted and completed == required
    emit(f'COVERAGE | {completed}/{required} supported required render workloads · {skipped} unsupported platform cases · {failed} failed workload(s) · {"complete" if complete else "INCOMPLETE"}')
    emit('Attribution: inspect RENDER/UPDATE pass timings, PROCESS CPU/RSS, hotspots.json and raw frames. A slow frame is not proof of an exclusive CPU hotspot. GPU timing unavailable is not zero.')
    return 0 if complete else 1


if __name__ == '__main__':
    raise SystemExit(main())
