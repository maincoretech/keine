"""Select existing CI checks without rebuilding unrelated platform targets."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def select_checks(event_name, event, paths):
    if event_name == 'schedule':
        return False, False, event.get('schedule') == '17 3 * * 1'
    if event_name == 'workflow_dispatch':
        selected = event.get('inputs', {}).get('checks', 'all')
        return selected == 'all', selected in ('all', 'android'), selected in ('all', 'media')
    # Missing history is conservative: run checks instead of silently skipping.
    if paths is None:
        return True, True, True
    relevant = [p for p in paths if not (
        p.startswith('dev/docs/') or p in ('README.md', 'AGENTS.md', '.github/dependabot.yml')
    )]
    android = any(not p.startswith('crates/editor/') for p in relevant)
    media = any(
        p in ('Cargo.lock', 'Cargo.toml', 'src/scene/images.rs', 'src/scene/video.rs',
              '.github/workflows/ci.yml', 'dev/scripts/ci-scope.py')
        or p.startswith(('crates/media/', 'tests/fuzz/', 'src/scene/video/',
                         '.github/actions/setup-video/'))
        for p in relevant
    )
    # Preserve the original media workflow's main-only push filter.
    if event_name == 'push' and event.get('ref') != 'refs/heads/main':
        media = False
    return bool(relevant), android, media


def changed_paths(event_name, event):
    if event_name not in ('push', 'pull_request'):
        return None
    base = (event.get('pull_request', {}).get('base', {}).get('sha')
            if event_name == 'pull_request' else event.get('before'))
    head = os.environ.get('GITHUB_SHA', '')
    if not all(isinstance(s, str) and re.fullmatch(r'[0-9a-fA-F]{40}', s)
               and s != '0' * 40 for s in (base, head)):
        return None
    try:
        # Fetch only the comparison commit; no full-history checkout or API token.
        subprocess.run(['git', 'fetch', '--no-tags', '--depth=1', 'origin', base],
                       check=True, capture_output=True, timeout=60)
        changed = subprocess.check_output(
            ['git', 'diff', '--no-renames', '--name-only', '-z', base, head, '--'])
        return changed.decode('utf-8', errors='surrogateescape').rstrip('\0').split('\0') if changed else []
    except (subprocess.SubprocessError, OSError):
        print('Comparison history unavailable; running all applicable checks.', file=sys.stderr)
        return None


if __name__ == '__main__':
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    event_name = os.environ['GITHUB_EVENT_NAME']
    checks = select_checks(event_name, event, changed_paths(event_name, event))
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        for name, enabled in zip(('desktop', 'android', 'media'), checks):
            print(f'{name}={str(enabled).lower()}', file=output)
