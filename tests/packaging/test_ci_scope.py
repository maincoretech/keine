"""CI routing must not turn audits into builds or miss shared runtime changes."""
import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'dev/scripts/ci-scope.py'
spec = importlib.util.spec_from_file_location('ci_scope', SCRIPT)
scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scope)


class CiScopeTests(unittest.TestCase):
    def test_changes_keep_platform_and_media_boundaries(self):
        cases = [
            (['dev/docs/editor.md', 'README.md'], (False, False, False)),
            (['crates/editor/src/app.rs'], (True, False, False)),
            (['crates/editor/src/app.rs', 'src/runtime.rs'], (True, True, False)),
            (['Cargo.lock'], (True, True, True)),
            (['src/scene/video/native.rs'], (True, True, True)),
            (['.github/workflows/ci.yml'], (True, True, True)),
            ([], (False, False, False)),
            (None, (True, True, True)),
        ]
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(scope.select_checks('push', {'ref': 'refs/heads/main'}, paths), expected)
                self.assertEqual(scope.select_checks('pull_request', {}, paths), expected)
        self.assertEqual(scope.select_checks('push', {'ref': 'refs/heads/topic'}, ['Cargo.lock']),
                         (True, True, False))

    def test_scheduled_and_manual_checks_do_not_start_unrequested_builds(self):
        for selected, expected in [
            ('all', (True, True, True)),
            ('android', (False, True, False)),
            ('media', (False, False, True)),
            ('dependencies', (False, False, False)),
        ]:
            with self.subTest(selected=selected):
                self.assertEqual(scope.select_checks('workflow_dispatch', {'inputs': {'checks': selected}}, None), expected)
        self.assertEqual(scope.select_checks('schedule', {'schedule': '17 2 * * *'}, None),
                         (False, False, False))
        self.assertEqual(scope.select_checks('schedule', {'schedule': '17 3 * * 1'}, None),
                         (False, False, True))


if __name__ == '__main__':
    unittest.main()
