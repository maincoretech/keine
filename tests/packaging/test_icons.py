"""Compiled icon references must survive release resource path shortening."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('verify_icons', ROOT / 'dev/scripts/verify-icons.py')
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)


def compiled_icon_fixture(apk, shortened=True, missing_logo=False, wrong_size=False):
    source = ROOT / 'dev/android/app/src/main/icon-res'
    adaptive = 'res/BW.xml' if shortened else 'res/mipmap-anydpi-v26/ic_launcher.xml'
    foreground = 'res/Qr.xml' if shortened else 'res/drawable/ic_launcher_foreground.xml'
    logo = 'res/TO.png' if shortened else 'res/drawable-nodpi-v4/ic_launcher_logo.png'
    table = (f'    resource 0x7f010000 color/background\n      () #ff20392f\n'
             f'    resource 0x7f020000 drawable/foreground\n      () (file) {foreground} type=XML\n'
             f'    resource 0x7f020001 drawable/logo\n      (nodpi) (file) {logo} type=PNG\n'
             '    resource 0x7f030000 mipmap/launcher\n')
    with zipfile.ZipFile(apk, 'w') as package:
        for index, density in enumerate(('mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi')):
            path = f'res/{index}.png' if shortened else f'res/mipmap-{density}-v4/ic_launcher.png'
            table += f'      ({density}) (file) {path} type=PNG\n'
            actual_density = 'hdpi' if wrong_size and index == 0 else density
            package.writestr(path, (source / f'mipmap-{actual_density}/ic_launcher.png').read_bytes())
        table += f'      (anydpi) (file) {adaptive} type=XML\n'
        package.writestr(adaptive, b'compiled XML')
        package.writestr(foreground, b'compiled XML')
        if not missing_logo:
            package.writestr(logo, (source / 'drawable-nodpi/ic_launcher_logo.png').read_bytes())
    trees = {
        'AndroidManifest.xml': '  E: application (line=1)\n    A: android:icon(0x01010002)=@0x7f030000\n    A: android:roundIcon(0x0101052c)=@0x7f030000\n',
        adaptive: '  E: adaptive-icon (line=1)\n    E: background (line=1)\n      A: android:drawable(0x01010199)=@0x7f010000\n    E: foreground (line=1)\n      A: android:drawable(0x01010199)=@0x7f020000\n',
        foreground: '  E: layer-list (line=1)\n    E: item (line=1)\n      E: bitmap (line=1)\n        A: android:src(0x01010119)=@0x7f020001\n',
    }

    def dump(args, text):
        return table if args[2] == 'resources' else trees[args[-1]]
    return dump, trees


class CompiledIconTests(unittest.TestCase):
    def test_debug_and_shortened_release_follow_ids_even_with_different_resource_names(self):
        for shortened in (False, True):
            with self.subTest(shortened=shortened), tempfile.TemporaryDirectory() as temporary:
                apk = Path(temporary) / 'game.apk'
                dump, _ = compiled_icon_fixture(apk, shortened)
                with patch.object(icons.subprocess, 'check_output', side_effect=dump):
                    icons.verify_apk(apk, Path('aapt2'))

    def test_invalid_compiled_icon_resources_are_rejected(self):
        cases = (
            ('missing bitmap', {'missing_logo': True}, 'Missing compiled icon file'),
            ('wrong density dimensions', {'wrong_size': True}, None),
            ('missing manifest resource', {}, 'Unresolved icon ID'),
        )
        for name, options, message in cases:
            with self.subTest(case=name), tempfile.TemporaryDirectory() as temporary:
                apk = Path(temporary) / 'game.apk'
                dump, trees = compiled_icon_fixture(apk, **options)
                if name == 'missing manifest resource':
                    trees['AndroidManifest.xml'] = trees['AndroidManifest.xml'].replace(
                        '@0x7f030000', '@0x7f030001')
                with patch.object(icons.subprocess, 'check_output', side_effect=dump):
                    expected_error = (self.assertRaisesRegex(AssertionError, message)
                                      if message else self.assertRaises(AssertionError))
                    with expected_error:
                        icons.verify_apk(apk, Path('aapt2'))


if __name__ == '__main__':
    unittest.main()
