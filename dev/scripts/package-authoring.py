#!/usr/bin/env python3
"""Package the prebuilt Editor and Engine for desktop downloads."""
import argparse
import filecmp
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tempfile
from package_notices import append_file, cargo_metadata, write
from package_windows import libraries as windows_runtime, sdk_root

REPO = Path(__file__).resolve().parents[2]
# Keep the host's libc/loader ABI and graphics drivers. Bundle linked application
# dependencies, including FFmpeg, so Preview and Build exports can find them.
HOST_ABI = ('ld-linux', 'libc.so', 'libdl.so', 'libm.so', 'libpthread.so', 'librt.so',
            'libmvec.so', 'libresolv.so', 'libnss_', 'libanl.so', 'libutil.so')


def linux_libraries(executables, output):
    libraries = output / 'lib'
    libraries.mkdir()
    notices = output / 'NOTICE'
    recorded_notices = set()
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
                    if package not in recorded_notices:
                        append_file(notices, f'Native SDK / {package}', copyright_file)
                        recorded_notices.add(package)
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
    root = sdk_root()
    windows_runtime(output)
    copyrights = list((root / 'share').glob('*/copyright'))
    if not copyrights:
        raise RuntimeError('Native SDK copyright notices are missing')
    for notice in sorted(copyrights):
        append_file(output / 'NOTICE', f'Native SDK / {notice.parent.name}', notice)


def macos_app(editor, engine, output):
    app = output / 'Kēne Editor.app'
    binaries, resources = app / 'Contents/MacOS', app / 'Contents/Resources'
    binaries.mkdir(parents=True)
    resources.mkdir()
    for name, source in [('editor', editor), ('keine', engine)]:
        target = binaries / name
        shutil.copy2(source, target)
        target.chmod(target.stat().st_mode | 0o111)
    icons = Path(os.environ.get('KEINE_APP_ICON_DIR', REPO / 'src/assets/icons'))
    shutil.copy2(icons / 'keine.icns', resources / 'keine.icns')
    metadata = cargo_metadata()
    write(resources / 'NOTICE', metadata)
    version = next(package['version'] for package in metadata['packages'] if package['name'] == 'keine')
    with (app / 'Contents/Info.plist').open('wb') as target:
        plistlib.dump({
            'CFBundleExecutable': 'editor', 'CFBundleIdentifier': 'moe.maincore.keine-editor',
            'CFBundleName': 'Kēne Editor', 'CFBundlePackageType': 'APPL',
            'CFBundleIconFile': 'keine.icns', 'CFBundleShortVersionString': version,
        }, target)
    subprocess.run(['bash', str(REPO / 'dev/scripts/sign-macos.sh'), str(app)], check=True)


def package(editor, engine, output):
    system = platform.system()
    engine, output = engine.resolve(strict=True), output.absolute()
    if output.exists() or output.is_symlink():
        raise RuntimeError('Choose a fresh output directory')
    binaries = [('keine', engine)]
    if editor is not None:
        editor = editor.resolve(strict=True)
        binaries.insert(0, ('editor', editor))
    if any(not source.is_file() for _, source in binaries):
        raise RuntimeError('Expected prebuilt executables')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.keine-authoring-', dir=output.parent) as temporary:
        staging = Path(temporary) / 'package'
        if system == 'Darwin' and editor is not None:
            macos_app(editor, engine, staging)
            runtime = staging / 'Kēne Editor.app/Contents/MacOS/keine'
        else:
            if system not in ('Linux', 'Windows', 'Darwin'):
                raise RuntimeError(f'Unsupported authoring platform: {system}')
            staging.mkdir()
            suffix = '.exe' if system == 'Windows' else ''
            shipped = []
            for name, source in binaries:
                target = staging / (name + suffix)
                shutil.copy2(source, target)
                shipped.append(target)
            runtime = staging / ('keine' + suffix)
            write(staging / 'NOTICE', cargo_metadata())
            if system == 'Linux':
                linux_libraries(shipped, staging)
                if editor is not None:
                    icons = Path(os.environ.get('KEINE_APP_ICON_DIR', REPO / 'src/assets/icons'))
                    shutil.copy2(icons / 'keine-512.png', staging / 'keine.png')
                    shutil.copy2(REPO / 'dev/scripts/install-desktop.py', staging / 'install-desktop.py')
                    (staging / 'DESKTOP.json').write_text(json.dumps({
                        'id': 'moe.maincore.keine-editor', 'name': 'Kēne Editor',
                        'executable': 'editor', 'category': 'Development',
                    }) + '\n', encoding='utf-8')
            elif system == 'Windows':
                windows_libraries(staging)
            elif system == 'Darwin':
                subprocess.run(['codesign', '--verify', '--strict', str(runtime)], check=True)
        runtime_env = os.environ.copy()
        if system == 'Windows':
            runtime_env['PATH'] = os.pathsep.join([str(Path(os.environ['SystemRoot']) / 'System32'), os.environ['SystemRoot']])
        subprocess.run([str(runtime), 'validate', str(REPO / 'tests/fixtures/native-smoke')], check=True,
                       cwd=temporary, env=runtime_env)
        subprocess.run([str(runtime), '--version'], check=True, cwd=temporary, env=runtime_env)
        (staging / 'BUILD.json').write_text(json.dumps({
            'commit': os.environ.get('KEINE_BUILD_COMMIT', 'local'),
            'platform': system, 'architecture': platform.machine(),
            'features': os.environ.get('KEINE_AUTHORING_FEATURES', ''),
        }, indent=2) + '\n', encoding='utf-8')
        instructions = (
            'Kēne Editor development package\n'
            'macOS: open Kēne Editor.app. Windows: editor.exe. Linux: ./editor.\n'
            'Linux optional launcher: python3 install-desktop.py --install (after unpacking at its final location).\n'
            'Keep the entire package together; the matching Preview Engine is included.\n'
            'Open your project directory. No demo/game content or publisher identity is bundled.\n'
            'Build view exports a runnable playtest without Cargo or publisher keys.\n'
            'Image import is built in. Noncanonical audio/video conversion needs an external\n'
            'FFmpeg executable with libopus/libx264 on PATH. It is not included here.\n'
            'Linux: requires a graphical desktop, audio/graphics drivers and compatible host libc.\n'
            'macOS: ad hoc development signature, not Apple notarized.\n'
        ) if editor is not None else (
            'Kēne Engine development package\n'
            'After extracting the ZIP, run ./keine dev /path/to/project from a terminal.\n'
            'Windows: .\\keine.exe dev "C:\\path\\to\\project"\n'
            'Use validate instead of dev to check a project without opening a window.\n'
            'Keep the entire package together. No game or Editor files are included.\n'
            'See the publishing guide to build a distributable game.\n'
            'https://github.com/maincoretech/keine/blob/main/dev/docs/release.md\n'
        )
        (staging / 'START.txt').write_text(instructions, encoding='utf-8')
        staging.rename(output)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('editor', type=Path)
    parser.add_argument('engine', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--engine-output', type=Path, help='Also stage a standalone Engine package')
    args = parser.parse_args()
    package(args.editor, args.engine, args.output)
    if args.engine_output is not None:
        package(None, args.engine, args.engine_output)
