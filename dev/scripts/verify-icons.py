#!/usr/bin/env python3
"""Verify committed platform representations and, optionally, the compiled APK."""
import argparse
import os
from pathlib import Path
import struct
import subprocess
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def png_size(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'Not a PNG'
    return struct.unpack('>II', data[16:24])


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
        manifest = subprocess.check_output([tools / 'aapt2', 'dump', 'xmltree', args.apk,
                                           '--file', 'AndroidManifest.xml'], text=True)
        assert ':icon(' in manifest and ':roundIcon(' in manifest, manifest
        with zipfile.ZipFile(args.apk) as apk:
            names = apk.namelist()
            assert any('mipmap-anydpi-v26/ic_launcher.xml' in name for name in names), names
            assert any(name.startswith('res/drawable-nodpi') and name.endswith('/ic_launcher_logo.png')
                       for name in names), names
        adaptive = subprocess.check_output([tools / 'aapt2', 'dump', 'xmltree', args.apk,
                                            '--file', 'res/mipmap-anydpi-v26/ic_launcher.xml'], text=True)
        assert 'adaptive-icon' in adaptive and 'foreground' in adaptive and 'background' in adaptive
    print('Icons: Windows 7 sizes, macOS standard/Retina ICNS, Linux 512 px and Android adaptive resources passed')


if __name__ == '__main__':
    main()
