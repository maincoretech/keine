#!/usr/bin/env python3
"""Derive all platform application icons with the shared, host-only Rust tool."""
import argparse
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output', type=Path)
parser.add_argument('--source', type=Path, default=ROOT / 'src/assets/icons/keine.png')
args = parser.parse_args()
subprocess.run(['cargo', 'run', '--locked', '-p', 'keine-media', '--features', 'icons',
                '--example', 'icons', '--target-dir', str(ROOT / 'target/icon-tool'),
                '--', str(args.source.absolute()), str(args.output.absolute())], cwd=ROOT, check=True)
