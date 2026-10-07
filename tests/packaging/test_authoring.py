"""Packaging boundary: RPATH lookup must retain original library provenance."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / 'dev/scripts/package-authoring.py'
spec = importlib.util.spec_from_file_location('package_authoring', SCRIPT)
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class LinuxPackageTests(unittest.TestCase):
    def test_second_executable_reuses_packaged_library_and_original_notice(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            output = root / 'package'
            output.mkdir()
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
            self.assertEqual((output / 'THIRD-PARTY/libgcc-s1.txt').read_text(), notice.read_text())

    def test_unresolved_dependency_rejects_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            result = subprocess.CompletedProcess([], 0, 'libavcodec.so => not found\n', '')
            with patch.object(packaging.subprocess, 'run', return_value=result):
                with self.assertRaisesRegex(RuntimeError, 'Unresolved runtime dependency'):
                    packaging.linux_libraries([root / 'keine'], root)


if __name__ == '__main__':
    unittest.main()
