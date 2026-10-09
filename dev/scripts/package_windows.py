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
        'type': 'win32', 'name': 'lib', 'version': '1.0.0.0', 'processorArchitecture': 'amd64',
    })
    for dll in dlls:
        if dll.name.lower() == 'lib.dll':
            raise RuntimeError('lib.dll would shadow the private assembly manifest')
        shutil.copy2(dll, destination / dll.name)
        ET.SubElement(assembly, f'{{{namespace}}}file', name=dll.name)
    ET.ElementTree(assembly).write(destination / 'lib.manifest', encoding='utf-8', xml_declaration=True)


def verify_assembly(output):
    """Ask Windows to bind the real packaged assembly before a Rust build.

    ACTCTXW layout/flags: https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-actctxw
    """
    import ctypes
    from ctypes import wintypes

    class ACTCTXW(ctypes.Structure):
        _fields_ = [('cbSize', wintypes.ULONG), ('dwFlags', wintypes.DWORD),
                    ('lpSource', wintypes.LPCWSTR), ('wProcessorArchitecture', wintypes.USHORT),
                    ('wLangId', wintypes.USHORT), ('lpAssemblyDirectory', wintypes.LPCWSTR),
                    ('lpResourceName', wintypes.LPCWSTR), ('lpApplicationName', wintypes.LPCWSTR),
                    ('hModule', wintypes.HMODULE)]

    output = Path(output).resolve()
    manifest = output / 'runtime.manifest'
    shutil.copyfile(Path(__file__).resolve().parents[1] / 'windows/runtime.manifest', manifest)
    try:
        context = ACTCTXW()
        context.cbSize = ctypes.sizeof(context)
        context.dwFlags = 0x4  # ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID
        context.lpSource = str(manifest)
        context.lpAssemblyDirectory = str(output)
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.CreateActCtxW.argtypes = [ctypes.POINTER(ACTCTXW)]
        kernel.CreateActCtxW.restype = wintypes.HANDLE
        kernel.ReleaseActCtx.argtypes = [wintypes.HANDLE]
        kernel.ReleaseActCtx.restype = None
        handle = kernel.CreateActCtxW(ctypes.byref(context))
        if handle == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        kernel.ReleaseActCtx(handle)
        print('Windows private runtime assembly: activation passed')
    finally:
        manifest.unlink()


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--verify', action='store_true', help='Bind the copied assembly on Windows')
    args = parser.parse_args()
    libraries(args.output)
    if args.verify:
        verify_assembly(args.output)
