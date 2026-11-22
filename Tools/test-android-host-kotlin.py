#!/usr/bin/env python3
"""Run the Android host's Kotlin behaviour tests on the host JVM.

Each program under Platforms/Android/tests is compiled together with the
production Kotlin sources it exercises against the SDK's android.jar and run
as a plain `main`. No Android process, device, emulator, Gradle build or test
runner is involved; the android.jar only provides the compile-time API.

Inputs (no download is attempted):
  KOTLINC       kotlinc launcher; otherwise `kotlinc` on PATH, otherwise the
                Android Studio Kotlin plugin (`<studio>/plugins/Kotlin/kotlinc`).
  ANDROID_HOME  SDK root (or ANDROID_SDK_ROOT); the newest `platforms/android-*`
                containing android.jar is used, or pass ANDROID_JAR directly.
  JAVA_HOME     JDK used to run the compiled tests; otherwise `java` on PATH.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ANDROID = ROOT / 'Platforms/Android'
SOURCES = ANDROID / 'app/src/main/java/org/shinkirou/shinkiemu'
TESTS = ANDROID / 'tests'
# Matches `compilerOptions.jvmTarget` in Platforms/Android/app/build.gradle.kts.
JVM_TARGET = '17'
PACKAGE = 'org.shinkirou.shinkiemu'

# Test program -> production sources it is compiled with (see each file's header).
PROGRAMS = {
    'GameWindowTest.kt': ['ShinkiGameWindow.kt'],
    'PickerHostTest.kt': ['ShinkiPlatformBridge.kt', 'ShinkiLibraryTree.kt'],
}


def executable(name):
    return name + ('.bat' if os.name == 'nt' else '')


def kotlinc():
    explicit = os.environ.get('KOTLINC')
    if explicit:
        return Path(explicit)
    on_path = shutil.which('kotlinc')
    if on_path:
        return Path(on_path)
    studios = [Path(p) for p in os.environ.get('ANDROID_STUDIO', '').split(os.pathsep) if p]
    if os.name == 'nt':
        for root in (os.environ.get('ProgramFiles'), os.environ.get('LOCALAPPDATA')):
            if root:
                studios.append(Path(root) / 'Android/Android Studio')
                studios.append(Path(root) / 'Programs/Android Studio')
        studios += [Path(d + ':/Android/Android Studio') for d in 'CDEF']
    elif sys.platform == 'darwin':
        studios.append(Path('/Applications/Android Studio.app/Contents'))
    else:
        studios += [Path('/opt/android-studio'), Path.home() / 'android-studio']
    for studio in studios:
        launcher = studio / 'plugins/Kotlin/kotlinc/bin' / executable('kotlinc')
        if launcher.is_file():
            return launcher
    raise FileNotFoundError('kotlinc not found: set KOTLINC, put kotlinc on PATH, or set ANDROID_STUDIO')


def android_jar():
    explicit = os.environ.get('ANDROID_JAR')
    if explicit:
        return Path(explicit)
    sdk = os.environ.get('ANDROID_HOME') or os.environ.get('ANDROID_SDK_ROOT')
    if not sdk:
        raise FileNotFoundError('set ANDROID_HOME (or ANDROID_JAR) to locate platforms/android-*/android.jar')
    platforms = sorted((Path(sdk) / 'platforms').glob('android-*/android.jar'),
                       key=lambda jar: [int(part) for part in jar.parent.name[8:].split('.') if part.isdigit()])
    if not platforms:
        raise FileNotFoundError(f'no platforms/android-*/android.jar under {sdk}')
    return platforms[-1]


def java():
    home = os.environ.get('JAVA_HOME')
    if home:
        return Path(home) / 'bin' / ('java.exe' if os.name == 'nt' else 'java')
    on_path = shutil.which('java')
    if not on_path:
        raise FileNotFoundError('java not found: set JAVA_HOME or put java on PATH')
    return Path(on_path)


class AndroidHostKotlin(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kotlinc = kotlinc()
        cls.jar = android_jar()
        cls.java = java()
        cls.stdlib = cls.kotlinc.parent.parent / 'lib/kotlin-stdlib.jar'
        if not cls.stdlib.is_file():
            raise FileNotFoundError(f'kotlin-stdlib.jar missing beside {cls.kotlinc}')
        print(f'kotlinc={cls.kotlinc}\nandroid.jar={cls.jar}\njava={cls.java}', file=sys.stderr)

    def run_program(self, test, sources):
        with tempfile.TemporaryDirectory(prefix='android-host-kotlin-') as temporary:
            classes = Path(temporary) / 'classes.jar'
            compile_args = [str(self.kotlinc), '-jvm-target', JVM_TARGET, '-cp', str(self.jar),
                            '-d', str(classes), *(str(SOURCES / s) for s in sources), str(TESTS / test)]
            compiled = subprocess.run(compile_args, capture_output=True, text=True, shell=os.name == 'nt')
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            main_class = f'{PACKAGE}.{Path(test).stem}Kt'
            classpath = os.pathsep.join([str(classes), str(self.jar), str(self.stdlib)])
            ran = subprocess.run([str(self.java), '-cp', classpath, main_class],
                                 capture_output=True, text=True, timeout=300)
            sys.stderr.write(ran.stdout)
            self.assertEqual(ran.returncode, 0, ran.stderr)

    def test_game_window_policy(self):
        self.run_program('GameWindowTest.kt', PROGRAMS['GameWindowTest.kt'])

    def test_picker_host(self):
        self.run_program('PickerHostTest.kt', PROGRAMS['PickerHostTest.kt'])

    def test_every_program_is_listed(self):
        self.assertEqual(sorted(p.name for p in TESTS.glob('*.kt')), sorted(PROGRAMS))


if __name__ == '__main__':
    unittest.main()
