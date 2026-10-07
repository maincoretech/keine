#!/usr/bin/env python3
"""Package prebuilt Editor + Preview executables, without game content or keys."""
import argparse
import filecmp
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[2]
# Keep the host's libc/loader ABI and graphics drivers. Bundle linked application
# dependencies, including FFmpeg, so Preview and Build exports can find them.
HOST_ABI = ('ld-linux', 'libc.so', 'libdl.so', 'libm.so', 'libpthread.so', 'librt.so')


def linux_libraries(executables, output):
    libraries = output / 'lib'
    libraries.mkdir()
    notices = output / 'THIRD-PARTY'
    notices.mkdir()
    queue = list(executables)
    visited = set()
    origins = {}
    while queue:
        binary = queue.pop()
        result = subprocess.run(['ldd', str(binary)], check=True, capture_output=True, text=True)
        if '=> not found' in result.stdout:
            raise RuntimeError(f'Unresolved runtime dependency: {binary}\n{result.stdout}')
        for line in result.stdout.splitlines():
            words = line.split('=>', 1)[-1].split()
            if not words or not words[0].startswith('/'):
                continue
            library = Path(words[0])
            resolved = library.resolve(strict=True)
            # RPATH may resolve dependencies from our partly assembled lib/ on
            # the second executable. Keep the original SDK provenance.
            source = origins.get(resolved, resolved)
            if source.name.startswith(HOST_ABI):
                continue
            destination = libraries / library.name
            if destination.exists() and not filecmp.cmp(destination, source, shallow=False):
                raise RuntimeError(f'Conflicting runtime libraries: {library.name}')
            if not destination.exists():
                shutil.copy2(source, destination)
            origins[destination.resolve()] = source
            if source not in visited:
                visited.add(source)
                queue.append(source)
                owners = subprocess.run(['dpkg-query', '-S', str(source)], capture_output=True, text=True)
                if owners.returncode and library != source:
                    owners = subprocess.run(['dpkg-query', '-S', str(library)], capture_output=True, text=True)
                if owners.returncode:
                    raise RuntimeError(f'Missing native library source/license record: {source}')
                for owner in owners.stdout.splitlines():
                    package = owner.rsplit(': ', 1)[0].split(':', 1)[0]
                    copyright_file = Path('/usr/share/doc') / package / 'copyright'
                    if not copyright_file.is_file():
                        raise RuntimeError(f'Missing native library copyright: {package}')
                    shutil.copy2(copyright_file, notices / f'{package}.txt')
    for executable in executables:
        result = subprocess.run(['ldd', str(executable)], check=True, capture_output=True, text=True)
        if '=> not found' in result.stdout:
            raise RuntimeError(f'Packaged executable has unresolved libraries: {executable}')
        for line in result.stdout.splitlines():
            if '=>' in line and not 'not found' in line:
                source = Path(line.split('=>', 1)[1].split()[0])
                if not source.name.startswith(HOST_ABI) and source.parent.resolve() != libraries.resolve():
                    raise RuntimeError(f'Packaged dependency escapes lib/: {source}')


def windows_libraries(output):
    root = Path(os.environ['VCPKG_ROOT']) / 'installed' / os.environ.get('VCPKG_TARGET_TRIPLET', 'x64-windows')
    dlls = list((root / 'bin').glob('*.dll'))
    if not dlls:
        raise RuntimeError('FFmpeg runtime DLLs are missing')
    for library in dlls:
        shutil.copy2(library, output / library.name)
    copyrights = list((root / 'share').glob('*/copyright'))
    if not copyrights:
        raise RuntimeError('Native SDK copyright notices are missing')
    notices = output / 'THIRD-PARTY'
    notices.mkdir()
    for notice in copyrights:
        shutil.copy2(notice, notices / f'{notice.parent.name}.txt')


def package(editor, engine, output):
    system = platform.system()
    editor, engine, output = editor.resolve(strict=True), engine.resolve(strict=True), output.absolute()
    if output.exists() or output.is_symlink():
        raise RuntimeError('Choose a fresh output directory')
    if not editor.is_file() or not engine.is_file():
        raise RuntimeError('Both executables are required')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.keine-editor-', dir=output.parent) as temporary:
        staging = Path(temporary) / 'package'
        if system == 'Darwin':
            subprocess.run(['bash', str(REPO / 'dev/scripts/package-authoring-macos.sh'),
                            str(editor), str(engine), str(staging)], check=True)
        else:
            staging.mkdir()
            suffix = '.exe' if system == 'Windows' else ''
            shipped = []
            for name, source in [('editor', editor), ('keine', engine)]:
                target = staging / (name + suffix)
                shutil.copy2(source, target)
                shipped.append(target)
            for source in [REPO / 'LICENSE', REPO / 'NOTICE', REPO / 'src/assets/fonts/FONT-LICENSES.txt']:
                shutil.copy2(source, staging / source.name)
            if system == 'Linux':
                linux_libraries(shipped, staging)
            elif system == 'Windows':
                windows_libraries(staging)
            else:
                raise RuntimeError(f'Unsupported authoring platform: {system}')
        runtime = staging / ('Kēne Editor.app/Contents/MacOS/keine' if system == 'Darwin'
                             else 'keine.exe' if system == 'Windows' else 'keine')
        subprocess.run([str(runtime), 'validate', str(REPO / 'tests/fixtures/native-smoke')], check=True)
        subprocess.run([str(runtime), '--version'], check=True)
        (staging / 'BUILD.json').write_text(json.dumps({
            'commit': os.environ.get('KEINE_BUILD_COMMIT', 'local'),
            'platform': system, 'architecture': platform.machine(),
            'features': os.environ.get('KEINE_AUTHORING_FEATURES', ''),
        }, indent=2) + '\n', encoding='utf-8')
        (staging / 'START.txt').write_text(
            'Kēne Editor development package\n'
            'macOS: open Kēne Editor.app. Windows: editor.exe. Linux: ./editor.\n'
            'Keep the entire package together; the matching Preview Engine is included.\n'
            'Open your project directory. No demo/game content or publisher identity is bundled.\n'
            'Build view exports a runnable playtest without Cargo or publisher keys.\n'
            'Image import is built in. Noncanonical audio/video conversion needs an external\n'
            'FFmpeg executable with libopus/libx264 on PATH. It is not included here.\n'
            'Linux: requires a graphical desktop, audio/graphics drivers and compatible host libc.\n'
            'macOS: ad hoc development signature, not Apple notarized.\n', encoding='utf-8')
        staging.rename(output)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('editor', type=Path)
    parser.add_argument('engine', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    package(args.editor, args.engine, args.output)
