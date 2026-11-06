#!/usr/bin/env python3
"""Exercise Android's native cache-flush linkage with the pinned NDK.

Run with ANDROID_NDK_HOME set. This builds real AArch64 ELFs; no device is
required. The failure fixture matches libffi's native cache-flush reference.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class AndroidCompilerRuntime(unittest.TestCase):
    def test_native_cache_flush_resolves_before_device_load(self):
        ndk = Path(os.environ['ANDROID_NDK_HOME'])
        host = {'win32': 'windows-x86_64', 'darwin': 'darwin-x86_64'}.get(
            sys.platform, 'linux-x86_64')
        suffix = '.exe' if sys.platform == 'win32' else ''
        tools = ndk / 'toolchains/llvm/prebuilt' / host / 'bin'
        clang = tools / ('clang' + suffix)
        nm = tools / ('llvm-nm' + suffix)
        target = '--target=aarch64-linux-android26'
        archive = Path(subprocess.run(
            [str(clang), target, '--print-libgcc-file-name'], check=True,
            capture_output=True, text=True).stdout.strip())
        self.assertTrue(archive.is_file(), archive)
        with tempfile.TemporaryDirectory(prefix='android-compiler-rt-') as temporary:
            root = Path(temporary)
            source = root / 'flush.c'
            source.write_text(
                'void flush(char *begin, char *end) {\n'
                '    __builtin___clear_cache(begin, end);\n'
                '}\n', encoding='utf-8')
            library = root / 'libflush.so'
            args = [str(clang), target, '-shared', '-fPIC', '-nodefaultlibs',
                    '-Wl,--no-undefined', str(source), '-o', str(library)]
            missing = subprocess.run(args, capture_output=True, text=True)
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn('__clear_cache', missing.stderr)
            linked = subprocess.run(args + [str(archive), '-lc'],
                                    capture_output=True, text=True)
            self.assertEqual(linked.returncode, 0, linked.stderr)
            undefined = subprocess.run(
                [str(nm), '-D', '--undefined-only', str(library)], check=True,
                capture_output=True, text=True).stdout
            self.assertNotIn('__clear_cache', undefined)


if __name__ == '__main__':
    unittest.main()
