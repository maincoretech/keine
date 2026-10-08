#!/usr/bin/env python3
"""Verify committed platform representations and, optionally, the compiled APK."""
import argparse
import os
from pathlib import Path
import re
import struct
import subprocess
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def png_size(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'Not a PNG'
    return struct.unpack('>II', data[16:24])


def resource_files(table):
    """Resolve compiled IDs, without assuming source names or APK file paths."""
    resources, current = {}, None
    for line in table.splitlines():
        entry = re.match(r'\s*resource (0x[0-9a-fA-F]+)\s', line)
        if entry:
            current = int(entry[1], 16)
            resources[current] = []
        file = re.match(r'\s*\(([^)]*)\) \(file\) (\S+) type=(\w+)', line)
        if file and current is not None:
            resources[current].append(file.groups())
    return resources


def reference(tree, attribute, element=None):
    if element:
        node = re.search(rf'(?m)^\s*E: {re.escape(element)}[^\n]*\n((?:\s*A:[^\n]*\n?)*)', tree)
        assert node, f'Missing icon element: {element}'
        tree = node[1]
    value = re.search(rf':{re.escape(attribute)}\(0x[0-9a-fA-F]+\)=@(0x[0-9a-fA-F]+)', tree)
    assert value, f'Missing icon resource reference: {attribute}'
    return int(value[1], 16)


def verify_apk(apk, aapt2):
    def dump(kind, path=None):
        args = [aapt2, 'dump', kind, apk]
        if path is not None:
            args += ['--file', path]
        return subprocess.check_output(args, text=True)

    manifest = dump('xmltree', 'AndroidManifest.xml')
    resources = resource_files(dump('resources'))
    with zipfile.ZipFile(apk) as package:
        def files(resource_id, kind):
            assert resource_id in resources, f'Unresolved icon ID: {resource_id:#x}'
            entries = [entry for entry in resources[resource_id] if entry[2] == kind]
            assert entries, f'Missing {kind} icon resource: {resource_id:#x}'
            for _, path, _ in entries:
                assert path in package.namelist(), f'Missing compiled icon file: {path}'
            return entries

        # Release AAPT2 may shorten paths and resource names. Follow the same
        # IDs Android uses, rather than requiring the source directory layout.
        for icon in {reference(manifest, 'icon'), reference(manifest, 'roundIcon')}:
            density_sizes = {'mdpi': 48, 'hdpi': 72, 'xhdpi': 96, 'xxhdpi': 144, 'xxxhdpi': 192}
            for density, size in density_sizes.items():
                entries = [entry for entry in files(icon, 'PNG') if density in entry[0].split('-')]
                assert entries, f'Missing launcher density: {density}'
                for _, path, _ in entries:
                    assert png_size(package.read(path)) == (size, size), path
            for _, path, _ in files(icon, 'XML'):
                adaptive = dump('xmltree', path)
                assert re.search(r'(?m)^\s*E: adaptive-icon\b', adaptive), path
                background = reference(adaptive, 'drawable', 'background')
                assert background in resources, 'Unresolved adaptive background'
                foreground = reference(adaptive, 'drawable', 'foreground')
                for _, foreground_path, _ in files(foreground, 'XML'):
                    layer = dump('xmltree', foreground_path)
                    logo = reference(layer, 'src', 'bitmap')
                    for _, logo_path, _ in files(logo, 'PNG'):
                        assert png_size(package.read(logo_path)) == (264, 264), logo_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk', type=Path)
    parser.add_argument('--icon-dir', type=Path)
    args = parser.parse_args()
    icons = args.icon_dir or ROOT / 'src/assets/icons'
    for size in (256, 512):
        assert png_size((icons / f'keine-{size}.png').read_bytes()) == (size, size)
    ico = (icons / 'keine.ico').read_bytes()
    reserved, kind, count = struct.unpack_from('<HHH', ico)
    assert (reserved, kind, count) == (0, 1, 7)
    sizes = []
    for index in range(count):
        width, height, _, _, _, depth, length, offset = struct.unpack_from('<BBBBHHII', ico, 6 + index * 16)
        width, height = width or 256, height or 256
        assert depth == 32 and png_size(ico[offset:offset + length]) == (width, height)
        sizes.append(width)
    assert sizes == [16, 24, 32, 48, 64, 128, 256], sizes
    icns = (icons / 'keine.icns').read_bytes()
    assert icns[:4] == b'icns' and struct.unpack_from('>I', icns, 4)[0] == len(icns)
    offset, types = 8, set()
    while offset < len(icns):
        kind, length = struct.unpack_from('>4sI', icns, offset)
        assert length >= 8 and offset + length <= len(icns)
        types.add(kind)
        offset += length
    # iconutil can use ic04/ic05 (legacy RGB + alpha) for the 16/32 px entries.
    assert (b'icp4' in types or b'ic04' in types) and (b'icp5' in types or b'ic05' in types)
    assert {b'ic07', b'ic08', b'ic09', b'ic10', b'ic11', b'ic12', b'ic13', b'ic14'} <= types
    res = icons / 'android' if args.icon_dir else ROOT / 'dev/android/app/src/main/icon-res'
    for density, size in [('mdpi', 48), ('hdpi', 72), ('xhdpi', 96), ('xxhdpi', 144), ('xxxhdpi', 192)]:
        assert png_size((res / f'mipmap-{density}/ic_launcher.png').read_bytes()) == (size, size)
    assert png_size((res / 'drawable-nodpi/ic_launcher_logo.png').read_bytes()) == (264, 264)
    adaptive = ET.parse(res / 'mipmap-anydpi-v26/ic_launcher.xml').getroot()
    assert adaptive.tag == 'adaptive-icon' and {node.tag for node in adaptive} == {'foreground', 'background'}
    if args.apk:
        tools = Path(os.environ['ANDROID_HOME']) / 'build-tools/36.0.0'
        verify_apk(args.apk, tools / 'aapt2')
    print('Icons: Windows 7 sizes, macOS standard/Retina ICNS, Linux 512 px and Android adaptive resources passed')


if __name__ == '__main__':
    main()
