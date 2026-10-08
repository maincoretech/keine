"""Private Win32 assembly: resolve SDK DLLs from lib/ before main starts."""
import os
from pathlib import Path
import shutil
import xml.etree.ElementTree as ET


def sdk_root():
    return Path(os.environ['VCPKG_ROOT']) / 'installed' / os.environ.get('VCPKG_TARGET_TRIPLET', 'x64-windows')


def libraries(output):
    source = sdk_root() / 'bin'
    dlls = sorted(source.glob('*.dll'))
    if not dlls:
        raise RuntimeError(f'FFmpeg runtime DLLs are missing: {source}')
    destination = Path(output) / 'lib'
    destination.mkdir(exist_ok=True)
    namespace = 'urn:schemas-microsoft-com:asm.v1'
    ET.register_namespace('', namespace)
    assembly = ET.Element(f'{{{namespace}}}assembly', manifestVersion='1.0')
    ET.SubElement(assembly, f'{{{namespace}}}assemblyIdentity', {
        'type': 'win32', 'name': 'lib', 'version': '1.0.0.0', 'processorArchitecture': '*',
    })
    for dll in dlls:
        if dll.name.lower() == 'lib.dll':
            raise RuntimeError('lib.dll would shadow the private assembly manifest')
        shutil.copy2(dll, destination / dll.name)
        ET.SubElement(assembly, f'{{{namespace}}}file', name=dll.name)
    ET.ElementTree(assembly).write(destination / 'lib.manifest', encoding='utf-8', xml_declaration=True)


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    libraries(parser.parse_args().output)
