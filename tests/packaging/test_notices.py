"""Distribution notices retain full terms and reject incomplete cached output."""
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / 'dev/scripts'))
import package_notices as notices

spec = importlib.util.spec_from_file_location('android_notices', REPO / 'dev/scripts/android-notices.py')
android = importlib.util.module_from_spec(spec)
spec.loader.exec_module(android)


class NoticesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for relative in notices.BASE_FILES:
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((REPO / relative).read_bytes())
        self.metadata = {'workspace_root': str(self.root), 'packages': [
            {'name': 'keine', 'version': 'test'},
        ]}
        for name, relatives in notices.NATIVE_FILES.items():
            directory = self.root / name
            for relative in relatives:
                target = directory / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(f'Copyright {name}\r\nComplete terms: {relative}\r\n'.encode())
            self.metadata['packages'].append({'name': name, 'version': '1.2.3',
                                             'manifest_path': str(directory / 'Cargo.toml')})
        self.game = self.root / 'GAME-LICENSE'
        self.game.write_text('Copyright 2026 author. All rights reserved.\n', encoding='utf-8')

    def test_game_and_engine_builds_leave_one_notice_without_stale_game_terms(self):
        output = self.root / 'target/android/notices'
        output.mkdir(parents=True)
        for name in notices.LEGACY_FILES:
            (output / name).write_text('old cached notice')
        android.stage(self.metadata, self.game)
        self.assertEqual([path.name for path in output.iterdir()], ['NOTICE'])
        text = (output / 'NOTICE').read_bytes().decode()
        for relative in notices.BASE_FILES:
            self.assertIn((REPO / relative).read_bytes().decode(), text)
        for package in self.metadata['packages'][1:]:
            for relative in notices.NATIVE_FILES[package['name']]:
                original = (Path(package['manifest_path']).parent / relative).read_bytes().decode()
                self.assertIn(original, text)
            self.assertIn(f"https://crates.io/api/v1/crates/{package['name']}/1.2.3/download", text)
        self.assertIn(self.game.read_text(), text)
        android.stage(self.metadata)
        self.assertNotIn('===== GAME-LICENSE =====', (output / 'NOTICE').read_text())

    def test_missing_terms_do_not_replace_previous_document(self):
        output = self.root / 'NOTICE-output'
        output.write_text('previous complete release')
        (self.root / 'libwebp-sys/vendor/PATENTS').unlink()
        with self.assertRaises(FileNotFoundError):
            notices.write(output, self.metadata, root=self.root)
        self.assertEqual(output.read_text(), 'previous complete release')

    def test_apk_verification_rejects_truncated_terms_and_legacy_files(self):
        text = notices.document(self.root, self.metadata, self.game)
        for contents, legacy, valid in [(text, False, True),
                                         (text.replace('Complete terms: vendor/PATENTS', 'removed'), False, False),
                                         (text, True, False)]:
            archive = io.BytesIO()
            with zipfile.ZipFile(archive, 'w') as apk:
                apk.writestr('assets/NOTICE', contents)
                if legacy:
                    apk.writestr('assets/LICENSE', 'stale')
            with zipfile.ZipFile(archive) as apk:
                if valid:
                    notices.verify_apk(apk, require_game=True, metadata=self.metadata)
                else:
                    with self.assertRaises(AssertionError):
                        notices.verify_apk(apk, require_game=True, metadata=self.metadata)

    def test_sdk_append_preserves_existing_document_and_raw_line_endings(self):
        output = self.root / 'NOTICE-output'
        notices.write(output, root=self.root)
        original = output.read_bytes()
        sdk = self.root / 'sdk-license'
        sdk.write_bytes(b'Copyright SDK\r\nFull redistribution terms\r\n')
        notices.append_file(output, 'Native SDK / sdk', sdk)
        self.assertTrue(output.read_bytes().startswith(original))
        self.assertTrue(output.read_bytes().endswith(sdk.read_bytes()))


if __name__ == '__main__':
    unittest.main()
