"""Packaging boundary: RPATH lookup must retain original library provenance."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import sys
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / 'dev/scripts/package-authoring.py'
sys.path.insert(0, str(SCRIPT.parent))
spec = importlib.util.spec_from_file_location('package_authoring', SCRIPT)
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class LinuxPackageTests(unittest.TestCase):
    def test_second_executable_reuses_packaged_library_and_original_notice(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            output = root / 'package'
            output.mkdir()
            (output / 'NOTICE').write_text('Engine notices\n')
            binaries = [output / 'editor', output / 'keine']
            for binary in binaries:
                binary.write_bytes(b'executable')
            sdk = root / 'sdk'
            sdk.mkdir()
            original = sdk / 'libgcc_s.so.1'
            original.write_bytes(b'system library')
            bundled = output / 'lib/libgcc_s.so.1'
            notice = root / 'doc/libgcc-s1/copyright'
            notice.parent.mkdir(parents=True)
            notice.write_text('Copyright and source information')
            queried = []

            def path(value):
                return root / 'doc' if value == '/usr/share/doc' else Path(value)

            def run(command, **kwargs):
                if command[0] == 'ldd':
                    binary = Path(command[1])
                    dependency = bundled if bundled.exists() else original
                    stdout = (f'libgcc_s.so.1 => {dependency} (0x1234)\n'
                              if binary in binaries else '')
                    return subprocess.CompletedProcess(command, 0, stdout, '')
                self.assertEqual(command[:2], ['dpkg-query', '-S'])
                queried.append(Path(command[2]))
                return subprocess.CompletedProcess(command, 0 if queried[-1] == original else 1,
                                                   f'libgcc-s1:amd64: {original}\n', '')

            with patch.object(packaging, 'Path', side_effect=path), \
                    patch.object(packaging.subprocess, 'run', side_effect=run):
                packaging.linux_libraries(binaries, output)
            self.assertEqual(queried, [original])
            self.assertEqual(bundled.read_bytes(), original.read_bytes())
            contents = (output / 'NOTICE').read_text()
            self.assertTrue(contents.startswith('Engine notices\n'))
            self.assertEqual(contents.count(notice.read_text()), 1)
            self.assertFalse((output / 'THIRD-PARTY').exists())

    def test_unresolved_dependency_rejects_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            result = subprocess.CompletedProcess([], 0, 'libavcodec.so => not found\n', '')
            with patch.object(packaging.subprocess, 'run', return_value=result):
                with self.assertRaisesRegex(RuntimeError, 'Unresolved runtime dependency'):
                    packaging.linux_libraries([root / 'keine'], root)


class WindowsPackageTests(unittest.TestCase):
    def test_sdk_terms_are_appended_without_a_separate_notice_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / 'package'
            output.mkdir()
            (output / 'NOTICE').write_text('Engine terms\n')
            sdk = root / 'vcpkg/installed/x64-windows'
            (sdk / 'bin').mkdir(parents=True)
            (sdk / 'bin/avcodec.dll').write_bytes(b'library')
            copyright_file = sdk / 'share/ffmpeg/copyright'
            copyright_file.parent.mkdir(parents=True)
            copyright_file.write_bytes(b'Copyright FFmpeg\r\nFull native SDK terms\r\n')
            with patch.dict(os.environ, {'VCPKG_ROOT': str(root / 'vcpkg'),
                                         'VCPKG_TARGET_TRIPLET': 'x64-windows'}):
                packaging.windows_libraries(output)
            self.assertEqual((output / 'avcodec.dll').read_bytes(), b'library')
            self.assertTrue((output / 'NOTICE').read_bytes().endswith(copyright_file.read_bytes()))
            self.assertFalse((output / 'THIRD-PARTY').exists())


if __name__ == '__main__':
    unittest.main()
