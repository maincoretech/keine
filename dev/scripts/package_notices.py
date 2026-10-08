"""Combine complete distribution notices; source texts are never summarized."""
import argparse
import json
from pathlib import Path
import subprocess

REPO = Path(__file__).resolve().parents[2]
HEADER = ('Kēne distribution notices\n'
          'Source document names below identify sections in this file or the upstream source tree.\n'
          'Each component retains its own license; game content is not licensed by the engine.\n')
BASE_FILES = ('NOTICE', 'LICENSE', 'src/assets/fonts/FONT-LICENSES.txt')
NATIVE_FILES = {'libwebp-sys': ('vendor/COPYING', 'vendor/PATENTS'),
                'opusic-sys': ('opus/COPYING',)}
LEGACY_FILES = ('LICENSE', 'FONT-LICENSES.txt', 'GAME-LICENSE', 'TDAY-LICENSE',
                'NATIVE-SOURCES.txt', 'libwebp-sys-COPYING.txt', 'libwebp-sys-PATENTS.txt',
                'opusic-sys-COPYING.txt')


def section(name, text):
    return f'\n===== {name} =====\n{text}'


def read_text(path):
    with path.open('rb') as source:
        data = source.read(4 * 1024 * 1024 + 1)
    if len(data) > 4 * 1024 * 1024:
        raise ValueError(f'Notice exceeds 4 MiB: {path}')
    text = data.decode('utf-8')
    if not text.strip():
        raise ValueError(f'Empty notice: {path}')
    return text


def document(root=REPO, metadata=None, game_license=None):
    result = HEADER
    for relative in BASE_FILES:
        result += section(Path(relative).name, read_text(root / relative))
    if metadata is not None:
        result += native_document(metadata)
    if game_license is not None:
        result += section('GAME-LICENSE', read_text(game_license))
    return result


def native_document(metadata):
    result = ''
    packages = {package['name']: package for package in metadata['packages']}
    sources = []
    for name, relatives in NATIVE_FILES.items():
        package = packages[name]
        directory = Path(package['manifest_path']).parent
        for relative in relatives:
            result += section(f'{name}-{Path(relative).name}.txt', read_text(directory / relative))
        sources.append(f"{name} {package['version']}: bundled native sources and license files in "
                       f"https://crates.io/api/v1/crates/{name}/{package['version']}/download\n")
    return result + section('NATIVE-SOURCES.txt', ''.join(sources))


def append_file(output, name, source):
    with output.open('ab') as target:
        target.write(section(name, read_text(source)).encode('utf-8'))


def cargo_metadata():
    return json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=REPO))


def write(output, metadata=None, game_license=None, root=REPO):
    # Build everything before replacing an existing document: missing terms must fail closed.
    contents = document(root, metadata, game_license).encode('utf-8')
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(contents)
    return len(contents)


def verify_apk(apk, require_game=False, metadata=None):
    names = apk.namelist()
    contents = apk.read('assets/NOTICE').decode('utf-8')
    for relative in BASE_FILES:
        assert section(Path(relative).name, read_text(REPO / relative)) in contents, relative
    packages = {package['name']: package for package in (metadata or cargo_metadata())['packages']}
    for name, relatives in NATIVE_FILES.items():
        package = packages[name]
        for relative in relatives:
            text = read_text(Path(package['manifest_path']).parent / relative)
            assert section(f'{name}-{Path(relative).name}.txt', text) in contents, relative
        assert f"https://crates.io/api/v1/crates/{name}/{package['version']}/download" in contents, name
    assert '\n===== NATIVE-SOURCES.txt =====\n' in contents
    if require_game:
        assert '\n===== GAME-LICENSE =====\n' in contents
        assert contents.split('\n===== GAME-LICENSE =====\n', 1)[1].strip()
    assert not any(f'assets/{name}' in names for name in LEGACY_FILES), names


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--append-native', action='store_true')
    parser.add_argument('--game-license', type=Path)
    args = parser.parse_args()
    if args.append_native:
        contents = read_text(args.output) + native_document(cargo_metadata())
        args.output.write_bytes(contents.encode('utf-8'))
    else:
        write(args.output, cargo_metadata() if args.native else None, args.game_license)
