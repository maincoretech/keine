#!/usr/bin/env python3
"""Check the actual APK, including its readonly payload and ARM64 ABI closure."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zipfile

apk, provenance = map(Path, sys.argv[1:])
game = json.loads(provenance.read_text(encoding="utf-8"))
tools = Path(os.environ["ANDROID_HOME"]) / "build-tools/36.0.0"
badging = subprocess.check_output([tools / "aapt2", "dump", "badging", apk], text=True)
assert f"package: name='{game['application_id']}'" in badging, badging
assert "launchable-activity: name='moe.maincore.keine.EngineActivity'" in badging, badging
assert "uses-gl-es: '0x30000'" in badging, badging
assert "uses-feature-not-required: name='android.hardware.vulkan.level'" in badging, badging
assert "application-debuggable" not in badging, badging
assert "native-code: 'arm64-v8a'" in badging, badging
manifest = subprocess.check_output(
    [tools / "aapt2", "dump", "xmltree", apk, "--file", "AndroidManifest.xml"], text=True
)
assert re.search(r":launchMode\(.*\)=(2|0x0*2)$", manifest, re.MULTILINE), manifest
with zipfile.ZipFile(apk) as package:
    entries = package.namelist()
    assert "assets/keine-game/game.haku" in entries, entries
    segments = [name for name in entries if name.startswith("assets/keine-game/data/")]
    assert segments and all(re.fullmatch(r"assets/keine-game/data/[0-9a-f]+\.taku", name) for name in segments), segments
    for name in ["assets/keine-game/game.haku", *segments]:
        assert package.getinfo(name).compress_type == zipfile.ZIP_STORED, name
    for name in ("LICENSE", "NOTICE", "FONT-LICENSES.txt", "GAME-LICENSE", "NATIVE-SOURCES.txt",
                 "libwebp-sys-COPYING.txt", "libwebp-sys-PATENTS.txt", "opusic-sys-COPYING.txt"):
        assert f"assets/{name}" in entries, name
    assert not any(name.startswith("kotlin/") or "kotlin-stdlib" in name for name in entries), entries
    assert not any(name.endswith(("publisher.key", ".keystore", ".jks", ".shou", ".wg")) for name in entries), entries
    natives = [name for name in entries if name.startswith("lib/") and name.endswith(".so")]
    assert natives == ["lib/arm64-v8a/libkeine.so"], natives
    readelf = next((Path(os.environ["ANDROID_NDK_HOME"]) / "toolchains/llvm/prebuilt").glob("*/bin/llvm-readelf"))
    with tempfile.TemporaryDirectory() as temporary:
        library = Path(temporary) / "libkeine.so"
        library.write_bytes(package.read(natives[0]))
        headers = subprocess.check_output([readelf, "-lW", library], text=True)
        loads = [line.split() for line in headers.splitlines() if line.strip().startswith("LOAD ")]
        assert loads and all(int(line[-1], 16) >= 16384 for line in loads), headers
        relro = [line.split() for line in headers.splitlines() if line.strip().startswith("GNU_RELRO ")]
        assert relro and all((int(line[2], 16) + int(line[5], 16)) % 16384 == 0 for line in relro), headers
        symbols = subprocess.check_output([readelf, "--dyn-syms", "--wide", library], text=True)
        for symbol in ("ANativeActivity_onCreate", "Java_moe_maincore_keine_EngineActivity_nativeBack",
                       "Java_moe_maincore_keine_EngineActivity_nativeBackupResult"):
            assert symbol in symbols, symbol
print("Android release APK: manifest, encrypted assets, notices, native entry points and 16 KB alignment passed")
