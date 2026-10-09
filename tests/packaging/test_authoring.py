"""Packaging boundary: RPATH lookup must retain original library provenance."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import sys
import xml.etree.ElementTree as ET
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[2] / 'dev/scripts/package-authoring.py'
sys.path.insert(0, str(SCRIPT.parent))
spec = importlib.util.spec_from_file_location('package_authoring', SCRIPT)
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class LinuxPackageTests(unittest.TestCase):
    def test_container_gate_validates_snapshot_or_authoring_fixture_and_rejects_errors(self):
        script = SCRIPT.with_name('verify-linux-package.sh').read_text()
        validation_template = script[script.index('# An unrelated cwd'):]
        # Exercise the actual shell gate without installing container packages.
        # A failed CLI currently logs an error even when its exit status is zero.
        for packaged, invalid in [(True, False), (False, False), (True, True)]:
            with self.subTest(packaged=packaged, invalid=invalid), \
                    tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                package, fixture = root / 'package', root / 'native-smoke'
                package.mkdir()
                fixture.mkdir()
                (fixture / 'config.yaml').write_text('authoring fixture')
                if packaged:
                    (package / 'game.haku').write_bytes(b'snapshot')
                engine = package / 'keine'
                engine.write_text('#!/bin/bash\n'
                    'if [[ "$1" == --version ]]; then echo "Kēne test"; exit 0; fi\n'
                    'printf "%s\\n" "$2" > "$VALIDATED_PATH"\n'
                    'if [[ "$1" != validate || "$INVALID_PROJECT" == 1 ]]; then\n'
                    '  echo "ERROR failed to open project"; exit 0\n'
                    'fi\n'
                    'if [[ -f "$2" || -f "$2/config.yaml" ]]; then\n'
                    '  echo "project valid · test"\n'
                    'else echo "ERROR project config does not exist"; fi\n')
                engine.chmod(0o755)
                validation = validation_template.replace('/tmp/package', str(package)) \
                    .replace('/native-smoke', str(fixture)) \
                    .replace('/tmp/validation.log', str(root / 'validation.log'))
                result = subprocess.run(['bash', '-c',
                    'set -euo pipefail\ntimeout() { shift; "$@"; }\n' + validation],
                    env={**os.environ, 'VALIDATED_PATH': str(root / 'validated-path'),
                         'INVALID_PROJECT': '1' if invalid else '0'},
                    text=True, capture_output=True)
                self.assertEqual((root / 'validated-path').read_text().strip(),
                                 str(package / 'game.haku' if packaged else fixture))
                self.assertEqual(result.returncode == 0, not invalid, result.stdout + result.stderr)
                self.assertEqual('validation passed' in result.stdout, not invalid)

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
            self.assertEqual((output / 'lib/avcodec.dll').read_bytes(), b'library')
            self.assertFalse((output / 'avcodec.dll').exists())
            ns = {'a': 'urn:schemas-microsoft-com:asm.v1'}
            assembly = ET.parse(output / 'lib/lib.manifest').getroot()
            identity = assembly.find('a:assemblyIdentity', ns).attrib
            app = ET.parse(SCRIPT.parents[1] / 'windows/runtime.manifest').getroot()
            self.assertEqual(app.find('a:dependency/a:dependentAssembly/a:assemblyIdentity', ns).attrib, identity)
            self.assertEqual([file.attrib['name'] for file in assembly.findall('a:file', ns)], ['avcodec.dll'])
            self.assertTrue((output / 'NOTICE').read_bytes().endswith(copyright_file.read_bytes()))
            self.assertFalse((output / 'THIRD-PARTY').exists())


class LinuxAbiTests(unittest.TestCase):
    def test_required_versions_reject_newer_libc_but_ignore_export_definitions(self):
        spec = importlib.util.spec_from_file_location('linux_abi', SCRIPT.with_name('verify-linux-abi.py'))
        abi = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(abi)
        for version, rejected in [('2.2.5', False), ('2.39', False), ('2.42', True),
                                  ('2.43', True), ('ABI_DT_X86_64_PLT', True),
                                  ('PRIVATE', True), ('ABI_DT_RELR', False)]:
            with self.subTest(version=version):
                text = ('Version definition section .gnu.version_d:\nName: GLIBC_9.99\n'
                        f'Version needs section .gnu.version_r:\nName: GLIBC_{version}\n')
                self.assertEqual(abi.requirements(text), {f'GLIBC_{version}'})
                self.assertEqual(bool(abi.incompatible(abi.requirements(text))), rejected)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'keine').write_bytes(b'\x7fELFexecutable')
            (root / 'lib').mkdir()
            (root / 'lib/bad.so').write_bytes(b'\x7fELFlibrary')
            def readelf(command, **kwargs):
                newer = Path(command[-1]).name == 'bad.so'
                return subprocess.CompletedProcess(command, 0,
                    f'Version needs section:\nName: GLIBC_{"2.43" if newer else "2.39"}\n', '')
            with patch.object(abi.subprocess, 'run', side_effect=readelf):
                with self.assertRaisesRegex(RuntimeError, 'bad.so.*GLIBC_2.43'):
                    abi.verify(root)


if __name__ == '__main__':
    unittest.main()
