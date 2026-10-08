#!/usr/bin/env python3
"""Reject an executable or bundled ELF requiring a newer Linux host ABI."""
import argparse
import os
from pathlib import Path
import re
import subprocess

BASELINE = (2, 39)


def requirements(text):
    # Definitions describe exported versions, not the host's requirements.
    needs = text.split('Version needs section', 1)[-1] if 'Version needs section' in text else ''
    return set(re.findall(r'\bName: (GLIBC_[A-Za-z0-9_.]+)', needs))


def incompatible(versions):
    failures = []
    for version in sorted(versions):
        suffix = version.removeprefix('GLIBC_')
        if suffix == 'ABI_DT_RELR':  # present since glibc 2.36
            continue
        if not re.fullmatch(r'\d+(?:\.\d+)+', suffix) or tuple(map(int, suffix.split('.'))) > BASELINE:
            failures.append(version)
    return failures


def verify(root):
    inspected = 0
    for path in sorted(Path(root).rglob('*')):
        if not path.is_file():
            continue
        with path.open('rb') as stream:
            if stream.read(4) != b'\x7fELF':
                continue
        inspected += 1
        result = subprocess.run(['readelf', '--version-info', '--wide', str(path)],
                                check=True, capture_output=True, text=True, env={**os.environ, 'LC_ALL': 'C'})
        rejected = incompatible(requirements(result.stdout))
        if rejected:
            raise RuntimeError(f'{path}: requires {", ".join(rejected)}; maximum shipping GLIBC is 2.39')
    if not inspected:
        raise RuntimeError('No ELF executable or library was inspected')
    print(f'Linux ABI: {inspected} ELF files require at most GLIBC 2.39')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    verify(parser.parse_args().package)
