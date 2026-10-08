"""Real launcher generation: relocation, quoting, identity and user-local install."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'dev/scripts/install-desktop.py'
spec = importlib.util.spec_from_file_location('install_desktop', SCRIPT)
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)


class DesktopEntryTests(unittest.TestCase):
    def test_install_uses_final_location_and_preserves_spaces_quotes_and_percent(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / '游戏 "test" 100% $files \\ folder'
            package.mkdir()
            (package / 'editor').touch()
            (package / 'keine.png').write_bytes(b'icon')
            shutil.copyfile(SCRIPT, package / SCRIPT.name)
            config = {'id': 'moe.maincore.keine-editor', 'name': 'Kēne\nEditor',
                      'executable': 'editor', 'category': 'Development'}
            (package / 'DESKTOP.json').write_text(json.dumps(config))
            data = root / 'userdata'
            result = subprocess.run(['python3', package / SCRIPT.name, '--install'], check=True,
                                    env={**os.environ, 'XDG_DATA_HOME': str(data)}, capture_output=True, text=True)
            self.assertIn('Installed', result.stdout)
            entry = (data / 'applications/moe.maincore.keine-editor.desktop').read_text()
            self.assertIn('Name=Kēne\\nEditor\n', entry)
            self.assertIn('100%%', entry)
            self.assertIn('StartupWMClass=moe.maincore.keine-editor\n', entry)
            self.assertEqual((data / 'icons/hicolor/512x512/apps/moe.maincore.keine-editor.png').read_bytes(), b'icon')
            if shutil.which('desktop-file-validate'):
                subprocess.run(['desktop-file-validate', data / 'applications/moe.maincore.keine-editor.desktop'], check=True)
            relocated = root / 'moved'
            package.rename(relocated)
            config['arguments'] = ['project']
            entry = desktop.desktop_entry(relocated, config)
            self.assertIn(str(relocated / 'editor'), entry)
            self.assertIn('"project"', entry)

    def test_invalid_identity_incomplete_package_and_control_paths_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            config = {'id': '../escape', 'name': 'test', 'executable': 'keine', 'category': 'Game'}
            with self.assertRaisesRegex(ValueError, 'application ID'):
                desktop.desktop_entry(package, config)
            config['id'] = 'org.example.game'
            with self.assertRaisesRegex(ValueError, 'complete'):
                desktop.desktop_entry(package, config)
            with self.assertRaisesRegex(ValueError, 'control'):
                desktop.exec_argument('/tmp/invalid\npath')


if __name__ == '__main__':
    unittest.main()
