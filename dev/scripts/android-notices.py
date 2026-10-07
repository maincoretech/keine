#!/usr/bin/env python3
"""Stage Engine and bundled native-code notices from Cargo's locked sources."""

import json
from pathlib import Path
import shutil
import sys

metadata = json.load(sys.stdin)
root = Path(metadata["workspace_root"])
output = root / "target/android/notices"
output.mkdir(parents=True, exist_ok=True)
for source in (root / "LICENSE", root / "NOTICE", root / "src/assets/fonts/FONT-LICENSES.txt"):
    shutil.copyfile(source, output / source.name)

packages = {package["name"]: package for package in metadata["packages"]}
sources = []
for name, files in (
    ("libwebp-sys", ("vendor/COPYING", "vendor/PATENTS")),
    ("opusic-sys", ("opus/COPYING",)),
):
    package = packages[name]
    directory = Path(package["manifest_path"]).parent
    for relative in files:
        source = directory / relative
        shutil.copyfile(source, output / f"{name}-{source.name}.txt")
    sources.append(
        f"{name} {package['version']}: bundled native sources and license files in "
        f"https://crates.io/api/v1/crates/{name}/{package['version']}/download\n"
    )
(output / "NATIVE-SOURCES.txt").write_text("".join(sources), encoding="utf-8")
print(packages["keine"]["version"])
