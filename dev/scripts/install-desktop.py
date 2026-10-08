#!/usr/bin/env python3
"""Print, or explicitly install, this unpacked Linux application's launcher.

Keep the package at its final location before installing. Only user-local
desktop metadata and the existing icon are copied; no application is moved.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil


def entry_value(value):
    return value.replace('\\', '\\\\').replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t')


def exec_argument(value):
    if any(ord(char) < 32 for char in value):
        raise ValueError('Launcher paths must not contain control characters')
    # Exec has its own quoting followed by desktop-entry string escaping.
    value = value.replace('%', '%%')
    value = re.sub(r'([\\"`$])', r'\\\1', value)
    return entry_value('"' + value + '"')


def desktop_entry(package, config):
    app_id = config['id']
    if not re.fullmatch(r'[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+){2,}', app_id):
        raise ValueError('Invalid desktop application ID')
    if config['executable'] not in ('editor', 'keine'):
        raise ValueError('Unknown package executable')
    if config['category'] not in ('Development', 'Game'):
        raise ValueError('Unknown launcher category')
    executable = package / config['executable']
    if not executable.is_file() or not (package / 'keine.png').is_file():
        raise ValueError('Keep the complete application package together')
    command = ' '.join(exec_argument(str(value)) for value in
                       [executable, *config.get('arguments', [])])
    return ('[Desktop Entry]\nType=Application\n'
            f"Name={entry_value(config['name'])}\nExec={command}\n"
            f'Icon={app_id}\nStartupWMClass={app_id}\n'
            f"Categories={config['category']};\nTerminal=false\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--install', action='store_true', help='install for the current user')
    args = parser.parse_args()
    package = Path(__file__).resolve().parent
    config = json.loads((package / 'DESKTOP.json').read_text(encoding='utf-8'))
    entry = desktop_entry(package, config)
    if not args.install:
        print(entry, end='')
        return
    data = Path(os.environ.get('XDG_DATA_HOME') or Path.home() / '.local/share').absolute()
    application = data / 'applications' / (config['id'] + '.desktop')
    icon = data / 'icons/hicolor/512x512/apps' / (config['id'] + '.png')
    application.parent.mkdir(parents=True, exist_ok=True)
    icon.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(package / 'keine.png', icon)
    temporary = application.with_suffix('.desktop.tmp')
    temporary.write_text(entry, encoding='utf-8')
    temporary.replace(application)
    print(f'Installed {application}; rerun after moving the package.')


if __name__ == '__main__':
    main()
