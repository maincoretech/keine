#!/usr/bin/env python3
"""Stage Engine and bundled native-code notices from Cargo's locked sources."""

import json
from pathlib import Path
import sys
import argparse

from package_notices import LEGACY_FILES, write


def stage(metadata, game_license=None):
    root = Path(metadata['workspace_root'])
    output = root / 'target/android/notices'
    write(output / 'NOTICE', metadata, game_license, root)
    # The staging directory is reused between Engine and game builds.
    # Remove only our previous generated files, including cached CI output.
    for name in LEGACY_FILES:
        (output / name).unlink(missing_ok=True)
    return next(package['version'] for package in metadata['packages'] if package['name'] == 'keine')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-license', type=Path)
    args = parser.parse_args()
    print(stage(json.load(sys.stdin), args.game_license))
