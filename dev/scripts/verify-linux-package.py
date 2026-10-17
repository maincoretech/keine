#!/usr/bin/env python3
"""Verify an existing Linux package: host ABI, libraries and project loading."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile

BASELINE = (2, 39)


def verify_runtime(root):
    root = Path(root).resolve(strict=True)
    for name in ('keine', 'editor'):
        binary = root / name
        if name == 'editor' and not binary.exists():
            continue
        if not os.access(binary, os.X_OK):
            raise RuntimeError(f'Missing executable permission: {binary}')
        result = subprocess.run(['ldd', str(binary)], check=True, capture_output=True, text=True)
        print(result.stdout, end='')
        if 'not found' in result.stdout:
            raise RuntimeError(f'Unresolved runtime library: {binary}')
        for line in result.stdout.splitlines():
            if '=>' not in line:
                continue
            library, target = line.split('=>', 1)
            library, target = library.strip(), Path(target.rsplit(' (', 1)[0].strip())
            # Installed SDKs must not mask a missing/broken bundled FFmpeg library.
            if library.startswith(('libavcodec.', 'libavformat.', 'libavutil.',
                                   'libswscale.', 'libswresample.')) or (root / 'lib' / library).exists():
                if not target.resolve().is_relative_to(root / 'lib'):
                    raise RuntimeError(f'Packaged dependency escapes lib/: {library} => {target}')


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


def verify_package(package, fixture):
    package, fixture = package.resolve(strict=True), fixture.resolve(strict=True)
    # Installed SDKs must not mask missing bundled libraries.
    os.environ.pop('LD_LIBRARY_PATH', None)
    os.environ.pop('LD_PRELOAD', None)
    with tempfile.TemporaryDirectory(prefix='keine-linux-package-') as temporary:
        work = Path(temporary)
        if not package.is_dir():
            archive, package = package, work / 'package'
            package.mkdir()
            subprocess.run(['unzip', '-q', str(archive), '-d', str(package)], check=True)
        verify(package)
        verify_runtime(package)
        # An unrelated cwd cannot pick up build-tree assets.
        subprocess.run([str(package / 'keine'), '--version'], cwd=work, check=True, timeout=30)
        project = package / 'game.haku' if (package / 'game.haku').is_file() else fixture
        result = subprocess.run([str(package / 'keine'), 'validate', str(project)],
                                cwd=work, check=True, timeout=60, capture_output=True, text=True)
        print(result.stdout, end='')
        if not any(line.startswith('project valid') for line in result.stdout.splitlines()):
            raise RuntimeError(f'Project validation failed: {result.stderr}')
    print('Final Linux package: loader and project validation passed.')
    # No graphical desktop, GPU, audio or performance verdict.


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('fixture', type=Path, nargs='?',
                        default=Path(__file__).resolve().parent / '../../tests/fixtures/native-smoke')
    args = parser.parse_args()
    verify_package(args.package, args.fixture)
