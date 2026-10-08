#!/usr/bin/env python3
"""Derive the supported Android benchmark; never change the desktop fixture."""
from pathlib import Path
import argparse
import shutil

ROOT = Path(__file__).resolve().parents[2]


def prepare(output):
    source = ROOT / 'tests/fixtures/native-benchmark'
    if output.exists():
        raise ValueError(f'output already exists: {output}')
    # Do not infer a different workload from an arbitrary user's game.
    media = (source / 'scripts/media.shou').read_text(encoding='utf-8')
    head, sep, _ = media.partition('scene video_fullscreen {')
    if not sep or head.count('scene ') != 2:
        raise ValueError('desktop media fixture changed; review Android derivation')
    stress = (source / 'scripts/stress.shou').read_text(encoding='utf-8')
    lines = stress.splitlines(keepends=True)
    video = [line for line in lines if 'video.play(' in line]
    if len(video) != 1 or stress.count('bench_stress_composition') != 1:
        raise ValueError('desktop stress fixture changed; review Android derivation')
    assets = (source / 'assets.yaml').read_text(encoding='utf-8')
    marker = 'videos:\n  movie: assets/movie.mp4\n'
    if assets.count(marker) != 1:
        raise ValueError('desktop video mapping changed; review Android derivation')
    shutil.copytree(source, output, ignore=shutil.ignore_patterns('movie.mp4', 'publisher.key'))
    shutil.copyfile(ROOT / 'LICENSE', output / 'LICENSE')
    (output / 'scripts/media.shou').write_text(head, encoding='utf-8')
    (output / 'scripts/stress.shou').write_text(''.join(line for line in lines if line not in video)
        .replace('bench_stress_composition', 'bench_stress_android'), encoding='utf-8')
    (output / 'assets.yaml').write_text(assets.replace(marker, ''), encoding='utf-8')
    config = (output / 'config.yaml').read_text(encoding='utf-8')
    if config.count('id: keine-runtime-benchmark') != 1:
        raise ValueError('desktop project identity changed')
    config = config.replace('id: keine-runtime-benchmark',
        'id: keine-android-benchmark\n  bundle_identifier: moe.maincore.keine.benchmark')
    (output / 'config.yaml').write_text(config, encoding='utf-8')
    for path in (output / 'scripts').glob('*.shou'):
        if 'video.' in path.read_text(encoding='utf-8'):
            raise ValueError(f'unsupported video remains in {path}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    prepare(parser.parse_args().output.resolve())
